//! Plans a copy set from two directory listings.
//!
//! This crate never deletes files. A plan lists what would be deleted so a
//! person can see it. Copies are applied by `termix-session`.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListedFile {
    pub relative_path: String,
    pub size: u64,
    pub modified_unix: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    LocalToRemote,
    RemoteToLocal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyOp {
    pub relative_path: String,
    pub direction: Direction,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncPlan {
    pub copies: Vec<CopyOp>,
    pub conflicts: Vec<String>,
    pub would_delete: Vec<String>,
}

/// A rule excludes a file when it equals one of the path's folders or names,
/// or the start of the path. `target` matches `target/a.o` but not `retarget.txt`.
fn is_excluded(path: &str, excludes: &[String]) -> bool {
    excludes.iter().any(|rule| {
        let rule = rule.trim_matches('/');
        path == rule
            || path.starts_with(&format!("{rule}/"))
            || path.split('/').any(|part| part == rule)
    })
}

pub fn plan(
    direction: Direction,
    local: &[ListedFile],
    remote: &[ListedFile],
    excludes: &[String],
) -> SyncPlan {
    let (source, destination, missing, newer) = match direction {
        Direction::LocalToRemote => (
            local,
            remote,
            "missing on remote",
            "local is newer or a different size",
        ),
        Direction::RemoteToLocal => (
            remote,
            local,
            "missing locally",
            "remote is newer or a different size",
        ),
    };
    let existing: HashMap<&str, &ListedFile> = destination
        .iter()
        .map(|file| (file.relative_path.as_str(), file))
        .collect();
    let sources: HashMap<&str, &ListedFile> = source
        .iter()
        .map(|file| (file.relative_path.as_str(), file))
        .collect();

    let mut copies = Vec::new();
    let mut conflicts = Vec::new();
    for file in source {
        if is_excluded(&file.relative_path, excludes) {
            continue;
        }
        let reason = match existing.get(file.relative_path.as_str()) {
            None => missing,
            Some(other) if other.modified_unix > file.modified_unix && other.size != file.size => {
                conflicts.push(file.relative_path.clone());
                continue;
            }
            Some(other) if other.size != file.size || other.modified_unix < file.modified_unix => {
                newer
            }
            Some(_) => continue,
        };
        copies.push(CopyOp {
            relative_path: file.relative_path.clone(),
            direction,
            reason: reason.into(),
        });
    }
    let would_delete = destination
        .iter()
        .filter(|file| !is_excluded(&file.relative_path, excludes))
        .filter(|file| !sources.contains_key(file.relative_path.as_str()))
        .map(|file| file.relative_path.clone())
        .collect();

    SyncPlan {
        copies,
        conflicts,
        would_delete,
    }
}

/// True when a relative path stays inside the folder it is joined to.
pub fn is_safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && !path.contains(':')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Regular files below `root` with `/` separators. Symlinks are not followed.
pub fn list_local(root: &Path) -> std::io::Result<Vec<ListedFile>> {
    let mut files = Vec::new();
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = pending.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let relative = format!("{prefix}{name}");
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push((entry.path(), format!("{relative}/")));
            } else if kind.is_file() {
                let metadata = entry.metadata()?;
                let modified_unix = metadata
                    .modified()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |elapsed| elapsed.as_secs() as i64);
                files.push(ListedFile {
                    relative_path: relative,
                    size: metadata.len(),
                    modified_unix,
                });
            }
        }
    }
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, size: u64, modified_unix: i64) -> ListedFile {
        ListedFile {
            relative_path: path.into(),
            size,
            modified_unix,
        }
    }

    #[test]
    fn plan_does_not_hide_deletes_and_does_not_copy_conflicts() {
        let local = vec![file("a.txt", 2, 10)];
        let remote = vec![file("a.txt", 9, 20), file("only-remote.txt", 1, 1)];
        let plan = plan(Direction::LocalToRemote, &local, &remote, &[]);
        assert!(plan.copies.is_empty());
        assert_eq!(plan.conflicts, vec!["a.txt".to_string()]);
        assert_eq!(plan.would_delete, vec!["only-remote.txt".to_string()]);
    }

    #[test]
    fn both_directions_behave_the_same() {
        let near = vec![file("new.txt", 1, 1), file("same.txt", 1, 1)];
        let far = vec![file("same.txt", 1, 1), file("extra.txt", 1, 1)];
        let up = plan(Direction::LocalToRemote, &near, &far, &[]);
        let down = plan(Direction::RemoteToLocal, &far, &near, &[]);
        assert_eq!(up.would_delete, vec!["extra.txt".to_string()]);
        assert_eq!(down.would_delete, up.would_delete);
        assert_eq!(up.copies[0].relative_path, "new.txt");
        assert_eq!(down.copies[0].relative_path, "new.txt");
    }

    #[test]
    fn excludes_match_whole_path_parts() {
        let local = vec![
            file("target/a.o", 1, 1),
            file("src/target/b.o", 1, 1),
            file("retarget.txt", 1, 1),
        ];
        let plan = plan(
            Direction::LocalToRemote,
            &local,
            &[],
            &["target".to_string()],
        );
        let copied: Vec<_> = plan
            .copies
            .iter()
            .map(|c| c.relative_path.as_str())
            .collect();
        assert_eq!(copied, vec!["retarget.txt"]);
    }

    #[test]
    fn unsafe_relative_paths_are_rejected() {
        for path in [
            "",
            "/etc/passwd",
            "../x",
            "a/../../x",
            "a\\b",
            "c:/x",
            "a//b",
        ] {
            assert!(!is_safe_relative(path), "{path}");
        }
        assert!(is_safe_relative("a/b.txt"));
    }

    #[test]
    fn lists_files_recursively_with_forward_slashes() {
        let root = std::env::temp_dir().join(format!("termix-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.txt"), b"12").unwrap();
        std::fs::write(root.join("sub").join("b.txt"), b"345").unwrap();
        let files = list_local(&root).unwrap();
        let names: Vec<_> = files.iter().map(|f| f.relative_path.as_str()).collect();
        assert_eq!(names, vec!["a.txt", "sub/b.txt"]);
        assert_eq!(files[1].size, 3);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
