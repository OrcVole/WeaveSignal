#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The station switcher (ADR-0285) in a real build of the studio app, driven over WebDriver:
# an install from before the list carries over without signing in again; a station is added,
# switched to, left while on air, given a name, signed out of and removed; and after each
# switch the station left receives nothing more. 37 checks, written to tests/stations/run/.
#
# It runs the app on a display of its own (cage, headless) in a throwaway profile under
# tests/stations/run/, so the installed studio app, its settings and the screen are left
# alone. The keychain is the session's own: the run adds, and removes again, only entries for
# the three loopback addresses below, and its last check is that nothing else changed.
#
# The stack, all on loopback: one real control plane ("Kettle FM", :26513, with PostgreSQL on
# :26532, a station key and one member of staff) and two stand-in stations (fake-station.mjs,
# :26511 and :26512) whose request logs say which key went where. Test keys only.
#
# Needs: podman, node, cage, wlr-randr, tauri-driver, WebKitWebDriver, secret-tool; the studio
# app built with `cd apps/studio && pnpm tauri build --debug --no-bundle`; and, for the
# carry-over, a studio binary built the same way from before the list (r105, 73cb2a9):
#
#   OLD_STUDIO=/path/to/old/weavesignal-studio tests/stations/run-stations.sh
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); root=$(cd "$here/../.." && pwd); run="$here/run"
PG_PORT=26532; C_PORT=26513; A_PORT=26511; B_PORT=26512
: "${OLD_STUDIO:?the path of a studio binary built from before the list (see the head of this file)}"
for t in podman node cage wlr-randr tauri-driver WebKitWebDriver secret-tool; do command -v "$t" >/dev/null || { echo "$t is needed" >&2; exit 1; }; done
taken() { ss -H -ln -t "sport = :$1" | grep -q .; }
for p in $PG_PORT $C_PORT $A_PORT $B_PORT 4455 4456; do taken "$p" && { echo "port $p/tcp is in use" >&2; exit 1; }; done

rm -rf "$run"; mkdir -p "$run"/{cards,episodes,library,carts,recordings,sock} "$run"/profile/{config,data,cache}
pids=()
cleanup() {
  # The driver first: cage ends when its one client has gone, and does not end on a signal to itself.
  pkill -f "tauri-driver --port 4455" 2>/dev/null || true
  pkill -f "WebKitWebDriver --port=4456" 2>/dev/null || true
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  podman rm -f weavesignal-stations-pg >/dev/null 2>&1 || true
  # Whatever the run left in the keychain for the loopback stations goes; nothing else is touched.
  for p in $A_PORT $B_PORT $C_PORT; do secret-tool clear service weavesignal-studio username "http://127.0.0.1:$p" 2>/dev/null || true; done
}
trap cleanup EXIT; trap 'exit 130' INT TERM
podman rm -f weavesignal-stations-pg >/dev/null 2>&1 || true

cat > "$run/control.toml" <<TOML
[station]
id = "6f1c1f43-7d0e-4d3e-9b39-6c8f3f8d2a10"
name = "Kettle FM"
[ports]
control = $C_PORT
[playout]
socket = "$run/sock/playout.sock"
library_dir = "$run/library"
[carts]
media_dir = "$run/carts"
[video]
still_file = "$run/carts/stills/slate.png"
[retention]
recordings_dir = "$run/recordings"
[publishing]
public_url = "https://kettle.example"
place = "Dunfermline"
zone = "Europe/London"
cards_dir = "$run/cards"
episodes_dir = "$run/episodes"
[studio]
show = "The Morning Kettle"
[log]
format = "pretty"
filter = "info"
TOML
podman run -d --name weavesignal-stations-pg -p "127.0.0.1:$PG_PORT:5432" -e POSTGRES_PASSWORD=test-only -e POSTGRES_DB=weavesignal "${PG_IMAGE:-docker.io/library/postgres:18}" >/dev/null
cargo build -q -p weavesignal-control
for i in $(seq 1 60); do podman exec weavesignal-stations-pg pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 && break; sleep 0.5; done
WEAVESIGNAL_DATABASE_URL="postgres://postgres:test-only@127.0.0.1:$PG_PORT/weavesignal" WEAVESIGNAL_CONTROL_ADMIN_TOKEN=proof-key-kettle-real \
  "$root/target/debug/weavesignal-control" --config "$run/control.toml" > "$run/control.log" 2>&1 &
pids+=($!)
for i in $(seq 1 120); do
  curl -fs "http://127.0.0.1:$C_PORT/health" >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 120 ] && { echo "the control plane did not come up" >&2; exit 1; }
done
# A producer: a presenter with no show of their own is met by the show-setup dialog (ADR-0218).
curl -fsS -X POST "http://127.0.0.1:$C_PORT/api/v1/staff" -H 'Authorization: Bearer proof-key-kettle-real' -H 'Content-Type: application/json' \
  -d '{"username":"eilidh","name":"Eilidh Fraser-MacLeod","role":"producer","password":"a long password for the proof"}' >/dev/null
TEMPLATE=$(curl -fsS "http://127.0.0.1:$C_PORT/api/v1/studio" -H 'Authorization: Bearer proof-key-kettle-real')
VERSION=$(curl -fsS "http://127.0.0.1:$C_PORT/health" | sed 's/.*"version":"\([^"]*\)".*/\1/')
NAME="Kettle FM" PORT=$A_PORT KEY=proof-key-a VERSION=$VERSION ON_AIR=0 TEMPLATE="$TEMPLATE" node "$here/fake-station.mjs" > "$run/a.log" 2>&1 &
pids+=($!)
NAME="Radio Ross" PORT=$B_PORT KEY=proof-key-b VERSION=1.0.3 ON_AIR=1 TEMPLATE="$TEMPLATE" node "$here/fake-station.mjs" > "$run/b.log" 2>&1 &
pids+=($!)

# The app's own display and profile.
before=$(ls "$XDG_RUNTIME_DIR" | grep -E '^wayland-[0-9]+$' || true)
env -u DISPLAY XDG_CONFIG_HOME="$run/profile/config" XDG_DATA_HOME="$run/profile/data" XDG_CACHE_HOME="$run/profile/cache" \
  WEBKIT_DISABLE_DMABUF_RENDERER=1 WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman \
  cage -- tauri-driver --port 4455 --native-port 4456 > "$run/driver.log" 2>&1 &
pids+=($!)
for i in $(seq 1 40); do taken 4455 && break; sleep 0.25; done
display=$(ls "$XDG_RUNTIME_DIR" | grep -E '^wayland-[0-9]+$' | grep -vxF "$before" | head -1)
WAYLAND_DISPLAY=$display wlr-randr --output HEADLESS-1 --custom-mode 1440x900

node "$here/carry-over.mjs"
node "$here/drive.mjs"
! grep -q '^FAIL' "$run/result.txt"
