//! Interactive terminals. Output reaches the window through a Tauri channel, so
//! the same code serves Windows, macOS, Linux, iOS, and Android. A local shell
//! exists only on desktop: iOS and Android do not let an app start one.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::ipc::Channel;
use termix_ssh::SshSession;
use tokio::sync::mpsc;

use crate::session::AppError;

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShellEvent {
    Output { data: Vec<u8> },
    Closed,
}

enum Input {
    Data(Vec<u8>),
    Resize(u32, u32),
    Close,
}

#[derive(Default)]
pub struct Shells {
    next: AtomicU32,
    open: Mutex<HashMap<u32, mpsc::UnboundedSender<Input>>>,
}

impl Shells {
    fn register(&self, sender: mpsc::UnboundedSender<Input>) -> u32 {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.open.lock().expect("shell lock").insert(id, sender);
        id
    }

    fn send(&self, id: u32, input: Input) -> Result<(), AppError> {
        self.open
            .lock()
            .expect("shell lock")
            .get(&id)
            .ok_or_else(|| AppError::other("this terminal is closed"))?
            .send(input)
            .map_err(|_| AppError::other("this terminal is closed"))
    }

    pub fn write(&self, id: u32, data: String) -> Result<(), AppError> {
        self.send(id, Input::Data(data.into_bytes()))
    }

    pub fn resize(&self, id: u32, columns: u32, rows: u32) -> Result<(), AppError> {
        self.send(id, Input::Resize(columns, rows))
    }

    /// Ask a terminal to end. Closing one that already ended is not an error.
    pub fn close(&self, id: u32) {
        if let Some(sender) = self.open.lock().expect("shell lock").remove(&id) {
            let _ = sender.send(Input::Close);
        }
    }

    /// The task owns the session, so the connection lives exactly as long as the terminal.
    pub async fn start_ssh(
        &self,
        session: SshSession,
        columns: u32,
        rows: u32,
        output: Channel<ShellEvent>,
    ) -> Result<u32, AppError> {
        let mut shell = session
            .open_shell(columns, rows)
            .await
            .map_err(AppError::other)?;
        let (sender, mut inputs) = mpsc::unbounded_channel();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    input = inputs.recv() => {
                        let written = match input {
                            Some(Input::Data(data)) => shell.write(&data).await,
                            Some(Input::Resize(columns, rows)) => shell.resize(columns, rows).await,
                            Some(Input::Close) | None => break,
                        };
                        if written.is_err() {
                            break;
                        }
                    }
                    chunk = shell.read_some() => match chunk {
                        Ok(Some(data)) if data.is_empty() => {}
                        Ok(Some(data)) => {
                            if output.send(ShellEvent::Output { data }).is_err() {
                                break;
                            }
                        }
                        Ok(None) | Err(_) => break,
                    },
                }
            }
            let _ = shell.close().await;
            let _ = session.disconnect().await;
            let _ = output.send(ShellEvent::Closed);
        });
        Ok(self.register(sender))
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn start_local(
        &self,
        columns: u16,
        rows: u16,
        output: Channel<ShellEvent>,
    ) -> Result<u32, AppError> {
        use std::io::Read;

        let pty = termix_terminal::LocalPty::spawn_shell(columns, rows).map_err(AppError::other)?;
        let termix_terminal::LocalPtyParts {
            mut control,
            mut reader,
        } = pty.into_parts();
        let output_for_reader = output.clone();
        // The PTY API blocks, so it gets its own threads.
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                let data = buffer[..count].to_vec();
                if output_for_reader.send(ShellEvent::Output { data }).is_err() {
                    break;
                }
            }
            let _ = output_for_reader.send(ShellEvent::Closed);
        });
        let (sender, mut inputs) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            while let Some(input) = inputs.blocking_recv() {
                let result = match input {
                    Input::Data(data) => control.write(&data).map_err(|_| ()),
                    Input::Resize(columns, rows) => {
                        control.resize(columns as u16, rows as u16).map_err(|_| ())
                    }
                    Input::Close => break,
                };
                if result.is_err() {
                    break;
                }
            }
            let _ = control.kill();
        });
        Ok(self.register(sender))
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub fn start_local(
        &self,
        _columns: u16,
        _rows: u16,
        _output: Channel<ShellEvent>,
    ) -> Result<u32, AppError> {
        Err(AppError::other(
            "iOS and Android do not allow a local shell. Connect to a machine instead.",
        ))
    }
}
