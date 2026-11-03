//! Metrics come from the local OS or from command output the caller collected.
//! Nothing in this crate synthesizes a CPU percentage.

use serde::{Deserialize, Serialize};
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, ProcessRefreshKind, RefreshKind, System};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub source: String,
    pub cpu_percent: Option<f64>,
    pub memory_total_bytes: Option<u64>,
    pub memory_used_bytes: Option<u64>,
    pub uptime_seconds: Option<u64>,
    pub disks: Vec<Disk>,
    pub processes: Vec<ProcessInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Disk {
    pub filesystem: String,
    pub mount: String,
    pub total_kib: u64,
    pub used_kib: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcStat {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
}

impl ProcStat {
    pub fn total(self) -> u64 {
        self.user
            + self.nice
            + self.system
            + self.idle
            + self.iowait
            + self.irq
            + self.softirq
            + self.steal
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("could not parse {0}")]
    Invalid(&'static str),
}

pub fn parse_proc_stat(text: &str) -> Result<ProcStat, ParseError> {
    let line = text
        .lines()
        .find(|line| line.starts_with("cpu "))
        .ok_or(ParseError::Invalid("/proc/stat"))?;
    let nums: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|part| part.parse().ok())
        .collect();
    if nums.len() < 8 {
        return Err(ParseError::Invalid("/proc/stat"));
    }
    Ok(ProcStat {
        user: nums[0],
        nice: nums[1],
        system: nums[2],
        idle: nums[3],
        iowait: nums[4],
        irq: nums[5],
        softirq: nums[6],
        steal: nums[7],
    })
}

pub fn cpu_percent(prev: ProcStat, next: ProcStat) -> Option<f64> {
    let total = next.total().saturating_sub(prev.total());
    if total == 0 {
        return None;
    }
    let idle = next.idle.saturating_sub(prev.idle) + next.iowait.saturating_sub(prev.iowait);
    let busy = total.saturating_sub(idle);
    Some((busy as f64) * 100.0 / (total as f64))
}

pub fn parse_meminfo(text: &str) -> Result<(u64, u64), ParseError> {
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total = Some(kib_field(rest)? * 1024);
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            available = Some(kib_field(rest)? * 1024);
        }
    }
    let total = total.ok_or(ParseError::Invalid("MemTotal"))?;
    let available = available.ok_or(ParseError::Invalid("MemAvailable"))?;
    Ok((total, total.saturating_sub(available)))
}

fn kib_field(rest: &str) -> Result<u64, ParseError> {
    rest.split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .ok_or(ParseError::Invalid("meminfo field"))
}

pub fn parse_uptime(text: &str) -> Result<u64, ParseError> {
    let seconds = text
        .split_whitespace()
        .next()
        .ok_or(ParseError::Invalid("/proc/uptime"))?;
    let value: f64 = seconds
        .parse()
        .map_err(|_| ParseError::Invalid("/proc/uptime"))?;
    Ok(value as u64)
}

/// Parses `df -P` output. The first line is the header.
pub fn parse_df_p(text: &str) -> Result<Vec<Disk>, ParseError> {
    let mut disks = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if index == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 6 {
            return Err(ParseError::Invalid("df"));
        }
        disks.push(Disk {
            filesystem: parts[0].to_string(),
            total_kib: parts[1].parse().map_err(|_| ParseError::Invalid("df"))?,
            used_kib: parts[2].parse().map_err(|_| ParseError::Invalid("df"))?,
            mount: parts[5].to_string(),
        });
    }
    Ok(disks)
}

pub fn sample_linux_text(
    stat_prev: &str,
    stat_next: &str,
    meminfo: &str,
    uptime: &str,
    df: &str,
) -> Result<Sample, ParseError> {
    let cpu = cpu_percent(parse_proc_stat(stat_prev)?, parse_proc_stat(stat_next)?);
    let (memory_total_bytes, memory_used_bytes) = parse_meminfo(meminfo)?;
    Ok(Sample {
        source: "ssh:/proc".into(),
        cpu_percent: cpu,
        memory_total_bytes: Some(memory_total_bytes),
        memory_used_bytes: Some(memory_used_bytes),
        uptime_seconds: Some(parse_uptime(uptime)?),
        disks: parse_df_p(df)?,
        processes: Vec::new(),
    })
}

const SECTION: &str = "@@termix-section@@";

/// Run this on a Linux host, then pass its stdout to [`parse_remote_sample`].
/// `/proc/stat` is read twice because CPU usage is the change between readings.
pub const REMOTE_SAMPLE_COMMAND: &str = "cat /proc/stat && echo @@termix-section@@ && sleep 1 && \
    cat /proc/stat && echo @@termix-section@@ && \
    cat /proc/meminfo && echo @@termix-section@@ && \
    cat /proc/uptime && echo @@termix-section@@ && LC_ALL=C df -P -k";

