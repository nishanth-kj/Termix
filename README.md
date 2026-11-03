# Termix

Every machine. One control center.

Termix is a remote-machine manager for Windows, macOS, iOS, and Android. The shell is [Tauri 2](https://v2.tauri.app/). SSH, SFTP, encryption, and the local database are shared Rust crates. There is no separate Java, Swift, or Electron app.

Repository: https://github.com/nishanth-kj/Termix

## What is implemented

| Area | Status |
| --- | --- |
| Saved machines in SQLite | Implemented |
| AES-256-GCM secrets, Argon2id passphrase wrapping | Implemented |
| SSH password and private-key auth | Implemented |
| SSH host-key check, unknown keys rejected | Implemented |
| One-shot remote commands | Implemented |
| SFTP list, read, write, mkdir, rename, delete | Implemented |
| Local PTY and a basic VT screen | Implemented |
| Local metrics via sysinfo, Linux metrics parsed from real command output | Implemented |
| Folder sync planner (no deletes applied) | Implemented |
| CLI (`termix`) | Implemented |
| Tauri shell: machines, command, local metrics, support list | Implemented |
| C ABI `termix_support_json` | Implemented |
| Port forwarding, jump hosts, SOCKS, SSH agent | Not implemented |
| Applying a sync plan | Not implemented |
| Docker, Kubernetes, RDP, VNC, Telnet, serial | Not implemented |
| OS keychain (Keychain, Credential Manager, Keystore) | Not implemented. The master key is a 0600 file. |

Nothing in this tree invents CPU numbers, container lists, or online status. A TCP probe only means the port accepted a connection.

## Layout

```
crates/termix-core          domain types and the support matrix
crates/termix-security      encryption
crates/termix-storage       SQLite
crates/termix-ssh           russh client
crates/termix-sftp          SFTP
crates/termix-terminal      VT screen and local PTY
crates/termix-monitor       parsers and local sampling
crates/termix-sync          sync plans
crates/termix-automation    destructive-command confirmation
crates/termix-ffi           C ABI
crates/termix-cli           `termix` binary
app/                        Tauri shell (one UI for desktop and mobile)
```

## Build

```bash
cargo test
cargo run -p termix-cli -- support
cargo run -p termix-cli -- machine add --name edge --host 10.0.0.8 --username ops
cargo run -p termix-cli -- local
```

SSH password comes from `TERMIX_PASSWORD`. Private keys are passed with `--key`. Do not put passwords in arguments.

The first time a host key is unseen, the command fails and prints the fingerprint. Re-run with `--trust` only after you have checked that fingerprint. A changed key is refused even with `--trust`.

Destructive-looking commands (`rm`, `mkfs`, `kubectl delete`, …) require `--yes`.

### Desktop, iOS, and Android

The UI is one Tauri app. Install the Tauri CLI, then from `app/`:

```bash
cargo install tauri-cli --version "^2" --locked
cargo tauri dev
cargo tauri android init
cargo tauri ios init
cargo tauri android dev
cargo tauri ios dev
```

iOS builds require macOS and Xcode. Android builds require the Android SDK and NDK. Every target uses the system webview (WebView2, WKWebView, Android WebView, WebKitGTK). The Rust commands behind that window are the same on all four systems.

Linux desktop builds need WebKitGTK (`libwebkit2gtk-4.1` and `libgtk-3`). This environment does not have those libraries, so the window was not launched here. The command crate still typechecks when those libraries are present.

## License

Apache-2.0. See [LICENSE](LICENSE).
