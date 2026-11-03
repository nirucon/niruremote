#!/usr/bin/env bash
set -euo pipefail

VERSION=${NIRUREMOTE_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)}
VERSION=${VERSION:-0.1.0}
MODE=system
PREFIX=
FORCE_BUILD=0
TMP=

usage() {
    cat <<EOF
Usage: ./install.sh [--user|--system|--prefix DIR] [--build]

  --system        Install to /usr/local/bin (default, may use sudo)
  --user          Install to ~/.local/bin
  --prefix DIR    Install to DIR
  --build         Force a local cargo build
  --help          Show this help

The installer only installs the niruremote binary. It does not install
x11vnc, a VNC viewer or change any firewall. Run \`niruremote setup\` after.
EOF
}

cleanup() {
    [ -z "$TMP" ] || rm -rf "$TMP"
}
trap cleanup EXIT

while [ "$#" -gt 0 ]; do
    case "$1" in
        --user) MODE=user ;;
        --system) MODE=system ;;
        --prefix)
            [ "$#" -ge 2 ] || { echo "install.sh: --prefix needs a directory" >&2; exit 1; }
            MODE=prefix
            PREFIX=$2
            shift
            ;;
        --build) FORCE_BUILD=1 ;;
        --help|-h) usage; exit 0 ;;
        *) echo "install.sh: unknown option: $1" >&2; usage >&2; exit 1 ;;
    esac
    shift
done

SCRIPT_DIR=$(unset CDPATH; cd -- "$(dirname -- "$0")" && pwd)
cd "$SCRIPT_DIR"

download_binary() {
    local url="https://github.com/nirucon/niruremote/releases/download/v${VERSION}/niruremote"
    local sums="https://github.com/nirucon/niruremote/releases/download/v${VERSION}/SHA256SUMS"
    command -v sha256sum >/dev/null 2>&1 || {
        echo "install.sh: sha256sum is required to verify the release binary" >&2
        exit 1
    }
    TMP=$(mktemp -d "${TMPDIR:-/tmp}/niruremote-install.XXXXXX")
    echo "Downloading prebuilt niruremote $VERSION ..."
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 3 -o "$TMP/niruremote" "$url"
        curl -fsSL --retry 3 -o "$TMP/SHA256SUMS" "$sums"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$TMP/niruremote" "$url"
        wget -qO "$TMP/SHA256SUMS" "$sums"
    else
        echo "install.sh: curl or wget is required to download the release binary" >&2
        exit 1
    fi
    local expected actual
    expected=$(awk '$2 == "niruremote" || $2 == "*niruremote" { print $1; exit }' "$TMP/SHA256SUMS")
    actual=$(sha256sum "$TMP/niruremote" | awk '{print $1}')
    if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
        echo "install.sh: downloaded binary failed SHA-256 verification" >&2
        exit 1
    fi
    chmod +x "$TMP/niruremote"
    BIN=$TMP/niruremote
}

fresh_binary() {
    local binary=$1
    [ -x "$binary" ] || return 1
    local newer
    newer=$(find "$SCRIPT_DIR/src" "$SCRIPT_DIR/Cargo.toml" "$SCRIPT_DIR/Cargo.lock" \
        -newer "$binary" -print -quit 2>/dev/null || true)
    [ -z "$newer" ]
}

BIN=
if [ "$FORCE_BUILD" = 1 ]; then
    command -v cargo >/dev/null 2>&1 || {
        echo "install.sh: cargo is required for --build" >&2
        exit 1
    }
    echo "Building niruremote $VERSION ..."
    cargo build --release --locked
    BIN=$SCRIPT_DIR/target/release/niruremote
elif [ -x "$SCRIPT_DIR/niruremote" ]; then
    BIN=$SCRIPT_DIR/niruremote
elif fresh_binary "$SCRIPT_DIR/target/release/niruremote"; then
    BIN=$SCRIPT_DIR/target/release/niruremote
elif fresh_binary "$SCRIPT_DIR/target/x86_64-unknown-linux-musl/release/niruremote"; then
    BIN=$SCRIPT_DIR/target/x86_64-unknown-linux-musl/release/niruremote
else
    download_binary
fi

[ -x "$BIN" ] || { echo "install.sh: no runnable binary at $BIN" >&2; exit 1; }

as_root() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
        return
    fi
    if command -v sudo >/dev/null 2>&1; then
        sudo "$@"
        return
    fi
    if command -v doas >/dev/null 2>&1; then
        doas "$@"
        return
    fi
    echo "install.sh: root, sudo or doas is required to write /usr/local/bin" >&2
    exit 1
}

install_system() {
    if [ "$(id -u)" -eq 0 ] || [ -w /usr/local/bin ]; then
        install -Dm755 "$BIN" /usr/local/bin/niruremote
    else
        as_root install -Dm755 "$BIN" /usr/local/bin/niruremote
    fi
    # NIRUX extras detect installed extras in ~/.local/bin. Keep a user-visible
    # symlink even when the real binary lives in /usr/local/bin.
    mkdir -p "$HOME/.local/bin"
    ln -sf /usr/local/bin/niruremote "$HOME/.local/bin/niruremote"
    echo "Installed /usr/local/bin/niruremote"
    echo "Symlinked $HOME/.local/bin/niruremote"
}

install_user() {
    install -Dm755 "$BIN" "$HOME/.local/bin/niruremote"
    echo "Installed $HOME/.local/bin/niruremote"
    case ":$PATH:" in
        *":$HOME/.local/bin:"*) ;;
        *) echo "Note: add $HOME/.local/bin to PATH for non-login SSH sessions." ;;
    esac
}

case "$MODE" in
    system) install_system ;;
    user) install_user ;;
    prefix)
        install -Dm755 "$BIN" "$PREFIX/niruremote"
        echo "Installed $PREFIX/niruremote"
        ;;
esac

cat <<EOF

Next steps:
  niruremote setup           # check x11vnc and a VNC viewer on this machine
  niruremote probe --json    # inspect the local X11 session
  niruremote enable          # on the machine you want to connect to
  niruremote peers --refresh # on the machine you sit at
  niruremote connect HOST    # graphical remote desktop over Tailscale + SSH
EOF
