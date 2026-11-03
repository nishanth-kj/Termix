# Features

Implemented means a caller can exercise the behavior in this repository today.

- Machine records, tags, groups, favorites.
- Passwords saved as encrypted blobs. The database column is ciphertext.
- Direct SSH with a password or a private key file.
- Host keys pinned per host and port to the exact fingerprint the user verified.
- One remote command at a time, with exit code. Activity is logged redacted.
- Interactive SSH terminal. Local terminal on desktop. Terminals drawn with xterm.js.
- SFTP list, streaming upload and download, mkdir, rename, remove.
- TCP reachability probe.
- Local CPU, memory, uptime, disks, and largest processes.
- Linux metrics over SSH from `/proc` and `df -P`.
- Sync plans that report copies, conflicts, and extra files, and applying the copies. Nothing is deleted.

Not implemented, and not presented as working:

- Remote and dynamic port forwards, SOCKS, jump hosts, proxies.
- SSH agent.
- Deleting files during sync.
- Docker and Kubernetes.
- RDP, VNC, Telnet, serial.
- Windows Credential Manager, macOS Keychain, Android Keystore, iOS Keychain.
- Biometric unlock.
- Private keys and local file transfer on iOS and Android, which need the platforms' file APIs.
