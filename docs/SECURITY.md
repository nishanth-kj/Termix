# Security design

See the repository [SECURITY.md](../SECURITY.md) for reporting.

Secrets are encrypted before `INSERT`. Activity rows store redacted command text. Host keys are not trusted on first use unless the user passes `--trust` after seeing the fingerprint. A fingerprint that does not match the stored one aborts the connection.

Trust is bound to one fingerprint. The caller names the fingerprint it verified, the connection accepts only that key, and it is pinned after authentication succeeds. No credentials are sent to an unverified key.

Names in a remote directory listing must be a single path component, and sync paths are checked again before writing, so a hostile server cannot write outside the chosen folder.
