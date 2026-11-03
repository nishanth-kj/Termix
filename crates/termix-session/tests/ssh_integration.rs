//! Exercises the real client stack against an in-process SSH and SFTP server.

mod support;

use std::sync::atomic::Ordering;
use std::time::Duration;

use russh::keys::{Algorithm, PrivateKey};
use support::{start_server, temp_dir, TestServer, PASSWORD};
use termix_core::{Machine, NewMachine, Protocol};
use termix_session::{remote_metrics, SessionError, Workspace};
use termix_sftp::Sftp;
use termix_ssh::{Auth, ConnectRequest, SshError, SshSession};
use termix_sync::Direction;
use tokio::net::TcpListener;

struct Fixture {
    server: TestServer,
    workspace: Workspace,
    machine: Machine,
    client_key: PrivateKey,
}

async fn fixture() -> Fixture {
    let client_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let server = start_server(client_key.public_key().clone()).await;
    let workspace = Workspace::open(&temp_dir("workspace")).unwrap();
    let machine = workspace
        .db
        .insert_machine(NewMachine {
            name: "box".into(),
            host: "127.0.0.1".into(),
            port: server.port,
            username: "ops".into(),
            protocol: Protocol::Ssh,
            os_hint: None,
            notes: String::new(),
            tags: Vec::new(),
            group: None,
        })
        .unwrap();
    Fixture {
        server,
        workspace,
        machine,
        client_key,
    }
}

fn password() -> Auth {
    Auth::Password(PASSWORD.into())
}

impl Fixture {
    async fn trusted_session(&self) -> SshSession {
        self.workspace
            .connect(&self.machine, password(), Some(&self.server.fingerprint))
            .await
            .unwrap()
    }

    fn pin(&self) -> Option<String> {
        self.workspace
            .db
            .trusted_fingerprint("127.0.0.1", self.server.port)
            .unwrap()
    }
}

#[tokio::test]
async fn unknown_host_is_refused_before_credentials_are_sent() {
    let f = fixture().await;
    let error = f
        .workspace
        .connect(&f.machine, password(), None)
        .await
        .err()
        .unwrap();
    match error {
        SessionError::Ssh(SshError::HostKeyUnknown { fingerprint, .. }) => {
            assert_eq!(fingerprint, f.server.fingerprint);
        }
        other => panic!("unexpected: {other}"),
    }
    assert_eq!(f.server.password_attempts.load(Ordering::SeqCst), 0);
    assert_eq!(f.pin(), None);
}

#[tokio::test]
async fn trusting_a_different_fingerprint_is_refused_and_not_saved() {
    let f = fixture().await;
    let error = f
        .workspace
        .connect(&f.machine, password(), Some("SHA256:not-the-real-key"))
        .await
        .err()
        .unwrap();
    assert!(matches!(
        error,
        SessionError::Ssh(SshError::HostKeyMismatch { .. })
    ));
    assert_eq!(f.server.password_attempts.load(Ordering::SeqCst), 0);
    assert_eq!(f.pin(), None);
}

