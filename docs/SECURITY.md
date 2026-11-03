# Security model

NIRUREMOTE is remote control. Treat it as an access service, not a convenience
feature. The default design keeps the network surface as small as possible.

## Trust boundaries

1. **Tailscale** decides which devices can reach each other and under which
   tailnet ACLs.
2. **SSH** authenticates the client and protects the control channel.
3. **The local user session** is the authority that starts x11vnc.
4. **The viewer** is the only process that receives screen content and sends
   input.

x11vnc is not exposed to Tailscale by default. It binds `127.0.0.1`; the viewer
reaches it through an SSH local forward. `-nopw` is used because the tunnel is
the authentication boundary. This is safe only while all of these remain true:

- the listener is loopback-only;
- SSH host keys are verified;
- the host is not shared with untrusted local users;
- no firewall or proxy forwards the loopback port.

If you change any of those, add a real per-session VNC credential first.

## Host keys

`ssh` runs with `StrictHostKeyChecking=yes` and `BatchMode=yes`. A new or changed
host key fails closed. The UI must show `host-key-untrusted`; it must not offer a
one-click "accept anyway". The user verifies the fingerprint in a terminal.

## SSH keys

NIRUREMOTE never asks for or stores an SSH password. It relies on the user's
existing SSH agent/key configuration. A locked key or missing key is reported as
`ssh-denied`, with a terminal fallback.

## Agent privileges

The agent runs as the logged-in user. It does not use root, PolicyKit or a
setuid helper. x11vnc runs as the same user and uses that user's Xauthority.
The agent refuses to attach when it cannot prove the X display belongs to the
current user.

## Session privacy

- The lock screen is not bypassed. A remote viewer sees the lock screen if the
  session is locked.
- The agent never unlocks the session, reads the clipboard, logs keystrokes or
  stores screen content.
- x11vnc logs contain connection metadata and errors, not screen pixels.
- NIRUNOTIFY/NIRUBAR connection indicators are planned so the local user can
  see that a remote session exists.

## Enablement

- `connect` is temporary: x11vnc is held by the SSH channel and dies when the
  channel closes.
- `enable` is the explicit opt-in that leaves a loopback listener running.
- `disable` stops it. Logout/reboot also stops the 0.1.x service.
- There is no default-on listener after installation.

## What is not exposed

- No Tailscale Funnel.
- No public HTTP, VNC or RDP port.
- No relay or rendezvous account.
- No remote package installation without a visible terminal and sudo prompt.
- No `curl | sh` bootstrap.

## Windows and macOS

For Windows, NIRUREMOTE checks port 3389 and starts FreeRDP. RDP security,
Network Level Authentication, account policy and firewall configuration remain
the responsibility of the Windows host. `/cert:tofu` is trust-on-first-use; a
managed environment should pin the certificate.

For macOS, NIRUREMOTE checks port 5900 and starts a VNC viewer. Screen Sharing
permissions and the macOS firewall remain the host's responsibility.

NIRUREMOTE does not enable, configure or weaken either service remotely.

## Threat notes

- **Stolen SSH key:** the attacker can start a session if the user is logged in
  to X11. This is the same blast radius as SSH access plus `DISPLAY`/Xauthority
  access. Use SSH key passphrases, an agent, and Tailscale ACLs.
- **Malicious tailnet member:** cannot reach the VNC port because it is loopback
  only. They would need SSH access and a trusted host key.
- **Local multi-user host:** another local user could connect to a loopback
  `-nopw` VNC port. Do not use the current credential model on a shared
  workstation. The per-session password item in the roadmap addresses this.
- **Compromised remote agent:** it can return false JSON, but it already runs
  as the user with access to the display. Do not execute probe output as
  commands.
- **Relay latency:** DERP-relayed Tailscale links are slower. This is a
  performance issue, not a confidentiality break; Tailscale transport is still
  encrypted.

## Reporting

Keep private hostnames, IPs, fingerprints and logs out of public issues. Use a
redacted `niruremote doctor` output when asking for help.
