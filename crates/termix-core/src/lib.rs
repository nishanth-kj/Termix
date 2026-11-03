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
    /// Every capability, in the order the support list shows them.
    pub const ALL: [Capability; 20] = [
        Self::Ssh,
        Self::Sftp,
        Self::LocalTerminal,
        Self::HostKeyVerification,
        Self::EncryptedSecrets,
        Self::SystemMonitorLocal,
        Self::SystemMonitorSsh,
        Self::FolderSyncPlan,
        Self::FolderSyncApply,
        Self::PortForwardLocal,
        Self::PortForwardRemote,
        Self::Socks,
        Self::JumpHost,
        Self::SshAgent,
        Self::Docker,
        Self::Kubernetes,
        Self::Rdp,
        Self::Vnc,
        Self::Telnet,
        Self::Serial,
    ];

    pub fn support(self) -> Support {
        match self {
            Self::Ssh
            | Self::Sftp
            | Self::LocalTerminal
            | Self::HostKeyVerification
            | Self::EncryptedSecrets
            | Self::SystemMonitorSsh
            | Self::SystemMonitorLocal
            | Self::FolderSyncPlan
            | Self::FolderSyncApply => Support::Implemented,
            Self::PortForwardLocal
            | Self::PortForwardRemote
            | Self::Socks
            | Self::JumpHost
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
            Self::FolderSyncApply => "Folder sync apply (copies only)",
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
///
/// The command is split the way a shell would split it, so `git add .` is not
/// mistaken for `dd` and quoted text such as `echo "rm me"` is not a command.
pub fn looks_destructive(command: &str) -> bool {
    let lowered = command.to_ascii_lowercase();
    const ANYWHERE: &[&str] = &[
        "drop table",
        "drop database",
        "truncate table",
        ":(){",
        ":() {",
    ];
    if ANYWHERE.iter().any(|needle| lowered.contains(needle)) || writes_to_device(&lowered) {
        return true;
    }
    split_commands(&lowered)
        .iter()
        .any(|words| segment_is_destructive(words))
}

fn writes_to_device(command: &str) -> bool {
    const HARMLESS: &[&str] = &["null", "zero", "tty", "stdout", "stderr", "fd/"];
    command.match_indices('>').any(|(index, _)| {
        command[index + 1..]
            .trim_start()
            .strip_prefix("/dev/")
            .is_some_and(|device| !HARMLESS.iter().any(|name| device.starts_with(name)))
    })
}

/// Splits on `;`, `&`, `|`, newlines, parentheses, and backticks outside quotes.
fn split_commands(command: &str) -> Vec<Vec<String>> {
    let mut segments = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut chars = command.chars();
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some(open), _) if ch == open => quote = None,
            (Some(_), _) => word.push(ch),
            (None, '\'' | '"') => quote = Some(ch),
            (None, '\\') => word.extend(chars.next()),
            (None, ' ' | '\t') => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            (None, ';' | '&' | '|' | '\n' | '(' | ')' | '`') => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
                if !words.is_empty() {
                    segments.push(std::mem::take(&mut words));
                }
            }
            (None, _) => word.push(ch),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    if !words.is_empty() {
        segments.push(words);
    }
    segments
}

fn command_name(word: &str) -> &str {
    let name = word.rsplit(['/', '\\']).next().unwrap_or(word);
    name.strip_suffix(".exe").unwrap_or(name)
}

fn is_destructive_name(name: &str) -> bool {
    const NAMES: &[&str] = &[
        "rm",
        "rmdir",
        "shred",
        "wipefs",
        "dd",
        "shutdown",
        "reboot",
        "poweroff",
        "halt",
        "telinit",
        "fdisk",
        "parted",
        "sgdisk",
        "mkswap",
        "blkdiscard",
        "truncate",
        "userdel",
        "dropdb",
        "del",
        "erase",
        "rd",
        "format",
        "remove-item",
    ];
    NAMES.contains(&name) || name.starts_with("mkfs")
}

