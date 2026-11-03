# Termix

Every machine. One control center.

Termix is a remote-machine manager for Windows, macOS, Linux, iOS, and Android, built as one [Tauri 2](https://v2.tauri.app/) app with a TypeScript UI. SSH, SFTP, encryption, and the local database are shared Rust crates. There is no separate Swift or Electron app.

Repository: <https://github.com/nishanth-kj/Termix>

## What is implemented

| Area | App | CLI |
| --- | --- | --- |
| Saved machines | Yes | Yes |
| Password and private-key sign-in | Yes (password only on iOS and Android) | Yes |
| Saved passwords, AES-256-GCM encrypted | Yes | Yes |
| Host keys pinned to the exact fingerprint you verified | Yes | Yes |
| Interactive SSH terminal | Yes | No |
| Local terminal | Desktop only | No |
| One-off commands with exit code | Yes | Yes |
| SFTP browse, upload, download, rename, delete | Yes (no local file transfer on iOS and Android) | Yes |
| Metrics for this device | Yes | Yes |
| Metrics for Linux machines over SSH | Yes | Yes |
| Folder sync: plan, then copy, never delete | Desktop only | Yes |

Not implemented: port forwarding, jump hosts, SOCKS, SSH agent, Docker, Kubernetes, RDP, VNC, Telnet, serial, and the OS keychain. The master key is a file; see [SECURITY.md](SECURITY.md). The passphrase-wrapping functions exist in `termix-security` but nothing uses them yet.

Nothing in this tree invents CPU numbers, container lists, or online status. A TCP probe only means the port accepted a connection.

## Platform status

| Target | Status |
| --- | --- |
| Windows | Built and tested end to end by driving the real window |
| Android | A debug APK builds. Not yet run on a device or emulator |
| Linux, macOS, iOS | Same code, set up in CI. Not yet built or run |

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
crates/termix-session       workflows shared by the CLI and the app
crates/termix-ffi           C ABI
crates/termix-cli           `termix` binary
app/ui                      TypeScript UI (Vite)
app/src-tauri               Tauri app for every platform
scripts/e2e-*.mts           drives the real Windows window and the CLI
```

## Command line

```bash
cargo test
cargo run -p termix-cli -- machine add --name edge --host 10.0.0.8 --username ops
cargo run -p termix-cli -- probe edge
TERMIX_PASSWORD=... cargo run -p termix-cli -- exec edge uname -a
```

The password comes from `TERMIX_PASSWORD` or `termix password save`. Private keys are passed with `--key`. Passwords are never accepted as arguments.

The first connection to a host fails and prints its fingerprint, before any credentials are sent. Check it on the server, then re-run with `--trust SHA256:...`. Only that exact key is accepted, and a key that differs from the pinned one is refused until you run `termix host forget`.

Commands that look destructive (`rm`, `mkfs`, `git clean`, `kubectl delete`, …) require `--yes`. This is a heuristic, not a sandbox.

## App

```bash
cargo install tauri-cli --version "^2" --locked
cd app
npm ci
cargo tauri dev           # desktop
cargo tauri build         # desktop installer
cargo tauri android dev
cargo tauri ios init      # once, on macOS
cargo tauri ios dev
```

Prerequisites for each platform are in [docs/BUILDING.md](docs/BUILDING.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
