#!/usr/bin/env bash
# Install taskvim-sync (and, with --taskvim, this checkout's taskvim) on this machine.
#   sync/install.sh             the sync service only
#   sync/install.sh --taskvim   also install taskvim: built here if cargo exists,
#                               else the prebuilt taskvim-x86_64-linux next to this repo
#   PREBUILT=1 sync/install.sh --taskvim   never build, use the prebuilt binary
#   sync/install.sh --status    what runs
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(dirname "$here")"
bin="$HOME/.local/bin"
unit="$HOME/.config/systemd/user/taskvim-sync.service"

if [ "${1:-}" = "--status" ]; then
    systemctl --user --no-pager status taskvim-sync | head -5 || true
    journalctl --user -u taskvim-sync -n 10 --no-pager || true
    exit 0
fi

python3 -I "$here/taskvim_sync.py" --test >/dev/null 2>&1 || { echo "self-tests failed; not installing"; exit 1; }
mkdir -p "$bin" "$(dirname "$unit")"
install -m 755 "$here/taskvim_sync.py" "$bin/taskvim-sync"

if [ "${1:-}" = "--taskvim" ]; then
    # PREBUILT=1 skips cargo (the Mac mini's Core 2 Duo takes ages to build)
    if [ -z "${PREBUILT:-}" ] && command -v cargo >/dev/null; then
        (cd "$repo" && cargo build --release)
        new="$repo/target/release/taskvim"
    else
        new="$repo/taskvim-x86_64-linux"
    fi
    [ -f "$bin/taskvim" ] && cp -p "$bin/taskvim" "$bin/taskvim.pre-sync"
    install -m 755 "$new" "$bin/taskvim"   # replaces the file, safe while taskvim runs
    echo "taskvim installed; quit and reopen any running taskvim (pgrep -x taskvim)"
fi

cat > "$unit" <<EOF
[Unit]
Description=taskvim-sync: stamp and merge taskvim's data.json across machines

[Service]
ExecStart=/usr/bin/python3 -I $bin/taskvim-sync
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
EOF
systemctl --user daemon-reload
systemctl --user enable taskvim-sync >/dev/null 2>&1
systemctl --user restart taskvim-sync
sleep 2
systemctl --user is-active taskvim-sync
