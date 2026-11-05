#!/bin/bash
# The sandbox copy of the app (as `make -C foldav/linux sandbox`) for a waydriver-mcp session: give its
# path as start_session's command, with app_name "foldav-waydriver". waydriver runs the app on the
# host's session bus (the keyring works) in a headless Mutter of its own; the sandbox copy is not
# unique there, so it runs beside the normal app, with its own data in target/sandbox.
#
# waydriver finds the app in the accessibility tree by name, on the host's bus: under its own
# name ("foldav-waydriver", from argv[0]) a copy on the desktop is never taken for it.
# Build it first (`cargo build -p foldav-linux`).
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
# No portals: file dialogs and the launcher stay in the test's own window, not on the desktop.
export FOLDAV_SANDBOX=1 XDG_DATA_HOME="$root/target/sandbox" GTK_USE_PORTAL=0
# Files the Upload actions pick without a dialog (`:`-separated paths in /tmp/foldav-pick).
[ -f /tmp/foldav-pick ] && export FOLDAV_SANDBOX_PICK=$(cat /tmp/foldav-pick)
exec -a foldav-waydriver "$root/target/debug/foldav-linux" "$@"
