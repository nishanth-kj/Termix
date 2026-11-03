//! Direct TCP reachability. This does not authenticate and does not mean a host is healthy.

use std::time::Duration;

use tokio::net::TcpStream;
use tokio::time::timeout;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    Reachable { elapsed_ms: u128 },
    Unreachable { message: String },
}

pub async fn tcp_probe(host: &str, port: u16, limit: Duration) -> Probe {
    let started = std::time::Instant::now();
    match timeout(limit, TcpStream::connect((host, port))).await {
        Ok(Ok(_)) => Probe::Reachable {
            elapsed_ms: started.elapsed().as_millis(),
        },
        Ok(Err(error)) => Probe::Unreachable {
            message: error.to_string(),
        },
        Err(_) => Probe::Unreachable {
            message: format!("timed out after {} ms", limit.as_millis()),
        },
    }
}
