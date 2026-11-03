//! A small VT screen. It only changes when bytes are fed into it.
//! It is not a full xterm: it handles printing, newline, carriage return,
//! backspace, scrolling, and a few CSI cursor commands. The app draws its
//! terminal with xterm.js; this screen serves tests and text-only callers.

use std::io::{Read, Write};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use vte::{Params, Parser, Perform};

pub const COLS: usize = 80;
pub const ROWS: usize = 24;

pub struct Screen {
    cells: Vec<char>,
    cursor_x: usize,
    cursor_y: usize,
    // Kept between calls: an escape sequence can span two reads.
    parser: Parser,
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            cells: vec![' '; COLS * ROWS],
            cursor_x: 0,
            cursor_y: 0,
            parser: Parser::new(),
        }
    }
}

impl Screen {
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut parser = std::mem::take(&mut self.parser);
        for byte in bytes {
            parser.advance(self, *byte);
        }
        self.parser = parser;
    }

    pub fn rows(&self) -> impl Iterator<Item = String> + '_ {
        (0..ROWS).map(|row| {
            let start = row * COLS;
            self.cells[start..start + COLS].iter().collect::<String>()
        })
    }

    pub fn text(&self) -> String {
        self.rows().collect::<Vec<_>>().join("\n")
    }

    fn line_feed(&mut self) {
        if self.cursor_y + 1 < ROWS {
            self.cursor_y += 1;
        } else {
            self.cells.drain(..COLS);
            self.cells.resize(COLS * ROWS, ' ');
        }
    }

    fn put_char(&mut self, ch: char) {
        if ch == '\n' {
            self.line_feed();
            return;
        }
        if self.cursor_x >= COLS {
            self.cursor_x = 0;
            self.line_feed();
        }
        self.cells[self.cursor_y * COLS + self.cursor_x] = ch;
        self.cursor_x += 1;
    }
}

impl Perform for Screen {
    fn print(&mut self, ch: char) {
        self.put_char(ch);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.put_char('\n'),
            b'\r' => self.cursor_x = 0,
            0x08 if self.cursor_x > 0 => self.cursor_x -= 1,
            _ => {}
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}

    fn csi_dispatch(
        &mut self,
        params: &Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let p = |index: usize, default: usize| -> usize {
            params
                .iter()
                .nth(index)
                .and_then(|item| item.first().copied())
                .map(|value| value as usize)
                .filter(|value| *value > 0)
                .unwrap_or(default)
        };
        match action {
            'H' | 'f' => {
                self.cursor_y = p(0, 1).saturating_sub(1).min(ROWS - 1);
                self.cursor_x = p(1, 1).saturating_sub(1).min(COLS - 1);
            }
            'G' => self.cursor_x = p(0, 1).saturating_sub(1).min(COLS - 1),
            'K' => {
                let start = self.cursor_y * COLS + self.cursor_x;
                let end = (self.cursor_y + 1) * COLS;
                for cell in &mut self.cells[start..end] {
                    *cell = ' ';
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
}

#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Pty(String),
}

pub struct LocalPty {
    // Dropping the master closes the pseudo console, so it lives as long as the shell.
    master: Box<dyn MasterPty + Send>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send>,
}

impl LocalPty {
    pub fn spawn_shell(columns: u16, rows: u16) -> Result<Self, PtyError> {
        let system = native_pty_system();
        let pair = system
            .openpty(PtySize {
                rows,
                cols: columns,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| PtyError::Pty(error.to_string()))?;
        let mut command = CommandBuilder::new(default_shell());
        command.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| PtyError::Pty(error.to_string()))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| PtyError::Pty(error.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| PtyError::Pty(error.to_string()))?;
        Ok(Self {
            master: pair.master,
            reader,
            writer,
            child,
        })
    }

    pub fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buf)
    }

    pub fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()
    }

    pub fn resize(&self, columns: u16, rows: u16) -> Result<(), PtyError> {
        self.master
            .resize(PtySize {
                rows,
                cols: columns,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| PtyError::Pty(error.to_string()))
    }

    /// Split into parts that can be used from different threads.
    pub fn into_parts(self) -> LocalPtyParts {
        LocalPtyParts {
            control: LocalPtyControl {
                master: self.master,
                writer: self.writer,
                child: self.child,
            },
            reader: self.reader,
        }
    }

    pub fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill()
    }
}

pub struct LocalPtyParts {
    pub control: LocalPtyControl,
    pub reader: Box<dyn Read + Send>,
}

pub struct LocalPtyControl {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send>,
}

impl LocalPtyControl {
    pub fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()
    }

    pub fn resize(&self, columns: u16, rows: u16) -> Result<(), PtyError> {
        self.master
            .resize(PtySize {
                rows,
                cols: columns,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| PtyError::Pty(error.to_string()))
    }

    pub fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill()
    }
}

fn default_shell() -> String {
    if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_renders_printed_text() {
        let mut screen = Screen::default();
        screen.feed(b"hi\r\nthere");
        let text = screen.text();
        assert!(text.starts_with("hi"));
        assert!(text.contains("there"));
    }

    #[test]
    fn escape_sequence_split_across_reads_is_not_printed() {
        let mut screen = Screen::default();
        screen.feed(b"a\x1b[");
        screen.feed(b"2Kb");
        assert!(!screen.text().contains('['));
        assert!(!screen.text().contains("2K"));
    }

    #[test]
    fn output_scrolls_at_the_bottom() {
        let mut screen = Screen::default();
        for line in 0..(ROWS + 5) {
            screen.feed(format!("line{line}\r\n").as_bytes());
        }
        let text = screen.text();
        assert!(!text.contains("line0 "));
        assert!(text.contains(&format!("line{}", ROWS + 4)));
    }

    #[test]
    fn local_shell_runs_a_command() {
        let pty = LocalPty::spawn_shell(80, 24).unwrap();
        let LocalPtyParts {
            mut control,
            mut reader,
        } = pty.into_parts();
        control.write(b"echo termix-pty-ok\r\n").unwrap();
        control.resize(100, 30).unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            let mut seen = String::new();
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                seen.push_str(&String::from_utf8_lossy(&buffer[..count]));
                // The typed command echoes too, so require the output on its own line.
                if seen.matches("termix-pty-ok").count() >= 2 {
                    let _ = sender.send(());
                    break;
                }
            }
        });
        receiver
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("the shell should echo the command");
        control.kill().unwrap();
    }
}
