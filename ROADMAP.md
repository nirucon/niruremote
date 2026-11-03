# NIRUREMOTE roadmap

Status after **0.1.6** (2026-11-02). This is a working list, not a promise that
every item belongs in the next release. Security and access-affecting work comes
before cosmetics.

## Done in 0.1.6

- [x] Rust CLI with `probe`, `status`, `enable`, `disable`, `ensure`,
  `peers`, `connect`, `setup`, `doctor` and `version`.
- [x] Tailscale discovery with offline peers, OS, last seen, tags and user.
- [x] Read-only SSH capability probe with bounded timeout, host-key
  verification and a truthful state model.
- [x] x11vnc lifecycle on loopback, with `PR_SET_PDEATHSIG` and stdin-EOF
  cleanup.
- [x] SSH local-forward connection and TigerVNC/Remmina launch.
- [x] Peer cache with `--refresh` and bounded parallel probing.
- [x] Windows RDP and macOS VNC client paths.
- [x] `setup` with Debian/Ubuntu, Arch, Void, Fedora and openSUSE package hints.
- [x] `install.sh`, `uninstall.sh`, unit tests and a fake-backend smoke test.
- [x] README, specification, security and launcher integration docs.
- [x] Static x86_64 release `v0.1.6` with `SHA256SUMS`.
- [x] NIRUX extras row for 0.1.6 on `nirux` main.
- [x] Platform instructions for Debian/Ubuntu, Arch, Void, Windows RDP,
  macOS Screen Sharing and Windows/macOS clients to a NIRUX host.

## P0 — before calling this a NIRUX extra

- [ ] Run the bare-metal acceptance matrix in [docs/ACCEPTANCE.md](docs/ACCEPTANCE.md)
      on two real NIRUX/X11 machines. Record the result.
- [ ] Verify session discovery on the real NIRUX session manager (logind,
      startx and `who` fallbacks) and record which path was used.
- [ ] Add a NIRUX launcher view (`nirulauncher --remote` or the Tailscale view)
      using `niruremote peers --json` and `niruremote connect`.
- [ ] Add NIRUBAR: a small screen indicator while a local remote session is
      active, and a "Remote desktop" action in the Tailscale detail.
- [ ] Verify `nirux extras install niruremote`, `extras update` and uninstall
      on a real NIRUX release that includes the 0.1.6 extras row.

## P1 — security and lifecycle

- [ ] Per-session VNC password in a 0600 file, passed to x11vnc without argv,
      and to the viewer without shell interpolation. Keep `-localhost`.
- [ ] A user service (`systemd --user` and runit) so `enable` survives
      logout/reboot when explicitly requested. Default remains off.
- [ ] A visible local indicator and NIRUNOTIFY on connection/disconnection,
      with a Disconnect action.
- [ ] Bounded audit history: source Tailscale/SSH address, start/stop, errors.
      No screen content, keystrokes or secrets.
- [ ] Refuse to attach when the X server belongs to another user; test the
      `/proc` ownership check on multi-user machines.
- [ ] Add a `disconnect`/session list for the local supervisor (today `connect`
      is a foreground process).
- [ ] Make `enable` optionally auto-disable after a chosen idle time.

## P2 — usability and performance

- [ ] Launcher: background `--refresh`, incremental "checking…" rows, cached
      results first, no UI freeze on a slow peer.
- [ ] Launcher: per-machine action sheet instead of one flat row per action.
- [ ] Launcher: search by machine name and recent connections; pin favourites.
- [ ] Viewer quality presets (LAN/direct vs DERP relay), fullscreen and
      keyboard-grab hints.
- [ ] Multi-monitor selection through XRandR/NIRUDISPLAY.
- [ ] Clipboard opt-in and a clear warning for secrets.
- [ ] Keep the host display awake while a client is attached; wake it on
      connect. Report "no active display" for a closed laptop.
- [ ] Direct-tailnet mode (no SSH tunnel) for clients that cannot tunnel,
      opt-in only, with Tailscale ACLs and a per-session credential.
- [ ] Optional browser mode: x11vnc + noVNC through a local tunnel, or
      Tailscale Serve with app-level authentication. Never Tailscale Funnel.
- [ ] Diagnose DERP relay vs direct path and show expected latency.

## P3 — broader protocols

- [ ] Wayland host support through a portal/`wayvnc` backend. The state model
      already has `unsupported-wayland` as a distinct outcome.
- [ ] Optional xpra shadow backend for reconnect/HTML5. Evaluate against
      x11vnc before adding a second dependency.
- [ ] Optional RustDesk backend for cross-OS hosts, clearly labelled as a
      separate trust/relay model. Not a replacement for the SSH/Tailscale path.
- [ ] Audio, file transfer and printer/USB redirection. Files belong to
      NIRUSHARE; do not duplicate it here.
- [ ] Headless virtual desktop (Xvnc + a NIRUX session) only if a real need
      appears. Servers usually have web UIs and should not run a desktop by
      default.
- [ ] Wake-on-LAN through an always-on tailnet peer, with explicit limits.

## Release rules

- Do not claim Windows/macOS host control. 0.1.x is a client for targets whose
  RDP/VNC service is already enabled.
- Do not claim Wayland, audio or persistent service support until tested.
- Keep private hostnames, IPs and SSH fingerprints out of public issues and
  release assets.
- Keep the extra install path verifiable: SHA256, tag, source archive and a
  working `niruremote --version` (the last field is the version NIRUX extras
  reads).
