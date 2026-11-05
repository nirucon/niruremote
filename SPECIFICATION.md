# NIRUREMOTE specification

This document is the behavior contract for 0.2.x. UI labels and exact CLI
arguments may evolve, but the safety and truthfulness rules must not.

## 1. Scope

Deliver:

1. Tailscale peer discovery on the client, including offline peers and OS.
2. A read-only remote capability probe that distinguishes installed, inactive,
   ready, missing backend, no X11 session, Wayland, SSH failure and unknown.
3. A Linux/X11 host agent that serves x11vnc over the SSH channel itself
   (`stream`, no listening port) and stops cleanly.
4. An SSH-carried graphical connection from the client.
5. Client-only RDP/VNC paths for Windows and macOS targets.
6. Temporary `connect`, plus an explicit password-protected `enable` listener
   for manual Windows/macOS tunnels, and `disable`.
7. Machine-readable JSON for NIRULAUNCHER and future NIRUBAR integration.
8. A peer cache with timestamps and bounded parallel refresh.
9. Install, diagnostics, documentation and local smoke tests.

Not in 0.2.x:

- Wayland host capture.
- Audio, file transfer, printer/USB redirection, multi-monitor selection.
- Remote installation on Windows or macOS.
- A persistent user service across reboot/logout.
- Public exposure, Tailscale Funnel, or any relay.
- Automatic host-key acceptance.

## 2. Peer discovery

`tailscale status --json` is the only discovery source. The client must:

- include offline peers, with `online=false` and last-seen information;
- expose hostname, DNS name, IPv4 Tailscale address, OS, user and tags;
- exclude the local machine from the normal peer list, but allow `connect` to
  resolve the local machine by short name, DNS name or IP;
- tolerate missing fields and unexpected OS values;
- bound the command with a timeout and an output cap.

Being online is never sufficient evidence that remote desktop is ready.

## 3. Remote probe

The client runs, over SSH:

```text
ssh [bounded options] HOST niruremote probe --json
```

Requirements:

- `BatchMode=yes`, `ConnectTimeout`, server-alive bounds, output cap.
- `StrictHostKeyChecking=yes`. No `StrictHostKeyChecking=no`, no automatic
  `known_hosts` modification.
- The host name is validated and passed as one argv element. No shell string.
- The remote command is fixed. Probe output is parsed as data, never executed.
- A schema version mismatch is an explicit `protocol-error`, not a guess.

The probe response contains:

```json
{
  "schema": 1,
  "agent_version": "0.1.6",
  "hostname": "desktop",
  "user": "niru",
  "os": "linux",
  "session": {
    "type": "x11",
    "display": ":0",
    "active": true,
    "source": "logind",
    "auth_file": "/home/niru/.Xauthority"
  },
  "backend": {
    "name": "x11vnc",
    "installed": true,
    "path": "/usr/bin/x11vnc",
    "version": "x11vnc: 0.9.16",
    "install_hint": "sudo apt-get install -y x11vnc tigervnc-viewer"
  },
  "state": "inactive",
  "running": false,
  "enabled": false,
  "port": null,
  "pid": null,
  "since": null,
  "mode": "loopback",
  "owner": null,
  "last_error": null,
  "supported_modes": ["ssh-tunnel"]
}
```

## 4. Local state vocabulary

| State | Contract |
|---|---|
| `ready` | the `enable` listener is running and its loopback port is open |
| `starting` | a PID exists but the port is not open yet |
| `inactive` | X11 session and backend exist; no `enable` listener (connect still works) |
| `backend-missing` | X11 session exists, x11vnc is absent |
| `no-session` | no active X11 session for this user |
| `unsupported-wayland` | a Wayland session was found |
| `error` | unexpected failure; `last_error` explains |

`enabled` means a runtime state record exists. `running` means the loopback port
is actually open (read from `/proc/net/tcp`; the probe never connects to it).
`supported_modes` contains `ssh-stdio` when `connect` can be served.
`locker` names a running screen locker; `sessions` counts live `stream`s. An offline/stale PID must be cleaned up, not displayed as
ready.

## 5. Session discovery

Order:

1. An explicit `NIRUREMOTE_DISPLAY` or `DISPLAY` with an existing X socket.
2. `loginctl` sessions for this UID with an X11 display.
3. `who` entries for this user with `:N`.
4. A `/proc` scan for an X server owned by this user.

The agent must never attach to another user's display or to a login greeter.
The Xauthority candidates are `NIRUREMOTE_XAUTHORITY`, `XAUTHORITY`,
`~/.Xauthority`, and common display-manager paths under `/run/user/UID/`.

## 6. Host agent lifecycle

### `stream` (the `connect` path, 0.2.0+)

- Run by the client as the fixed remote command `niruremote stream`.
- Refuses without an X11 session or x11vnc (message on stderr, exit 1).
- Starts `x11vnc -inetd` on one end of a socketpair: **no TCP or Unix
  listener exists**. `-rfbport 0 -noipv6 -nocmds -noremote -norc -ping 5`.
- Relays bytes between its stdin/stdout (the SSH channel) and x11vnc.
- Ends the session, stops x11vnc and removes its record when: the viewer side
  reaches EOF, x11vnc exits, SIGINT/SIGTERM/SIGHUP arrive, or no byte has come
  from the viewer for 30 s (x11vnc pings every 5 s, so a live viewer always
  answers; this catches a silently dropped network path).
- x11vnc has `PR_SET_PDEATHSIG`, so killing the agent kills it too.
- Records itself in `$XDG_RUNTIME_DIR/niruremote/session-PID.json` and sends a
  desktop notification at start and end (best effort).

