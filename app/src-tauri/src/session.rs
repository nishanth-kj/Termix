use std::time::Duration;

use termix_automation::prepare;
use termix_core::Protocol;
use termix_logging::redact;
use termix_security::Vault;
use termix_ssh::{Auth, ConnectRequest, SshError, SshSession};
use termix_storage::{data_dir, Database};

#[derive(serde::Serialize)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<u32>,
}

pub fn load(db: &Database, id_or_name: &str) -> Result<termix_core::Machine, String> {
    db.resolve(id_or_name).map_err(|error| error.to_string())
}

pub async fn run(
    db: &Database,
    id: &str,
    command: &str,
    password: &str,
    trust: bool,
    yes: bool,
) -> Result<CommandOutput, String> {
    let prepared = prepare(command, yes)
        .map_err(|_| "this command looks destructive; confirm it before running".to_string())?;
    if password.is_empty() {
        return Err("password is empty".into());
    }
    let machine = load(db, id)?;
    if machine.protocol != Protocol::Ssh && machine.protocol != Protocol::Sftp {
        return Err(format!("{} is not an SSH machine", machine.protocol));
    }
    let session = connect(db, &machine, trust, password).await?;
    let result = session
        .exec(&prepared.body)
        .await
        .map_err(|error| error.to_string())?;
    db.log(Some(machine.id), "exec", &redact(&prepared.body))
        .map_err(|error| error.to_string())?;
    db.mark_connected(machine.id)
        .map_err(|error| error.to_string())?;
    Ok(CommandOutput {
        stdout: result.stdout,
        stderr: result.stderr,
        exit_code: result.exit_code,
    })
}

async fn connect(
    db: &Database,
    machine: &termix_core::Machine,
    trust: bool,
    password: &str,
) -> Result<SshSession, String> {
    let _vault = open_vault()?;
    let trusted = db
        .trusted_fingerprint(&machine.host, machine.port)
        .map_err(|error| error.to_string())?;
    let request = |fingerprint: Option<String>| ConnectRequest {
        host: machine.host.clone(),
        port: machine.port,
        username: machine.username.clone(),
        auth: Auth::Password(password.to_string()),
        trusted_fingerprint: fingerprint,
        timeout: Duration::from_secs(15),
        jump_host: None,
    };
    match SshSession::connect(request(trusted.clone())).await {
        Ok(session) => Ok(session),
        Err(SshError::HostKeyUnknown {
            algorithm,
            fingerprint,
            ..
        }) if trust && trusted.is_none() => {
            db.trust_host(&machine.host, machine.port, &algorithm, &fingerprint)
                .map_err(|error| error.to_string())?;
            SshSession::connect(request(Some(fingerprint)))
                .await
                .map_err(|error| error.to_string())
        }
        Err(SshError::HostKeyUnknown {
            algorithm,
            fingerprint,
            host,
            port,
        }) => Err(format!(
            "host key is not trusted for {host}:{port}: {algorithm} {fingerprint}"
        )),
        Err(error) => Err(error.to_string()),
    }
}

fn open_vault() -> Result<Vault, String> {
    let path = data_dir().join("master.key");
    if path.exists() {
        termix_security::load_key_file(&path).map_err(|error| error.to_string())
    } else {
        let vault = Vault::generate();
        termix_security::store_key_file(&path, &vault).map_err(|error| error.to_string())?;
        Ok(vault)
    }
}
