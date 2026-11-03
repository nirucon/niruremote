# NIRUREMOTE specification

This document is the behavior contract for 0.1.x. UI labels and exact CLI
arguments may evolve, but the safety and truthfulness rules must not.

## 1. Scope

Deliver:

1. Tailscale peer discovery on the client, including offline peers and OS.
2. A read-only remote capability probe that distinguishes installed, inactive,
   ready, missing backend, no X11 session, Wayland, SSH failure and unknown.
3. A Linux/X11 host agent that starts x11vnc on loopback and stops cleanly.
4. An SSH-tunneled graphical connection from the client.
5. Client-only RDP/VNC paths for Windows and macOS targets.
6. Explicit `enable`/`disable` and temporary `connect`.
7. Machine-readable JSON for NIRULAUNCHER and future NIRUBAR integration.
8. A peer cache with timestamps and bounded parallel refresh.
9. Install, diagnostics, documentation and local smoke tests.

Not in 0.1.x:

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
| `ready` | x11vnc is running and its loopback port is open |
| `starting` | a PID exists but the port is not open yet |
| `inactive` | X11 session and backend exist; nothing is running |
| `backend-missing` | X11 session exists, x11vnc is absent |
| `no-session` | no active X11 session for this user |
| `unsupported-wayland` | a Wayland session was found |
| `error` | unexpected failure; `last_error` explains |

`enabled` means a runtime state record exists. `running` means the loopback port
is actually open. An offline/stale PID must be cleaned up, not displayed as
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

### `enable`

- Acquires the state lock.
- Refuses to start without an X11 session or x11vnc.
- Chooses a free loopback port.
- Starts x11vnc detached, with stdout/stderr in a private log.
- Writes runtime state (`pid`, `port`, `display`, `auth_file`, `started_at`,
  `owner=enable`).
- Does not survive logout/reboot in 0.1.x.

### `disable`

- Acquires the state lock.
- Sends SIGTERM and then SIGKILL after a grace period.
- Removes runtime state.
- Never removes the log or peer cache.

### `ensure --json --hold`

- Reuses an existing listener and returns `owned=false`.
- Otherwise starts x11vnc as a held child with `PR_SET_PDEATHSIG`.
- Prints exactly one compact JSON line to stdout.
- Waits while stdin is open. On stdin EOF or child exit, stops x11vnc and
  removes state.
- This is the temporary connection path used by `connect`; it leaves nothing
  behind after the SSH channel closes.

### `ensure --json` without `--hold`

- Starts or reuses a detached listener and returns. It is for scripted setup;
  `connect` uses `--hold`.

## 7. Connection flow (Linux/X11)

1. Resolve the peer by Tailscale host, short name, DNS name or IP.
2. Probe the remote agent. Refuse with a specific error for `backend-missing`,
   `no-session`, `unsupported-wayland`, `host-key-untrusted`, `ssh-denied` and
   `not-installed`.
3. Run `ensure --json --hold` over SSH and read one JSON line.
4. Allocate a local loopback port and open `ssh -N -L`.
5. Start the best available viewer:
   - TigerVNC: `vncviewer -SecurityTypes None -Shared 127.0.0.1::PORT`
   - Remmina: `remmina -c vnc://127.0.0.1:PORT`
6. Wait for the viewer. On viewer exit, Ctrl+C, SIGTERM or SIGHUP, close the
   tunnel and the ensure channel. PDEATHSIG and stdin EOF are the two cleanup
   mechanisms.
7. No orphan ssh, x11vnc or viewer process may remain.

If the peer is the local machine, skip SSH and the tunnel; start the local agent
and point the viewer at `127.0.0.1:PORT`.

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

- x11vnc runs as the session user, binds `127.0.0.1`, uses `-nopw` only because
  the SSH tunnel is the authentication boundary, and never runs as root.
- SSH host keys fail closed.
- No public listener, no Funnel, no ACL changes, no firewall changes.
- No secret in argv, logs or JSON. The current design does not use a VNC
  password; a future per-session password must not appear in argv.
- The local user must be able to see that a remote session is possible
  (`enabled`) and active (`ready`). A NIRUBAR indicator is planned.
- `setup --run` is explicit and visible. No `curl | sh` bootstrap.

## 11. Acceptance for 0.1.6

- Unit tests for Tailscale parsing, RFC3339 parsing, host validation and
  distribution hints.
- Local smoke test with fake x11vnc/SSH/viewer covering enable, probe, ensure,
  disable, connect and cleanup.
- On bare metal, two NIRUX/X11 machines:
  - `probe` finds `:0` and the right Xauthority;
  - `enable` listens only on `127.0.0.1`;
  - `connect` shows the real desktop;
  - closing the viewer or Ctrl+C leaves no x11vnc/ssh process;
  - `disable` stops the listener;
  - an untrusted host key fails closed;
  - an offline peer shows `offline`;
  - Windows RDP and macOS VNC show `client-ready` only when reachable.
