# Bare-metal acceptance

Use two Linux/NIRUX machines with X11. Call them **host** (the desktop to reach)
and **client** (the machine you sit at). Do not use a headless server as the
host for the first pass.

0.2.0 was run natively between Debian 13, Arch and Void hosts; the record is in
[REVIEW-0.2.md](REVIEW-0.2.md). Both ends must run 0.2.0 or newer.

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
niruremote version          # must print: niruremote 0.2.0
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
- [ ] `probe --json` includes `"schema": 1`, `"state": "inactive"` and
      `"ssh-stdio"` in `supported_modes`.
- [ ] With the screen locked, `probe` shows `screen: locked (nirulock)`.

## 1. Client discovery

On the client:

```sh
niruremote peers --refresh --timeout 5
```

- [ ] The host appears as `ready` (not `not-installed`); a 0.1.x host shows
      `update-needed`.
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

## 2. Graphical connection

On the host, before and during the session:

```sh
ss -ltnp | grep -v -e '127.0.0.' -e '\[::1\]'   # non-loopback listeners
ss -ltnpx | grep x11vnc                          # must stay empty
```

On the client:

```sh
niruremote connect HOST
```

- [ ] A real TigerVNC window opens and shows the host desktop.
- [ ] Mouse and keyboard work; Super/keyboard grabs behave as expected.
- [ ] The host shows a "Remote desktop — session started" notification.
- [ ] During the session x11vnc has **no** TCP or Unix listener on the host,
      and the client has none either (`ss -ltnpx`).
- [ ] Closing the viewer prints `disconnected`; Ctrl+C prints `interrupted`.
- [ ] After exit: no `x11vnc` on the host, no `ssh … niruremote stream` on the
      client, `niruremote probe` shows `sessions: 0`.
- [ ] `kill -9` of the client process: x11vnc on the host is gone within
      seconds.
- [ ] Dropped path (suspend the client or `kill -STOP` its ssh child): x11vnc
      on the host is gone within ~30 s.
- [ ] `niruremote disable` on the host ends a running session.

## 3. Locked screen

Lock the host (NIRULOCK), then connect.

- [ ] `connect` says the screen is locked.
- [ ] The viewer shows the lock screen, not the desktop.
- [ ] Mouse/key input does not unlock it; the same NIRULOCK process is still
      running afterwards. Only the account password typed into NIRULOCK
      unlocks (PAM), exactly as at the machine.

## 3b. Manual-tunnel listener (`enable`)

- [ ] `niruremote enable` prints a port and a password.
- [ ] `ss -ltnp` shows x11vnc only on `127.0.0.1:<port>` (no `[::1]:5900`).
- [ ] A viewer without the password is refused; the password is not in
      `ps`, the log or `probe --json`.
- [ ] `niruremote disable` stops it; running it again is harmless.

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
- [ ] `connect` works without any service (runit + elogind).
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

- [ ] On the host, `niruremote enable` prints a loopback port and password.
- [ ] The manual `ssh -N -L 5900:127.0.0.1:<PORT> user@HOST` tunnel works.
- [ ] A VNC viewer connects to `127.0.0.1:5900`, asks for the password and
      shows the host desktop.
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

- [ ] `connect` creates no listener on either machine; `enable` only on
      `127.0.0.1`.
- [ ] No Tailscale Funnel/Serve configuration was created.
- [ ] `niruremote doctor` does not print secrets or the Xauthority contents.
- [ ] `~/.local/state/niruremote` is `0700` and its files are `0600`.
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
