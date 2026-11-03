//! Domain types shared by every Termix frontend.
//!
//! This crate does not open sockets or touch the disk.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type MachineId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Ssh,
    Sftp,
    Local,
    Rdp,
    Vnc,
    Telnet,
    Serial,
}

impl Protocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ssh => "ssh",
            Self::Sftp => "sftp",
            Self::Local => "local",
            Self::Rdp => "rdp",
            Self::Vnc => "vnc",
            Self::Telnet => "telnet",
            Self::Serial => "serial",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "ssh" => Self::Ssh,
            "sftp" => Self::Sftp,
            "local" => Self::Local,
            "rdp" => Self::Rdp,
            "vnc" => Self::Vnc,
            "telnet" => Self::Telnet,
            "serial" => Self::Serial,
            _ => return None,
        })
    }

    /// Protocols with a working implementation in this repository.
    pub fn is_implemented(self) -> bool {
        matches!(self, Self::Ssh | Self::Sftp | Self::Local)
    }
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Machine {
    pub id: MachineId,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub protocol: Protocol,
    pub os_hint: Option<String>,
    pub favorite: bool,
    pub notes: String,
    pub tags: Vec<String>,
    pub group: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_connected_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewMachine {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub protocol: Protocol,
    pub os_hint: Option<String>,
    pub notes: String,
    pub tags: Vec<String>,
    pub group: Option<String>,
}

impl NewMachine {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.name.trim().is_empty() {
            return Err(CoreError::Invalid("machine name is empty"));
        }
        if self.host.trim().is_empty() {
            return Err(CoreError::Invalid("host is empty"));
        }
        if self.port == 0 {
            return Err(CoreError::Invalid("port must be between 1 and 65535"));
        }
        if self.protocol != Protocol::Local && self.username.trim().is_empty() {
            return Err(CoreError::Invalid("username is empty"));
        }
        if !self.protocol.is_implemented() {
            return Err(CoreError::UnsupportedProtocol(self.protocol));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Implemented,
    NotImplemented,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Ssh,
    Sftp,
    LocalTerminal,
    HostKeyVerification,
    EncryptedSecrets,
    PortForwardLocal,
    PortForwardRemote,
    Socks,
    JumpHost,
    FolderSyncPlan,
    FolderSyncApply,
    SystemMonitorSsh,
    SystemMonitorLocal,
    Docker,
    Kubernetes,
    Rdp,
    Vnc,
    Telnet,
    Serial,
    SshAgent,
}

impl Capability {
    pub fn support(self) -> Support {
        match self {
            Self::Ssh
            | Self::Sftp
            | Self::LocalTerminal
            | Self::HostKeyVerification
            | Self::EncryptedSecrets
            | Self::SystemMonitorSsh
            | Self::SystemMonitorLocal
            | Self::FolderSyncPlan => Support::Implemented,
            Self::PortForwardLocal
            | Self::PortForwardRemote
            | Self::Socks
            | Self::JumpHost
            | Self::FolderSyncApply
            | Self::Docker
            | Self::Kubernetes
            | Self::Rdp
            | Self::Vnc
            | Self::Telnet
            | Self::Serial
            | Self::SshAgent => Support::NotImplemented,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ssh => "SSH",
            Self::Sftp => "SFTP",
            Self::LocalTerminal => "Local terminal",
            Self::HostKeyVerification => "SSH host-key verification",
            Self::EncryptedSecrets => "Encrypted secret storage",
            Self::PortForwardLocal => "Local port forwarding",
            Self::PortForwardRemote => "Remote port forwarding",
            Self::Socks => "SOCKS dynamic forwarding",
            Self::JumpHost => "Jump hosts",
            Self::FolderSyncPlan => "Folder sync planning",
            Self::FolderSyncApply => "Folder sync apply",
            Self::SystemMonitorSsh => "SSH system metrics",
            Self::SystemMonitorLocal => "Local system metrics",
            Self::Docker => "Docker",
            Self::Kubernetes => "Kubernetes",
            Self::Rdp => "RDP",
            Self::Vnc => "VNC",
            Self::Telnet => "Telnet",
            Self::Serial => "Serial",
            Self::SshAgent => "SSH agent",
        }
    }
}

/// Commands that can destroy data or interrupt a machine.
/// This is a heuristic, not a sandbox. Callers must still confirm.
pub fn looks_destructive(command: &str) -> bool {
    let lowered = command.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "rm ",
        "rm\t",
        "rm -",
        "mkfs",
        "dd ",
        "shutdown",
        "reboot",
        "poweroff",
        "halt",
        "kubectl delete",
        "docker rm",
        "docker rmi",
        "drop table",
        "drop database",
        ":(){",
        "chmod -r 777",
        "> /dev/",
    ];
    NEEDLES.iter().any(|needle| lowered.contains(needle))
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CoreError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("protocol {0} is not implemented")]
    UnsupportedProtocol(Protocol),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destructive_commands_are_flagged() {
        assert!(looks_destructive("sudo rm -rf /var/lib"));
        assert!(looks_destructive("kubectl delete ns prod"));
        assert!(!looks_destructive("uname -a"));
        assert!(!looks_destructive("ls -la"));
    }

    #[test]
    fn unimplemented_protocols_are_rejected() {
        let machine = NewMachine {
            name: "win".into(),
            host: "10.0.0.5".into(),
            port: 3389,
            username: "admin".into(),
            protocol: Protocol::Rdp,
            os_hint: None,
            notes: String::new(),
            tags: vec![],
            group: None,
        };
        assert!(matches!(
            machine.validate(),
            Err(CoreError::UnsupportedProtocol(Protocol::Rdp))
        ));
    }
}
