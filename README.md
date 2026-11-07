# NIRUREMOTE

Graphical remote desktop between your own machines, over Tailscale and SSH, with no open port.

**Status: alpha** · Part of the [NIRUX](https://github.com/nirucon/nirux) suite · [MIT License](LICENSE)

## About

NIRUREMOTE lists every machine in your Tailscale tailnet, shows whether remote
desktop is actually installed and ready on it, and opens a real graphical
session. Linux/X11 hosts are served by **x11vnc** attached to the existing
session and carried **inside the SSH connection itself**: no VNC port exists on
the host, the client, the LAN or the tailnet. Windows targets open with RDP and
macOS targets with VNC when those services are enabled on the target.

Why it is built this way:

- **Tailscale is the network.** No relay, account, cloud inbox or public port.
- **SSH is the control plane.** Remote checks use a fixed command
  (`niruremote probe --json`), never a shell string.
- **No port at all.** `connect` runs `x11vnc -inetd` on the SSH channel; on the
  client the viewer uses a private one-shot Unix socket. Nothing listens.
- **Truthful states.** "Online" is not "ready"; every peer gets a precise state
  and NIRUREMOTE says when the remote screen is locked.
- **Temporary by default.** A session lives exactly as long as the SSH channel.

Release 0.2.0 was a security and reliability release after the
[0.2 review](docs/REVIEW-0.2.md). Both ends must run 0.2.0 or newer: a 0.1.x
agent shows as `update-needed`, and a 0.1.x client is told to update.

## Features

- Peer list from `tailscale status`, with parallel, bounded SSH probes and a
  private cache.
- One-command connect to Linux/X11 hosts through TigerVNC over SSH.
- Windows targets through FreeRDP (`xfreerdp3`, `xfreerdp` or `wlfreerdp`) and
  macOS targets through TigerVNC.
- `enable`/`disable`: a password-protected, loopback-only listener for manual
  SSH tunnels from Windows or macOS clients.
- Sessions end when the viewer closes, on Ctrl+C, when the client is killed or
  the network path dies (within 30 s on the host, about 15 s on the client).
- Desktop notification on the host when a session starts and ends.
- The lock screen is respected: a viewer of a locked session sees NIRULOCK and
  needs the account password. NIRUREMOTE never unlocks.
- Hardened SSH (`BatchMode=yes`, `StrictHostKeyChecking=yes`, no forwarding or
  connection sharing from user config); x11vnc runs as the logged-in user, never
  root; Tailscale Funnel is never used. Full threat model in
  [docs/SECURITY.md](docs/SECURITY.md).
- `setup` finds and, with `--run`, installs x11vnc and a viewer with a visible
  sudo/doas prompt.

| Role | Platform | Status in 0.2.0 |
|---|---|---|
| Host (share a desktop) | Debian / Ubuntu, Arch, Void (X11) | **supported** |
| Client (open a session) | Debian / Ubuntu, Arch, Void | **supported** |
| Target | Windows with Remote Desktop enabled | **supported** (RDP client) |
| Target | macOS with Screen Sharing enabled | **supported** (VNC client) |
| Host | Windows / macOS | not configured by NIRUREMOTE; use their native RDP/VNC server and a client |
| Host | Wayland | reported as `unsupported-wayland` |
| Client | Windows / macOS | no NIRUREMOTE build; use the documented SSH tunnel + VNC viewer, or connect to Windows/macOS targets from a Linux client |

## Requirements

- **Tailscale** installed and logged in on both ends. NIRUREMOTE only reads the
  local Tailscale daemon for discovery; it does not change your tailnet.
- **An OpenSSH server on every Linux host** and **key-based SSH login** from the
  client; password-only SSH shows `ssh-denied`. Tailscale SSH is not required.
- **A real X11 session on the host** (NIRUX is X11). A headless server shows
  `no-session`; NIRUREMOTE does not create a virtual desktop.
- Host: `x11vnc`, and `niruremote` in a non-interactive SSH `PATH`
  (`/usr/local/bin`, the default install, works).
- Client: `ssh`, a TigerVNC viewer, and FreeRDP for Windows targets.
- x86_64 Linux. The release is a statically linked binary, the same for Debian,
  Arch and Void; no Rust is needed.

| Distribution | Host backend | VNC viewer | Windows RDP client |
|---|---|---|---|
| Debian / Ubuntu | `x11vnc` | `tigervnc-viewer` (`xtigervncviewer`) | `freerdp3-x11` (Debian 12: `freerdp2-x11`) |
| Arch Linux | `x11vnc` | `tigervnc` (`vncviewer`) | `freerdp` (`xfreerdp`/`wlfreerdp`) |
| Void Linux | `x11vnc` | `tigervnc` | `freerdp` |
| Fedora | `x11vnc` | `tigervnc` | `freerdp` |

Per-distribution commands, including SSH server and Tailscale, are in
[docs/INSTALL-DISTROS.md](docs/INSTALL-DISTROS.md). Testing status per platform
is in [docs/TESTING-STATUS.md](docs/TESTING-STATUS.md).

## Installation

### With NIRUX

```sh
nirux extras install niruremote
niruremote setup --run
nirux extras update            # later, for the extras you selected
```

### Standalone

```sh
git clone https://github.com/nirucon/niruremote
cd niruremote
./install.sh              # verified release binary into /usr/local/bin (uses sudo or doas)
./install.sh --user       # into ~/.local/bin instead
./install.sh --prefix DIR # into DIR
./install.sh --build      # force a local cargo build
niruremote setup          # check x11vnc and the viewer
niruremote setup --run    # install them with a visible sudo/doas prompt
```

The default install writes `/usr/local/bin/niruremote` and a
`~/.local/bin/niruremote` symlink so NIRUX can detect it. With `--user`, a
non-interactive SSH command must still find `niruremote` in its `PATH`. The
installer installs only the binary: NIRUREMOTE never pipes `curl` into a shell,
changes Tailscale ACLs, opens a firewall port or installs anything silently.

Install it on **every** machine: hosts need the agent, clients the client.

## Usage

On the host (the machine you want to reach), nothing has to run:

```sh
niruremote probe            # is there an X11 session? is x11vnc installed?
```

On the client (the machine you sit at):

```sh
niruremote peers --refresh  # Tailscale machines + verified readiness
niruremote connect desktop
```

`connect` checks the remote agent and X11 session over SSH, opens the VNC viewer
on a private one-shot Unix socket, runs `ssh HOST niruremote stream` (which
starts `x11vnc -inetd` on the SSH channel) and ends everything when the session
ends. You do not need `enable` for `connect`. Details:
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

### Commands

```text
niruremote probe [--json]              inspect the local agent
niruremote status [--json]             alias for probe
niruremote enable                      password-protected loopback listener for
                                       manual Windows/macOS tunnels
niruremote disable                     stop the listener and end live sessions
niruremote stream                      serve one session on stdin/stdout
                                       (run by connect over SSH)
niruremote setup [--run]               check/install x11vnc and a VNC viewer
niruremote peers [--json] [--refresh] [--timeout N]
niruremote connect MACHINE [--print] [--timeout N]
niruremote doctor                      local diagnostics
niruremote version
```

`--timeout` bounds the SSH probe (1–30 seconds, default 3). `--print` shows the
planned SSH/VNC/RDP path without starting a session.

### Peer states

| State | Meaning |
|---|---|
| `ready` | Verified: agent 0.2+, X11 session and x11vnc — connect any time |
| `update-needed` | The machine runs NIRUREMOTE 0.1.x; update it there |
| `not-installed` | SSH works but `niruremote` was not found remotely |
| `backend-missing` | Agent installed but `x11vnc` is missing |
| `no-session` | Agent installed but no active X11 desktop was found |
| `unsupported-wayland` | Agent installed, but the session is Wayland |
| `host-key-untrusted` | SSH host key is not in `known_hosts`; verify it first |
| `ssh-denied` | SSH authentication failed |
| `ssh-timeout` / `ssh-unavailable` / `unreachable` | SSH path failed |
| `offline` | Tailscale reports the peer offline; last-known state is kept |
| `client-ready` | Windows/macOS target is reachable and a local client exists |
| `no-local-client` | Target is reachable but the local RDP/VNC client is missing |
| `unsupported` | Mobile/other peer, not a desktop host |

The detail column adds `screen locked (nirulock)`, active sessions and an
enabled manual-tunnel listener when they apply. Without `--refresh`, `peers`
reads the cache; with `--refresh` it probes online peers in parallel. Offline
peers are never probed.

### Windows and macOS

Windows (RDP) and macOS (Screen Sharing/VNC) are supported as **targets** when
their own remote service is enabled; NIRUREMOTE never configures them. Connecting
**from** Windows or macOS to a NIRUX host uses `niruremote enable` and an SSH
tunnel. Step-by-step setup for all of this is in
[docs/WINDOWS-MACOS.md](docs/WINDOWS-MACOS.md).

## Configuration

There is no configuration file. What matters is SSH: set up key-based login from
the client to every Linux host, against the Tailscale DNS name that
`niruremote peers` shows, and verify the host key once in a terminal:

```sh
ssh-copy-id user@desktop.tailnet.ts.net
ssh user@desktop.tailnet.ts.net niruremote probe --json
```

A passphrase-protected key must be loaded in an agent. NIRUREMOTE looks for a
working one itself (`SSH_AUTH_SOCK`, then `gcr/ssh`, `openssh_agent`,
`keyring/ssh` and `ssh-agent`). The full guide is in [docs/SSH.md](docs/SSH.md).

## Files and paths

| Path | Purpose |
|---|---|
| `/usr/local/bin/niruremote` | Program (default install) |
| `~/.local/bin/niruremote` | Symlink for NIRUX, or the program with `--user` |
| `${XDG_CACHE_HOME:-~/.cache}/niruremote/peers.json` | Peer cache |
| `${XDG_STATE_HOME:-~/.local/state}/niruremote/` | State (`enable` listener, sessions) |
| `$XDG_RUNTIME_DIR/niruremote/` | Viewer sockets and session records |

Directories are `0700`, files `0600`.

## Troubleshooting

```sh
niruremote doctor
niruremote probe --json
niruremote peers --refresh --timeout 5
```

- **`no-session`**: nobody is logged in to an X11 desktop on the host. Log in at
  the machine first.
- **`backend-missing`**: install `x11vnc` on the host.
- **`host-key-untrusted`**: run `ssh HOST` once in a terminal and verify the
  fingerprint. NIRUREMOTE never accepts a new key automatically.
- **`ssh-denied`**: unlock the SSH key or add it to the remote `authorized_keys`.
- **`not-installed`**: install NIRUREMOTE in `/usr/local/bin` on the host so a
  non-interactive SSH command can find it.
- **`update-needed`**: the host runs 0.1.x; update it there
  (`nirux extras update` or `./install.sh`).
- **Locked screen**: you see NIRULOCK; type the account password in the viewer,
  exactly as at the machine.
- **Viewer missing**: install `tigervnc-viewer` (Debian) or `tigervnc`
  (Arch/Void) on the client.
- **Black screen**: the host display may be off, the lid closed or the session
  on another VT. Wake the display and try again.
- **RDP/VNC target unreachable**: see [docs/WINDOWS-MACOS.md](docs/WINDOWS-MACOS.md).

Known limits in 0.2.0: Linux/X11 hosts only (Wayland is reported, not
supported); `enable` does not survive logout or reboot; no audio, file transfer,
printer/USB redirection or monitor picker (use NIRUSHARE for files); an active
session costs about 1.5 % of one core for x11vnc's polling, nothing when idle;
the NIRU Launcher view is specified in [docs/LAUNCHER.md](docs/LAUNCHER.md) but
not shipped yet ([ROADMAP.md](ROADMAP.md)).

## Uninstall

```sh
./uninstall.sh            # remove /usr/local/bin/niruremote and the symlink
./uninstall.sh --user     # remove ~/.local/bin/niruremote
./uninstall.sh --purge    # also remove the state and cache directories
```

## Building from source

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release --locked
./tests/smoke-local.sh    # full Linux path with fake x11vnc, SSH and viewer
```

`rust-version` is 1.85 (Debian 13's rustc); CI builds and tests on it.
Environment overrides for tests and debugging:

```text
NIRUREMOTE_STATE_DIR   state directory
NIRUREMOTE_CACHE_DIR   peer cache directory
NIRUREMOTE_DISPLAY     force an X11 display
NIRUREMOTE_XAUTHORITY  force an Xauthority file
NIRUREMOTE_X11VNC      force the x11vnc binary (fake backend in tests)
NIRUREMOTE_SSH         force the ssh binary (fake SSH in tests)
NIRUREMOTE_VIEWER      force the local VNC viewer
NIRUREMOTE_TAILSCALE   force the tailscale binary
NIRUREMOTE_RUNTIME_DIR runtime directory (sockets, session records)
NIRUREMOTE_IDLE_SECS   dead-path watchdog, 2-600 s (default 30)
NIRUREMOTE_NOTIFY=off  no desktop notifications from `stream`
```

More: [specification](SPECIFICATION.md), [security policy](SECURITY.md),
[release checklist](docs/RELEASE.md), [acceptance](docs/ACCEPTANCE.md),
[changelog](CHANGELOG.md).

## Status, disclaimer and how it's made

**Status: alpha.** Linux-to-Linux sessions over Tailscale are tested on real Debian, Arch and Void machines; Windows and macOS targets are documented but not yet tested against real machines, and the launcher integration is still to come.

NIRUX and every niru* project are made by me, Nicklas Rudolfsson
([@nirucon](https://github.com/nirucon)), for me. They are tailored to my own
needs, preferences and workflows, on my own machines. If they fit yours too,
that's a nice bonus.

- **Free and open source.** Anyone is free to use, study, change and share
  it under the [MIT License](LICENSE).
- **No guarantees.** There is no warranty of any kind. Use it at your own
  risk, read what you install and keep backups of anything that matters.
- **How it's made.** Some of these projects I built from scratch by hand. In
  more recent releases I have used Claude, Anthropic's AI, to do much of the
  heavy lifting under my direction: I decide what to build and how it should
  behave, and I test it on my own machines. It saves me a lot of time, and
  the result is quite OK.
- **Alpha, beta, dev, and maybe forever.** Most things here are alpha, beta
  or dev, and may stay that way. Maybe one day there will be a real stable
  release, but given how fast development moves, and how I am as a person,
  maybe never :)

| Label | Meaning |
|---|---|
| dev | Under active construction; may not build, install or work yet. |
| alpha | Works on my machines, but incomplete or rough; expect breaking changes. |
| beta | In daily use on my machines and mostly complete; rough edges remain. |

## License

MIT License, see [LICENSE](LICENSE). Copyright (c) 2026 Nicklas Rudolfsson.
