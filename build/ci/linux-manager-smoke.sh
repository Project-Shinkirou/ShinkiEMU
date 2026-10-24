#!/usr/bin/env bash
# Launches the packaged Linux Manager under Xvfb and requires it to still be
# running when the timeout fires. Usage: linux-manager-smoke.sh <manager.zip>
set -euo pipefail
archive=$1
work=$(mktemp -d "${TMPDIR:-/tmp}/shinki-linux-smoke.XXXXXX")
trap 'rm -rf "$work"' EXIT
unzip -q "$archive" -d "$work"
mkdir -p "$work/state"/{data,config,cache,runtime}
chmod 700 "$work/state/runtime"
export XDG_DATA_HOME="$work/state/data" XDG_CONFIG_HOME="$work/state/config"
export XDG_CACHE_HOME="$work/state/cache" XDG_RUNTIME_DIR="$work/state/runtime"
set +e
LIBGL_ALWAYS_SOFTWARE=1 xvfb-run -a timeout 15s "$work/shinki-emu-manager" > "$work/manager.log" 2>&1
status=$?
set -e
cat "$work/manager.log"
echo "manager exit status: $status (124 = still running at timeout)"
# The Manager writes its own diagnostics under the XDG state roots.
find "$work/state" -type f \( -name '*.log' -o -name '*.jsonl' \) -exec tail -n 40 {} +
test "$status" -eq 124
