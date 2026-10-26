#!/usr/bin/env bash
# npad — one-shot verification gate.
#
# Runs the release build, the unit tests, the clippy gate and a UI screenshot
# check, then prints "VERIFY OK". Every step is checked explicitly; the first
# failing step prints "VERIFY FAILED: <step>" and exits with status 1.
#
# Usage: tools/verify.sh   (callable from any working directory)
set -euo pipefail

cd "$(dirname "$0")/.."

TMP_ROOT="${TMPDIR:-/tmp}"
SHOT="$TMP_ROOT/npad-verify.png"
LOG_DIR="$TMP_ROOT/npad-verify-logs"

fail() {
    echo "VERIFY FAILED: $1"
    exit 1
}

step() {
    local name="$1"
    shift
    echo "=== VERIFY STEP: $name"
    if ! "$@" 2>&1 | tee "$LOG_DIR/$name.log"; then
        echo "VERIFY FAILED: $name"
        exit 1
    fi
    echo "--- ok: $name"
}

mkdir -p "$LOG_DIR"
rm -f "$SHOT"

step "cargo build --release" cargo build --release
step "cargo test" cargo test
step "cargo clippy" cargo clippy --all-targets -- -D warnings
step "screenshot" bash tools/screenshot.sh "$SHOT"

echo "=== VERIFY STEP: screenshot png"
if [ ! -f "$SHOT" ]; then
    fail "screenshot png missing ($SHOT)"
fi

shot_size=$(wc -c < "$SHOT")
if [ "$shot_size" -le 1024 ]; then
    fail "screenshot png too small ($shot_size bytes, expected > 1024)"
fi

# PNG magic bytes: 89 50 4E 47 0D 0A 1A 0A
shot_magic=$(head -c 8 "$SHOT" | od -An -tx1 | tr -d ' \n')
if [ "$shot_magic" != "89504e470d0a1a0a" ]; then
    fail "screenshot png magic bytes wrong (got '$shot_magic')"
fi

if command -v file > /dev/null 2>&1; then
    if ! file -b "$SHOT" | grep -q 'PNG image data'; then
        fail "screenshot png is not a PNG ($(file -b "$SHOT"))"
    fi
fi

echo "--- ok: screenshot png ($shot_size bytes)"

echo "VERIFY OK"
