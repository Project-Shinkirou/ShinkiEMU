#!/usr/bin/env bash
# Launches the packaged Linux Manager under Xvfb and requires it to still be
# running when the timeout fires. Usage: linux-manager-smoke.sh <manager.zip>
set -euo pipefail
archive=$1
work=$(mktemp -d "${TMPDIR:-/tmp}/shinki-linux-smoke.XXXXXX")
xvfb=
trap 'if [ -n "$xvfb" ]; then kill "$xvfb" 2>/dev/null; wait "$xvfb" 2>/dev/null; fi; rm -rf "$work"' EXIT
unzip -q "$archive" -d "$work"
mkdir -p "$work/state"/{data,config,cache,runtime}
chmod 700 "$work/state/runtime"
export XDG_DATA_HOME="$work/state/data" XDG_CONFIG_HOME="$work/state/config"
export XDG_CACHE_HOME="$work/state/cache" XDG_RUNTIME_DIR="$work/state/runtime"
# Xvfb directly rather than xvfb-run: xvfb-run exits 5 when its own temporary
# directory cleanup fails, replacing the Manager's status.
Xvfb -displayfd 3 -screen 0 1280x800x24 -nolisten tcp 3>"$work/display" 2>"$work/xvfb.log" &
xvfb=$!
for _ in $(seq 100); do
  [ -s "$work/display" ] && break
  kill -0 "$xvfb" 2>/dev/null || { cat "$work/xvfb.log"; exit 1; }
  sleep 0.1
done
display=$(head -n 1 "$work/display")
test -n "$display"
set +e
# Hosted runners have no GPU: the only adapters are Mesa's software GL and
# lavapipe Vulkan, which Slint uses only when CPU adapters are allowed.
DISPLAY=":$display" LIBGL_ALWAYS_SOFTWARE=1 SLINT_WGPU_CPU=1 timeout 15s "$work/shinki-emu-manager" > "$work/manager.log" 2>&1
status=$?
set -e
cat "$work/manager.log"
echo "manager exit status: $status (124 = still running at timeout)"
# The Manager writes its own diagnostics under the XDG state roots.
find "$work/state" -type f \( -name '*.log' -o -name '*.jsonl' \) -exec tail -n 40 {} +
test "$status" -eq 124
