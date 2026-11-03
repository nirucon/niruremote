#!/usr/bin/env bash
set -euo pipefail

PURGE=0
USER_ONLY=0
for argument in "$@"; do
    case "$argument" in
        --purge) PURGE=1 ;;
        --user) USER_ONLY=1 ;;
        --help|-h)
            echo "Usage: ./uninstall.sh [--user] [--purge]"
            echo "  --user   remove ~/.local/bin/niruremote"
            echo "  --purge  also remove ~/.local/state/niruremote and ~/.cache/niruremote"
            exit 0
            ;;
        *) echo "uninstall.sh: unknown option: $argument" >&2; exit 1 ;;
    esac
done

remove_path() {
    if [ -e "$1" ] || [ -L "$1" ]; then
        if [ -w "$(dirname "$1")" ]; then
            rm -f "$1"
        elif command -v sudo >/dev/null 2>&1; then
            sudo rm -f "$1"
        elif command -v doas >/dev/null 2>&1; then
            doas rm -f "$1"
        else
            echo "uninstall.sh: root, sudo or doas is required to remove $1" >&2
            exit 1
        fi
        echo "Removed $1"
    fi
}

if [ "$USER_ONLY" = 1 ]; then
    remove_path "$HOME/.local/bin/niruremote"
else
    remove_path /usr/local/bin/niruremote
    if [ -L "$HOME/.local/bin/niruremote" ]; then
        rm -f "$HOME/.local/bin/niruremote"
        echo "Removed $HOME/.local/bin/niruremote"
    fi
fi

if [ "$PURGE" = 1 ]; then
    rm -rf "$HOME/.local/state/niruremote" "$HOME/.cache/niruremote"
    echo "Removed local state and cache"
else
    echo "Preserved local state in ~/.local/state/niruremote and ~/.cache/niruremote"
    echo "Use --purge to remove them explicitly."
fi
