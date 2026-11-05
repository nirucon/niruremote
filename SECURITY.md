# Security policy

NIRUREMOTE is a remote-access tool. Read [docs/SECURITY.md](docs/SECURITY.md)
before enabling it on a shared or multi-user machine.

- `connect` runs x11vnc in `-inetd` mode on the authenticated SSH channel: no
  port is opened on the host or the client. `enable` (manual tunnels) is a
  loopback-only listener with a per-enable password. Never expose it directly.
- SSH host keys must be verified. NIRUREMOTE never accepts a new host key
  automatically.
- A locked screen stays locked; NIRUREMOTE never unlocks or bypasses it.
- See [docs/REVIEW-0.2.md](docs/REVIEW-0.2.md) for the 0.2 security review.
- Keep private hostnames, IPs, fingerprints and logs out of public issues.

Report security issues privately to the repository owner rather than opening a
public issue with host details.
