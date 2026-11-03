# Architecture

NIRUREMOTE has two roles in one binary: a **client** that discovers peers and
opens a viewer, and a **host agent** that exposes the current X11 session on
loopback. The client never parses a shell transcript; the agent returns
versioned JSON.

## Module map

| Module | Responsibility |
|---|---|
| `main.rs` | Command parsing and help |
| `tailscale.rs` | `tailscale status --json` discovery and peer model |
| `client.rs` | Peer cache, parallel probe, state mapping, connect orchestration |
| `ssh.rs` | Bounded SSH probe, held `ensure` channel, local forward |
| `agent.rs` | `probe`, `enable`, `disable`, `ensure`, `setup`, `doctor` |
| `session.rs` | X11 display and Xauthority discovery |
| `x11vnc.rs` | Backend detection, version, start/stop and package hint |
| `state.rs` | Runtime state, lock, peer cache paths |
| `viewer.rs` | TigerVNC/Remmina/FreeRDP detection and launch |
| `util.rs` | Timeout-bounded process I/O, atomic private writes, signals |
| `json.rs` | Versioned JSON structs |

## Data flow

```text
client
  tailscale status --json
        |
        v
  peers/cache -------- ssh probe -------- host
        |                                   |
        |                             niruremote probe
        |                                   |
        v                                   v
  NIRULAUNCHER / CLI                  session + x11vnc state
        |
        v
  connect HOST
        |
        +-- ssh host niruremote ensure --json --hold
        |         |
        |         +-- starts/reuses x11vnc on 127.0.0.1
        |         +-- prints one JSON line, then holds stdin
        |
        +-- ssh -N -L 127.0.0.1:LOCAL:127.0.0.1:REMOTE host
        |
        +-- vncviewer 127.0.0.1::LOCAL
```

## Process lifetime

- `connect` owns the tunnel child, the held `ensure` child and the viewer child.
- Every SSH/viewer child gets `PR_SET_PDEATHSIG(SIGTERM)`. If `connect` dies,
  they die.
- The remote `ensure` child watches its stdin. When the SSH channel closes, it
  reads EOF, stops x11vnc and removes state.
- `enable` starts x11vnc detached with `setsid`; it is intentionally not tied
  to the terminal. `disable` is the explicit stop.
- `RemoteSession::drop` closes stdin, sends SIGTERM to the SSH child and reaps
  it. `Tunnel::drop` does the same for the forward.

This gives two independent cleanup paths: parent-death signals for the local
SSH/viewer processes, and stdin EOF for the remote agent.

## State

| Path | Contents |
|---|---|
| `$XDG_STATE_HOME/niruremote/state.json` | runtime PID, port, display, owner |
| `$XDG_STATE_HOME/niruremote/x11vnc.log` | private x11vnc log, rotated at 1 MiB |
| `$XDG_STATE_HOME/niruremote/state.lock` | short-lived operation lock |
| `$XDG_CACHE_HOME/niruremote/peers.json` | last probe result per Tailscale node |

`state.json` is written atomically and only after x11vnc has opened its port.
A PID that no longer exists is removed on the next probe. A listener that is
still starting is reported as `starting`, never `ready`.

## Peer cache

The cache key is the Tailscale node identity, not the hostname. A rename keeps
the entry; a reinstall with a new node key gets a new entry. `checked_at` makes
staleness visible. `peers` without `--refresh` is a local, fast operation;
`--refresh` is the explicit network step.

## Why x11vnc + SSH

- NIRUX is X11. x11vnc attaches to the existing session rather than creating a
  new login.
- SSH is already the suite's remote administration path and is enabled on the
  fleet. Tailscale SSH is not required.
- Loopback plus `ssh -L` avoids a new tailnet port and avoids VNC's weak
  authentication becoming an Internet-facing surface.
- A native viewer gives better keyboard handling than a browser and keeps the
  launcher thin.

RDP and VNC are used for Windows/macOS targets because those operating systems
provide the host service themselves. NIRUREMOTE only detects reachability and
launches a client.

## Deliberate non-goals

- No custom cryptography, relay or rendezvous service.
- No shell strings for remote commands.
- No automatic host-key trust.
- No desktop on headless servers by default.
- No second file-transfer stack; NIRUSHARE owns that.
