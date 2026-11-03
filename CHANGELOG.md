# Changelog

## Unreleased

- New app design built around machines, with a real file browser, metrics meters, and a phone layout.
- The UI is now TypeScript, built with Vite.
- Interactive SSH and local terminals, SFTP, remote metrics, folder sync, and saved passwords in the app.
- Folder sync can now copy files. It never deletes.
- `--trust` now takes the exact fingerprint. **Breaking:** `--trust` with no value no longer works.
- Destructive-command detection parses commands instead of matching substrings.
- Redaction covers `--password value`, tokens, URLs with credentials, and quoted values.
- The master key file is created with its final permissions and never overwritten.
- The connect timeout now covers the SSH handshake and login. Keepalives replace the idle cutoff.
- Integration tests against a real SSH and SFTP server, and end-to-end tests of the real Windows window.
- Minimum Rust is now 1.95, which the dependencies already required.

## 0.1.0

- Cargo workspace and shared Rust core.
- SQLite machine database and encrypted secret blobs.
- SSH client that rejects unknown host keys.
- SFTP file operations.
- Local metrics and Linux `/proc` parsers.
- Tauri 2 shell shared by Windows, macOS, iOS, and Android.
- `termix` CLI.
