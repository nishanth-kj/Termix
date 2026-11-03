//! SFTP on top of an already authenticated [`termix_ssh::SshSession`].

use russh_sftp::client::SftpSession;
use termix_ssh::{session_handle, SshError, SshSession};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

#[derive(Debug, thiserror::Error)]
pub enum SftpError {
    #[error(transparent)]
    Ssh(#[from] SshError),
    #[error(transparent)]
    Russh(#[from] russh::Error),
    #[error(transparent)]
    Protocol(#[from] russh_sftp::client::error::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("server returned an unsafe file name: {0:?}")]
    UnsafeName(String),
}

/// A name from a directory listing must be one path component. Otherwise a
/// hostile server could point a later download outside the target folder.
pub fn is_safe_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified_unix: i64,
}

pub struct Sftp {
    inner: SftpSession,
}

impl Sftp {
    pub async fn open(session: &SshSession) -> Result<Self, SftpError> {
        let channel = session_handle(session).channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let inner = SftpSession::new(channel.into_stream()).await?;
        Ok(Self { inner })
    }

    pub async fn list(&self, path: &str) -> Result<Vec<RemoteEntry>, SftpError> {
        let entries = self.inner.read_dir(path).await?;
        let mut out = Vec::new();
        for entry in entries {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            if !is_safe_name(&name) {
                return Err(SftpError::UnsafeName(name));
            }
            let metadata = entry.metadata();
            let file_type = entry.file_type();
            let child = if path == "/" {
                format!("/{name}")
            } else {
                format!("{}/{}", path.trim_end_matches('/'), name)
            };
            out.push(RemoteEntry {
                name,
                path: child,
                is_dir: file_type.is_dir(),
                is_symlink: file_type.is_symlink(),
                size: metadata.len(),
                modified_unix: metadata.mtime.map_or(0, i64::from),
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Every regular file below `root`, with paths relative to it. Symlinks are
    /// not followed, so a link cannot pull files from outside the tree.
    pub async fn walk(&self, root: &str) -> Result<Vec<(String, RemoteEntry)>, SftpError> {
        let trimmed = root.trim_end_matches('/');
        let start = if trimmed.is_empty() { "/" } else { trimmed };
        let mut pending = vec![(start.to_string(), String::new())];
        let mut files = Vec::new();
        while let Some((directory, prefix)) = pending.pop() {
            for entry in self.list(&directory).await? {
                let relative = format!("{prefix}{}", entry.name);
                if entry.is_dir {
                    pending.push((entry.path.clone(), format!("{relative}/")));
                } else if !entry.is_symlink {
                    files.push((relative, entry));
                }
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(files)
    }

    /// Stream a remote file into `output` without holding it in memory.
    pub async fn download<W>(&self, path: &str, output: &mut W) -> Result<u64, SftpError>
    where
        W: AsyncWrite + Unpin,
    {
        let mut file = self.inner.open(path).await?;
        Ok(tokio::io::copy(&mut file, output).await?)
    }

    /// Stream `input` into a remote file, replacing it if it exists.
    pub async fn upload<R>(&self, input: &mut R, path: &str) -> Result<u64, SftpError>
    where
        R: AsyncRead + Unpin,
    {
        let mut file = self.inner.create(path).await?;
        let copied = tokio::io::copy(input, &mut file).await?;
        file.shutdown().await?;
        Ok(copied)
    }

    pub async fn read(&self, path: &str) -> Result<Vec<u8>, SftpError> {
        Ok(self.inner.read(path).await?)
    }

    pub async fn write(&self, path: &str, data: &[u8]) -> Result<(), SftpError> {
        self.inner.write(path, data).await?;
        Ok(())
    }

    pub async fn create_dir(&self, path: &str) -> Result<(), SftpError> {
        self.inner.create_dir(path).await?;
        Ok(())
    }

    pub async fn remove_file(&self, path: &str) -> Result<(), SftpError> {
        self.inner.remove_file(path).await?;
        Ok(())
    }

    pub async fn remove_dir(&self, path: &str) -> Result<(), SftpError> {
        self.inner.remove_dir(path).await?;
        Ok(())
    }

    pub async fn rename(&self, from: &str, to: &str) -> Result<(), SftpError> {
        self.inner.rename(from, to).await?;
        Ok(())
    }

    pub async fn canonicalize(&self, path: &str) -> Result<String, SftpError> {
        Ok(self.inner.canonicalize(path).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::is_safe_name;

    #[test]
    fn names_that_leave_the_folder_are_unsafe() {
        for name in [
            "",
            ".",
            "..",
            "../etc",
            "a/b",
            "a\\b",
            "/etc/passwd",
            "a\0b",
        ] {
            assert!(!is_safe_name(name), "{name:?}");
        }
        for name in ["a.txt", "..hidden", "with space"] {
            assert!(is_safe_name(name), "{name:?}");
        }
    }
}
