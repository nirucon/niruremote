# NIRUREMOTE 0.2 review — exposure, lifetime, locked screens and energy

Scope: NIRUREMOTE 0.1.6 (commit `9157aa7`) as a NIRUX extra: the x11vnc
lifecycle, what listens where and for how long, authentication, SSH usage,
the remote command, locked-screen behaviour, state/UX truthfulness, energy and
the build contract (Debian 13 rustc 1.85).

Every finding marked *reproduced* was shown on 0.1.6 on the real hosts
(othala Debian 13, algiz Arch, void Void) over Tailscale, or with
`cargo +1.85`. Reproductions that needed a desktop used a throw-away Xvfb
display, never the owner's unlocked session. No firewall, sshd or Tailscale
configuration was changed.

## Threat model

* The person who connects is authenticated by **SSH** (key, verified host
  key) as the same account that owns the shared X session. That account can
  already read the screen and inject input through X11, so a viewer for that
  account adds no privilege.
* Attackers: **another local uid** on the host or on the client (a service
  account, a container with host networking, a second user), **another
  tailnet/LAN machine**, and a **malicious tailnet peer name or remote agent**
  feeding text to the client.
* A locked session must stay locked unless someone types the account password
  into the locker (PAM).

## Findings (ranked)

| # | Severity | Area | Finding | Evidence | 0.2.0 |
|---|---|---|---|---|---|
| S1 | High | Exposure | x11vnc ran with `-localhost -nopw -forever -shared` on a loopback TCP port for the whole session (and indefinitely with `enable`). **Any local uid** could connect without a password and get full view and control of the desktop. | reproduced (R1): `sudo -u nobody` → security types `[1]` (None), `SecurityResult 0`, `ServerInit 640x480 "NIRUREMOTE"` | **Fixed.** `connect` runs `x11vnc -inetd` on a socketpair; the agent relays it over the SSH channel. x11vnc has no TCP or Unix listener (native: `tcp=0 unix=0` on every host). |
| S2 | High | Exposure | The client side forwarded `ssh -L 127.0.0.1:LOCAL:…`; any local uid on the **client** could reach the remote desktop through that port, again without a password. | code (same `-nopw` endpoint) | **Fixed.** No `-L`. The viewer connects through a Unix socket (0600, 0700 dir) that accepts one connection, checks `SO_PEERCRED` = our uid and is unlinked before data flows. Remmina/old TigerVNC fall back to a loopback port with the same uid check via `/proc/net/tcp`. |
| S3 | Medium | Exposure | x11vnc also listened on **`[::1]:5900`** and `[::1]:PORT` (libvncserver IPv6 default), neither documented nor checked. | reproduced on void: `ss` showed `127.0.0.1:33081`, `[::1]:5900`, `[::1]:33081` for one session | **Fixed.** `-noipv6`, and `-rfbportv6 -1` for the listener (test showed `-noipv6` alone still opens `[::1]:5900`). |
| C1 | High | Lifetime | A silently dropped network left the unauthenticated x11vnc running until sshd noticed the dead TCP connection (hours with default keepalives). | reproduced (R2): client ssh frozen 90 s, x11vnc on void still alive with its listener | **Fixed.** The agent ends the session 30 s after the last byte from the viewer; x11vnc `-ping 5` keeps a live viewer talking. Native: x11vnc gone 31 s after the path froze. The client's SSH keepalive (5 s × 3) ends the client side. |
| S4 | Medium | Exposure | `enable` kept the same unauthenticated, shared listener until `disable`/logout. | code + R1 | **Fixed.** Random 8-character VNC password per `enable` via `-passwdfile rm:` (0600 file, deleted by x11vnc), printed once; `-nevershared -dontdisconnect`. Native: None not offered (`[2]` only), wrong password fails, other uid gets only VNC auth, password absent from argv/log/JSON. `enable` is no longer needed for `connect`. |
| S5 | Medium | SSH | A user `~/.ssh/config` could add `LocalForward`/`DynamicForward` listeners, agent or X11 forwarding, a `RemoteCommand`, or a `ControlMaster`/`ControlPersist` master that outlives the session. | code | **Fixed.** `ClearAllForwardings=yes`, `ForwardAgent=no`, `ForwardX11=no`, `PermitLocalCommand=no`, `ControlMaster=no`, `ControlPath=none`, `RemoteCommand=none`, `EscapeChar=none`; host after `--`. Tests guard the argument list. |
| B1 | Medium | Build | `rust-version = "1.88"` and let-chains: `--locked` did not build on Debian 13's rustc 1.85 (global invariant). | reproduced (`cargo +1.85 build --locked`: 10 × E0658) | **Fixed.** Let-chains removed, `rust-version = "1.85"`, CI job on 1.85 (build + test). |
| U1 | Medium | UX | A connectable host showed `inactive` ("remote desktop is not enabled"), suggesting `enable` was required; nothing said the remote screen was locked; a 0.1.x/0.2.x mix had no clear state. | code | **Fixed.** `ready` = connect any time; `update-needed` for 0.1.x agents; detail adds `screen locked (nirulock)`, active sessions and an enabled listener; `connect` prints *checking → opening the viewer → starting → connected*; 0.1.x clients get "update NIRUREMOTE on the client". |
| S6 | Low | Hardening | x11vnc accepted X11VNC_REMOTE control properties and could run external commands. | x11vnc defaults | **Fixed.** `-noremote -nocmds` (`-norc` was already set). |
| S7 | Low | Terminal | Tailnet names and remote-agent strings were printed raw (escape sequences from a malicious peer or agent reach the terminal). | code | **Fixed.** Control characters replaced before printing; test with ESC/BEL payloads. |
| U2 | Low | Visibility | The person at the host could not tell that someone was connected. | code | **Fixed.** Desktop notification at start/end (notify-send → NIRUNOTIFY), `probe` shows live sessions, `disable` ends them. |
| E1 | Low | Energy | Polling during a session: agent 100 ms loop, client 200 ms loop, plus a second SSH connection with 2 s keepalives. | measured (below) | **Fixed.** Blocking event channel (child exit via `waitid(WNOWAIT)`, self-pipe signals), one SSH connection, watchdog deadline instead of a tick. |
| C2 | Low | Probe | `probe` checked readiness by opening a TCP connection to x11vnc (a VNC client per probe). | code | **Fixed.** Reads `/proc/net/tcp` instead. |
| C3 | Low | Cleanup | (Found during native tests of the candidate.) After `kill -9` of the client its SSH-stderr temp dir stayed in `$XDG_RUNTIME_DIR`. | native, othala | **Fixed.** Stale `view-PID`/`stream-PID-*` of dead PIDs are removed on the next `connect`; smoke-tested. |
| OK1 | — | Injection | The remote commands are constant (`niruremote probe --json`, `niruremote stream`); host names are validated (no leading `-`, no shell metacharacters) before reaching `ssh`. No user or tailnet data is ever part of the remote command. | code + tests | Kept; host now also after `--`. |
| OK2 | — | Host keys | `StrictHostKeyChecking=yes`, `BatchMode=yes`; unknown keys are reported as `host-key-untrusted`, never accepted. | native: every FQDN was `host-key-untrusted` until a key verified out of band was present | Kept. |
| OK3 | — | Secrets | No secret in argv in 0.1.6 (no password existed). | code | 0.2.0's only secret (the `enable` password) is never in argv, logs or JSON (native check). |
| L1 | — | Lock screen | Does connecting bypass NIRULOCK? | native (below) | **No.** The viewer shows NIRULOCK; pointer motion and key input (no Return) reached it; the same NIRULOCK process was running afterwards. Only PAM unlocks. `probe` now reports the locker. |
| D1 | Info | Energy | During a session x11vnc itself polls the screen: ~617 wakeups/s, 1.5 % of one core on void (same in 0.1.6; `-xdamage`, `-nocursorpos`, `-nocursor` changed nothing). Nothing runs when no session is open. | measured | **Deferred** (x11vnc tuning). |
| D2 | Info | Platforms | Windows RDP / macOS VNC targets and the Remmina fallback were not tested (no such machine online). | — | **Deferred.** |
| D3 | Info | UI | NIRUREMOTE is still a CLI; the NIRULAUNCHER view and a NIRUBAR indicator remain on the roadmap. | — | **Deferred.** |
| D4 | Info | Backend | Run directly, `x11vnc -inetd` did not exit on stdin EOF by itself. | Void, manual run | The agent never relies on it: it stops x11vnc explicitly on every exit path. |

