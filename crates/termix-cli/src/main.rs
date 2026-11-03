use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand};
use termix_automation::prepare;
use termix_core::{Capability, NewMachine, Protocol, Support};
use termix_logging::redact;
use termix_monitor::sample_local;
use termix_network::tcp_probe;
use termix_security::Vault;
use termix_ssh::{Auth, ConnectRequest, SshError, SshSession};
use termix_storage::{data_dir, Database};

#[derive(Parser)]
#[command(name = "termix", version, about = "Every machine. One control center.")]
struct Cli {
    /// Directory for the SQLite database and master key. Defaults to the OS data directory.
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show which capabilities are actually implemented.
    Support,
    /// Saved machines.
    Machine {
        #[command(subcommand)]
        action: MachineAction,
    },
    /// TCP connect check. This does not authenticate.
    Probe {
        /// Saved machine id or unique name.
        id: String,
    },
    /// Run one remote command over SSH. Password is read from TERMIX_PASSWORD.
    Exec {
        /// Saved machine id or unique name.
        id: String,
        /// Accept the presented host key. Refused if a different key was already trusted.
        #[arg(long)]
        trust: bool,
        /// Private key path. Otherwise TERMIX_PASSWORD is used.
        #[arg(long)]
        key: Option<PathBuf>,
        /// Required when the command looks destructive.
        #[arg(long)]
        yes: bool,
        #[arg(required = true, num_args = 1..)]
        command: Vec<String>,
    },
    /// SFTP listing.
    Sftp {
        #[command(subcommand)]
        action: SftpAction,
    },
    /// Metrics for this computer, from the operating system.
    Local,
}

#[derive(Subcommand)]
enum MachineAction {
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        host: String,
        #[arg(long, default_value_t = 22)]
        port: u16,
        #[arg(long)]
        username: String,
        #[arg(long, default_value = "ssh")]
        protocol: String,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long)]
        group: Option<String>,
    },
    List,
    Remove {
        /// Saved machine id or unique name.
        id: String,
    },
}

