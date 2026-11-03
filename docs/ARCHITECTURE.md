# Architecture

```
Tauri app with a TypeScript UI (Windows, macOS, Linux, iOS, Android)
        |
        v
termix-cli / termix-ffi
        |
termix-session (trust, credentials, commands, metrics, sync)
        |
        +-- termix-ssh ------ russh
        +-- termix-sftp ----- russh-sftp
        +-- termix-storage -- SQLite
        +-- termix-security - AES-GCM + Argon2id
        +-- termix-monitor -- sysinfo or parsed command output
        +-- termix-terminal - VTE screen + local PTY
```

The webview does not open its own sockets. Tauri commands call the same crates as the CLI. There is no second SSH stack for mobile.

Tauri uses the operating system webview on every target.