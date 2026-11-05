#!/usr/bin/env bash
# Local end-to-end smoke test with fake Tailscale, SSH, x11vnc and viewer.
# It never touches the real tailnet, SSH configuration, X server or ports
# outside 127.0.0.1.
set -euo pipefail

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

WORK=$(mktemp -d "${TMPDIR:-/tmp}/niruremote-smoke.XXXXXX")
BG=
cleanup() {
    [ -z "$BG" ] || kill "$BG" 2>/dev/null || true
    if [ -f "$WORK/x11vnc.pids" ]; then
        while read -r pid; do kill "$pid" 2>/dev/null || true; done < "$WORK/x11vnc.pids"
    fi
    rm -rf "$WORK"
}
trap cleanup EXIT

cargo build --quiet
BIN="$ROOT/target/debug/niruremote"

touch "$WORK/Xauthority"

cat > "$WORK/tailscale" <<'TS'
#!/bin/sh
cat <<'JSON'
{
  "BackendState": "Running",
  "Self": {"ID": "self", "HostName": "client", "DNSName": "client.example.ts.net.",
           "OS": "linux", "UserID": 1, "Online": true, "TailscaleIPs": ["100.64.0.1"]},
  "User": {"1": {"LoginName": "test@example.com"}},
  "Peer": {
    "peer1": {"HostName": "fakehost", "DNSName": "fakehost.example.ts.net.", "OS": "linux",
              "UserID": 1, "Online": true, "TailscaleIPs": ["100.64.0.2"]}
  }
}
JSON
TS
chmod +x "$WORK/tailscale"

# Fake x11vnc: -inetd speaks a tiny RFB-like stream on stdio; the listener
# mode records how the password file looked and then deletes it like rm:.
cat > "$WORK/x11vnc" <<'PY'
#!/usr/bin/env python3
import os, socket, stat, sys
work = os.environ["SMOKE_WORK"]
if "-version" in sys.argv:
    print("x11vnc: 0.9.17 fake"); sys.exit(0)
with open(os.path.join(work, "x11vnc.pids"), "a") as f:
    f.write(f"{os.getpid()}\n")
with open(os.path.join(work, "x11vnc.args"), "a") as f:
    f.write(" ".join(sys.argv[1:]) + "\n")
if "-inetd" in sys.argv:
    out = os.fdopen(1, "wb", buffering=0)
    out.write(b"RFB 003.008\n")
    while True:
        data = os.read(0, 65536)
        if not data:
            sys.exit(0)
        out.write(b"ok")
spec = sys.argv[sys.argv.index("-passwdfile") + 1]
path = spec[3:] if spec.startswith("rm:") else spec
st = os.stat(path)
with open(path) as f:
    secret = f.readline().strip()
os.remove(path)
with open(os.path.join(work, "pwinfo"), "w") as f:
    f.write(f"{oct(stat.S_IMODE(st.st_mode))} {len(secret)}\n")
port = int(sys.argv[sys.argv.index("-rfbport") + 1])
host = "127.0.0.1" if "-localhost" in sys.argv else "0.0.0.0"
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind((host, port)); s.listen(4)
while True:
    conn, _ = s.accept(); conn.close()
PY
chmod +x "$WORK/x11vnc"

# Fake viewer: connects to the endpoint NIRUREMOTE gives it.
cat > "$WORK/viewer" <<'PY'
#!/usr/bin/env python3
import os, socket, sys, time
target = sys.argv[-1]
mode = os.environ.get("FAKE_VIEWER_MODE", "quick")
if "/" in target:
    s = socket.socket(socket.AF_UNIX); s.connect(target)
else:
    host, port = target.split("::"); s = socket.create_connection((host, int(port)))
banner = s.recv(12)
assert banner == b"RFB 003.008\n", banner
if mode == "silent":
    time.sleep(60); sys.exit(0)
for _ in range(600 if mode == "hold" else 2):
    s.sendall(b"x")
    assert s.recv(2) == b"ok"
    time.sleep(0.25)
PY
chmod +x "$WORK/viewer"

# Fake ssh: run the fixed remote command locally with the same stdio.
cat > "$WORK/ssh" <<'PY'
#!/usr/bin/env python3
import os, sys
args = sys.argv[1:]
for flag in ("-L", "-R", "-D"):
    if flag in args:
        sys.exit("fake ssh: port forwarding must not be used")
if "StrictHostKeyChecking=yes" not in args:
    sys.exit("fake ssh: host key checking must be strict")
i = args.index("niruremote")
os.execvp("niruremote", args[i:])
PY
chmod +x "$WORK/ssh"

export SMOKE_WORK="$WORK"
export PATH="$ROOT/target/debug:$PATH"
export NIRUREMOTE_STATE_DIR="$WORK/state"
export NIRUREMOTE_CACHE_DIR="$WORK/cache"
export NIRUREMOTE_RUNTIME_DIR="$WORK/run"
export NIRUREMOTE_DISPLAY=":0"
export NIRUREMOTE_XAUTHORITY="$WORK/Xauthority"
export NIRUREMOTE_X11VNC="$WORK/x11vnc"
export NIRUREMOTE_VIEWER="$WORK/viewer"
export NIRUREMOTE_VIEWER_UNIX=1
export NIRUREMOTE_SSH="$WORK/ssh"
export NIRUREMOTE_TAILSCALE="$WORK/tailscale"
export NIRUREMOTE_NOTIFY=off

