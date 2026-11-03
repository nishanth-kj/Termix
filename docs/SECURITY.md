# Security design

See the repository [SECURITY.md](../SECURITY.md) for reporting.

Secrets are encrypted before `INSERT`. Activity rows store redacted command text. Host keys are not trusted on first use unless the user passes `--trust` after seeing the fingerprint. A fingerprint that does not match the stored one aborts the connection.
