#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The Schedule tab and the card editor against the control plane on loopback (the studio
# screens build): a throwaway PostgreSQL 18, the control plane on the host with a database, a
# playout socket path (nothing listens on it: the schedule's routes need only the database),
# cards on and the slate's stills under the run (so the show editor's picture works), seeded
# with Kettle FM's week (tests/screens/seed.mjs: shows, series and episodes from the Tartan
# list, clocks and the week grid, a few tone tracks, the log generated), and the
# interface on Vite's dev server, which passes /api to the control plane with the admin bearer.
# Then it waits: open http://127.0.0.1:$STUDIO_PORT/ (or drive it with tests/screens/drive.mjs)
# and stop it with Ctrl+C. No LiveKit, no Liquidsoap, no desk: the room is not this test's.
# Ports sit in 26xxx by default, clear of the station defaults and the other stack tests.
#
#   tests/screens/run-screens.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/screens/run"
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-screens
CONTROL_PORT=${CONTROL_PORT:-26310}
STUDIO_PORT=${STUDIO_PORT:-26420}
PG_PORT=${PG_PORT:-26432}
PG_IMAGE=${PG_IMAGE:-docker.io/library/postgres:18}
ADMIN_TOKEN=screens-admin-test-only

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$CONTROL_PORT" "$STUDIO_PORT" "$PG_PORT"; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done

rm -rf "$run"; mkdir -p "$run/cards" "$run/episodes" "$run/library" "$run/carts" "$run/recordings" "$run/sock"
pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
  podman logs "$prefix-pg" > "$run/pg.log" 2>&1 || true
  podman rm -f "$prefix-pg" >/dev/null 2>&1 || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM
cleanup

cat > "$run/control.toml" <<TOML
[station]
id = "6f1c1f43-7d0e-4d3e-9b39-6c8f3f8d2a10"
name = "Kettle FM"
[ports]
control = $CONTROL_PORT
[playout]
socket = "$run/sock/playout.sock"
library_dir = "$run/library"
[carts]
media_dir = "$run/carts"
worker = "$root/target/release/weavesignal-control"
[video]
still_file = "$run/carts/stills/slate.png"
[retention]
recordings_dir = "$run/recordings"
[publishing]
public_url = "https://kettle.example"
place = "Dunfermline"
zone = "${STATION_ZONE:-Europe/London}"
cards_dir = "$run/cards"
episodes_dir = "$run/episodes"
[studio]
show = "The Morning Kettle"
[log]
format = "pretty"
filter = "info"
TOML

podman run -d --name "$prefix-pg" -p "127.0.0.1:$PG_PORT:5432" -e POSTGRES_PASSWORD=test-only \
  -e POSTGRES_DB=weavesignal "$PG_IMAGE" >/dev/null
cargo build -q -p weavesignal-control
cargo build -q --release -p weavesignal-control
for i in $(seq 1 60); do
  podman exec "$prefix-pg" pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && { echo "PostgreSQL did not come up" >&2; exit 1; }
done
WEAVESIGNAL_DATABASE_URL="postgres://postgres:test-only@127.0.0.1:$PG_PORT/weavesignal" \
WEAVESIGNAL_CONTROL_ADMIN_TOKEN=$ADMIN_TOKEN \
  "$root/target/debug/weavesignal-control" --config "$run/control.toml" > "$run/control.log" 2>&1 &
pids+=($!)
for i in $(seq 1 60); do
  curl -fs "http://127.0.0.1:$CONTROL_PORT/health" >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && { echo "the control plane did not come up" >&2; exit 1; }
done

# Kettle FM's week.
CONTROL="http://127.0.0.1:$CONTROL_PORT" ADMIN_TOKEN=$ADMIN_TOKEN node "$root/tests/screens/seed.mjs"

# The interface, with /api passed to the control plane and the bearer added by the server.
(
  cd "$root/apps/studio"
  WEAVESIGNAL_CONTROL_URL="http://127.0.0.1:$CONTROL_PORT" WEAVESIGNAL_CONTROL_TOKEN=$ADMIN_TOKEN \
    WEAVESIGNAL_STUDIO_PORT=$STUDIO_PORT exec pnpm exec vite --logLevel warn
) > "$run/vite.log" 2>&1 &
pids+=($!)
for i in $(seq 1 60); do
  curl -fs "http://127.0.0.1:$STUDIO_PORT/" >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && { echo "the interface did not come up" >&2; exit 1; }
done
echo "up: the studio app at http://127.0.0.1:$STUDIO_PORT/ (control :$CONTROL_PORT, PostgreSQL :$PG_PORT); end: Ctrl+C"
wait "${pids[0]}"
