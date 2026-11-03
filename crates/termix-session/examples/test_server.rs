//! Starts the integration-test SSH server so the CLI and app can be tried without
//! a real machine. Listens on 127.0.0.1 only. Never expose it: the password is fixed.

#[path = "../tests/support/mod.rs"]
#[allow(dead_code)]
mod support;

use russh::keys::{Algorithm, PrivateKey};

#[tokio::main]
async fn main() {
    let client_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let server = support::start_server(client_key.public_key().clone()).await;
    println!("port {}", server.port);
    println!("fingerprint {}", server.fingerprint);
    println!("password {}", support::PASSWORD);
    println!("files {}", server.files.display());
    tokio::signal::ctrl_c().await.unwrap();
}
