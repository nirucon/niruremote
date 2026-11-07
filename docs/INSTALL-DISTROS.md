# Packages on Debian, Arch and Void

Back to the [README](../README.md).

Host and client packages, SSH server and Tailscale per distribution. `niruremote setup`
prints the exact package command for the current machine and `niruremote setup --run`
runs it with a visible sudo/doas prompt.

## Debian / Ubuntu

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

## Arch Linux

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

## Void Linux

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
