# Contributing

- Keep SSH, SFTP, crypto, and storage in the Rust crates. Do not reimplement them in the UI.
- Do not mark a protocol implemented in `termix_core::Capability` until a real backend exists.
- Do not log passwords, passphrases, or private keys. Run text through `termix_logging::redact`.
- Unknown SSH host keys stay rejected until the user trusts that exact fingerprint.
- Tests that need a live SSH server should be `#[ignore]`.
- `cargo test` and `cargo fmt` should pass before a pull request.
