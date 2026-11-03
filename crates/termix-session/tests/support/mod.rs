//! An SSH and SFTP server that runs inside a test process.
//! It is for tests and local trials only: it accepts one fixed password.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use russh::keys::{Algorithm, HashAlg, PrivateKey, PublicKey};
use russh::server::{Auth as ServerAuth, ChannelOpenHandle, Msg, Session};
use russh::{Channel, ChannelId};
use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version,
};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

pub const PASSWORD: &str = "correct-password";
const METRICS: &str = "cpu  100 0 50 850 0 0 0 0 0 0\n@@termix-section@@\n\
cpu  150 0 80 900 0 0 0 0 0 0\n@@termix-section@@\n\
MemTotal: 1000000 kB\nMemAvailable: 250000 kB\n@@termix-section@@\n\
3661.5 1000.0\n@@termix-section@@\n\
Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda1 1000 400 600 40% /\n";

pub struct TestServer {
    pub port: u16,
    pub fingerprint: String,
    pub password_attempts: Arc<AtomicUsize>,
    pub executed: Arc<std::sync::Mutex<Vec<String>>>,
    pub files: PathBuf,
}

#[derive(Clone)]
struct Shared {
    password_attempts: Arc<AtomicUsize>,
    executed: Arc<std::sync::Mutex<Vec<String>>>,
    client_key: PublicKey,
    files: PathBuf,
}

struct ServerSession {
    shared: Shared,
    channels: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
    shells: Vec<ChannelId>,
}

impl russh::server::Handler for ServerSession {
    type Error = russh::Error;

    async fn auth_password(&mut self, _: &str, password: &str) -> Result<ServerAuth, Self::Error> {
        self.shared.password_attempts.fetch_add(1, Ordering::SeqCst);
        Ok(if password == PASSWORD {
            ServerAuth::Accept
        } else {
            ServerAuth::reject()
        })
    }

    async fn auth_publickey(
        &mut self,
        _: &str,
        key: &PublicKey,
    ) -> Result<ServerAuth, Self::Error> {
        Ok(if *key == self.shared.client_key {
            ServerAuth::Accept
        } else {
            ServerAuth::reject()
        })
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.lock().await.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let command = String::from_utf8_lossy(data).to_string();
        self.shared.executed.lock().unwrap().push(command.clone());
        session.channel_success(channel)?;
        let (stdout, stderr, code) = if command.contains("/proc/stat") {
            (METRICS, "", 0)
        } else if command == "fail" {
            ("partial\n", "broken\n", 3)
        } else {
            ("hello\n", "", 0)
        };
        session.data(channel, stdout.as_bytes().to_vec())?;
        if !stderr.is_empty() {
            session.extended_data(channel, 1, stderr.as_bytes().to_vec())?;
        }
        session.exit_status_request(channel, code)?;
        session.eof(channel)?;
        session.close(channel)?;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _: &str,
        _: u32,
        _: u32,
        _: u32,
        _: u32,
        _: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.shells.push(channel);
        session.channel_success(channel)?;
        Ok(())
    }

    // The fake shell echoes what it receives. SFTP channels also pass through here.
    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.shells.contains(&channel) {
            session.data(channel, data.to_vec())?;
        }
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if name != "sftp" {
            session.channel_failure(channel_id)?;
            return Ok(());
        }
        let channel = self.channels.lock().await.remove(&channel_id).unwrap();
        session.channel_success(channel_id)?;
        let handler = FolderSftp {
            root: self.shared.files.clone(),
            open: HashMap::new(),
            listed: HashMap::new(),
            next: 0,
        };
        russh_sftp::server::run(channel.into_stream(), handler).await;
        Ok(())
    }
}

/// A tiny SFTP server rooted in a temporary folder.
struct FolderSftp {
    root: PathBuf,
    open: HashMap<String, PathBuf>,
    listed: HashMap<String, bool>,
    next: u32,
}

impl FolderSftp {
    fn resolve(&self, path: &str) -> PathBuf {
        path.split('/')
            .filter(|part| !part.is_empty() && *part != ".")
            .fold(self.root.clone(), |full, part| full.join(part))
    }

    fn handle(&mut self, path: PathBuf) -> String {
        self.next += 1;
        let handle = self.next.to_string();
        self.open.insert(handle.clone(), path);
        handle
    }
}

fn attrs_of(path: &Path) -> Result<FileAttributes, StatusCode> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| StatusCode::NoSuchFile)?;
    let mut attrs = FileAttributes::empty();
    attrs.size = Some(metadata.len());
    attrs.permissions = Some(if metadata.is_dir() {
        0o040755
    } else {
        0o100644
    });
    attrs.mtime = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_secs() as u32);
    Ok(attrs)
}

fn ok(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".into(),
        language_tag: "en-US".into(),
    }
}

