# Architecture

NIRUREMOTE has two roles in one binary: a **client** that discovers peers and
opens a viewer, and a **host agent** that serves the current X11 session over
the SSH channel it was started on. The client never parses a shell transcript; the agent returns
versioned JSON.

## Module map

| Module | Responsibility |
|---|---|
| `main.rs` | Command parsing and help |
| `tailscale.rs` | `tailscale status --json` discovery and peer model |
| `client.rs` | Peer cache, parallel probe, state mapping, connect orchestration |
| `ssh.rs` | Hardened SSH options, bounded probe, the `stream` command |
| `agent.rs` | `probe`, `stream` relay + watchdog, `enable`, `disable`, `setup`, `doctor` |
| `events.rs` | Blocking child-exit and signal events (no polling) |
| `session.rs` | X11 display, Xauthority and screen-locker discovery |
| `x11vnc.rs` | Backend detection, `-inetd` and password-listener argument sets |
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
        +-- bind private viewer endpoint (Unix socket 0600 in 0700 dir)
        +-- vncviewer /run/user/UID/niruremote/view-PID.sock
        +-- accept ONE connection, check peer uid, unlink the socket
        |
        +-- ssh -T ... -- host niruremote stream
                stdin/stdout = the viewer connection
                      |
                      host: niruremote stream
                            socketpair <-> x11vnc -inetd (no listener)
```

## Process lifetime

- `connect` owns the viewer and the SSH child and blocks on an event channel:
  viewer exit, SSH exit, or a signal (self-pipe). No timer polling.
- Every SSH/viewer child gets `PR_SET_PDEATHSIG(SIGTERM)`. If `connect` is
  killed (even with SIGKILL), they die; the SSH channel closes; the remote
  agent sees EOF and stops x11vnc.
- The remote `stream` agent relays bytes and ends on EOF, x11vnc exit, a
  signal, or 30 s without bytes from the viewer (x11vnc `-ping 5` keeps a live
  viewer talking). That last rule covers a network path that died silently,
  which sshd may not notice for hours.
- x11vnc is the agent's child with PDEATHSIG; killing the agent kills it.
- `enable` starts a detached, password-protected loopback listener; `disable`
  stops it and ends live `stream` sessions.

## State

| Path | Contents |
|---|---|
| `$XDG_STATE_HOME/niruremote/state.json` | runtime PID, port, display, owner |
| `$XDG_STATE_HOME/niruremote/x11vnc.log` | private x11vnc log, rotated at 1 MiB |
| `$XDG_STATE_HOME/niruremote/state.lock` | short-lived operation lock |
| `$XDG_RUNTIME_DIR/niruremote/` (0700) | viewer sockets, live session records, SSH stderr, transient `enable` password file |
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
- `x11vnc -inetd` over the SSH channel means no port at all: nothing for
  another local user, the LAN or the tailnet to reach, and VNC's weak
  authentication is never the boundary for `connect`.
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
