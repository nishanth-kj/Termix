//! Tauri shell. The same commands run on Windows, macOS, Linux, iOS, and Android.
//! The window is a webview. SSH and storage stay in the Rust crates.

mod session;

use termix_core::{Capability, NewMachine, Protocol, Support};
use termix_monitor::sample_local;
use termix_storage::{data_dir, Database};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    termix_logging::init();
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            support,
            list_machines,
            add_machine,
            local_metrics,
            probe_machine,
            run_command
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Termix");
}

fn database() -> Result<Database, String> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Database::open(&dir.join("termix.sqlite")).map_err(|error| error.to_string())
}

#[tauri::command]
fn support() -> Vec<SupportRow> {
    [
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
    ]
    .into_iter()
    .map(|capability| SupportRow {
        name: capability.label().to_string(),
        implemented: capability.support() == Support::Implemented,
    })
    .collect()
}

#[derive(serde::Serialize)]
struct SupportRow {
    name: String,
    implemented: bool,
}

#[tauri::command]
fn list_machines() -> Result<Vec<termix_core::Machine>, String> {
    database()?
        .list_machines()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn add_machine(
    name: String,
    host: String,
    username: String,
) -> Result<termix_core::Machine, String> {
    database()?
        .insert_machine(NewMachine {
            name,
            host,
            port: 22,
            username,
            protocol: Protocol::Ssh,
            os_hint: None,
            notes: String::new(),
            tags: Vec::new(),
            group: None,
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn local_metrics() -> Result<termix_monitor::Sample, String> {
    sample_local()
}

#[tauri::command]
async fn probe_machine(id: String) -> Result<String, String> {
    let db = database()?;
    let machine = session::load(&db, &id)?;
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
        termix_network::Probe::Unreachable { message } => Err(format!("unreachable: {message}")),
    }
}

#[tauri::command]
async fn run_command(
    id: String,
    command: String,
    password: String,
    trust: bool,
    yes: bool,
) -> Result<session::CommandOutput, String> {
    let db = database()?;
    session::run(&db, &id, &command, &password, trust, yes).await
}