pub fn parse_remote_sample(output: &str) -> Option<Result<Sample, ParseError>> {
    let sections: Vec<&str> = output.split(SECTION).map(str::trim_start).collect();
    match sections.as_slice() {
        [stat_prev, stat_next, meminfo, uptime, df] => {
            Some(sample_linux_text(stat_prev, stat_next, meminfo, uptime, df))
        }
        _ => None,
    }
}

/// Reads this machine through sysinfo. CPU usage needs two refreshes; the first
/// call can return `cpu_percent: None` because the kernel has no delta yet.
pub fn sample_local() -> Result<Sample, String> {
    let kind = RefreshKind::nothing()
        .with_cpu(CpuRefreshKind::everything())
        .with_memory(MemoryRefreshKind::everything())
        .with_processes(ProcessRefreshKind::everything());
    let mut system = System::new_with_specifics(kind).map_err(|error| error.to_string())?;
    system.refresh_cpu_usage();
    std::thread::sleep(std::time::Duration::from_millis(200));
    system.refresh_cpu_usage();
    system.refresh_memory();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let mut processes: Vec<ProcessInfo> = system
        .processes()
        .iter()
        .map(|(pid, process)| ProcessInfo {
            pid: pid.as_u32(),
            name: process.name().to_string_lossy().to_string(),
            memory_bytes: process.memory(),
        })
        .collect();
    processes.sort_by_key(|process| std::cmp::Reverse(process.memory_bytes));
    processes.truncate(12);
    let disks = Disks::new_with_refreshed_list()
        .map_err(|error| error.to_string())?
        .iter()
        .map(|disk| Disk {
            filesystem: disk.name().to_string_lossy().to_string(),
            mount: disk.mount_point().to_string_lossy().to_string(),
            total_kib: disk.total_space() / 1024,
            used_kib: disk.total_space().saturating_sub(disk.available_space()) / 1024,
        })
        .collect();
    Ok(Sample {
        source: "local:sysinfo".into(),
        cpu_percent: Some(f64::from(system.global_cpu_usage())),
        memory_total_bytes: Some(system.total_memory()),
        memory_used_bytes: Some(system.used_memory()),
        uptime_seconds: System::uptime().ok(),
        disks,
        processes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_linux_fixtures() {
        let prev = "cpu  100 0 50 850 0 0 0 0 0 0\n";
        let next = "cpu  150 0 80 900 0 0 0 0 0 0\n";
        let mem = "MemTotal:       1000000 kB\nMemAvailable:    250000 kB\n";
        let up = "3661.5 1000.0\n";
        let df = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda1 1000 400 600 40% /\n";
        let sample = sample_linux_text(prev, next, mem, up, df).unwrap();
        let cpu = sample.cpu_percent.unwrap();
        assert!(cpu > 40.0 && cpu < 80.0, "{cpu}");
        assert_eq!(sample.memory_total_bytes, Some(1_000_000 * 1024));
        assert_eq!(sample.memory_used_bytes, Some(750_000 * 1024));
        assert_eq!(sample.uptime_seconds, Some(3661));
        assert_eq!(sample.disks[0].mount, "/");
    }

    #[test]
    fn parses_remote_output_and_rejects_other_systems() {
        let output = "cpu  100 0 50 850 0 0 0 0 0 0\n@@termix-section@@\n\
            cpu  150 0 80 900 0 0 0 0 0 0\n@@termix-section@@\n\
            MemTotal: 1000000 kB\nMemAvailable: 250000 kB\n@@termix-section@@\n\
            3661.5 1000.0\n@@termix-section@@\n\
            Filesystem 1024-blocks Used Available Capacity Mounted on\n\
            /dev/sda1 1000 400 600 40% /\n";
        let sample = parse_remote_sample(output).unwrap().unwrap();
        assert_eq!(sample.uptime_seconds, Some(3661));
        assert_eq!(sample.disks.len(), 1);
        assert!(parse_remote_sample("cat: /proc/stat: No such file").is_none());
    }

    #[test]
    fn remote_command_uses_the_section_marker() {
        assert_eq!(REMOTE_SAMPLE_COMMAND.matches(SECTION).count(), 4);
    }

    #[test]
    fn local_sample_comes_from_the_os() {
        let sample = sample_local().unwrap();
        assert_eq!(sample.source, "local:sysinfo");
        assert!(sample.memory_total_bytes.unwrap_or(0) > 0);
        assert!(sample.processes.len() <= 12);
        assert!(sample
            .processes
            .windows(2)
            .all(|pair| pair[0].memory_bytes >= pair[1].memory_bytes));
    }
}