#[derive(Subcommand)]
enum SftpAction {
    Ls {
        /// Saved machine id or unique name.
        id: String,
        #[arg(long, default_value = ".")]
        path: String,
        #[arg(long)]
        trust: bool,
        #[arg(long)]
        key: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() {
    termix_logging::init();
    let cli = Cli::parse();
    let dir = cli.data_dir.unwrap_or_else(data_dir);
    if let Err(error) = run(dir, cli.command).await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

async fn run(dir: PathBuf, command: Command) -> Result<(), String> {
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let db = Database::open(&dir.join("termix.sqlite")).map_err(|e| e.to_string())?;
    let _vault = open_vault(&dir)?;
    match command {
        Command::Support => {
            for capability in [
                Capability::Ssh,
                Capability::Sftp,
                Capability::LocalTerminal,
                Capability::HostKeyVerification,
                Capability::EncryptedSecrets,
                Capability::SystemMonitorLocal,
                Capability::SystemMonitorSsh,
                Capability::FolderSyncPlan,
                Capability::FolderSyncApply,
                Capability::PortForwardLocal,
                Capability::JumpHost,
                Capability::Docker,
                Capability::Kubernetes,
                Capability::Rdp,
                Capability::Vnc,
                Capability::SshAgent,
            ] {
                let state = match capability.support() {
                    Support::Implemented => "implemented",
                    Support::NotImplemented => "not implemented",
                };
                println!("{:<28} {state}", capability.label());
            }
        }
        Command::Machine { action } => match action {
            MachineAction::Add {
                name,
                host,
                port,
                username,
                protocol,
                tag,
                group,
            } => {
                let protocol = Protocol::parse(&protocol).ok_or("unknown protocol")?;
                let machine = db
                    .insert_machine(NewMachine {
                        name,
                        host,
                        port,
                        username,
                        protocol,
                        os_hint: None,
                        notes: String::new(),
                        tags: tag,
                        group,
                    })
                    .map_err(|e| e.to_string())?;
                println!("{}", machine.id);
            }
            MachineAction::List => {
                for machine in db.list_machines().map_err(|e| e.to_string())? {
                    println!(
                        "{}\t{}\t{}@{}:{}\t{}\t{}",
                        machine.id,
                        machine.name,
                        machine.username,
                        machine.host,
                        machine.port,
                        machine.protocol,
                        machine.tags.join(",")
                    );
                }
            }
            MachineAction::Remove { id } => {
                let machine = load(&db, &id)?;
                db.delete_machine(machine.id).map_err(|e| e.to_string())?;
            }
        },
        Command::Probe { id } => {
            let machine = load(&db, &id)?;
            match tcp_probe(&machine.host, machine.port, Duration::from_secs(5)).await {
                termix_network::Probe::Reachable { elapsed_ms } => {
                    println!("reachable in {elapsed_ms} ms");
                }
                termix_network::Probe::Unreachable { message } => {
                    return Err(format!("unreachable: {message}"));
                }
            }
        }
        Command::Exec {
            id,
            trust,
            key,
            yes,
            command,
        } => {
            let body = command.join(" ");
            let prepared = prepare(&body, yes).map_err(|_| {
                "this command looks destructive; pass --yes if you intend to run it".to_string()
            })?;
            let machine = load(&db, &id)?;
            let session = connect(&db, &machine, trust, key).await?;
            let result = session
                .exec(&prepared.body)
                .await
                .map_err(|e| e.to_string())?;
            print!("{}", result.stdout);
            if !result.stderr.is_empty() {
                eprint!("{}", result.stderr);
            }
            db.log(Some(machine.id), "exec", &redact(&prepared.body))
                .map_err(|e| e.to_string())?;
            db.mark_connected(machine.id).map_err(|e| e.to_string())?;
            if result.exit_code.unwrap_or(1) != 0 {
                return Err(format!("exit {}", result.exit_code.unwrap_or(1)));
            }
            let _ = _vault;
        }
        Command::Sftp { action } => match action {
            SftpAction::Ls {
                id,
                path,
                trust,
                key,
            } => {
                let machine = load(&db, &id)?;
                let session = connect(&db, &machine, trust, key).await?;
                let sftp = termix_sftp::Sftp::open(&session)
                    .await
                    .map_err(|e| e.to_string())?;
                for entry in sftp.list(&path).await.map_err(|e| e.to_string())? {
                    println!("{}", entry.name);
                }
                db.mark_connected(machine.id).map_err(|e| e.to_string())?;
            }
        },
        Command::Local => {
            let sample = sample_local()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&sample).map_err(|e| e.to_string())?
            );
        }
    }
    Ok(())
}

fn open_vault(dir: &Path) -> Result<Vault, String> {
    let path = dir.join("master.key");
    if path.exists() {
        termix_security::load_key_file(&path).map_err(|e| e.to_string())
    } else {
        let vault = Vault::generate();
        termix_security::store_key_file(&path, &vault).map_err(|e| e.to_string())?;
        Ok(vault)
    }
}

fn load(db: &Database, id_or_name: &str) -> Result<termix_core::Machine, String> {
    db.resolve(id_or_name).map_err(|e| e.to_string())
}

async fn connect(
    db: &Database,
    machine: &termix_core::Machine,
    trust: bool,
    key: Option<PathBuf>,
) -> Result<SshSession, String> {
    if machine.protocol != Protocol::Ssh && machine.protocol != Protocol::Sftp {
        return Err(format!("{} is not an SSH machine", machine.protocol));
    }
    let auth = auth_from(key)?;
    let trusted = db
        .trusted_fingerprint(&machine.host, machine.port)
        .map_err(|e| e.to_string())?;
    let request = ConnectRequest {
        host: machine.host.clone(),
        port: machine.port,
        username: machine.username.clone(),
        auth: auth.clone(),
        trusted_fingerprint: trusted.clone(),
        timeout: Duration::from_secs(15),
        jump_host: None,
    };
    match SshSession::connect(request).await {
        Ok(session) => Ok(session),
        Err(SshError::HostKeyUnknown {
            algorithm,
            fingerprint,
            ..
        }) if trust && trusted.is_none() => {
            db.trust_host(&machine.host, machine.port, &algorithm, &fingerprint)
                .map_err(|e| e.to_string())?;
            println!("trusted {algorithm} {fingerprint}");
            let request = ConnectRequest {
                host: machine.host.clone(),
                port: machine.port,
                username: machine.username.clone(),
                auth,
                trusted_fingerprint: Some(fingerprint),
                timeout: Duration::from_secs(15),
                jump_host: None,
            };
            SshSession::connect(request)
                .await
                .map_err(|e| e.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn auth_from(key: Option<PathBuf>) -> Result<Auth, String> {
    if let Some(path) = key {
        return Ok(Auth::PrivateKey {
            path,
            passphrase: std::env::var("TERMIX_KEY_PASSPHRASE").ok(),
        });
    }
    match std::env::var("TERMIX_PASSWORD") {
        Ok(password) if !password.is_empty() => Ok(Auth::Password(password)),
        _ => Err(
            "set TERMIX_PASSWORD or pass --key. Passwords are not accepted as arguments.".into(),
        ),
    }
}
