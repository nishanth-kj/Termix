//! The workflows the CLI and the app share, so both enforce the same rules.
//!
//! A host key is pinned only when the caller names the exact fingerprint it
//! wants to trust. "Trust whatever is presented" is deliberately not possible.

use std::path::{Path, PathBuf};
use std::time::Duration;

use termix_automation::prepare;
use termix_core::{Machine, Protocol};
use termix_logging::redact;
use termix_monitor::{parse_remote_sample, Sample, REMOTE_SAMPLE_COMMAND};
use termix_security::{load_or_create_key_file, Vault};
use termix_sftp::{Sftp, SftpError};
use termix_ssh::{Auth, ConnectRequest, ExecResult, SshError, SshSession};
use termix_storage::{Database, StoreError};
use termix_sync::{is_safe_relative, list_local, plan, Direction, ListedFile, SyncPlan};
use tokio::io::AsyncWriteExt;
use zeroize::Zeroizing;

const PASSWORD_SECRET: &str = "password";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error(transparent)]
    Ssh(#[from] SshError),
    #[error(transparent)]
    Sftp(#[from] SftpError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0} is not an SSH machine")]
    NotSsh(String),
    #[error("this command looks destructive; confirm it before running")]
    NeedsConfirmation,
    #[error(
        "{host}:{port} is already pinned to {pinned}. Forget that host first if its key really changed"
    )]
    AlreadyPinned {
        host: String,
        port: u16,
        pinned: String,
    },
    #[error("no saved credentials for {0}; provide a password or a key")]
    NoCredentials(String),
    #[error("{0} did not return /proc metrics; remote metrics support Linux hosts")]
    NotLinux(String),
    #[error("could not parse metrics from {0}: {1}")]
    BadMetrics(String, String),
    #[error("refusing an unsafe path from the sync plan: {0}")]
    UnsafePath(String),
}

/// The local database and the key that protects its secrets.
pub struct Workspace {
    pub db: Database,
    pub vault: Vault,
}

impl Workspace {
    pub fn open(dir: &Path) -> Result<Self, SessionError> {
        std::fs::create_dir_all(dir)?;
        Ok(Self {
            db: Database::open(&dir.join("termix.sqlite"))?,
            vault: load_or_create_key_file(&dir.join("master.key"))?,
        })
    }

    pub fn remember_password(&self, machine: &Machine, password: &str) -> Result<(), SessionError> {
        self.db.set_machine_secret(
            &self.vault,
            machine.id,
            PASSWORD_SECRET,
            password.as_bytes(),
        )?;
        Ok(())
    }

    pub fn forget_password(&self, machine: &Machine) -> Result<(), SessionError> {
        self.db.clear_machine_secret(machine.id, PASSWORD_SECRET)?;
        Ok(())
    }

    pub fn has_saved_password(&self, machine: &Machine) -> Result<bool, SessionError> {
        Ok(self.saved_password(machine)?.is_some())
    }

    /// The saved password as credentials, or an error if nothing was saved.
    pub fn saved_auth(&self, machine: &Machine) -> Result<Auth, SessionError> {
        let password = self
            .saved_password(machine)?
            .ok_or_else(|| SessionError::NoCredentials(machine.name.clone()))?;
        Ok(Auth::Password(password.to_string()))
    }

    fn saved_password(&self, machine: &Machine) -> Result<Option<Zeroizing<String>>, SessionError> {
        let secret = self
            .db
            .machine_secret(&self.vault, machine.id, PASSWORD_SECRET)?;
        Ok(secret.map(|bytes| Zeroizing::new(String::from_utf8_lossy(&bytes).into_owned())))
    }

    /// Connect and verify the host key.
    ///
    /// With no pin and no `trust_fingerprint` this fails with
    /// [`SshError::HostKeyUnknown`], which carries the fingerprint to show the
    /// user, and no credentials are sent. The fingerprint is pinned only after
    /// the server has presented exactly that key and authentication succeeded.
    pub async fn connect(
        &self,
        machine: &Machine,
        auth: Auth,
        trust_fingerprint: Option<&str>,
    ) -> Result<SshSession, SessionError> {
        if machine.protocol != Protocol::Ssh && machine.protocol != Protocol::Sftp {
            return Err(SessionError::NotSsh(machine.name.clone()));
        }
        let pinned = self.db.trusted_fingerprint(&machine.host, machine.port)?;
        if let (Some(pinned), Some(wanted)) = (&pinned, trust_fingerprint) {
            if pinned != wanted {
                return Err(SessionError::AlreadyPinned {
                    host: machine.host.clone(),
                    port: machine.port,
                    pinned: pinned.clone(),
                });
            }
        }
        let is_new = pinned.is_none();
        let session = SshSession::connect(ConnectRequest {
            host: machine.host.clone(),
            port: machine.port,
            username: machine.username.clone(),
            auth,
            trusted_fingerprint: pinned.or_else(|| trust_fingerprint.map(str::to_string)),
            timeout: CONNECT_TIMEOUT,
            jump_host: None,
        })
        .await?;
        if is_new {
            let key = session.host_key();
            self.db.trust_host(
                &machine.host,
                machine.port,
                &key.algorithm,
                &key.fingerprint,
            )?;
        }
        self.db.mark_connected(machine.id)?;
        Ok(session)
    }

