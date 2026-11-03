#!/usr/bin/env bash
# Local end-to-end smoke test with fake Tailscale, SSH, x11vnc and viewer.
# It never touches the real tailnet, SSH configuration, X server or ports
# outside 127.0.0.1.
set -euo pipefail

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

WORK=$(mktemp -d "${TMPDIR:-/tmp}/niruremote-smoke.XXXXXX")
trap 'rm -rf "$WORK"' EXIT

cargo build --quiet
BIN="$ROOT/target/debug/niruremote"

touch "$WORK/Xauthority"

cat > "$WORK/tailscale" <<'EOF'
#!/bin/sh
cat <<'JSON'
{
  "BackendState": "Running",
  "Self": {
    "ID": "self",
    "HostName": "client",
    "DNSName": "client.example.ts.net.",
    "OS": "linux",
    "UserID": 1,
    "Online": true,
    "TailscaleIPs": ["100.64.0.1"]
  },
  "User": {"1": {"LoginName": "test@example.com"}},
  "Peer": {
    "peer1": {
      "HostName": "fakehost",
      "DNSName": "fakehost.example.ts.net.",
      "OS": "linux",
      "UserID": 1,
      "Online": true,
      "TailscaleIPs": ["100.64.0.2"]
    }
  }
}
JSON
EOF
chmod +x "$WORK/tailscale"

cat > "$WORK/x11vnc" <<'EOF'
#!/usr/bin/env python3
import socket, sys
if "-version" in sys.argv:
    print("x11vnc: 0.9.16 fake")
    sys.exit(0)
port = int(sys.argv[sys.argv.index("-rfbport") + 1])
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", port))
s.listen(8)
while True:
    conn, _ = s.accept()
    conn.close()
EOF
chmod +x "$WORK/x11vnc"

cat > "$WORK/viewer" <<'EOF'
#!/bin/sh
sleep 1
exit 0
EOF
chmod +x "$WORK/viewer"

cat > "$WORK/ssh" <<'EOF'
#!/usr/bin/env python3
import os, socket, sys, threading
args = sys.argv[1:]
if "-N" in args and "-L" in args:
    spec = args[args.index("-L") + 1]
    parts = spec.split(":")
    local_port = int(parts[1])
    remote_port = int(parts[3])
    server = socket.socket()
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(("127.0.0.1", local_port))
    server.listen(8)
    def copy(src, dst):
        try:
            while True:
                data = src.recv(65536)
                if not data:
                    break
                dst.sendall(data)
        except Exception:
            pass
        finally:
            try:
                dst.shutdown(socket.SHUT_WR)
            except Exception:
                pass
    def handle(client):
        try:
            upstream = socket.create_connection(("127.0.0.1", remote_port), timeout=3)
        except Exception:
            client.close()
            return
        threading.Thread(target=copy, args=(client, upstream), daemon=True).start()
        threading.Thread(target=copy, args=(upstream, client), daemon=True).start()
    while True:
        client, _ = server.accept()
        threading.Thread(target=handle, args=(client,), daemon=True).start()
elif "niruremote" in args:
    command = args[args.index("niruremote") + 1:]
    os.execvp("niruremote", ["niruremote"] + command)
else:
    sys.exit(2)
EOF
chmod +x "$WORK/ssh"

export PATH="$ROOT/target/debug:$PATH"
export NIRUREMOTE_STATE_DIR="$WORK/state"
export NIRUREMOTE_CACHE_DIR="$WORK/cache"
export NIRUREMOTE_DISPLAY=":0"
export NIRUREMOTE_XAUTHORITY="$WORK/Xauthority"
export NIRUREMOTE_X11VNC="$WORK/x11vnc"
export NIRUREMOTE_VIEWER="$WORK/viewer"
export NIRUREMOTE_SSH="$WORK/ssh"
export NIRUREMOTE_TAILSCALE="$WORK/tailscale"

pass() { printf 'PASS %s\n' "$1"; }
fail() { printf 'FAIL %s\n' "$1" >&2; exit 1; }

"$BIN" probe --json | grep -q '"state": "inactive"' \
    || fail "probe should start inactive"
pass "probe reports inactive"

"$BIN" enable >/dev/null
"$BIN" probe --json | grep -q '"state": "ready"' \
    || fail "enable should make the agent ready"
"$BIN" probe --json | grep -q '"owner": "enable"' \
    || fail "enable should record owner=enable"
PORT=$("$BIN" probe --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["port"])')
python3 - "$PORT" <<'PY'
import socket, sys
port = int(sys.argv[1])
s = socket.socket(); s.settimeout(1); s.connect(("127.0.0.1", port)); s.close()
PY
pass "enable listens on a loopback port"

"$BIN" peers --refresh --timeout 2 | grep -q 'ready' \
    || fail "the fake peer should be ready while enable is on"
pass "peers --refresh probes the fake peer"

"$BIN" ensure --json | grep -q '"owned":false' \
    || fail "ensure should reuse the enabled listener"
pass "ensure reuses an existing listener"

"$BIN" disable >/dev/null
"$BIN" probe --json | grep -q '"state": "inactive"' \
    || fail "disable should stop the listener"
pass "disable stops the listener"

python3 - "$BIN" <<'PY'
import json, os, socket, subprocess, sys, time
binary = sys.argv[1]
p = subprocess.Popen(
    [binary, "ensure", "--json", "--hold"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    text=True,
)
line = p.stdout.readline()
response = json.loads(line)
assert response["state"] == "ready", response
port = response["port"]
s = socket.socket(); s.settimeout(1); s.connect(("127.0.0.1", port)); s.close()
p.stdin.close()
assert p.wait(timeout=6) == 0
s = socket.socket(); s.settimeout(0.5)
try:
    s.connect(("127.0.0.1", port))
    raise SystemExit("held x11vnc did not stop after stdin EOF")
except OSError:
    pass
finally:
    s.close()
assert not os.path.exists(os.path.join(os.environ["NIRUREMOTE_STATE_DIR"], "state.json"))
PY
pass "ensure --hold cleans up on stdin EOF"

"$BIN" connect fakehost >/dev/null
"$BIN" probe --json | grep -q '"state": "inactive"' \
    || fail "connect should leave the agent inactive"
pass "connect fakehost completes and leaves no session"

printf 'All local smoke tests passed.\n'
