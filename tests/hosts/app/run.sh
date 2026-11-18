#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The screens for hosts in their own studios in a real build of the studio app, in its own
# webview (WebKitGTK; the browser driver, tests/hosts/shoot.mjs, is Chromium), driven over
# WebDriver against the same stand-in station: the source's tile on the desk, On from the tile,
# and Settings › Own studios with the setup and its once-only details. The checks are in
# drive.mjs. Results and screenshots go to OUT (target/hosts-app unless told), which is not kept.
#
# It runs the app on a display of its own (cage, headless), with a home folder and a profile of
# its own under OUT, so the installed studio app, its settings and the screen are left alone.
# The keychain is the session's own: the run adds, and removes again, one entry for the
# loopback address below. (The same arrangement as tests/calendar-path/run.sh on r105.)
#
# Needs: node, cage, wlr-randr, tauri-driver, WebKitWebDriver, secret-tool; and the studio app
# built with `cd apps/studio && pnpm tauri build --debug --no-bundle`.
#
#   tests/hosts/app/run.sh        # STATION_PORT and NATIVE_PORT move the ports
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); root=$(cd "$here/../../.." && pwd); run=${OUT:-$root/target/hosts-app}
STATION_PORT=${STATION_PORT:-28110}; DRIVER_PORT=4455; NATIVE_PORT=${NATIVE_PORT:-28111}
for t in node cage wlr-randr tauri-driver WebKitWebDriver secret-tool; do command -v "$t" >/dev/null || { echo "$t is needed" >&2; exit 1; }; done
[ -x "$root/target/debug/weavesignal-studio" ] || { echo "build the studio app first: cd apps/studio && pnpm tauri build --debug --no-bundle" >&2; exit 1; }
taken() { ss -H -ln -t "sport = :$1" | grep -q .; }
for p in $STATION_PORT $DRIVER_PORT $NATIVE_PORT; do taken "$p" && { echo "port $p/tcp is in use (tests/stations/wd.mjs speaks to tauri-driver on 4455)" >&2; exit 1; }; done

rm -rf "$run"; mkdir -p "$run"/home/{.config,.local/share,.cache}
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

STATION_PORT=$STATION_PORT RUN="$run" node "$here/drive.mjs"
grep -c '^PASS' "$run/result.txt" | xargs -I{} echo "{} passed"
! grep -q '^FAIL' "$run/result.txt"
