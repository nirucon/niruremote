# Bare-metal acceptance

Use two Linux/NIRUX machines with X11. Call them **host** (the desktop to reach)
and **client** (the machine you sit at). Do not use a headless server as the
host for the first pass.

Before bare metal, 0.1.6 was exercised in disposable Debian 12 and Arch Linux
containers with a real Xvfb server, real x11vnc and the real TigerVNC viewer.
Void package names were verified against void-packages, but the full Void
runtime test is still required below.

## Platform matrix

Record each platform you actually ran. The minimum target is Debian/Ubuntu,
Arch and Void for Linux, plus Windows and macOS as RDP/VNC targets.

- [ ] Debian or Ubuntu — host and client
- [ ] Arch Linux — host and client
- [ ] Void Linux — host and client (runit)
- [ ] Windows — RDP target from a Linux client
- [ ] macOS — Screen Sharing/VNC target from a Linux client
- [ ] Windows or macOS as a client to a NIRUX host via the manual SSH tunnel

## 0. Install

On both machines:

```sh
cd /path/to/niruremote
./install.sh
niruremote version          # must print: niruremote 0.1.6
niruremote setup --run      # Debian: x11vnc + tigervnc-viewer
                            # Arch:  x11vnc + tigervnc
                            # Void:  x11vnc + tigervnc
```

On the host, confirm a real X11 session:

```sh
niruremote probe
```

Expected: `session: x11 :0`, `x11vnc: installed`, `state: inactive`.

- [ ] `probe` finds `:0` and an Xauthority file.
- [ ] `probe --json` includes `"schema": 1` and `"state": "inactive"`.

## 1. Client discovery

On the client:

```sh
niruremote peers --refresh --timeout 5
```

- [ ] The host appears as `inactive` (not `not-installed`).
- [ ] Offline peers still appear with `offline` and last-seen age.
- [ ] Android/iOS peers show `unsupported`.
- [ ] A Windows peer with RDP enabled shows `client-ready` (if FreeRDP is
      installed locally).
- [ ] A macOS peer with Screen Sharing enabled shows `client-ready` (if a VNC
      viewer is installed locally).
- [ ] A peer with an untrusted host key shows `host-key-untrusted`, not a
      false success.

If the host shows `host-key-untrusted`, run `ssh HOST` once in a terminal and
verify the fingerprint. Do not disable host-key checking.

## 2. Enable and disable on the host

On the host:

```sh
niruremote enable
ss -ltnp | grep 127.0.0.1
```

- [ ] The listener is on `127.0.0.1:<port>`, never `0.0.0.0` or a Tailscale IP.
- [ ] `niruremote probe` reports `state: ready`, `owner: enable`.
- [ ] From the client, `niruremote peers --refresh` reports `ready`.
- [ ] `niruremote disable` stops it and the port disappears.
- [ ] `niruremote disable` again is harmless.

## 3. Graphical connection

On the client:

```sh
niruremote connect HOST
```

- [ ] A real TigerVNC window opens and shows the host desktop.
- [ ] Mouse and keyboard work; Super/keyboard grabs behave as expected.
- [ ] Closing the viewer prints `disconnected`.
- [ ] Ctrl+C in the terminal prints `interrupted` and closes the viewer.
- [ ] After exit, the client has no leftover `ssh -L` process:

```sh
pgrep -af 'ssh -N -L'
```

- [ ] After the host session was temporary (`connect` without `enable`), the
      host has no leftover x11vnc:

```sh
pgrep -af x11vnc
niruremote probe       # state: inactive
```

- [ ] With `enable` already active, `connect` reuses the listener and does not
      start a second x11vnc.

## 4. Platform-specific checks

### Debian / Ubuntu

- [ ] `tigervnc-viewer` provides `xtigervncviewer`; the client detects it.
- [ ] Optional Windows client: `freerdp3-x11` (or `freerdp2-x11` on Debian 12).
- [ ] `openssh-server` is enabled and key-based SSH works.

### Arch Linux

- [ ] `tigervnc` provides the viewer; the client detects it.
- [ ] `openssh`/`sshd` is enabled and key-based SSH works.
- [ ] Optional: `freerdp` provides the RDP client.

### Void Linux

- [ ] `x11vnc`, `tigervnc` and `openssh` are installed with `xbps-install`.
- [ ] `sshd` is enabled under runit.
- [ ] `enable` starts x11vnc without a systemd service.
- [ ] Optional: `freerdp` provides the RDP client.

### Windows target

- [ ] Remote Desktop is enabled and NLA is required.
- [ ] TCP 3389 is reachable over Tailscale; `client-ready` appears.
- [ ] `niruremote connect WINHOST` opens FreeRDP and accepts the account.
- [ ] Windows firewall rule applies to the Tailscale profile.
- [ ] A Windows Home edition is reported as unsupported by Windows itself.

### macOS target

- [ ] Screen Sharing is enabled with a VNC password.
- [ ] TCP 5900 is reachable over Tailscale; `client-ready` appears.
- [ ] `niruremote connect MACHOST` opens the VNC viewer and accepts the password.
- [ ] The macOS firewall allows Screen Sharing.

### Windows/macOS client to a NIRUX host

- [ ] On the host, `niruremote enable` prints a loopback port.
- [ ] The manual `ssh -N -L 5900:127.0.0.1:<PORT> user@HOST` tunnel works.
- [ ] A VNC viewer connects to `127.0.0.1:5900` and shows the host desktop.
- [ ] Closing the viewer and the SSH tunnel leaves no x11vnc on the host when
      the session was temporary (`connect` or `disable`).

## 5. Failure paths

- [ ] Offline host: `niruremote connect HOST` says offline with last seen.
- [ ] Host with no X session: `no active X11 desktop session`.
- [ ] Host without x11vnc: `x11vnc is missing` plus the install hint.
- [ ] Host without NIRUREMOTE: `NIRUREMOTE is not installed`.
- [ ] Wrong SSH key: `SSH authentication failed`.
- [ ] No local viewer: a clear message with the package name.
- [ ] Windows with RDP disabled: `RDP port 3389 is closed`.
- [ ] macOS with Screen Sharing disabled: port 5900 closed.

## 6. Security checks

- [ ] `ss -ltnp` on the host shows x11vnc only on `127.0.0.1`.
- [ ] `tailscale status` / another tailnet machine cannot reach the VNC port
      directly; only the SSH tunnel works.
- [ ] No Tailscale Funnel/Serve configuration was created.
- [ ] `niruremote doctor` does not print secrets or the Xauthority contents.
- [ ] `~/.local/state/niruremote` is `0700` and its files are `0600`.
- [ ] The host lock screen is visible through VNC and is not bypassed.
- [ ] Logging out on the host stops the session.

## 7. Record

Write the result here or in the release notes:

```text
date:
client host / version:
target host / version:
NIRUX version:
Tailscale version:
session discovery source (logind/who/proc/environment):
result:
notes:
```
