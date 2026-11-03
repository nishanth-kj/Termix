# Contributing

- Keep SSH, SFTP, crypto, and storage in the Rust crates. Do not reimplement them in the UI.
- Do not mark a protocol implemented in `termix_core::Capability` until a real backend exists.
- Do not log passwords, passphrases, or private keys. Run text through `termix_logging::redact`.
- Unknown SSH host keys stay rejected until the user trusts that exact fingerprint.
- Tests that need a live SSH server should be `#[ignore]`.
- `cargo test` and `cargo fmt` should pass before a pull request.
- Tests that need SSH use the in-process server in `crates/termix-session/tests/support`, so they run everywhere.
- UI changes: `npm --prefix app run build` must pass strict type checks. On Windows, also run `scripts/e2e-windows.mts`.