## Energy (before → after, idle connected session, 30 s)

| Process | 0.1.6 | 0.2.0 |
|---|---|---|
| client `niruremote` | 5.03 wakeups/s | 0.00 |
| client SSH | 1.00 + 0.40 (two connections) | 0.73 (one) |
| host agent | 10.00 wakeups/s, 0.03 % CPU | 0.63, 0.00 % |
| host x11vnc | 615 wakeups/s, 1.57 % | 618 wakeups/s, 1.57 % (D1) |
| no session | nothing running | nothing running |

## Native verification (2026-11-05, candidate 0.2.0, sha256 `b071beaa…`)

All three hosts on NIRUX 0.12.209 with the sessions **locked by NIRULOCK**
throughout (no unlock was needed). The candidate ran from a test directory;
the client used a test wrapper that only replaced the remote `niruremote`
path. SSH from host to host used a forwarded agent and host keys verified out
of band (both removed afterwards).

| Pair | Real TigerVNC session | Host listeners during | Viewer closed | Notes |
|---|---|---|---|---|
| othala → void | yes (`Connected to socket …/view-PID.sock`, RFB 3.8) | x11vnc tcp=0 unix=0; non-loopback set unchanged | `disconnected`, x11vnc gone | kill -9 of the client: viewer + ssh gone, void x11vnc gone < 3 s. Frozen path: x11vnc gone after 31 s. Lock frame captured: NIRULOCK. Input sent; NIRULOCK pid 959 unchanged. |
| void → othala | yes (TigerVNC 1.16.2) | tcp=0 unix=0 | `disconnected`, x11vnc gone | runit/elogind client |
| othala → algiz | yes | tcp=0 unix=0 | `disconnected`, x11vnc gone | Arch host |
| void → algiz | yes | tcp=0 unix=0 | `disconnected`, x11vnc gone | Void client, Arch host |

`enable` on othala (Xvfb): only `127.0.0.1:PORT` (no `[::1]:5900`), security
types `[2]`, right password OK, wrong password FAILED, uid 65534 offered VNC
auth only, password not in argv/log, password file gone, nothing listening
after `disable`.

## Compatibility

* A 0.2.0 client refuses 0.1.x agents (`update-needed`) instead of using
  their unauthenticated listener.
* A 0.1.x client gets `upgrade-required` from a 0.2.0 agent's `ensure`.
* Probe JSON stays `schema: 1`; new fields (`locker`, `sessions`,
  `ssh-stdio` mode) are additive.