pass() { printf 'PASS %s\n' "$1"; }
fail() { printf 'FAIL %s\n' "$1" >&2; exit 1; }
alive_x11vnc() {
    local n=0 pid
    [ -f "$WORK/x11vnc.pids" ] || { echo 0; return; }
    while read -r pid; do kill -0 "$pid" 2>/dev/null && n=$((n + 1)); done < "$WORK/x11vnc.pids"
    echo "$n"
}
wait_for() { # seconds, command...
    local limit=$1; shift
    for _ in $(seq 1 $((limit * 10))); do "$@" && return 0; sleep 0.1; done
    return 1
}
no_x11vnc() { [ "$(alive_x11vnc)" = 0 ]; }
has_x11vnc() { [ "$(alive_x11vnc)" != 0 ]; }

"$BIN" probe --json | grep -q '"state": "inactive"' || fail "probe should start inactive"
"$BIN" probe --json | grep -q '"ssh-stdio"' || fail "probe should offer ssh-stdio"
pass "probe reports inactive with the stream mode"

out=$("$BIN" enable)
echo "$out" | grep -Eq '^password:  [A-Za-z0-9]{8}$' || fail "enable should print an 8-character password"
[ "$(cat "$WORK/pwinfo")" = "0o600 8" ] || fail "password file must be 0600 with 8 characters"
[ -z "$(find "$WORK/run" -name '*.pw')" ] || fail "password file must be gone after start"
grep -q -- '-localhost' "$WORK/x11vnc.args" || fail "listener must be loopback-only"
if grep -q -- '-nopw' <(grep -- '-passwdfile' "$WORK/x11vnc.args"); then fail "listener must not use -nopw"; fi
"$BIN" probe --json | grep -q '"state": "ready"' || fail "enable should make the agent ready"
pass "enable starts a password-protected loopback listener"

"$BIN" peers --refresh --timeout 2 > "$WORK/peers.out" 2>&1; grep -q "ready" "$WORK/peers.out" || fail "the fake peer should be ready: $(cat "$WORK/peers.out")"
pass "peers --refresh probes the fake peer"

"$BIN" disable >/dev/null
"$BIN" probe --json | grep -q '"state": "inactive"' || fail "disable should stop the listener"
wait_for 5 no_x11vnc || fail "disable must stop x11vnc"
pass "disable stops the listener"

if "$BIN" ensure --json --hold > "$WORK/ensure.out" 2>/dev/null; then fail "ensure must refuse"; fi
grep -q '"state":"upgrade-required"' "$WORK/ensure.out" || fail "ensure should ask 0.1.x clients to update"
pass "ensure tells 0.1.x clients to update"

: > "$WORK/x11vnc.args"
mkdir -p "$WORK/run/stream-999999-1" && touch "$WORK/run/view-999999.sock"
out=$("$BIN" connect fakehost 2>&1) || fail "connect failed: $out"
echo "$out" | grep -q '^disconnected$' || fail "connect should end with disconnected: $out"
grep -q -- '-inetd' "$WORK/x11vnc.args" || fail "connect must use x11vnc -inetd"
if grep -Eq -- '-forever|-shared|-rfbport [1-9]' "$WORK/x11vnc.args"; then fail "stream x11vnc must not listen"; fi
wait_for 5 no_x11vnc || fail "x11vnc must stop after the viewer closes"
[ -z "$(find "$WORK/run" -mindepth 1 \( -name 'session-*' -o -name 'view-*' -o -name 'stream-*' \))" ] \
    || fail "runtime files left behind"
if [ -e "$WORK/run/stream-999999-1" ] || [ -e "$WORK/run/view-999999.sock" ]; then
    fail "stale runtime files of dead clients should be removed"
fi
pass "connect over the SSH channel, x11vnc gone after disconnect"

out=$(FAKE_VIEWER_MODE=silent NIRUREMOTE_IDLE_SECS=2 timeout 20 "$BIN" connect fakehost 2>&1) || true
echo "$out" | grep -q 'no traffic from the viewer' || fail "idle watchdog should end a dead session: $out"
wait_for 5 no_x11vnc || fail "x11vnc must stop after the watchdog fires"
pass "a dead path is detected and x11vnc stops"

FAKE_VIEWER_MODE=hold "$BIN" connect fakehost > "$WORK/hold.log" 2>&1 &
BG=$!
wait_for 10 has_x11vnc || fail "held session did not start"
kill -9 "$BG"; wait "$BG" 2>/dev/null || true; BG=
wait_for 5 no_x11vnc || fail "x11vnc must stop when the client is killed with SIGKILL"
pass "kill -9 of the client stops x11vnc"

FAKE_VIEWER_MODE=hold "$BIN" connect fakehost > "$WORK/hold.log" 2>&1 &
BG=$!
wait_for 10 has_x11vnc || fail "held session did not start"
"$BIN" probe --json | grep -q '"sessions": 1' || fail "probe should count the live session"
"$BIN" disable | grep -q 'ended remote session' || fail "disable should end live sessions"
wait_for 5 no_x11vnc || fail "x11vnc must stop after disable"
wait "$BG" 2>/dev/null || true; BG=
pass "disable ends live sessions"

printf 'All local smoke tests passed.\n'
