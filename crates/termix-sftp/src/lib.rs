//! SFTP on top of an already authenticated [`termix_ssh::SshSession`].

use russh_sftp::client::SftpSession;
use termix_ssh::{session_handle, SshError, SshSession};

#[derive(Debug, thiserror::Error)]
pub enum SftpError {
    #[error(transparent)]
    Ssh(#[from] SshError),
    #[error(transparent)]
    Russh(#[from] russh::Error),
    #[error(transparent)]
    Protocol(#[from] russh_sftp::client::error::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
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
            let child = if path == "/" {
                format!("/{name}")
            } else {
                format!("{}/{}", path.trim_end_matches('/'), name)
            };
            out.push(RemoteEntry { name, path: child });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
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
