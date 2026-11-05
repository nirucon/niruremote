# Changelog

## 0.2.0 — 2026-11-05

Security and reliability release after the [0.2 review](docs/REVIEW-0.2.md).
Both ends must run 0.2.0: a 0.1.x host shows `update-needed`, and a 0.1.x
client is told to update.

- **No listening port for `connect` (S1, S2).** 0.1.x ran
  `x11vnc -nopw -forever -shared` on a loopback port (plus an unadvertised
  `[::1]:5900`) and forwarded it with `ssh -L`; any local user on the host or
  the client could take full control of the desktop without a password. Now
  the client runs `ssh HOST niruremote stream`, the agent starts
  `x11vnc -inetd` on a socketpair and relays it over the SSH channel, and the
  viewer connects through a one-shot, same-uid Unix socket. Nothing listens.
- **Dropped networks end the session (C1).** The agent stops x11vnc 30 s after
  the viewer goes silent (x11vnc pings every 5 s). 0.1.x kept x11vnc running as
  long as sshd kept the dead connection (hours).
- **`enable` needs a password (S3).** Random 8-character VNC password per
  `enable`, passed through a 0600 `rm:` file, printed once; loopback only,
  one viewer at a time, no IPv6 listener.
- **Hardened SSH (S4).** `ClearAllForwardings`, `ForwardAgent=no`,
  `ForwardX11=no`, `ControlMaster=no`/`ControlPath=none`, `RemoteCommand=none`,
  `EscapeChar=none`; host after `--`. Host-key checking stays strict.
- **x11vnc hardening (S5).** `-nocmds -noremote -norc -noipv6`.
- **Terminal-escape sanitising (S6)** of tailnet names and remote-agent text.
- **Locked screens (L1).** `probe` reports the locker; `connect` says the screen
  is locked. Verified natively that NIRULOCK stays locked.
- **Session visibility (U2).** Desktop notification on start/end, live session
  count in `probe`, `disable` ends live sessions.
- **States (U1).** A host is `ready` whenever a connection is possible (no
  `enable` needed); new `update-needed`; clearer connect progress messages.
- **Energy (E1).** No polling while a session runs: the client went from 5 to 0
  wakeups/s and the host agent from 10 to 0.6 wakeups/s (blocking events, one
  watchdog deadline). Probes no longer open VNC connections to check a port.
- **Builds on rustc 1.85 (B1).** Let-chains removed; `rust-version = "1.85"`;
  CI builds and tests on 1.85 and runs the smoke test.
- Stale runtime files of a killed client are removed on the next `connect`.
- `ensure` remains only to answer 0.1.x clients with `upgrade-required`.

## 0.1.6 — 2026-11-03

- Removed personal host names, Tailscale names, IPs and user identifiers from
  the public README, launcher examples, specification and tests.
- The public docs now use generic `HOST`, `desktop`, `winhost` and `machost`
  examples. The live-verification note names only Debian 13.
- No functional change to the agent, SSH tunnel or viewer.

## 0.1.5 — 2026-11-03

- Detect a working SSH agent automatically. If `SSH_AUTH_SOCK` points at an
  empty agent, try `gcr/ssh`, `openssh_agent`, `keyring/ssh` and `ssh-agent`,
  and use the first socket that actually has identities.
- This fixes the common NIRUX/GNOME Keyring case where the session environment
  points at an empty `openssh_agent` while the unlocked key is in `gcr/ssh`.

## 0.1.4 — 2026-11-02

- Find `x11vnc` and the VNC viewer in `/usr/sbin` even when it is not in the
  user PATH. This is required on Arch Linux, where both live in `/usr/sbin`.
- Exit cleanly instead of panicking when stdout is a closed pipe.

## 0.1.3 — 2026-11-02

- Debian, Arch and Void package plans are tested explicitly.
- `setup --run` and the installers fall back to `doas` when `sudo` is absent.
- Detect `wlfreerdp3` on FreeRDP 3 systems.
- README now has step-by-step Debian/Ubuntu, Arch, Void, Windows RDP and macOS
  Screen Sharing instructions, plus Windows/macOS clients to a NIRUX host.

## 0.1.2 — 2026-11-02

- Require a Running Tailscale backend before listing or connecting.
- Treat an online peer with a cached offline state as "not checked yet".
- Add `-T` to SSH probe/tunnel and drain capped command output safely.
- Use `/cert:tofu` only for FreeRDP 3; older FreeRDP keeps its certificate
  prompt.
- Verify `who` displays against an existing X socket.
- Add `rust-version` metadata and a root security policy.

## 0.1.1 — 2026-11-02

- The installer now prefers the verified static release binary on source
  installs, so a target does not need Rust. `--build` still forces a local
  build.
- Release assets are statically linked x86_64 Linux binaries.

## 0.1.0 — 2026-11-02

First working core.

- Tailscale peer discovery with offline peers, OS, last seen and identity.
- Read-only SSH capability probe with a truthful state model.
- Linux/X11 host agent using x11vnc on loopback.
- SSH local-forward connection with TigerVNC/Remmina.
- Temporary `connect` and explicit `enable`/`disable`.
- Windows RDP and macOS VNC client paths.
- Peer cache, bounded parallel refresh and JSON output.
- `setup`, `doctor`, install/uninstall scripts and local smoke tests.
- Documentation for the NIRUX launcher integration and bare-metal acceptance.

Known limits: X11 only, no audio/file transfer, no persistent user service, no
Wayland, no remote Windows/macOS configuration, and no NIRULAUNCHER view yet.