#[tokio::test]
async fn trusting_the_exact_fingerprint_pins_it_for_later_connections() {
    let f = fixture().await;
    f.trusted_session().await;
    assert_eq!(f.pin().as_deref(), Some(f.server.fingerprint.as_str()));
    f.workspace
        .connect(&f.machine, password(), None)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_changed_host_key_is_refused_even_when_trust_is_requested() {
    let f = fixture().await;
    f.workspace
        .db
        .trust_host("127.0.0.1", f.server.port, "ssh-ed25519", "SHA256:old")
        .unwrap();
    let silent = f
        .workspace
        .connect(&f.machine, password(), None)
        .await
        .err()
        .unwrap();
    assert!(matches!(
        silent,
        SessionError::Ssh(SshError::HostKeyMismatch { .. })
    ));
    let forced = f
        .workspace
        .connect(&f.machine, password(), Some(&f.server.fingerprint))
        .await
        .err()
        .unwrap();
    assert!(matches!(forced, SessionError::AlreadyPinned { .. }));
    assert_eq!(f.server.password_attempts.load(Ordering::SeqCst), 0);
    assert_eq!(f.pin().as_deref(), Some("SHA256:old"));

    assert!(f
        .workspace
        .db
        .forget_host("127.0.0.1", f.server.port)
        .unwrap());
    f.trusted_session().await;
}

#[tokio::test]
async fn wrong_password_fails_and_does_not_pin() {
    let f = fixture().await;
    let error = f
        .workspace
        .connect(
            &f.machine,
            Auth::Password("wrong".into()),
            Some(&f.server.fingerprint),
        )
        .await
        .err()
        .unwrap();
    assert!(matches!(error, SessionError::Ssh(SshError::AuthFailed)));
    assert_eq!(f.pin(), None);
}

#[tokio::test]
async fn private_key_authentication_works() {
    let f = fixture().await;
    let path = temp_dir("key").join("id_ed25519");
    let pem = f
        .client_key
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .unwrap();
    std::fs::write(&path, pem.as_bytes()).unwrap();
    f.workspace
        .connect(
            &f.machine,
            Auth::PrivateKey {
                path,
                passphrase: None,
            },
            Some(&f.server.fingerprint),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn saved_password_round_trips_through_the_vault() {
    let f = fixture().await;
    assert!(matches!(
        f.workspace.saved_auth(&f.machine),
        Err(SessionError::NoCredentials(_))
    ));
    f.workspace.remember_password(&f.machine, PASSWORD).unwrap();
    assert!(f.workspace.has_saved_password(&f.machine).unwrap());
    let auth = f.workspace.saved_auth(&f.machine).unwrap();
    f.workspace
        .connect(&f.machine, auth, Some(&f.server.fingerprint))
        .await
        .unwrap();
    f.workspace.forget_password(&f.machine).unwrap();
    assert!(!f.workspace.has_saved_password(&f.machine).unwrap());
}

#[tokio::test]
async fn commands_report_output_stderr_and_exit_code_and_are_logged_redacted() {
    let f = fixture().await;
    let ok = f
        .workspace
        .run_command(
            &f.machine,
            password(),
            Some(&f.server.fingerprint),
            "deploy --token hunter2",
            false,
        )
        .await
        .unwrap();
    assert_eq!(ok.stdout, "hello\n");
    assert_eq!(ok.exit_code, Some(0));
    let activity = f.workspace.db.recent_activity(1).unwrap();
    assert!(activity[0].detail.contains("[redacted]"));
    assert!(!activity[0].detail.contains("hunter2"));

    let failed = f
        .workspace
        .run_command(&f.machine, password(), None, "fail", false)
        .await
        .unwrap();
    assert_eq!(failed.stdout, "partial\n");
    assert_eq!(failed.stderr, "broken\n");
    assert_eq!(failed.exit_code, Some(3));
}

#[tokio::test]
async fn destructive_command_needs_confirmation_and_never_reaches_the_server() {
    let f = fixture().await;
    let refused = f
        .workspace
        .run_command(
            &f.machine,
            password(),
            Some(&f.server.fingerprint),
            "rm -rf /srv",
            false,
        )
        .await
        .err()
        .unwrap();
    assert!(matches!(refused, SessionError::NeedsConfirmation));
    assert!(f.server.executed.lock().unwrap().is_empty());
    f.workspace
        .run_command(
            &f.machine,
            password(),
            Some(&f.server.fingerprint),
            "rm -rf /srv",
            true,
        )
        .await
        .unwrap();
    assert_eq!(f.server.executed.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_server_that_stalls_during_the_handshake_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            held.push(stream);
        }
    });
    let result = SshSession::connect(ConnectRequest {
        host: "127.0.0.1".into(),
        port,
        username: "ops".into(),
        auth: password(),
        trusted_fingerprint: None,
        timeout: Duration::from_millis(300),
        jump_host: None,
    })
    .await;
    assert!(matches!(result, Err(SshError::Timeout(_, _))));
}

#[tokio::test]
async fn interactive_shell_echoes_and_resizes() {
    let f = fixture().await;
    let session = f.trusted_session().await;
    let mut shell = session.open_shell(80, 24).await.unwrap();
    shell.resize(100, 30).await.unwrap();
    shell.write(b"ping").await.unwrap();
    let mut seen = Vec::new();
    while seen.len() < 4 {
        let chunk = tokio::time::timeout(Duration::from_secs(5), shell.read_some())
            .await
            .expect("shell output")
            .unwrap()
            .expect("shell stays open");
        seen.extend_from_slice(&chunk);
    }
    assert_eq!(seen, b"ping");
    shell.close().await.unwrap();
}

#[tokio::test]
async fn remote_metrics_parse_real_command_output() {
    let f = fixture().await;
    let session = f.trusted_session().await;
    let sample = remote_metrics(&session, "box").await.unwrap();
    assert_eq!(sample.source, "ssh:/proc");
    assert_eq!(sample.uptime_seconds, Some(3661));
    assert_eq!(sample.disks[0].mount, "/");
    assert!(sample.cpu_percent.unwrap() > 0.0);
}

#[tokio::test]
async fn sftp_lists_streams_renames_and_removes() {
    let f = fixture().await;
    let session = f.trusted_session().await;
    let sftp = Sftp::open(&session).await.unwrap();
    sftp.create_dir("/docs").await.unwrap();

    let payload = vec![7u8; 300_000];
    let mut input = payload.as_slice();
    assert_eq!(
        sftp.upload(&mut input, "/docs/big.bin").await.unwrap(),
        300_000
    );
    let mut output = Vec::new();
    assert_eq!(
        sftp.download("/docs/big.bin", &mut output).await.unwrap(),
        300_000
    );
    assert_eq!(output, payload);

    let listed = sftp.list("/docs").await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "big.bin");
    assert_eq!(listed[0].path, "/docs/big.bin");
    assert_eq!(listed[0].size, 300_000);
    assert!(!listed[0].is_dir);
    assert!(sftp.list("/").await.unwrap()[0].is_dir);

    sftp.rename("/docs/big.bin", "/docs/renamed.bin")
        .await
        .unwrap();
    sftp.remove_file("/docs/renamed.bin").await.unwrap();
    assert!(sftp.list("/docs").await.unwrap().is_empty());
}

