# Security policy

NIRUREMOTE is a remote-access tool. Read [docs/SECURITY.md](docs/SECURITY.md)
before enabling it on a shared or multi-user machine.

- The default Linux path binds x11vnc to `127.0.0.1` and reaches it through an
  authenticated SSH tunnel. Do not expose the VNC port directly.
- SSH host keys must be verified. NIRUREMOTE never accepts a new host key
  automatically.
- Do not use the current `-nopw` loopback model on a host with untrusted local
  users; per-session VNC credentials are on the roadmap.
- Keep private hostnames, IPs, fingerprints and logs out of public issues.

Report security issues privately to the repository owner rather than opening a
public issue with host details.
