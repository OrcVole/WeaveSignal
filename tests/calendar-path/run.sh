#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Import a calendar…'s pasted path and dropped file in a real build of the studio app, driven
# over WebDriver (the browser drivers cannot see either: a page cannot read a path), and a look
# at the week's fold in the app's own webview (WebKitGTK; the browser drivers are Chromium).
# The checks are in drive.mjs, and its head says what the run does not prove. Results and screenshots go
# to tests/calendar-path/run/, which is not kept.
#
# It runs the app on a display of its own (cage, headless), with a home folder and a profile of
# its own under tests/calendar-path/run/, so the installed studio app, its settings, the
# Downloads folder and the screen are left alone. The keychain is the session's own: the run
# adds, and removes again, one entry for the loopback address below.
#
# Needs: node, cage, wlr-randr, tauri-driver, WebKitWebDriver, secret-tool; and the studio app
# built with `cd apps/studio && pnpm tauri build --debug --no-bundle`.
#
#   tests/calendar-path/run.sh        # STATION_PORT, DRIVER_PORT and NATIVE_PORT move the ports
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); root=$(cd "$here/../.." && pwd); run="$here/run"
STATION_PORT=${STATION_PORT:-26541}; DRIVER_PORT=${DRIVER_PORT:-4455}; NATIVE_PORT=${NATIVE_PORT:-4456}
for t in node cage wlr-randr tauri-driver WebKitWebDriver secret-tool; do command -v "$t" >/dev/null || { echo "$t is needed" >&2; exit 1; }; done
[ "$DRIVER_PORT" = 4455 ] || { echo "tests/stations/wd.mjs speaks to tauri-driver on 4455" >&2; exit 1; }
[ -x "$root/target/debug/weavesignal-studio" ] || { echo "build the studio app first: cd apps/studio && pnpm tauri build --debug --no-bundle" >&2; exit 1; }
taken() { ss -H -ln -t "sport = :$1" | grep -q .; }
for p in $STATION_PORT $DRIVER_PORT $NATIVE_PORT; do taken "$p" && { echo "port $p/tcp is in use" >&2; exit 1; }; done

rm -rf "$run"; mkdir -p "$run"/home/{Downloads,.config,.local/share,.cache}
# The app asks the system which folder is Downloads: this home's own.
printf 'XDG_DOWNLOAD_DIR="$HOME/Downloads"\n' > "$run/home/.config/user-dirs.dirs"
pids=()
cleanup() {
  # The driver first: cage ends when its one client has gone, and does not end on a signal to itself.
  pkill -f "tauri-driver --port $DRIVER_PORT" 2>/dev/null || true
  pkill -f "WebKitWebDriver --port=$NATIVE_PORT" 2>/dev/null || true
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  secret-tool clear service weavesignal-studio username "http://127.0.0.1:$STATION_PORT" 2>/dev/null || true
}
trap cleanup EXIT; trap 'exit 130' INT TERM

PORT=$STATION_PORT node "$here/serve-station.mjs" > "$run/station.log" 2>&1 &
pids+=($!)
for i in $(seq 1 40); do taken "$STATION_PORT" && break; sleep 0.25; done

before=$(ls "$XDG_RUNTIME_DIR" | grep -E '^wayland-[0-9]+$' || true)
env -u DISPLAY HOME="$run/home" XDG_CONFIG_HOME="$run/home/.config" XDG_DATA_HOME="$run/home/.local/share" XDG_CACHE_HOME="$run/home/.cache" \
  WEBKIT_DISABLE_DMABUF_RENDERER=1 WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman \
  cage -- tauri-driver --port "$DRIVER_PORT" --native-port "$NATIVE_PORT" > "$run/driver.log" 2>&1 &
pids+=($!)
for i in $(seq 1 40); do taken "$DRIVER_PORT" && break; sleep 0.25; done
display=$(ls "$XDG_RUNTIME_DIR" | grep -E '^wayland-[0-9]+$' | grep -vxF "$before" | head -1)
WAYLAND_DISPLAY=$display wlr-randr --output HEADLESS-1 --custom-mode 1440x900

STATION_PORT=$STATION_PORT node "$here/drive.mjs"
cat "$run/result.txt" | grep -c '^PASS' | xargs -I{} echo "{} passed"
! grep -q '^FAIL' "$run/result.txt"