### `enable`

- Acquires the state lock; refuses without an X11 session or x11vnc.
- Chooses a free loopback port, generates an 8-character random VNC password,
  writes it to a 0600 file and passes `-passwdfile rm:FILE` (x11vnc reads and
  deletes it). The password is printed once to the terminal; never in argv,
  JSON or logs.
- x11vnc: `-localhost -rfbport PORT -rfbportv6 -1 -noipv6 -forever
  -nevershared -dontdisconnect`. Only `127.0.0.1:PORT` listens.
- Writes runtime state (`owner=enable`). Does not survive logout/reboot.

### `disable`

- Acquires the state lock, stops the `enable` listener (SIGTERM, then SIGKILL)
  and ends every live `stream` session.
- Never removes the log or peer cache.

### `ensure`

- Kept only to answer 0.1.x clients: returns `state: upgrade-required` and
  exits 1. 0.2.0 never starts an unauthenticated listener.

## 7. Connection flow (Linux/X11)

1. Resolve the peer by Tailscale host, short name, DNS name or IP.
2. Probe the remote agent. Refuse with a specific error for `backend-missing`,
   `no-session`, `unsupported-wayland`, `host-key-untrusted`, `ssh-denied`,
   `not-installed`, and for agents older than 0.2.0 (`update-needed`).
3. If the probe reports a screen locker, say so: the viewer will show the lock
   screen and only the account password unlocks it.
4. Bind a private endpoint for the viewer: a Unix socket
   `$XDG_RUNTIME_DIR/niruremote/view-PID.sock` (0600 in a 0700 directory) for
   TigerVNC, or a loopback TCP port for Remmina/old viewers. It accepts exactly
   one connection, verifies the peer uid (`SO_PEERCRED` or `/proc/net/tcp`),
   then closes and unlinks the listener.
5. Start `ssh … -- HOST niruremote stream` with that connection as its stdin
   and stdout. `connect` itself copies no bytes.
6. Wait (blocking, no polling) for the viewer to exit, the SSH child to exit,
   or SIGINT/SIGTERM/SIGHUP; then stop the other side. PDEATHSIG covers SIGKILL
   of `connect`; SSH keepalives (5 s × 3) end a dead path on the client side.
7. No orphan ssh, x11vnc or viewer process may remain.

If the peer is the local machine, run `niruremote stream` locally instead of
SSH.

## 8. Windows and macOS

Windows:

- Check TCP 3389 over Tailscale (2-second bound).
- `client-ready` only if the port is reachable and a local RDP client exists.
- Spawn FreeRDP with `/v:HOST /dynamic-resolution +clipboard /cert:tofu`, or
  Remmina with `rdp://HOST`.
- Never modify Windows settings or firewall rules.

macOS:

- Check TCP 5900.
- `client-ready` only if the port is reachable and a local VNC viewer exists.
- Spawn TigerVNC against `HOST::5900` or Remmina with `vnc://HOST:5900`.
- Never enable Screen Sharing remotely.

## 9. Cache and refresh

- Cache path: `$XDG_CACHE_HOME/niruremote/peers.json` (0600).
- Every cached entry has `checked_at`.
- Normal `peers` reads the cache and current Tailscale state. It does not run
  SSH probes.
- `peers --refresh` probes online Linux/Windows/macOS peers in parallel, writes
  the cache atomically, and returns the merged result.
- Offline peers are never probed. Their last-known state remains visible with
  an `offline` override and age.
- A failed probe is cached as its explicit failure state, not as "unknown
  success". A later `--refresh` replaces it.

## 10. Security requirements

- x11vnc runs as the session user and never as root. For `connect` it has no
  listening socket at all (`-inetd` over the authenticated SSH channel); the
  `enable` listener binds `127.0.0.1` only and requires a per-enable password.
- SSH host keys fail closed.
- No public listener, no Funnel, no ACL changes, no firewall changes.
- No secret in argv, logs or JSON. The `enable` password reaches x11vnc via a
  0600 `rm:` file and is printed only to the terminal that ran `enable`.
- SSH runs with `ClearAllForwardings`, `ForwardAgent=no`, `ForwardX11=no`,
  `ControlPath=none`, `RemoteCommand=none` so user configuration cannot add
  listeners or keep a session alive.
- Text from the tailnet or a remote agent is stripped of control characters
  before it is printed.
- The lock screen is never bypassed or unlocked; viewer input reaches the
  locker like local input.
- The local user must be able to see that a remote session is possible
  (`enabled`) and active (`ready`). A NIRUBAR indicator is planned.
- `setup --run` is explicit and visible. No `curl | sh` bootstrap.

## 11. Acceptance for 0.2.0

- Unit tests for Tailscale parsing, RFC3339 parsing, host validation and
  distribution hints.
- Local smoke test with fake x11vnc/SSH/viewer covering enable (password file),
  probe, ensure refusal, connect, idle watchdog, kill -9 and disable.
- On bare metal, two NIRUX/X11 machines:
  - `probe` finds `:0` and the right Xauthority;
  - `connect` creates no listener on either end; `enable` listens only on
    `127.0.0.1` and refuses unauthenticated viewers;
  - `connect` shows the real desktop;
  - closing the viewer, Ctrl+C, kill -9 of the client or a dropped path
    leaves no x11vnc/ssh process;
  - a locked screen stays locked;
  - `disable` stops the listener;
  - an untrusted host key fails closed;
  - an offline peer shows `offline`;
  - Windows RDP and macOS VNC show `client-ready` only when reachable.