#[tokio::test]
async fn sync_copies_up_and_down_without_deleting_and_is_idempotent() {
    let f = fixture().await;
    let session = f.trusted_session().await;
    let sftp = Sftp::open(&session).await.unwrap();
    let local = temp_dir("sync-local");
    std::fs::create_dir_all(local.join("sub")).unwrap();
    std::fs::write(local.join("a.txt"), b"alpha").unwrap();
    std::fs::write(local.join("sub").join("b.txt"), b"beta").unwrap();
    std::fs::create_dir_all(f.server.files.join("site")).unwrap();
    std::fs::write(f.server.files.join("site").join("keep.txt"), b"keep").unwrap();

    let up = f
        .workspace
        .plan_sync(&sftp, Direction::LocalToRemote, &local, "/site", &[])
        .await
        .unwrap();
    assert_eq!(up.copies.len(), 2);
    assert_eq!(up.would_delete, vec!["keep.txt".to_string()]);
    f.workspace
        .apply_sync(&f.machine, &sftp, &up, &local, "/site")
        .await
        .unwrap();
    let site = f.server.files.join("site");
    assert_eq!(
        std::fs::read(site.join("sub").join("b.txt")).unwrap(),
        b"beta"
    );
    assert_eq!(std::fs::read(site.join("keep.txt")).unwrap(), b"keep");

    let again = f
        .workspace
        .plan_sync(&sftp, Direction::LocalToRemote, &local, "/site", &[])
        .await
        .unwrap();
    assert!(again.copies.is_empty(), "{:?}", again.copies);

    let down_root = temp_dir("sync-down");
    let down = f
        .workspace
        .plan_sync(&sftp, Direction::RemoteToLocal, &down_root, "/site", &[])
        .await
        .unwrap();
    assert_eq!(down.copies.len(), 3);
    f.workspace
        .apply_sync(&f.machine, &sftp, &down, &down_root, "/site")
        .await
        .unwrap();
    assert_eq!(std::fs::read(down_root.join("a.txt")).unwrap(), b"alpha");
    assert!(!down_root.join("a.txt.termix-part").exists());
}

#[tokio::test]
async fn a_hostile_file_name_from_the_server_is_refused() {
    let f = fixture().await;
    std::fs::create_dir_all(f.server.files.join("evil")).unwrap();
    std::fs::write(f.server.files.join("evil").join("HOSTILEpwned"), b"x").unwrap();
    let session = f.trusted_session().await;
    let sftp = Sftp::open(&session).await.unwrap();
    let error = sftp.walk("/evil").await.err().unwrap();
    assert!(matches!(error, termix_sftp::SftpError::UnsafeName(_)));
}
