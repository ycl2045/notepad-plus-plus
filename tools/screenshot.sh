#!/usr/bin/env bash
# Render the app offscreen into a PNG via render-to-texture (works on Wayland
# where wlr-screencopy is unavailable). Usage: tools/screenshot.sh OUT.png
#
# The app runs against a throwaway config directory, so the shot never depends
# on - and never disturbs - the session and preferences of the real user.
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="${1:-/tmp/npad-shot.png}"
CONFIG="${TMPDIR:-/tmp}/npad-shot-config"
rm -rf "$CONFIG"
mkdir -p "$CONFIG"
cargo build -q
XDG_CONFIG_HOME="$CONFIG" NPAD_SHOT="$OUT" ./target/debug/npad >/tmp/npad-shot.log 2>&1 &
PID=$!
for _ in $(seq 1 40); do [ -s "$OUT" ] && break; sleep 0.25; done
kill "$PID" 2>/dev/null || true
[ -s "$OUT" ] && echo "OK $OUT" || { echo "FAILED"; cat /tmp/npad-shot.log; exit 1; }
