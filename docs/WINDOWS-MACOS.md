# Windows and macOS

Back to the [README](../README.md).

## What NIRUREMOTE does

In short:

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
4. On the Linux client, install FreeRDP (see [INSTALL-DISTROS.md](INSTALL-DISTROS.md)).
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
4. On the Linux client, install TigerVNC (see [INSTALL-DISTROS.md](INSTALL-DISTROS.md)).
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
