# NIRUREMOTE

**Graphical remote desktop for the [NIRUX](https://github.com/nirucon/nirux) suite, over Tailscale and SSH, by Nicklas Rudolfsson.**

NIRUREMOTE lets you list every machine in your Tailscale tailnet, see whether
remote desktop is actually installed and ready on it, and open a real graphical
session without opening any port. Linux/X11 hosts use **x11vnc** attached to
the existing session, carried **inside the SSH connection itself**: no VNC
port exists on the host, the client, the LAN or the tailnet. Windows hosts can be opened with RDP and macOS hosts with
VNC when those services are enabled on the target.

**Current release: 0.2.0** — a security and reliability release after the
[0.2 review](docs/REVIEW-0.2.md). The command-line client and agent are
complete; the NIRULAUNCHER view is the next integration step. Both ends must
run 0.2.0 or newer: a 0.1.x agent is shown as `update-needed`, and a 0.1.x
client is told to update.

## Testing status

- **Debian 12** and **Arch Linux**: full headless X11 test passed in disposable
  containers with a real Xvfb server, real `x11vnc` and the real TigerVNC
  viewer. Arch was tested with a normal user PATH where `/usr/sbin` is absent.
- **Void Linux**: package names (`x11vnc`, `tigervnc`, `xorg-server-xvfb`,
  `xauth`, `freerdp`) were verified against the current void-packages
  templates. The official Void container image is too old to update xbps, so
  the full Void runtime test is part of the bare-metal acceptance.
- **Windows**: RDP client path and firewall/Remote Desktop instructions are
  documented; a real Windows target test is pending.
- **macOS**: Screen Sharing/VNC client path and instructions are documented; a
  real macOS target test is pending.
- **Bare metal, 0.2.0 (2026-11-05):** real TigerVNC connections over Tailscale
  between Debian 13 (othala), Arch (algiz) and Void (void) in both directions
  that apply (othala→void, void→othala, othala→algiz, void→algiz), with
  `ss` showing no new listener, x11vnc gone after disconnect, `kill -9` of the
  client and a stalled network path, and NIRULOCK staying locked.
- **Bare metal is still required.** Run [docs/ACCEPTANCE.md](docs/ACCEPTANCE.md)
  before relying on it for daily access.

## Why this shape

- **Tailscale is the network.** No relay, account, cloud inbox or public port.
- **SSH is the control plane.** Remote capability checks use a fixed command
  (`niruremote probe --json`), never a shell string.
- **No port at all.** `connect` runs `x11vnc -inetd` on the SSH channel. On the
  client the viewer uses a private one-shot Unix socket. Nothing listens.
- **Truthful states.** "Online" is not "ready". NIRUREMOTE distinguishes
  `ready`, `not-installed`, `update-needed`, `no-session`, `backend-missing`,
  `host-key-untrusted`, `ssh-denied`, `offline` and `unsupported`, and says
  when the remote screen is locked.
- **Temporary by default.** A session lives exactly as long as the SSH
  channel: closing the viewer, Ctrl+C, killing the client or losing the
  network ends it. `enable` is only for manual Windows/macOS tunnels.

## Platform support

| Role | Platform | Status in 0.2.0 |
|---|---|---|
| Host (share a desktop) | Debian / Ubuntu, Arch, Void (X11) | **supported** |
| Client (open a session) | Debian / Ubuntu, Arch, Void | **supported** |
| Target | Windows with Remote Desktop enabled | **supported** (RDP client) |
| Target | macOS with Screen Sharing enabled | **supported** (VNC client) |
| Host | Windows / macOS | not configured by NIRUREMOTE; use their native RDP/VNC server and a client |
| Host | Wayland | reported as `unsupported-wayland` |
| Client | Windows / macOS | no NIRUREMOTE build; use the documented SSH tunnel + VNC viewer, or connect to Windows/macOS targets from a Linux client |

The release is a statically linked x86_64 Linux binary. Debian, Arch and Void
all use the same binary; no Rust installation is required on the target.

### Package names at a glance

| Distribution | Host backend | VNC viewer | Windows RDP client |
|---|---|---|---|
| Debian / Ubuntu | `x11vnc` | `tigervnc-viewer` (`xtigervncviewer`) | `freerdp3-x11` (Debian 12: `freerdp2-x11`) |
| Arch Linux | `x11vnc` | `tigervnc` (`vncviewer`) | `freerdp` (`xfreerdp`/`wlfreerdp`) |
| Void Linux | `x11vnc` | `tigervnc` | `freerdp` |
| Fedora | `x11vnc` | `tigervnc` | `freerdp` |

`niruremote setup` prints the exact command for the current distribution, and
`niruremote setup --run` executes it with a visible sudo/doas prompt.

### What every setup needs

- **Tailscale** installed and logged in on both ends. NIRUREMOTE uses the local
  Tailscale daemon only for discovery; it does not change your tailnet.
- **An SSH server on every Linux host** and **key-based SSH login** from the
  client. NIRUREMOTE runs `ssh` with `BatchMode=yes`; password-only SSH will
  show `ssh-denied`, not a hanging prompt.
- **A VNC viewer on the client** (TigerVNC). For Windows targets, **FreeRDP**.
- **A real X11 session on the host.** A headless server shows `no-session`;
  NIRUREMOTE does not create a virtual desktop.

## Requirements

On a Linux machine that will be shared (the host):

- X11 desktop session (NIRUX is X11; Wayland is not supported)
- `x11vnc`
- `sshd`, reachable over Tailscale
- NIRUREMOTE installed in a non-interactive SSH `PATH` (the default
  `/usr/local/bin` install is recommended)

On the Linux machine you sit at (the client):

- `tailscale` and `ssh`
- NIRUREMOTE
- TigerVNC viewer (`vncviewer`) for Linux/macOS targets, and/or FreeRDP
  (`xfreerdp3`, `xfreerdp` or `wlfreerdp`) for Windows targets

## Install on Debian, Arch and Void

### Debian / Ubuntu

```sh
# Host and client:
sudo apt-get update
sudo apt-get install -y x11vnc tigervnc-viewer openssh-server
sudo systemctl enable --now ssh

# Tailscale, if it is not already installed:
#   install it with the official Tailscale instructions for Debian/Ubuntu,
#   then: sudo tailscale up

# Optional, only when connecting to Windows targets:
sudo apt-get install -y freerdp3-x11
# Debian 12 provides freerdp2-x11 instead; NIRUREMOTE detects xfreerdp.
```

- `tigervnc-viewer` installs `xtigervncviewer`; NIRUREMOTE detects it.
- `freerdp3-x11` installs `xfreerdp3`; the older `freerdp2-x11` installs
  `xfreerdp`.
- The release binary is static; `./install.sh` does not need Rust.

### Arch Linux

```sh
sudo pacman -Syu --needed x11vnc tigervnc openssh

# Host: enable the SSH server.
sudo systemctl enable --now sshd

# Tailscale, if it is not already installed:
sudo pacman -S --needed tailscale
sudo systemctl enable --now tailscaled
sudo tailscale up

# Optional, only when connecting to Windows targets:
sudo pacman -S --needed freerdp
```

- `tigervnc` provides the viewer; NIRUREMOTE detects `vncviewer` and
  `xtigervncviewer`.
- `freerdp` provides the RDP client.

### Void Linux

```sh
sudo xbps-install -Syu
sudo xbps-install -S x11vnc tigervnc openssh

# Host: enable the SSH server under runit (skip if already enabled).
sudo ln -s /etc/sv/sshd /var/service/ 2>/dev/null || true

# Tailscale, if it is not already installed:
sudo xbps-install -S tailscale
sudo ln -s /etc/sv/tailscaled /var/service/ 2>/dev/null || true
sudo tailscale up

# Optional, only when connecting to Windows targets:
sudo xbps-install -S freerdp
```

- Void uses runit. NIRUREMOTE does not install a system service. `niruremote
  enable` starts a detached user-owned x11vnc until `disable`, logout or
  reboot.
- If you use `doas` instead of `sudo`, `install.sh` and `setup --run` fall back
  to `doas` automatically. If neither is installed, run the printed command as
  root.
- `tigervnc` provides the viewer; `x11vnc` provides the host backend.
- `freerdp` provides the RDP client.

### Install the NIRUREMOTE binary (all three distributions)

```sh
git clone https://github.com/nirucon/niruremote
cd niruremote
./install.sh              # verified release binary, or a fresh local build
./install.sh --build      # force a local cargo build
./install.sh --user       # install to ~/.local/bin instead of /usr/local/bin
```

Release assets are statically linked x86_64 Linux binaries. The default
install writes `/usr/local/bin/niruremote` and a `~/.local/bin/niruremote`
symlink for NIRUX extras detection. It uses `sudo` when available and falls back
to `doas`; `--user` avoids privilege escalation but a non-interactive SSH command
must then find `niruremote` in its `PATH`. If the target is offline, use
`./install.sh --build` with Rust or copy the verified binary manually.

Then verify the local packages:

```sh
niruremote setup
niruremote setup --run    # visible package command with a sudo prompt
```

NIRUREMOTE does not pipe `curl` into a shell, does not change Tailscale ACLs,
does not open a firewall port and does not install anything silently.

### NIRUX extras

The extras row is on `nirux` main and will be part of the next NIRUX release.
Once your installed `nirux` has the row:

```sh
nirux extras select niruremote
nirux extras install niruremote
niruremote setup --run
```

Until then, use `./install.sh` from this repository as shown above.

### SSH key access (Linux hosts)

The remote probe and connection are non-interactive. Set up key-based SSH from
the client to every Linux host **before** using `niruremote peers --refresh`:

```sh
# On the client, if you do not already have an SSH key:
ssh-keygen -t ed25519 -C "$USER@$(hostname)"

# Copy the public key to the host:
ssh-copy-id user@HOST

# Verify that a non-interactive command works:
ssh HOST niruremote probe --json
```

Use the **Tailscale DNS name that `niruremote peers` shows** (for example
`desktop.tailnet.ts.net`) or the Tailscale IPv4 address (`tailscale ip -4`
on the host). NIRUREMOTE probes that exact host name, so a key trusted only under
the short name `desktop` still produces `host-key-untrusted` under the FQDN.
Run `ssh-copy-id` and `ssh` against the FQDN once:

```sh
ssh-copy-id user@desktop.tailnet.ts.net
ssh user@desktop.tailnet.ts.net niruremote probe --json
```

Verify the host-key fingerprint the first time. If `ssh HOST niruremote probe
--json` fails, fix SSH first; NIRUREMOTE will report `host-key-untrusted`,
`ssh-denied` or `ssh-timeout` rather than prompting.

Tailscale SSH is **not** required or assumed. NIRUREMOTE uses the normal
OpenSSH server on the Tailscale address.

If your key is passphrase-protected, load it into an agent before using
NIRUREMOTE. The non-interactive probe uses `BatchMode=yes` and cannot prompt:

```sh
eval "$(ssh-agent -s)"
ssh-add ~/.ssh/id_ed25519
ssh HOST niruremote probe --json
```

In a NIRUX graphical session, start the agent from the session (or use your
keyring) so the launcher and terminal inherit `SSH_AUTH_SOCK`.

NIRUREMOTE also looks for a working agent itself: it checks the current
`SSH_AUTH_SOCK`, then `gcr/ssh`, `openssh_agent`, `keyring/ssh` and `ssh-agent`,
and uses the first socket that actually has identities. This handles the common
NIRUX/GNOME Keyring case where the session points at an empty
`/run/user/UID/openssh_agent` while the unlocked key is in
`/run/user/UID/gcr/ssh`.

## Windows target (RDP)

NIRUREMOTE connects **to** Windows; it does not enable or configure Windows.
Do this on the Windows machine:

1. Install Tailscale for Windows and sign in. Note the machine name and
   Tailscale IPv4 address (`tailscale ip -4` in PowerShell).
2. Open **Settings → System → Remote Desktop** and turn **Remote Desktop** on.
   Add the account you will use. Keep **Require devices to use Network Level
   Authentication** enabled.
3. Allow Remote Desktop through Windows Defender Firewall. If the Tailscale
   adapter is on a Private profile, the standard **Remote Desktop - User Mode
   (TCP-In)** rule normally applies. If it does not, allow that rule for the
   Private profile. Never forward 3389 from your router.
4. On the Linux client, install FreeRDP (see the Debian/Arch/Void commands
   above).
5. On the Linux client:

   ```sh
   niruremote peers --refresh
   niruremote connect winhost
   ```

   The peer should show `client-ready` when port 3389 is reachable and a local
   RDP client exists. `client-ready` only checks reachability and the local
   client; it does not validate your Windows password. FreeRDP will prompt for
   the account and password.
6. FreeRDP 3 (`xfreerdp3`) uses `/cert:tofu` on first use. FreeRDP 2
   (`xfreerdp`) keeps its own certificate prompt. If the Windows peer shows
   `unreachable`, check:

   ```powershell
   tailscale status
   Test-NetConnection <tailscale-ip> -Port 3389
   ```

   Also confirm Remote Desktop is on and the firewall rule applies to the
   Tailscale profile.
7. Windows Home editions do not provide the RDP **host** service. Use a
   Pro/Enterprise edition, or a different remote-access product. NIRUREMOTE
   cannot add an RDP server to Windows Home.

## macOS target (VNC / Screen Sharing)

NIRUREMOTE connects **to** macOS; it does not enable or configure macOS.

1. Install Tailscale for macOS and sign in. Note the machine name and Tailscale
   IPv4 address.
2. Open **System Settings → General → Sharing → Screen Sharing** and turn it
   on. Open the information panel, enable **VNC viewers may control screen with
   password**, and set a strong VNC password. Allow the macOS user account that
   may share the screen.
3. Allow Screen Sharing in the macOS firewall (System Settings → Network →
   Firewall → Options). Never forward 5900 from your router.
4. On the Linux client, install TigerVNC (see the Debian/Arch/Void commands
   above).
5. On the Linux client:

   ```sh
   niruremote peers --refresh
   niruremote connect machost
   ```

   The peer should show `client-ready` when port 5900 is reachable and a local
   VNC viewer exists. macOS may ask for the VNC password and/or the allowed
   macOS account.
6. If the peer shows `unreachable`, check Screen Sharing, the firewall and:

   ```sh
   nc -vz <tailscale-ip> 5900
   ```

   NIRUREMOTE never enables Screen Sharing or changes the macOS firewall.

## Connecting from Windows or macOS to a NIRUX host

NIRUREMOTE is a Linux client. To reach a NIRUX/Linux host from a Windows or
macOS machine, use the standard SSH client plus a VNC viewer against the
`enable` listener, which is loopback-only and password-protected.

On the NIRUX host:

```sh
niruremote enable
niruremote probe --json
```

`enable` prints the port (for example `43123`) and a one-time VNC password.
The listener is `127.0.0.1` only; the SSH tunnel is the only ingress, and the
viewer must also give the password. Run `niruremote disable` when done.

On Windows (PowerShell with OpenSSH) or macOS (Terminal), create a local tunnel
and keep it open:

```sh
ssh -N -L 5900:127.0.0.1:<REMOTE_PORT> user@desktop
```

For example, if the host reported `43123`:

```sh
ssh -N -L 5900:127.0.0.1:43123 user@desktop
```

Then connect a VNC viewer to `127.0.0.1:5900`:

- Windows: TigerVNC Viewer for Windows, RealVNC Viewer, or another VNC client.
- macOS: TigerVNC Viewer, or the built-in Screen Sharing client with
  `vnc://127.0.0.1:5900`.

Use the actual port that `enable` printed, not a hardcoded remote port. The
viewer asks for the VNC password. The local `5900` only needs to be free on the
Windows/macOS client. Verify the SSH host key fingerprint before accepting it.
Linux clients do not need any of this: `niruremote connect` opens no port.

## Quick start

On the host (the machine you want to reach):

```sh
niruremote probe            # is there an X11 session? is x11vnc installed?
```

Nothing else is needed on the host: no service, no open port.

On the client (the machine you sit at):

```sh
niruremote peers --refresh  # Tailscale machines + verified readiness
niruremote connect desktop
```

`connect` will:

1. check the remote agent and X11 session over SSH (and tell you if the
   screen is locked);
2. open the VNC viewer on a private one-shot Unix socket;
3. run `ssh HOST niruremote stream`, which starts `x11vnc -inetd` on the SSH
   channel — the viewer's connection *is* the SSH channel's stdin/stdout;
4. end everything when the viewer closes, you press Ctrl+C, the client is
   killed, or the network path dies (within 30 s on the host, ~15 s on the
   client).

You do **not** need `enable` for `connect`. The host shows a desktop
notification when a session starts and ends.

## Commands

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

## Peer states

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
enabled manual-tunnel listener when they apply. The cache lives in `~/.cache/niruremote/peers.json`. Without `--refresh` the
client reads the cache; with `--refresh` it probes online peers in parallel and
updates the cache. Offline peers are never probed.

## Linux/X11 connection

```text
client                                       host
  ssh host niruremote probe --json  ------->  (read-only)
  vncviewer /run/user/UID/niruremote/view-PID.sock
     | accepted once (same uid), socket unlinked
  ssh -T ... -- host niruremote stream ----->  niruremote stream
     stdin/stdout = viewer connection            socketpair <-> x11vnc -inetd
```

There is no `ssh -L` and no listening socket. The agent relays the SSH channel
to x11vnc and ends the session on EOF, on a signal, when x11vnc exits, or after
30 s without traffic from the viewer (x11vnc pings every 5 s). The client's
SSH and viewer children have `PR_SET_PDEATHSIG`, so a killed client closes the
channel and the host stops x11vnc immediately.

## Windows and macOS: what NIRUREMOTE does

The detailed setup is in **Windows target (RDP)** and **macOS target (VNC /
Screen Sharing)** above. In short:

- **Windows**: checks TCP 3389 over Tailscale and starts `xfreerdp3`,
  `xfreerdp` or `wlfreerdp` with `/dynamic-resolution` and clipboard. FreeRDP 3
  uses `/cert:tofu`; FreeRDP 2 keeps its own certificate prompt. Enable Remote
  Desktop on Windows first.
- **macOS**: checks TCP 5900 and starts TigerVNC against the host. Enable
  Screen Sharing and the VNC password in macOS first.

NIRUREMOTE does not remotely enable RDP, change Windows firewall rules or turn
on macOS Screen Sharing. Those are OS-level administration actions with their
own security model. The state shows `client-ready` when the port is reachable
and a local client is installed.

## Security model

- x11vnc runs as the logged-in user, never as root.
- `connect` opens no port anywhere. The SSH login (key, verified host key) is
  the authentication; the viewer endpoint on the client is a one-shot Unix
  socket checked for the same uid.
- `enable` is a `127.0.0.1`-only listener with a random per-enable VNC
  password, given to x11vnc through a 0600 file it deletes, never via argv.
- SSH uses `BatchMode=yes`, `StrictHostKeyChecking=yes`, a short
  `ConnectTimeout`, and clears forwardings, agent/X11 forwarding and
  connection sharing from user config. Unknown or changed host keys fail
  closed; verify them in a terminal first.
- The remote commands are fixed (`niruremote probe --json`,
  `niruremote stream`); host names are validated and passed after `--`.
- The agent attaches only to the current user's X11 session, never to another
  user or a login greeter.
- The lock screen is respected. A remote viewer of a locked session sees
  NIRULOCK; only the account password unlocks it. NIRUREMOTE never unlocks.
- The host gets a desktop notification when a session starts and ends;
  `niruremote disable` ends every session.
- Tailscale Funnel is never used. No public endpoint is created.
- State and cache files are private (`0700` directories, `0600` files).
- `setup --run` shows the package command and inherits the terminal, so sudo
  can prompt. No password is captured or logged.

See [docs/SECURITY.md](docs/SECURITY.md) for the full threat model.

## NIRUX and NIRULAUNCHER integration

The intended UI is the existing Tailscale view in NIRULAUNCHER: every machine,
OS, online state and a verified badge, with contextual **Connect**, **Allow
connections**, **Stop allowing**, **Install**, **Check again** and **Details**
actions. The CLI already returns the machine-readable data that view needs.

The exact integration plan, JSON fields and refresh strategy are in
[docs/LAUNCHER.md](docs/LAUNCHER.md). The NIRUX extras row is already on
`nirux` main; the remaining launcher work is tracked in [ROADMAP.md](ROADMAP.md).

## Troubleshooting

```sh
niruremote doctor
niruremote probe --json
niruremote peers --refresh --timeout 5
```

- **`no-session`**: the machine is headless or nobody is logged in to an X11
  desktop. Log in at the machine first; headless servers do not get a virtual
  desktop.
- **`backend-missing`**: install `x11vnc` on the host.
- **`host-key-untrusted`**: run `ssh HOST` once in a terminal and verify the
  fingerprint. NIRUREMOTE will not accept a new key automatically.
- **`ssh-denied`**: unlock the SSH key or add it to the remote
  `authorized_keys`.
- **`update-needed`**: the host runs NIRUREMOTE 0.1.x. Update it there
  (`nirux extras update`, or `./install.sh`).
- **Locked screen**: you see NIRULOCK; type the account password in the viewer
  to unlock, exactly as at the machine.
- **`not-installed`**: install NIRUREMOTE in `/usr/local/bin` on the host so a
  non-interactive SSH command can find it.
- **Viewer missing**: install `tigervnc-viewer` (Debian/Ubuntu) or `tigervnc`
  (Arch/Void) on the client.
- **Black screen**: the host display may be DPMS-off, the lid may be closed, or
  the X session may be on another VT. Wake the display and try again.
- **RDP/VNC target unreachable**: enable Remote Desktop (Windows) or Screen
  Sharing (macOS), and confirm the tailnet ACL allows the port.

Environment overrides used by the test suite and for debugging:

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

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release --locked
```

`rust-version` is 1.85 (Debian 13's rustc); CI builds and tests on it.
The local smoke test uses a fake x11vnc, a fake SSH and a fake viewer so the
full Linux path (including the dead-path watchdog and `kill -9`) can be
exercised without an X server:

```sh
./tests/smoke-local.sh
```

## Limits in 0.2.0

- Linux/X11 only for hosts. Wayland is reported, not supported.
- `enable` does not survive a reboot or logout.
- No audio, file transfer, printer/USB redirection or multi-monitor picker.
  Use NIRUSHARE for files.
- An active session costs what x11vnc's screen polling costs (about 1.5 % of
  one core and ~600 wakeups/s measured on a laptop); nothing runs when no
  session is open.
- Windows/macOS are targets only when their own remote service is already
  enabled. No remote OS configuration.
- NIRULAUNCHER/NIRUBAR integration is specified but not shipped.

## License

MIT. Copyright 2026 Nicklas Rudolfsson.