impl russh_sftp::server::Handler for FolderSftp {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(&mut self, _: u32, _: HashMap<String, String>) -> Result<Version, Self::Error> {
        Ok(Version::new())
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        flags: OpenFlags,
        _: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        let path = self.resolve(&filename);
        if flags.contains(OpenFlags::CREATE) {
            std::fs::write(&path, b"").map_err(|_| StatusCode::Failure)?;
        } else if !path.exists() {
            return Err(StatusCode::NoSuchFile);
        }
        Ok(Handle {
            id,
            handle: self.handle(path),
        })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        self.open.remove(&handle);
        self.listed.remove(&handle);
        Ok(ok(id))
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        let path = self.open.get(&handle).ok_or(StatusCode::Failure)?;
        let bytes = std::fs::read(path).map_err(|_| StatusCode::Failure)?;
        let start = offset as usize;
        if start >= bytes.len() {
            return Err(StatusCode::Eof);
        }
        let end = (start + len as usize).min(bytes.len());
        Ok(Data {
            id,
            data: bytes[start..end].to_vec(),
        })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        let path = self.open.get(&handle).ok_or(StatusCode::Failure)?;
        let mut bytes = std::fs::read(path).map_err(|_| StatusCode::Failure)?;
        let end = offset as usize + data.len();
        if bytes.len() < end {
            bytes.resize(end, 0);
        }
        bytes[offset as usize..end].copy_from_slice(&data);
        std::fs::write(path, bytes).map_err(|_| StatusCode::Failure)?;
        Ok(ok(id))
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        Ok(Attrs {
            id,
            attrs: attrs_of(&self.resolve(&path))?,
        })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        self.lstat(id, path).await
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
        let path = self.open.get(&handle).ok_or(StatusCode::Failure)?;
        Ok(Attrs {
            id,
            attrs: attrs_of(path)?,
        })
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let full = self.resolve(&path);
        if !full.is_dir() {
            return Err(StatusCode::NoSuchFile);
        }
        Ok(Handle {
            id,
            handle: self.handle(full),
        })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        if self.listed.insert(handle.clone(), true).is_some() {
            return Err(StatusCode::Eof);
        }
        let path = self.open.get(&handle).ok_or(StatusCode::Failure)?;
        let mut files = Vec::new();
        for entry in std::fs::read_dir(path).map_err(|_| StatusCode::Failure)? {
            let entry = entry.map_err(|_| StatusCode::Failure)?;
            let name = entry.file_name().to_string_lossy().to_string();
            // Lets a test play a hostile server.
            let name = name.replace("HOSTILE", "../..");
            files.push(File::new(name, attrs_of(&entry.path())?));
        }
        Ok(Name { id, files })
    }

    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        _: FileAttributes,
    ) -> Result<Status, Self::Error> {
        std::fs::create_dir(self.resolve(&path)).map_err(|_| StatusCode::Failure)?;
        Ok(ok(id))
    }

    async fn remove(&mut self, id: u32, path: String) -> Result<Status, Self::Error> {
        std::fs::remove_file(self.resolve(&path)).map_err(|_| StatusCode::NoSuchFile)?;
        Ok(ok(id))
    }

    async fn rename(&mut self, id: u32, from: String, to: String) -> Result<Status, Self::Error> {
        std::fs::rename(self.resolve(&from), self.resolve(&to)).map_err(|_| StatusCode::Failure)?;
        Ok(ok(id))
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        Ok(Name {
            id,
            files: vec![File::dummy(path)],
        })
    }
}

pub fn temp_dir(label: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "termix-it-{}-{label}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub async fn start_server(client_key: PublicKey) -> TestServer {
    let host_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let fingerprint = host_key
        .public_key()
        .fingerprint(HashAlg::Sha256)
        .to_string();
    let config = Arc::new(russh::server::Config {
        auth_rejection_time: Duration::from_millis(1),
        auth_rejection_time_initial: Some(Duration::from_millis(0)),
        keys: vec![host_key],
        ..Default::default()
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let files = temp_dir("files");
    let shared = Shared {
        password_attempts: Arc::new(AtomicUsize::new(0)),
        executed: Arc::new(std::sync::Mutex::new(Vec::new())),
        client_key,
        files: files.clone(),
    };
    let server = TestServer {
        port,
        fingerprint,
        password_attempts: shared.password_attempts.clone(),
        executed: shared.executed.clone(),
        files,
    };
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let handler = ServerSession {
                shared: shared.clone(),
                channels: Arc::new(Mutex::new(HashMap::new())),
                shells: Vec::new(),
            };
            let config = config.clone();
            tokio::spawn(async move {
                if let Ok(running) = russh::server::run_stream(config, stream, handler).await {
                    let _ = running.await;
                }
            });
        }
    });
    server
}
