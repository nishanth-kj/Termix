//! One interface for Windows, macOS, iOS, and Android.
//! Dioxus is the only UI toolkit. SSH, SFTP, and metrics live in the shared crates.
//! This screen reads the same SQLite database as the `termix` CLI.

use dioxus::prelude::*;
use termix_core::{Capability, Support};
use termix_monitor::sample_local;
use termix_storage::{data_dir, Database};

const CSS: &str = include_str!("style.css");

fn main() {
    dioxus::launch(App);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Machines,
    Monitor,
    Support,
}

#[component]
fn App() -> Element {
    let mut tab = use_signal(|| Tab::Machines);
    rsx! {
        style { "{CSS}" }
        div { class: "app",
            nav {
                h1 { "Termix" }
                p { "Every machine. One control center." }
                button { class: if *tab.read() == Tab::Machines { "nav active" } else { "nav" }, onclick: move |_| tab.set(Tab::Machines), "Machines" }
                button { class: if *tab.read() == Tab::Monitor { "nav active" } else { "nav" }, onclick: move |_| tab.set(Tab::Monitor), "This machine" }
                button { class: if *tab.read() == Tab::Support { "nav active" } else { "nav" }, onclick: move |_| tab.set(Tab::Support), "What works" }
            }
            main {
                match *tab.read() {
                    Tab::Machines => rsx! { MachinePage {} },
                    Tab::Monitor => rsx! { MonitorPage {} },
                    Tab::Support => rsx! { SupportPage {} },
                }
            }
        }
    }
}

#[component]
fn MachinePage() -> Element {
    let mut name = use_signal(String::new);
    let mut host = use_signal(String::new);
    let mut user = use_signal(String::new);
    let mut status = use_signal(|| String::new());
    let mut revision = use_signal(|| 0_u32);
    let machines = {
        let _ = revision();
        Database::open(&data_dir().join("termix.sqlite"))
            .and_then(|db| db.list_machines())
            .unwrap_or_default()
    };
    rsx! {
        h2 { "Machines" }
        p { class: "mute", "Saved on this device. Connecting from the window uses the same records as the termix command." }
        if machines.is_empty() {
            p { class: "mute", "No machines saved yet." }
        }
        for machine in machines {
            div { class: "card",
                strong { "{machine.name}" }
                p { class: "mute", "{machine.username}@{machine.host}:{machine.port} · {machine.protocol}" }
            }
        }
        div { class: "card",
            h2 { "Add a host" }
            label { "Name" input { value: "{name}", oninput: move |e| name.set(e.value()) } }
            label { "Host" input { value: "{host}", oninput: move |e| host.set(e.value()) } }
            label { "Username" input { value: "{user}", oninput: move |e| user.set(e.value()) } }
            button { class: "action",
                onclick: move |_| {
                    let result = (|| {
                        let db = Database::open(&data_dir().join("termix.sqlite"))?;
                        db.insert_machine(termix_core::NewMachine {
                            name: name.read().trim().to_string(),
                            host: host.read().trim().to_string(),
                            port: 22,
                            username: user.read().trim().to_string(),
                            protocol: termix_core::Protocol::Ssh,
                            os_hint: None,
                            notes: String::new(),
                            tags: Vec::new(),
                            group: None,
                        })?;
                        Ok::<(), termix_storage::StoreError>(())
                    })();
                    match result {
                        Ok(()) => {
                            name.set(String::new());
                            host.set(String::new());
                            user.set(String::new());
                            status.set("Saved.".into());
                            revision.set(revision() + 1);
                        }
                        Err(error) => status.set(error.to_string()),
                    }
                },
                "Save host"
            }
            p { class: "mute", "{status}" }
        }
    }
}

#[component]
fn MonitorPage() -> Element {
    let sample = sample_local().ok();
    let cpu = sample.as_ref().map(|sample| match sample.cpu_percent {
        Some(value) => format!("{value:.1}%"),
        None => "no delta yet".into(),
    });
    let memory = sample.as_ref().map(|sample| {
        format!(
            "{} / {}",
            format_bytes(sample.memory_used_bytes),
            format_bytes(sample.memory_total_bytes)
        )
    });
    let uptime = sample.as_ref().map(|sample| match sample.uptime_seconds {
        Some(seconds) => format!("{seconds}s"),
        None => "unknown".into(),
    });
    let memory = memory.unwrap_or_default();
    let uptime = uptime.unwrap_or_default();
    let processes = sample
        .as_ref()
        .map(|sample| sample.processes.clone())
        .unwrap_or_default();
    rsx! {
        h2 { "This machine" }
        p { class: "mute", "Numbers come from the operating system on this device. Remote metrics are collected over SSH by the termix command, not invented here." }
        if let Some(cpu) = cpu {
            div { class: "card",
                p { "CPU  " strong { "{cpu}" } }
                p { "Memory  " strong { "{memory}" } }
                p { "Uptime  " strong { "{uptime}" } }
            }
            h2 { "Processes" }
            for process in processes {
                div { class: "card", "{process.name}  pid {process.pid}  {process.memory_bytes} bytes" }
            }
        } else {
            p { class: "bad", "This device did not return a sample." }
        }
    }
}

#[component]
fn SupportPage() -> Element {
    let rows = [
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
    ];
    rsx! {
        h2 { "What works" }
        p { class: "mute", "Unfinished protocols stay on this list. They are not shown as connected." }
        for capability in rows {
            div { class: "card",
                span { "{capability.label()}" }
                if capability.support() == Support::Implemented {
                    span { class: "ok", "  implemented" }
                } else {
                    span { class: "bad", "  not implemented" }
                }
            }
        }
    }
}

fn format_bytes(value: Option<u64>) -> String {
    match value {
        Some(bytes) => format!("{:.1} GiB", bytes as f64 / 1024.0 / 1024.0 / 1024.0),
        None => "unknown".into(),
    }
}
