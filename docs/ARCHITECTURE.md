# Architecture

```
Dioxus UI (Windows, macOS, iOS, Android)
        |
        v
termix-cli / termix-ffi
        |
        +-- termix-ssh ------ russh
        +-- termix-sftp ----- russh-sftp
        +-- termix-storage -- SQLite
        +-- termix-security - AES-GCM + Argon2id
        +-- termix-monitor -- sysinfo or parsed command output
        +-- termix-terminal - VTE screen + local PTY
```

The UI does not open its own sockets. A future mobile build calls the same crates; it does not grow a second SSH stack.

Dioxus renders with the operating system webview. The experimental Dioxus native renderer is not used.
