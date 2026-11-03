//! Plans a copy set from two directory listings.
//!
//! This crate does not delete files. Applying a plan is intentionally not
//! implemented yet; a plan with `confirm_deletes` still only reports deletions.

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

pub fn plan(
    direction: Direction,
    local: &[ListedFile],
    remote: &[ListedFile],
    excludes: &[String],
) -> SyncPlan {
    let excluded = |path: &str| excludes.iter().any(|rule| path.contains(rule));
    let mut copies = Vec::new();
    let mut conflicts = Vec::new();
    let mut would_delete = Vec::new();

    match direction {
        Direction::LocalToRemote => {
            for file in local {
                if excluded(&file.relative_path) {
                    continue;
                }
                match remote
                    .iter()
                    .find(|r| r.relative_path == file.relative_path)
                {
                    None => copies.push(CopyOp {
                        relative_path: file.relative_path.clone(),
                        direction,
                        reason: "missing on remote".into(),
                    }),
                    Some(other)
                        if other.size != file.size || other.modified_unix < file.modified_unix =>
                    {
                        if other.modified_unix > file.modified_unix {
                            conflicts.push(file.relative_path.clone());
                        } else {
                            copies.push(CopyOp {
                                relative_path: file.relative_path.clone(),
                                direction,
                                reason: "local is newer or a different size".into(),
                            });
                        }
                    }
                    Some(_) => {}
                }
            }
            for file in remote {
                if excluded(&file.relative_path) {
                    continue;
                }
                if !local.iter().any(|l| l.relative_path == file.relative_path) {
                    would_delete.push(file.relative_path.clone());
                }
            }
        }
        Direction::RemoteToLocal => {
            for file in remote {
                if excluded(&file.relative_path) {
                    continue;
                }
                match local.iter().find(|l| l.relative_path == file.relative_path) {
                    None => copies.push(CopyOp {
                        relative_path: file.relative_path.clone(),
                        direction,
                        reason: "missing locally".into(),
                    }),
                    Some(other)
                        if other.size != file.size || other.modified_unix < file.modified_unix =>
                    {
                        if other.modified_unix > file.modified_unix {
                            conflicts.push(file.relative_path.clone());
                        } else {
                            copies.push(CopyOp {
                                relative_path: file.relative_path.clone(),
                                direction,
                                reason: "remote is newer or a different size".into(),
                            });
                        }
                    }
                    Some(_) => {}
                }
            }
        }
    }

    SyncPlan {
        copies,
        conflicts,
        would_delete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_does_not_hide_deletes_and_does_not_copy_conflicts() {
        let local = vec![ListedFile {
            relative_path: "a.txt".into(),
            size: 2,
            modified_unix: 10,
        }];
        let remote = vec![
            ListedFile {
                relative_path: "a.txt".into(),
                size: 9,
                modified_unix: 20,
            },
            ListedFile {
                relative_path: "only-remote.txt".into(),
                size: 1,
                modified_unix: 1,
            },
        ];
        let plan = plan(Direction::LocalToRemote, &local, &remote, &[]);
        assert!(plan.copies.is_empty());
        assert_eq!(plan.conflicts, vec!["a.txt".to_string()]);
        assert_eq!(plan.would_delete, vec!["only-remote.txt".to_string()]);
    }
}