fn segment_is_destructive(words: &[String]) -> bool {
    // Double quotes keep a substitution inside one word.
    if words
        .iter()
        .any(|word| word.contains("$(") && looks_destructive(&word.replace("$(", ";")))
    {
        return true;
    }
    let mut words = words;
    while words.first().is_some_and(|word| is_assignment(word)) {
        words = &words[1..];
    }
    let Some((head, args)) = words.split_first() else {
        return false;
    };
    let has = |wanted: &[&str]| args.iter().any(|arg| wanted.contains(&arg.as_str()));
    let first = args.first().map(String::as_str);
    match command_name(head) {
        // Wrappers run the command in their arguments. Their own flags are not
        // parsed, so any argument that names a destructive tool counts.
        "sudo" | "doas" | "env" | "nohup" | "nice" | "ionice" | "time" | "exec" | "command"
        | "xargs" | "busybox" | "watch" | "timeout" | "ssh" => (0..args.len()).any(|start| {
            is_destructive_name(command_name(&args[start]))
                || (is_multi_word_tool(command_name(&args[start]))
                    && segment_is_destructive(&args[start..]))
        }),
        "sh" | "bash" | "zsh" | "dash" | "ksh" | "fish" | "pwsh" | "powershell" | "cmd" => args
            .iter()
            .skip_while(|arg| !matches!(arg.as_str(), "-c" | "/c" | "-command"))
            .skip(1)
            .any(|arg| looks_destructive(arg)),
        "find" => {
            has(&["-delete"])
                || args.windows(2).any(|pair| {
                    matches!(pair[0].as_str(), "-exec" | "-execdir" | "-ok")
                        && is_destructive_name(command_name(&pair[1]))
                })
        }
        "git" => {
            first == Some("clean")
                || (first == Some("reset") && has(&["--hard"]))
                || (first == Some("push") && has(&["--force", "-f"]))
        }
        "kubectl" | "oc" => matches!(first, Some("delete" | "drain")),
        "helm" => matches!(first, Some("uninstall" | "delete")),
        "terraform" | "tofu" => first == Some("destroy"),
        "docker" | "podman" => has(&["rm", "rmi", "prune", "kill"]),
        "systemctl" => matches!(
            first,
            Some("stop" | "disable" | "mask" | "poweroff" | "reboot" | "halt")
        ),
        "chmod" => has(&["-r", "--recursive"]) && has(&["777", "000"]),
        "chown" => has(&["-r", "--recursive"]),
        "init" => matches!(first, Some("0" | "6")),
        name => is_destructive_name(name),
    }
}

fn is_multi_word_tool(name: &str) -> bool {
    matches!(
        name,
        "git"
            | "kubectl"
            | "oc"
            | "helm"
            | "terraform"
            | "tofu"
            | "docker"
            | "podman"
            | "systemctl"
            | "chmod"
            | "chown"
            | "find"
            | "init"
            | "sh"
            | "bash"
            | "zsh"
            | "dash"
    )
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    })
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
    fn substrings_of_other_words_are_not_destructive() {
        for command in [
            "git add .",
            "grep halted /var/log/syslog",
            "echo 'rm -rf /'",
            "ls > /dev/null",
            "uptime 2>/dev/null",
            "docker ps",
            "git reset --soft HEAD~1",
            "systemctl status nginx",
            "sudo git status",
        ] {
            assert!(!looks_destructive(command), "{command}");
        }
    }

    #[test]
    fn hidden_destructive_commands_are_flagged() {
        for command in [
            "cd /tmp && rm x",
            "ls; rm x",
            "/bin/rm x",
            "FOO=1 rm x",
            "sudo -u root rm x",
            "find / -delete",
            "find . -name '*.log' -exec rm {} ;",
            "ls | xargs -0 rm",
            "bash -c 'rm x'",
            "echo \"$(rm x)\"",
            "git clean -fdx",
            "git reset --hard",
            "sudo git push --force",
            "sudo systemctl stop nginx",
            "docker system prune",
            "chmod -R 777 /srv",
            "dd if=/dev/zero of=/dev/sda",
            "cat x > /dev/sda",
            "mkfs.ext4 /dev/sdb1",
            "psql -c 'DROP TABLE users'",
        ] {
            assert!(looks_destructive(command), "{command}");
        }
    }

    #[test]
    fn capability_list_is_complete_and_unique() {
        for (index, capability) in Capability::ALL.iter().enumerate() {
            assert!(!Capability::ALL[..index].contains(capability));
        }
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
