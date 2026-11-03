//! Credentials, connection, and the errors the window knows how to handle.

use serde::{Deserialize, Serialize};
use termix_core::Machine;
use termix_session::{SessionError, Workspace};
use termix_ssh::{Auth, SshError, SshSession};

/// What the window sent for one action. Nothing here is stored unless
/// `remember` is set, and then only encrypted, after the login has succeeded.
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Credentials {
    pub password: Option<String>,
    pub key_path: Option<String>,
    pub key_passphrase: Option<String>,
    pub remember: bool,
}

/// Errors the window reacts to, not just displays. `HostKeyUnknown` carries the
/// fingerprint so the person can compare it before it is sent back to be pinned.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppError {
    HostKeyUnknown {
        message: String,
        algorithm: String,
        fingerprint: String,
    },
    HostKeyChanged {
        message: String,
    },
    NeedsConfirmation {
        message: String,
    },
    /// Nothing to sign in with, or `failed` when the server refused what was tried.
    NeedsCredentials {
        message: String,
        failed: bool,
    },
    Other {
        message: String,
    },
}

impl AppError {
    pub fn other(message: impl ToString) -> Self {
        Self::Other {
            message: message.to_string(),
        }
    }
}

impl From<SessionError> for AppError {
    fn from(error: SessionError) -> Self {
        let message = error.to_string();
        match error {
            SessionError::Ssh(SshError::HostKeyUnknown {
                algorithm,
                fingerprint,
                ..
            }) => Self::HostKeyUnknown {
                message,
                algorithm,
                fingerprint,
            },
            SessionError::Ssh(SshError::HostKeyMismatch { .. })
            | SessionError::AlreadyPinned { .. } => Self::HostKeyChanged { message },
            SessionError::NeedsConfirmation => Self::NeedsConfirmation { message },
            SessionError::NoCredentials(_) => Self::NeedsCredentials {
                message,
                failed: false,
            },
            SessionError::Ssh(SshError::AuthFailed) => Self::NeedsCredentials {
                message,
                failed: true,
            },
            _ => Self::Other { message },
        }
    }
}

pub fn resolve_auth(
    workspace: &Workspace,
    machine: &Machine,
    credentials: &Credentials,
) -> Result<Auth, AppError> {
    if let Some(path) = credentials
        .key_path
        .as_deref()
        .filter(|path| !path.is_empty())
    {
        return Ok(Auth::PrivateKey {
            path: path.into(),
            passphrase: credentials
                .key_passphrase
                .clone()
                .filter(|passphrase| !passphrase.is_empty()),
        });
    }
    match credentials
        .password
        .as_deref()
        .filter(|password| !password.is_empty())
    {
        Some(password) => Ok(Auth::Password(password.to_string())),
        None => Ok(workspace.saved_auth(machine)?),
    }
}

pub async fn connect(
    workspace: &Workspace,
    machine: &Machine,
    credentials: &Credentials,
    trust: Option<&str>,
) -> Result<SshSession, AppError> {
    let auth = resolve_auth(workspace, machine, credentials)?;
    let session = workspace.connect(machine, auth, trust).await?;
    remember_if_asked(workspace, machine, credentials)?;
    Ok(session)
}

pub fn remember_if_asked(
    workspace: &Workspace,
    machine: &Machine,
    credentials: &Credentials,
) -> Result<(), AppError> {
    if !credentials.remember {
        return Ok(());
    }
    match credentials
        .password
        .as_deref()
        .filter(|password| !password.is_empty())
    {
        Some(password) => Ok(workspace.remember_password(machine, password)?),
        None => Ok(()),
    }
}
