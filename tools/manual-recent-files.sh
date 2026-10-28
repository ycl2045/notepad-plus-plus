#!/usr/bin/env bash
# Manual check of the "File > Recent files" submenu.
#
# GTK popovers are separate surfaces, so the offscreen screenshot tool cannot
# capture them: this script prepares a workspace with a known recent-files list
# and starts the editor on it.
#
#   tools/manual-recent-files.sh          a list with one deleted file in it
#   tools/manual-recent-files.sh --empty  an empty list
#
# What to look at:
#   1. File > Recent files: the deleted file must be gone, the others listed
#      with their full path, newest first.
#   2. Click one of them: the file opens in a tab and moves to the front.
#   3. With --empty: the submenu shows one greyed out "No recent files" row.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONFIG="$(mktemp -d "${TMPDIR:-/tmp}/npad-manual-XXXXXX")"
DEMO="$CONFIG/demo"
mkdir -p "$CONFIG/npad" "$DEMO"

printf 'the first recent file\n' >"$DEMO/alpha.txt"
printf 'the second recent file\n' >"$DEMO/beta.txt"
printf 'the third recent file\n' >"$DEMO/gamma.txt"

if [ "${1:-}" = "--empty" ]; then
    RECENT='[]'
    EXPECT="the submenu shows a single disabled 'No recent files' row"
else
    # gamma.txt is the newest; gone.txt does not exist any more and has to be
    # dropped from the list.
    RECENT="[\"$DEMO/gamma.txt\", \"$CONFIG/demo/gone.txt\", \"$DEMO/beta.txt\", \"$DEMO/alpha.txt\"]"
    EXPECT="the submenu lists gamma.txt, beta.txt and alpha.txt, and not gone.txt"
fi

cat >"$CONFIG/npad/session.json" <<EOF
{
  "open_files": [],
  "active_index": 0,
  "recent_files": $RECENT,
  "window": {"width": 1000, "height": 700}
}
EOF

cat <<EOF

npad -- manual check of File > Recent files

  workspace: $DEMO
  config:    $CONFIG

  expected: $EXPECT

  The window opens now; close it when you are done.

EOF

XDG_CONFIG_HOME="$CONFIG" "$ROOT/target/release/npad"
