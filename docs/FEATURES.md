# Features

Implemented means a caller can exercise the behavior in this repository today.

- Machine records, tags, groups, favorites.
- Encrypted secret blobs. The database column is ciphertext.
- Direct SSH with password or a private key file.
- Host-key trust stored per host and port.
- `exec` of one remote command, with exit code.
- Interactive SSH shell channel (`open_shell`) and a local PTY.
- SFTP list/read/write/mkdir/rename/remove.
- TCP reachability probe.
- Local CPU, memory, uptime, and a short process list.
- Parsing of `/proc/stat`, `/proc/meminfo`, `/proc/uptime`, and `df -P`.
- Sync plans that report copies, conflicts, and files that would be deleted. Nothing is deleted.

Not implemented, and not presented as working:

- Remote and dynamic port forwards, SOCKS, jump hosts, proxies.
- SSH agent.
- Applying sync, including any delete.
- Docker and Kubernetes.
- RDP, VNC, Telnet, serial.
- Windows Credential Manager, macOS Keychain, Android Keystore, iOS Keychain.
- Biometric unlock.
