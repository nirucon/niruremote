# Changelog

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
