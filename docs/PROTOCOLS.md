# Protocols

| Protocol | Backend | Notes |
| --- | --- | --- |
| SSH | russh 0.63 | Direct TCP only |
| SFTP | russh-sftp 3 | Subsystem on an authenticated session |
| Local terminal | portable-pty | The user's shell |
| RDP | none | `Capability::Rdp` is `NotImplemented` |
| VNC | none | `NotImplemented` |
| Telnet | none | `NotImplemented` |
| Serial | none | `NotImplemented` |
