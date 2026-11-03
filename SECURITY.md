# Security

## Supported use

Termix stores machine metadata in SQLite and encrypts passwords and key material with AES-256-GCM. The master key is a 32-byte file in the app data folder, next to the database. On Unix it is created with mode `0600` and a looser file is refused. On Windows it relies on the permissions of your user profile. This is not an OS keychain: anything running as you, or a backup of that folder, can read the key and therefore the saved passwords. Do not save passwords if that is a concern.

## Reporting

Open a private security advisory on https://github.com/nishanth-kj/Termix or email the maintainer. Do not file a public issue for an unfixed vulnerability.

## Current limits

- Host keys are pinned by SHA-256 fingerprint after an explicit trust. There is no certificate authority workflow.
- Jump hosts, agent forwarding, and proxy commands are not implemented.
- The destructive-command check is a keyword heuristic. It is not a sandbox.
- Clipboard clearing, biometric unlock, and secure wipe are not implemented.
