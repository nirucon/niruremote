# Testing status

Back to the [README](../README.md).

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
  between a Debian 13, an Arch and a Void machine in every direction
  that applies (Debian↔Void, Debian→Arch, Void→Arch), with
  `ss` showing no new listener, x11vnc gone after disconnect, `kill -9` of the
  client and a stalled network path, and NIRULOCK staying locked.
- **Bare metal is still required.** Run [ACCEPTANCE.md](ACCEPTANCE.md)
  before relying on it for daily access.