    /// Run one command. Destructive-looking commands need `confirmed`, and the
    /// check happens before any connection is made.
    pub async fn run_command(
        &self,
        machine: &Machine,
        auth: Auth,
        trust_fingerprint: Option<&str>,
        command: &str,
        confirmed: bool,
    ) -> Result<ExecResult, SessionError> {
        let prepared = prepare(command, confirmed).map_err(|_| SessionError::NeedsConfirmation)?;
        let session = self.connect(machine, auth, trust_fingerprint).await?;
        let result = session.exec(&prepared.body).await?;
        self.db
            .log(Some(machine.id), "exec", &redact(&prepared.body))?;
        let _ = session.disconnect().await;
        Ok(result)
    }

    /// Plan a folder sync. Listing both sides changes nothing.
    pub async fn plan_sync(
        &self,
        sftp: &Sftp,
        direction: Direction,
        local_root: &Path,
        remote_root: &str,
        excludes: &[String],
    ) -> Result<SyncPlan, SessionError> {
        let local = list_local(local_root)?;
        let remote: Vec<ListedFile> = sftp
            .walk(remote_root)
            .await?
            .into_iter()
            .map(|(relative_path, entry)| ListedFile {
                relative_path,
                size: entry.size,
                modified_unix: entry.modified_unix,
            })
            .collect();
        Ok(plan(direction, &local, &remote, excludes))
    }

    /// Apply the copies in a plan. Files in `conflicts` and `would_delete` are
    /// left alone. Returns the number of files copied.
    pub async fn apply_sync(
        &self,
        machine: &Machine,
        sftp: &Sftp,
        sync_plan: &SyncPlan,
        local_root: &Path,
        remote_root: &str,
    ) -> Result<usize, SessionError> {
        let remote_root = remote_root.trim_end_matches('/');
        for copy in &sync_plan.copies {
            if !is_safe_relative(&copy.relative_path) {
                return Err(SessionError::UnsafePath(copy.relative_path.clone()));
            }
            let remote = format!("{remote_root}/{}", copy.relative_path);
            let local = join_relative(local_root, &copy.relative_path);
            match copy.direction {
                Direction::LocalToRemote => {
                    make_remote_parents(sftp, remote_root, &copy.relative_path).await;
                    let mut input = tokio::fs::File::open(&local).await?;
                    sftp.upload(&mut input, &remote).await?;
                }
                Direction::RemoteToLocal => download_atomically(sftp, &remote, &local).await?,
            }
        }
        self.db.log(
            Some(machine.id),
            "sync",
            &format!("copied {} file(s)", sync_plan.copies.len()),
        )?;
        Ok(sync_plan.copies.len())
    }
}

/// Sample a Linux host over an open session.
pub async fn remote_metrics(session: &SshSession, host: &str) -> Result<Sample, SessionError> {
    let result = session.exec(REMOTE_SAMPLE_COMMAND).await?;
    match parse_remote_sample(&result.stdout) {
        Some(Ok(sample)) => Ok(sample),
        Some(Err(error)) => Err(SessionError::BadMetrics(host.into(), error.to_string())),
        None => Err(SessionError::NotLinux(host.into())),
    }
}

fn join_relative(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

// A folder that already exists makes create_dir fail, and a real failure shows
// up when the upload itself fails, so these errors are not interesting.
async fn make_remote_parents(sftp: &Sftp, remote_root: &str, relative: &str) {
    let mut current = remote_root.to_string();
    let _ = sftp.create_dir(&current).await;
    if let Some((folders, _)) = relative.rsplit_once('/') {
        for part in folders.split('/') {
            current = format!("{current}/{part}");
            let _ = sftp.create_dir(&current).await;
        }
    }
}

// An interrupted download must not leave a truncated file under the real name.
async fn download_atomically(sftp: &Sftp, remote: &str, local: &Path) -> Result<(), SessionError> {
    if let Some(parent) = local.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut partial = local.as_os_str().to_owned();
    partial.push(".termix-part");
    let partial = PathBuf::from(partial);
    let mut output = tokio::fs::File::create(&partial).await?;
    let copied = sftp.download(remote, &mut output).await;
    let flushed = output.flush().await;
    drop(output);
    if let Err(error) = copied {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(error.into());
    }
    flushed?;
    tokio::fs::rename(&partial, local).await?;
    Ok(())
}
