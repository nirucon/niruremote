# NIRULAUNCHER integration

The CLIs already provide everything the launcher needs. The launcher should stay
a thin UI: it must not implement SSH, tunnels or VNC lifecycle itself.

## View

Add **Remote desktop** to the existing Tailscale view (or add
`nirulauncher --remote` as an alias that opens the same view with the remote
filter). Do not add four unrelated global rows for install/activate/deactivate.
Those actions are contextual and need a selected machine.

Recommended machine list:

```text
☰ Tailscale · Remote desktop                         r refresh
──────────────────────────────────────────────────────────────
● desktop    linux    100.x.x.x   ready       VNC :0 · temporary
● laptop     linux    100.x.x.x   inactive    agent installed · off
● winhost      windows  100.x.x.x   client-ready RDP reachable
○ machost      macOS    100.x.x.x   offline      last known client-ready (14m)
○ server       linux    100.x.x.x   no-session   no X11 desktop
  phone        android  100.x.x.x   unsupported  not a desktop host
```

- Online peers first; ready/active first within online.
- Offline rows stay visible, dimmed, with last-seen age.
- A green state means verified `ready`/`client-ready`, never merely "online".
- Show `checked 2m ago`; a stale cache is a warning, not a failure.

## Data source

```sh
niruremote peers --json            # cached, fast, no network probes
niruremote peers --json --refresh  # probe online peers, update cache
```

Relevant fields:

```json
{
  "schema": 1,
  "refreshed_at": 1793656428,
  "peers": [
    {
      "id": "nodekey:...",
      "host": "desktop.tailnet.ts.net",
      "dns_name": "desktop.tailnet.ts.net",
      "ip": "100.64.0.10",
      "os": "linux",
      "online": true,
      "last_seen_epoch": 1793656400,
      "user": "user@example.com",
      "tags": [],
      "is_self": false,
      "exit_node_option": false,
      "ssh": {"state": "ok", "detail": "SSH reachable"},
      "remote": {
        "state": "ready",
        "detail": "VNC :0 · 127.0.0.1:43123 · temporary",
        "mode": "ssh-tunnel",
        "version": "0.1.6",
        "checked_at": 1793656428
      }
    }
  ]
}
```

The launcher should display `remote.state` and `remote.detail` directly; it
should not infer readiness from `online`, `ssh.state` or the OS.

## Actions

Enter on a row:

| State | Enter |
|---|---|
| `ready`, `inactive` | Connect |
| `client-ready` | Connect (RDP or VNC) |
| `starting` | Check again |
| `offline` | Details |
| anything else | Details / Install |

Action sheet (`→` or Ctrl+Enter):

- **Connect** — `niruremote connect HOST` in a terminal (so SSH errors and
  host-key instructions are visible), or as a detached spawn once the viewer
  path is stable.
- **Install remote desktop…** — only when `ssh.state` is usable and the OS is
  Linux. Open a terminal with a fixed command:
  `ssh -t HOST niruremote setup --run`. Show the package list before running.
- **Allow connections** — `ssh -t HOST niruremote enable`. The host stays
  loopback-only.
- **Stop allowing** — `ssh -t HOST niruremote disable`; confirm if the state is
  `ready`.
- **Check again** — refresh this peer (launcher may spawn
  `niruremote peers --refresh` once and reload).
- **SSH** — keep the existing `ssh HOST` row.
- **Details** — version, session, owner, checked age, SSH state, host key
  error, install hint.
- **Copy address** — Tailscale IP/hostname.

Do not add a silent global "Install remote desktop" that installs on every
machine. A batch action, if added later, must list the targets and require
confirmation per machine.

## Refresh strategy

`peers --refresh` runs bounded SSH probes and can take a few seconds on a slow
tailnet. The launcher is one-shot and currently synchronous.

Recommended:

1. Open the view immediately from `peers --json` (cache only).
2. Start `peers --json --refresh` in a background thread/process.
3. Show `checking…` on rows without a fresh result.
4. Repaint as results arrive, or offer an explicit **Refresh** action if the
   launcher's event loop cannot do incremental updates yet.
5. Never block the application list or the first paint on SSH.

If incremental repaint is too invasive for the first launcher release, ship the
cache-only view plus a **Refresh status** row that opens a terminal. That is
honest and fast; it avoids a frozen launcher.

## NIRUBAR

- Keep the existing Tailscale module and its click target.
- Add a small screen icon **only while this host has an active/remote
  listener** (`niruremote probe --json`, `running=true`). Hide it otherwise.
- The bar reads local state only; it must never SSH or probe the tailnet.
- Update the Tailscale detail text to mention "machines, SSH and remote
  desktop".
- A later NIRUNOTIFY action can offer **Disconnect** for a local session.

## NIRUX extras

Add a row to the NIRUX extras table once a tagged release exists:

```text
niruremote niruremote 0.1.6 <sha256-of-v0.1.6.tar.gz>
```

The extras installer downloads the GitHub tag archive, verifies the checksum and
runs `install.sh`. `install.sh` installs the binary; it does not install x11vnc
or a viewer. `niruremote setup --run` is the visible follow-up.

NIRUX extras reads the installed version from the last field of
`niruremote --version`, which is `0.1.6`.
