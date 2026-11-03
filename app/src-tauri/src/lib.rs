//! Tauri shell. The same commands run on Windows, macOS, Linux, iOS, and Android.
//! The window is a webview. SSH and storage stay in the Rust crates.

mod session;
mod shell;

use std::path::PathBuf;

use session::{connect, resolve_auth, AppError, Credentials};
use shell::{ShellEvent, Shells};
use tauri::ipc::Channel;
use tauri::{Manager, State};
use termix_core::{Capability, Machine, NewMachine, Protocol, Support};
use termix_monitor::{sample_local, Sample};
use termix_session::{remote_metrics, Workspace};
use termix_sftp::{RemoteEntry, Sftp};
use termix_sync::{Direction, SyncPlan};

struct AppState {
    workspace: Workspace,
    shells: Shells,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    termix_logging::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let workspace = Workspace::open(&storage_dir(app.handle())?)?;
            app.manage(AppState {
                workspace,
                shells: Shells::default(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            platform,
            support,
            list_machines,
            add_machine,
            delete_machine,
            forget_host,
            forget_password,
            local_metrics,
            machine_metrics,
            probe_machine,
            run_command,
            sftp_list,
            sftp_mkdir,
            sftp_rename,
            sftp_remove,
            sftp_download,
            sftp_upload,
            sync_plan,
            sync_apply,
            open_ssh_shell,
            open_local_shell,
            shell_write,
            shell_resize,
            shell_close
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Termix");
}

/// Desktop shares its data with the CLI. iOS and Android apps may only write
/// inside their own sandbox, which Tauri knows how to find.
fn storage_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        app.path().app_local_data_dir()
    } else {
        Ok(termix_storage::data_dir())
    }
}

fn load(state: &AppState, id: &str) -> Result<Machine, AppError> {
    state.workspace.db.resolve(id).map_err(AppError::other)
}

#[tauri::command]
fn platform() -> serde_json::Value {
    let mobile = cfg!(any(target_os = "android", target_os = "ios"));
    serde_json::json!({ "os": std::env::consts::OS, "mobile": mobile })
}

#[derive(serde::Serialize)]
struct SupportRow {
    name: String,
    implemented: bool,
}

#[tauri::command]
fn support() -> Vec<SupportRow> {
    Capability::ALL
        .into_iter()
        .map(|capability| SupportRow {
            name: capability.label().to_string(),
            implemented: capability.support() == Support::Implemented,
        })
        .collect()
}

#[derive(serde::Serialize)]
struct MachineRow {
    #[serde(flatten)]
    machine: Machine,
    has_saved_password: bool,
    pinned_fingerprint: Option<String>,
}

#[tauri::command]
fn list_machines(state: State<'_, AppState>) -> Result<Vec<MachineRow>, AppError> {
    let workspace = &state.workspace;
    workspace
        .db
        .list_machines()
        .map_err(AppError::other)?
        .into_iter()
        .map(|machine| {
            Ok(MachineRow {
                has_saved_password: workspace.has_saved_password(&machine)?,
                pinned_fingerprint: workspace
                    .db
                    .trusted_fingerprint(&machine.host, machine.port)
                    .map_err(AppError::other)?,
                machine,
            })
        })
        .collect()
}

#[tauri::command]
fn add_machine(
    state: State<'_, AppState>,
    name: String,
    host: String,
    username: String,
    port: Option<u16>,
) -> Result<Machine, AppError> {
    state
        .workspace
        .db
        .insert_machine(NewMachine {
            name,
            host,
            port: port.unwrap_or(22),
            username,
            protocol: Protocol::Ssh,
            os_hint: None,
            notes: String::new(),
            tags: Vec::new(),
            group: None,
        })
        .map_err(AppError::other)
}

#[tauri::command]
fn delete_machine(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    let machine = load(&state, &id)?;
    state
        .workspace
        .db
        .delete_machine(machine.id)
        .map_err(AppError::other)
}

#[tauri::command]
fn forget_host(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    let machine = load(&state, &id)?;
    state
        .workspace
        .db
        .forget_host(&machine.host, machine.port)
        .map_err(AppError::other)?;
    Ok(())
}

#[tauri::command]
fn forget_password(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    let machine = load(&state, &id)?;
    Ok(state.workspace.forget_password(&machine)?)
}

#[tauri::command]
async fn local_metrics() -> Result<Sample, AppError> {
    // Sampling sleeps between two CPU readings, so it stays off the async workers.
    tauri::async_runtime::spawn_blocking(sample_local)
        .await
        .map_err(AppError::other)?
        .map_err(AppError::other)
}

#[tauri::command]
async fn machine_metrics(
    state: State<'_, AppState>,
    id: String,
    credentials: Credentials,
    trust: Option<String>,
) -> Result<Sample, AppError> {
    let machine = load(&state, &id)?;
    let session = connect(&state.workspace, &machine, &credentials, trust.as_deref()).await?;
    let sample = remote_metrics(&session, &machine.host).await?;
    let _ = session.disconnect().await;
    Ok(sample)
}

#[tauri::command]
async fn probe_machine(state: State<'_, AppState>, id: String) -> Result<String, AppError> {
    let machine = load(&state, &id)?;
    match termix_network::tcp_probe(
        &machine.host,
        machine.port,
        std::time::Duration::from_secs(5),
    )
    .await
    {
        termix_network::Probe::Reachable { elapsed_ms } => {
            Ok(format!("reachable in {elapsed_ms} ms"))
        }
        termix_network::Probe::Unreachable { message } => {
            Err(AppError::other(format!("unreachable: {message}")))
        }
    }
}

#[derive(serde::Serialize)]
struct CommandOutput {
    stdout: String,
    stderr: String,
    exit_code: Option<u32>,
}

#[tauri::command]
async fn run_command(
    state: State<'_, AppState>,
    id: String,
    command: String,
    credentials: Credentials,
    trust: Option<String>,
    confirmed: bool,
) -> Result<CommandOutput, AppError> {
    let workspace = &state.workspace;
    let machine = load(&state, &id)?;
    let auth = resolve_auth(workspace, &machine, &credentials)?;
    let result = workspace
        .run_command(&machine, auth, trust.as_deref(), &command, confirmed)
        .await?;
    session::remember_if_asked(workspace, &machine, &credentials)?;
    Ok(CommandOutput {
        stdout: result.stdout,
        stderr: result.stderr,
        exit_code: result.exit_code,
    })
}

/// Each file action opens its own connection, so the window holds no session state.
async fn open_sftp(
    state: &AppState,
    id: &str,
    credentials: &Credentials,
    trust: Option<&str>,
) -> Result<(Machine, termix_ssh::SshSession, Sftp), AppError> {
    let machine = load(state, id)?;
    let session = connect(&state.workspace, &machine, credentials, trust).await?;
    let sftp = Sftp::open(&session).await.map_err(AppError::other)?;
    Ok((machine, session, sftp))
}

#[derive(serde::Serialize)]
struct Listing {
    path: String,
    entries: Vec<RemoteEntry>,
}

#[tauri::command]
async fn sftp_list(
    state: State<'_, AppState>,
    id: String,
    path: String,
    credentials: Credentials,
    trust: Option<String>,
) -> Result<Listing, AppError> {
    let (_, _session, sftp) = open_sftp(&state, &id, &credentials, trust.as_deref()).await?;
    let path = sftp.canonicalize(&path).await.map_err(AppError::other)?;
    let entries = sftp.list(&path).await.map_err(AppError::other)?;
    Ok(Listing { path, entries })
}

#[tauri::command]
async fn sftp_mkdir(
    state: State<'_, AppState>,
    id: String,
    path: String,
    credentials: Credentials,
    trust: Option<String>,
) -> Result<(), AppError> {
    let (_, _session, sftp) = open_sftp(&state, &id, &credentials, trust.as_deref()).await?;
    sftp.create_dir(&path).await.map_err(AppError::other)
}

#[tauri::command]
async fn sftp_rename(
    state: State<'_, AppState>,
    id: String,
    from: String,
    to: String,
    credentials: Credentials,
    trust: Option<String>,
) -> Result<(), AppError> {
    let (_, _session, sftp) = open_sftp(&state, &id, &credentials, trust.as_deref()).await?;
    sftp.rename(&from, &to).await.map_err(AppError::other)
}

#[tauri::command]
async fn sftp_remove(
    state: State<'_, AppState>,
    id: String,
    path: String,
    is_dir: bool,
    credentials: Credentials,
    trust: Option<String>,
) -> Result<(), AppError> {
    let (_, _session, sftp) = open_sftp(&state, &id, &credentials, trust.as_deref()).await?;
    let removed = if is_dir {
        sftp.remove_dir(&path).await
    } else {
        sftp.remove_file(&path).await
    };
    removed.map_err(AppError::other)
}

#[tauri::command]
async fn sftp_download(
    state: State<'_, AppState>,
    id: String,
    remote: String,
    local: String,
    credentials: Credentials,
    trust: Option<String>,
) -> Result<u64, AppError> {
    let (_, _session, sftp) = open_sftp(&state, &id, &credentials, trust.as_deref()).await?;
    let mut output = tokio::fs::File::create(&local)
        .await
        .map_err(AppError::other)?;
    sftp.download(&remote, &mut output)
        .await
        .map_err(AppError::other)
}

#[tauri::command]
async fn sftp_upload(
    state: State<'_, AppState>,
    id: String,
    local: String,
    remote: String,
    credentials: Credentials,
    trust: Option<String>,
) -> Result<u64, AppError> {
    let (_, _session, sftp) = open_sftp(&state, &id, &credentials, trust.as_deref()).await?;
    let mut input = tokio::fs::File::open(&local)
        .await
        .map_err(AppError::other)?;
    sftp.upload(&mut input, &remote)
        .await
        .map_err(AppError::other)
}

fn direction(pull: bool) -> Direction {
    if pull {
        Direction::RemoteToLocal
    } else {
        Direction::LocalToRemote
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn sync_plan(
    state: State<'_, AppState>,
    id: String,
    local: String,
    remote: String,
    pull: bool,
    excludes: Vec<String>,
    credentials: Credentials,
    trust: Option<String>,
) -> Result<SyncPlan, AppError> {
    let (_, _session, sftp) = open_sftp(&state, &id, &credentials, trust.as_deref()).await?;
    Ok(state
        .workspace
        .plan_sync(&sftp, direction(pull), local.as_ref(), &remote, &excludes)
        .await?)
}

/// The plan is rebuilt here rather than taken from the window, so what is
/// copied always reflects the folders as they are now.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn sync_apply(
    state: State<'_, AppState>,
    id: String,
    local: String,
    remote: String,
    pull: bool,
    excludes: Vec<String>,
    credentials: Credentials,
    trust: Option<String>,
) -> Result<usize, AppError> {
    let (machine, _session, sftp) = open_sftp(&state, &id, &credentials, trust.as_deref()).await?;
    let workspace = &state.workspace;
    let plan = workspace
        .plan_sync(&sftp, direction(pull), local.as_ref(), &remote, &excludes)
        .await?;
    Ok(workspace
        .apply_sync(&machine, &sftp, &plan, local.as_ref(), &remote)
        .await?)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn open_ssh_shell(
    state: State<'_, AppState>,
    id: String,
    columns: u32,
    rows: u32,
    credentials: Credentials,
    trust: Option<String>,
    output: Channel<ShellEvent>,
) -> Result<u32, AppError> {
    let machine = load(&state, &id)?;
    let session = connect(&state.workspace, &machine, &credentials, trust.as_deref()).await?;
    state.shells.start_ssh(session, columns, rows, output).await
}

#[tauri::command]
fn open_local_shell(
    state: State<'_, AppState>,
    columns: u16,
    rows: u16,
    output: Channel<ShellEvent>,
) -> Result<u32, AppError> {
    state.shells.start_local(columns, rows, output)
}

#[tauri::command]
fn shell_write(state: State<'_, AppState>, shell: u32, data: String) -> Result<(), AppError> {
    state.shells.write(shell, data)
}

#[tauri::command]
fn shell_resize(
    state: State<'_, AppState>,
    shell: u32,
    columns: u32,
    rows: u32,
) -> Result<(), AppError> {
    state.shells.resize(shell, columns, rows)
}

#[tauri::command]
fn shell_close(state: State<'_, AppState>, shell: u32) {
    state.shells.close(shell);
}
