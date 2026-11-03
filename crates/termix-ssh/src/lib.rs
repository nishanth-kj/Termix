//! Direct SSH sessions.
//!
//! Unknown and mismatched host keys are rejected. Callers must show the
//! fingerprint and, only after the user agrees, store it and reconnect.
//! Jump hosts, proxies, and ssh-agent are not implemented.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use russh::client::{self, Handler};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{ChannelMsg, Disconnect};
use tokio::net::TcpStream;
use tokio::time::timeout;

#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error(transparent)]
    Russh(#[from] russh::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("authentication failed")]
    AuthFailed,
    #[error("host key for {host}:{port} is not trusted ({algorithm} {fingerprint})")]
    HostKeyUnknown {
        host: String,
        port: u16,
        algorithm: String,
        fingerprint: String,
    },
    #[error("host key for {host}:{port} changed ({algorithm} {fingerprint})")]
    HostKeyMismatch {
        host: String,
        port: u16,
        algorithm: String,
        fingerprint: String,
    },
    #[error("timed out connecting to {0}:{1}")]
    Timeout(String, u16),
    #[error("{0} is not implemented")]
    NotImplemented(&'static str),
    #[error("{0}")]
    Message(String),
}

#[derive(Clone)]
pub enum Auth {
    Password(String),
    PrivateKey {
        path: std::path::PathBuf,
        passphrase: Option<String>,
    },
}

#[derive(Clone)]
pub struct ConnectRequest {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: Auth,
    /// Fingerprint previously accepted by the user, if any.
    pub trusted_fingerprint: Option<String>,
    pub timeout: Duration,
    pub jump_host: Option<String>,
}

pub struct HostVerifier {
    host: String,
    port: u16,
    trusted: Option<String>,
}

impl Handler for HostVerifier {
    type Error = SshError;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let (algorithm, fingerprint) = fingerprint_of(server_public_key);
        match &self.trusted {
            Some(expected) if expected == &fingerprint => Ok(true),
            Some(_) => Err(SshError::HostKeyMismatch {
                host: self.host.clone(),
                port: self.port,
                algorithm,
                fingerprint,
            }),
            None => Err(SshError::HostKeyUnknown {
                host: self.host.clone(),
                port: self.port,
                algorithm,
                fingerprint,
            }),
        }
    }
}

fn fingerprint_of(key: &PublicKeyOrCertificate) -> (String, String) {
    let public = key.public_key();
    let algorithm = public.algorithm().to_string();
    let fingerprint = public.fingerprint(HashAlg::Sha256).to_string();
    (algorithm, fingerprint)
}

pub struct SshSession {
    handle: client::Handle<HostVerifier>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<u32>,
}

impl SshSession {
    pub async fn connect(request: ConnectRequest) -> Result<Self, SshError> {
        if request.jump_host.is_some() {
            return Err(SshError::NotImplemented("jump hosts"));
        }
        let config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(60)),
            ..client::Config::default()
        });
        let verify = HostVerifier {
            host: request.host.clone(),
            port: request.port,
            trusted: request.trusted_fingerprint.clone(),
        };
        let stream = timeout(
            request.timeout,
            TcpStream::connect((request.host.as_str(), request.port)),
        )
        .await
        .map_err(|_| SshError::Timeout(request.host.clone(), request.port))?
        .map_err(SshError::Io)?;
        let mut handle = client::connect_stream(config, stream, verify).await?;
        match request.auth {
            Auth::Password(password) => {
                let result = handle
                    .authenticate_password(request.username, password)
                    .await?;
                if !result.success() {
                    return Err(SshError::AuthFailed);
                }
            }
            Auth::PrivateKey { path, passphrase } => {
                authenticate_key(&mut handle, &request.username, &path, passphrase.as_deref())
                    .await?;
            }
        }
        Ok(Self { handle })
    }

    pub async fn exec(&self, command: &str) -> Result<ExecResult, SshError> {
        let mut channel = self.handle.channel_open_session().await?;
        channel.exec(true, command).await?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code = None;
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => stdout.extend_from_slice(&data),
                Some(ChannelMsg::ExtendedData { data, ext: 1 }) => {
                    stderr.extend_from_slice(&data);
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => exit_code = Some(exit_status),
                Some(ChannelMsg::Eof) => {}
                Some(ChannelMsg::Close) | None => break,
                Some(_) => {}
            }
        }
        Ok(ExecResult {
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            exit_code,
        })
    }

    pub async fn open_shell(&self, columns: u32, rows: u32) -> Result<Shell, SshError> {
        let channel = self.handle.channel_open_session().await?;
        channel
            .request_pty(false, "xterm-256color", columns, rows, 0, 0, &[])
            .await?;
        channel.request_shell(true).await?;
        Ok(Shell { channel })
    }

    pub async fn disconnect(&self) -> Result<(), SshError> {
        self.handle
            .disconnect(Disconnect::ByApplication, "", "English")
            .await?;
        Ok(())
    }

    pub(crate) fn handle(&self) -> &client::Handle<HostVerifier> {
        &self.handle
    }
}

async fn authenticate_key(
    handle: &mut client::Handle<HostVerifier>,
    username: &str,
    path: &Path,
    passphrase: Option<&str>,
) -> Result<(), SshError> {
    let key = russh::keys::load_secret_key(path, passphrase)
        .map_err(|error| SshError::Message(format!("could not read private key: {error}")))?;
    let hash = handle.best_supported_rsa_hash().await?.flatten();
    let result = handle
        .authenticate_publickey(username, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
        .await?;
    if result.success() {
        Ok(())
    } else {
        Err(SshError::AuthFailed)
    }
}

pub struct Shell {
    channel: russh::Channel<russh::client::Msg>,
}

impl Shell {
    pub async fn write(&self, bytes: &[u8]) -> Result<(), SshError> {
        self.channel
            .data_bytes(bytes::Bytes::copy_from_slice(bytes))
            .await?;
        Ok(())
    }

    pub async fn read_some(&mut self) -> Result<Option<Vec<u8>>, SshError> {
        match self.channel.wait().await {
            Some(ChannelMsg::Data { data }) => Ok(Some(data.to_vec())),
            Some(ChannelMsg::ExtendedData { data, .. }) => Ok(Some(data.to_vec())),
            Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => Ok(None),
            Some(_) => Ok(Some(Vec::new())),
        }
    }

    pub async fn resize(&self, columns: u32, rows: u32) -> Result<(), SshError> {
        self.channel.window_change(columns, rows, 0, 0).await?;
        Ok(())
    }

    pub async fn close(&self) -> Result<(), SshError> {
        self.channel.close().await?;
        Ok(())
    }
}

/// Exposed for the SFTP crate in this workspace.
pub fn session_handle(session: &SshSession) -> &client::Handle<HostVerifier> {
    session.handle()
}
