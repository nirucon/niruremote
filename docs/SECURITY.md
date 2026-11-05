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

Since 0.2.0, `connect` gives x11vnc **no listening socket at all**. The
client runs the fixed command `ssh HOST niruremote stream`; the agent starts
`x11vnc -inetd` on a socketpair and relays it over that SSH channel. Nothing is
reachable from the LAN, the tailnet, or another local user on either machine.
On the client, the viewer connects through a Unix socket in a 0700 directory
that accepts exactly one connection from the same uid and is unlinked before
any desktop data flows.

`enable` (for manual Windows/macOS tunnels) is the only listener. It binds
`127.0.0.1` only (no IPv6, no `[::1]:5900`) and requires a random 8-character
VNC password generated per `enable`, handed to x11vnc in a 0600 `rm:` file and
shown once in the terminal. VNC authentication is weak (DES, 8 characters), so
`enable` is opt-in and documented as the weaker mode.

## Host keys

`ssh` runs with `StrictHostKeyChecking=yes` and `BatchMode=yes`, plus
`ClearAllForwardings=yes`, `ForwardAgent=no`, `ForwardX11=no`,
`ControlMaster=no`/`ControlPath=none` and `RemoteCommand=none`, so a user
`~/.ssh/config` cannot add listeners or keep a session alive after disconnect. A new or changed
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
- A desktop notification (notify-send/NIRUNOTIFY) announces session start and
  end; a NIRUBAR indicator is still planned.
- Verified natively on 0.2.0: connecting to a session locked by NIRULOCK shows
  the lock screen, pointer/key input reaches NIRULOCK, and the lock stays
  (same NIRULOCK process before and after).

## Enablement

- `connect` is temporary: x11vnc lives inside the SSH channel and stops when
  the channel closes, the client is killed, or the path goes quiet for 30 s.
- `enable` is the explicit opt-in that leaves a password-protected loopback
  listener running.
- `disable` stops it and ends live sessions. Logout/reboot also stops it.
- The person at the machine gets a desktop notification when a session starts
  and ends, and `niruremote probe` shows live sessions.
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
- **Malicious tailnet member:** there is no VNC port to reach. They would need
  SSH access and a trusted host key.
- **Local multi-user host:** `connect` opens no port; another local user has
  nothing to connect to. The `enable` listener needs its password. (0.1.x used
  an unauthenticated `-nopw` loopback port that any local uid could use; see
  REVIEW-0.2 S1.)
- **Dropped network:** the agent ends a session 30 s after the viewer stops
  sending; x11vnc never outlives a dead path for long.
- **Compromised remote agent:** it can return false JSON, but it already runs
  as the user with access to the display. Do not execute probe output as
  commands.
- **Relay latency:** DERP-relayed Tailscale links are slower. This is a
  performance issue, not a confidentiality break; Tailscale transport is still
  encrypted.

## Reporting

Keep private hostnames, IPs, fingerprints and logs out of public issues. Use a
redacted `niruremote doctor` output when asking for help.
