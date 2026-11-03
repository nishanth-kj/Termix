use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use termix_core::{Capability, Machine, NewMachine, Protocol, Support};
use termix_monitor::sample_local;
use termix_network::tcp_probe;
use termix_session::{remote_metrics, SessionError, Workspace};
use termix_sftp::Sftp;
use termix_ssh::{Auth, SshError, SshSession};
use termix_storage::data_dir;
use termix_sync::Direction;

#[derive(Parser)]
#[command(name = "termix", version, about = "Every machine. One control center.")]
struct Cli {
    /// Directory for the SQLite database and master key. Defaults to the OS data directory.
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct ConnectArgs {
    /// Pin this exact host key fingerprint (SHA256:...) after checking it out of band.
    /// Refused if the server presents another key or a different key is already pinned.
    #[arg(long, value_name = "FINGERPRINT")]
    trust: Option<String>,
    /// Private key path. Otherwise TERMIX_PASSWORD, then the saved password, is used.
    #[arg(long)]
    key: Option<PathBuf>,
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
    /// Pinned host keys.
    Host {
        #[command(subcommand)]
        action: HostAction,
    },
    /// Passwords saved encrypted in the local database.
    Password {
        #[command(subcommand)]
        action: PasswordAction,
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
        #[command(flatten)]
        connect: ConnectArgs,
        /// Required when the command looks destructive.
        #[arg(long)]
        yes: bool,
        #[arg(required = true, num_args = 1..)]
        command: Vec<String>,
    },
    /// SFTP file operations.
    Sftp {
        #[command(subcommand)]
        action: SftpAction,
    },
    /// Folder sync over SFTP. Files are copied and never deleted.
    Sync {
        #[command(subcommand)]
        action: SyncAction,
    },
    /// Metrics for a Linux machine, read from /proc over SSH.
    Metrics {
        /// Saved machine id or unique name.
        id: String,
        #[command(flatten)]
        connect: ConnectArgs,
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
enum HostAction {
    /// Remove a machine's pinned key, for when its key really changed.
    Forget {
        /// Saved machine id or unique name.
        id: String,
    },
}

#[derive(Subcommand)]
enum PasswordAction {
    /// Encrypt TERMIX_PASSWORD and save it for this machine.
    Save {
        /// Saved machine id or unique name.
        id: String,
    },
    Forget {
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
        #[command(flatten)]
        connect: ConnectArgs,
    },
    /// Download a remote file.
    Get {
        id: String,
        remote: String,
        local: PathBuf,
        #[command(flatten)]
        connect: ConnectArgs,
    },
    /// Upload a local file.
    Put {
        id: String,
        local: PathBuf,
        remote: String,
        #[command(flatten)]
        connect: ConnectArgs,
    },
    Mkdir {
        id: String,
        path: String,
        #[command(flatten)]
        connect: ConnectArgs,
    },
    Mv {
        id: String,
        from: String,
        to: String,
        #[command(flatten)]
        connect: ConnectArgs,
    },
    /// Delete a remote file. Requires --yes.
    Rm {
        id: String,
        path: String,
        #[arg(long)]
        yes: bool,
        #[command(flatten)]
        connect: ConnectArgs,
    },
}

#[derive(Args)]
struct SyncArgs {
    /// Saved machine id or unique name.
    id: String,
    /// Local folder.
    #[arg(long)]
    local: PathBuf,
    /// Remote folder.
    #[arg(long)]
    remote: String,
    /// Copy from the remote folder to the local one instead of local to remote.
    #[arg(long)]
    pull: bool,
    /// Folder or file names to skip.
    #[arg(long)]
    exclude: Vec<String>,
    #[command(flatten)]
    connect: ConnectArgs,
}

#[derive(Subcommand)]
enum SyncAction {
    /// Show what would be copied, what conflicts, and what a mirror would delete.
    Plan(SyncArgs),
    /// Copy the planned files. Conflicts are skipped and nothing is deleted.
    Apply(SyncArgs),
}

#[tokio::main]
async fn main() {
    termix_logging::init();
    let cli = Cli::parse();
    let dir = cli.data_dir.unwrap_or_else(data_dir);
    if let Err(error) = run(&dir, cli.command).await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

async fn run(dir: &Path, command: Command) -> Result<(), String> {
    let workspace = Workspace::open(dir).map_err(|e| e.to_string())?;
    match command {
        Command::Support => {
            for capability in Capability::ALL {
                let state = match capability.support() {
                    Support::Implemented => "implemented",
                    Support::NotImplemented => "not implemented",
                };
                println!("{:<34} {state}", capability.label());
            }
        }
        Command::Machine { action } => machine_command(&workspace, action)?,
        Command::Host {
            action: HostAction::Forget { id },
        } => {
            let machine = load(&workspace, &id)?;
            let removed = workspace
                .db
                .forget_host(&machine.host, machine.port)
                .map_err(|e| e.to_string())?;
            println!(
                "{}",
                if removed {
                    "forgotten"
                } else {
                    "nothing pinned"
                }
            );
        }
        Command::Password { action } => match action {
            PasswordAction::Save { id } => {
                let machine = load(&workspace, &id)?;
                let password = std::env::var("TERMIX_PASSWORD")
                    .ok()
                    .filter(|password| !password.is_empty())
                    .ok_or("set TERMIX_PASSWORD. Passwords are not accepted as arguments.")?;
                workspace
                    .remember_password(&machine, &password)
                    .map_err(|e| e.to_string())?;
            }
            PasswordAction::Forget { id } => {
                let machine = load(&workspace, &id)?;
                workspace
                    .forget_password(&machine)
                    .map_err(|e| e.to_string())?;
            }
        },
        Command::Probe { id } => {
            let machine = load(&workspace, &id)?;
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
            connect: args,
            yes,
            command,
        } => {
            let body = command.join(" ");
            let machine = load(&workspace, &id)?;
            let auth = auth_from(&workspace, &machine, args.key)?;
            let result = workspace
                .run_command(&machine, auth, args.trust.as_deref(), &body, yes)
                .await
                .map_err(|error| match error {
                    SessionError::NeedsConfirmation => {
                        "this command looks destructive; pass --yes if you intend to run it".into()
                    }
                    other => explain(other),
                })?;
            print!("{}", result.stdout);
            if !result.stderr.is_empty() {
                eprint!("{}", result.stderr);
            }
            if result.exit_code != Some(0) {
                return Err(match result.exit_code {
                    Some(code) => format!("exit {code}"),
                    None => "the connection closed before the command reported an exit code".into(),
                });
            }
        }
        Command::Sftp { action } => sftp_command(&workspace, action).await?,
        Command::Sync { action } => sync_command(&workspace, action).await?,
        Command::Metrics { id, connect: args } => {
            let machine = load(&workspace, &id)?;
            let session = connect(&workspace, &machine, args).await?;
            let sample = remote_metrics(&session, &machine.host)
                .await
                .map_err(|e| e.to_string())?;
            print_json(&sample)?;
        }
        Command::Local => print_json(&sample_local()?)?,
    }
    Ok(())
}

fn machine_command(workspace: &Workspace, action: MachineAction) -> Result<(), String> {
    match action {
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
            let machine = workspace
                .db
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
            for machine in workspace.db.list_machines().map_err(|e| e.to_string())? {
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
            let machine = load(workspace, &id)?;
            workspace
                .db
                .delete_machine(machine.id)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

async fn sftp_command(workspace: &Workspace, action: SftpAction) -> Result<(), String> {
    let (id, args) = match &action {
        SftpAction::Ls { id, connect, .. }
        | SftpAction::Get { id, connect, .. }
        | SftpAction::Put { id, connect, .. }
        | SftpAction::Mkdir { id, connect, .. }
        | SftpAction::Mv { id, connect, .. }
        | SftpAction::Rm { id, connect, .. } => (id, connect),
    };
    if let SftpAction::Rm { yes: false, .. } = &action {
        return Err("deleting a remote file needs --yes".into());
    }
    let machine = load(workspace, id)?;
    let session = connect(
        workspace,
        &machine,
        ConnectArgs {
            trust: args.trust.clone(),
            key: args.key.clone(),
        },
    )
    .await?;
    let sftp = Sftp::open(&session).await.map_err(|e| e.to_string())?;
    match action {
        SftpAction::Ls { path, .. } => {
            for entry in sftp.list(&path).await.map_err(|e| e.to_string())? {
                let suffix = if entry.is_dir { "/" } else { "" };
                println!("{:>12}  {}{suffix}", entry.size, entry.name);
            }
        }
        SftpAction::Get { remote, local, .. } => {
            let mut output = tokio::fs::File::create(&local)
                .await
                .map_err(|e| e.to_string())?;
            let bytes = sftp
                .download(&remote, &mut output)
                .await
                .map_err(|e| e.to_string())?;
            println!("downloaded {bytes} bytes");
        }
        SftpAction::Put { local, remote, .. } => {
            let mut input = tokio::fs::File::open(&local)
                .await
                .map_err(|e| e.to_string())?;
            let bytes = sftp
                .upload(&mut input, &remote)
                .await
                .map_err(|e| e.to_string())?;
            println!("uploaded {bytes} bytes");
        }
        SftpAction::Mkdir { path, .. } => {
            sftp.create_dir(&path).await.map_err(|e| e.to_string())?;
        }
        SftpAction::Mv { from, to, .. } => {
            sftp.rename(&from, &to).await.map_err(|e| e.to_string())?;
        }
        SftpAction::Rm { path, .. } => {
            sftp.remove_file(&path).await.map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

async fn sync_command(workspace: &Workspace, action: SyncAction) -> Result<(), String> {
    let (args, apply) = match action {
        SyncAction::Plan(args) => (args, false),
        SyncAction::Apply(args) => (args, true),
    };
    let machine = load(workspace, &args.id)?;
    let session = connect(workspace, &machine, args.connect).await?;
    let sftp = Sftp::open(&session).await.map_err(|e| e.to_string())?;
    let direction = if args.pull {
        Direction::RemoteToLocal
    } else {
        Direction::LocalToRemote
    };
    let plan = workspace
        .plan_sync(&sftp, direction, &args.local, &args.remote, &args.exclude)
        .await
        .map_err(|e| e.to_string())?;
    for copy in &plan.copies {
        println!("copy      {} ({})", copy.relative_path, copy.reason);
    }
    for path in &plan.conflicts {
        println!("conflict  {path} (destination is newer; skipped)");
    }
    for path in &plan.would_delete {
        println!("extra     {path} (only at the destination; never deleted)");
    }
    if apply {
        let copied = workspace
            .apply_sync(&machine, &sftp, &plan, &args.local, &args.remote)
            .await
            .map_err(|e| e.to_string())?;
        println!("copied {copied} file(s)");
    }
    Ok(())
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn load(workspace: &Workspace, id_or_name: &str) -> Result<Machine, String> {
    workspace.db.resolve(id_or_name).map_err(|e| e.to_string())
}

async fn connect(
    workspace: &Workspace,
    machine: &Machine,
    args: ConnectArgs,
) -> Result<SshSession, String> {
    let auth = auth_from(workspace, machine, args.key)?;
    workspace
        .connect(machine, auth, args.trust.as_deref())
        .await
        .map_err(explain)
}

fn explain(error: SessionError) -> String {
    match error {
        SessionError::Ssh(SshError::HostKeyUnknown {
            host,
            port,
            algorithm,
            fingerprint,
        }) => format!(
            "{host}:{port} is not trusted yet. It presented {algorithm} {fingerprint}\n\
             Check that fingerprint on the server (ssh-keygen -lf /etc/ssh/ssh_host_*.pub), \
             then re-run with --trust {fingerprint}"
        ),
        other => other.to_string(),
    }
}

fn auth_from(
    workspace: &Workspace,
    machine: &Machine,
    key: Option<PathBuf>,
) -> Result<Auth, String> {
    if let Some(path) = key {
        return Ok(Auth::PrivateKey {
            path,
            passphrase: std::env::var("TERMIX_KEY_PASSPHRASE").ok(),
        });
    }
    match std::env::var("TERMIX_PASSWORD") {
        Ok(password) if !password.is_empty() => Ok(Auth::Password(password)),
        _ => workspace.saved_auth(machine).map_err(|_| {
            "set TERMIX_PASSWORD, pass --key, or run `termix password save`. \
             Passwords are not accepted as arguments."
                .to_string()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn command_line_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn trust_requires_the_exact_fingerprint() {
        assert!(Cli::try_parse_from(["termix", "exec", "box", "--trust", "--", "ls"]).is_err());
        assert!(
            Cli::try_parse_from(["termix", "exec", "box", "--trust", "SHA256:abc", "ls"]).is_ok()
        );
    }
}
