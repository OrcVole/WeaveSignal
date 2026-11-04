#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The playout layer's stack test (M4): Liquidsoap 2.5.0-rc1 (playout and the listener process),
# a stand-in desk (Liquidsoap sending a 1 kHz tone over SRT, silenced while run/desk-quiet
# exists), a throwaway PostgreSQL 18 and the control plane
# (a release build, also the media worker). crates/control/tests/playout_stack.rs then drives
# it: dead air to automation and back, playout killed under a listener who stays connected,
# the per-play log, the audience counts, now playing with and without up-next, and the logger.
# When the test writes run/restart-playout, this script stops the playout container, waits 5 s,
# starts it again and writes run/playout-restarted.
# Ports sit in 33000-33999 by default, clear of the station defaults and the other stack tests.
#
#   CONTROL_PORT=33310 LISTENERS_PORT=33812 FEED_PORT=33815 SRT_PORT=33913 PG_PORT=33433 tests/playout/run-playout.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/playout/run"
CONTROL_PORT=${CONTROL_PORT:-33310}
LISTENERS_PORT=${LISTENERS_PORT:-33812}
FEED_PORT=${FEED_PORT:-33815}
SRT_PORT=${SRT_PORT:-33913}
PG_PORT=${PG_PORT:-33433}
LIQ_IMAGE=${LIQ_IMAGE:-docker.io/savonet/liquidsoap:v2.5.0-rc1}
PG_IMAGE=${PG_IMAGE:-docker.io/library/postgres:18}
SOURCE_PASSWORD=playout-test-only
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-playout

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$CONTROL_PORT" "$LISTENERS_PORT" "$FEED_PORT" "$PG_PORT"; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done
taken -u "$SRT_PORT" && { echo "port $SRT_PORT/udp is in use; set another" >&2; exit 1; }

rm -rf "$run"; mkdir -p "$run/library" "$run/fallback" "$run/recordings/logger" "$run/carts" "$run/sock"
# The station's fallback: a 440 Hz file, so a listener can tell it from the desk and the music.
ffmpeg -v error -f lavfi -i "sine=frequency=440:duration=20" -ac 2 -ar 48000 "$run/fallback/fallback-440.flac"
cat > "$run/playout.json" <<JSON
{
  "station_id": "00000000-0000-0000-0000-000000000000",
  "srt": { "host": "127.0.0.1", "port": $SRT_PORT, "format": "mpegts" },
  "listeners": { "bind": "127.0.0.1", "host": "127.0.0.1", "port": $LISTENERS_PORT, "mount": "programme.opus", "feed_port": $FEED_PORT, "control_socket": "/sock/listeners.sock" },
  "control_socket": "/sock/playout.sock",
  "carts": { "dir": "/media/carts" },
  "library": { "dir": "/media/library" },
  "fallback": { "dir": "/media/fallback" },
  "logger": { "dir": "/media/recordings/logger", "bitrate": 48 },
  "programme": { "mode": "live", "dead_air_s": 4.0, "dead_air_db": -50.0 }
}
JSON
# The stand-in desk: the SRT listener, as the desk is; a tone, or silence while desk-quiet exists.
# MPEG-TS, not the desk's Matroska: Liquidsoap sends a container's header once, so a playout that
# connects again would get none (the real desk sends it to each connection).
cat > "$run/desk.liq" <<LIQ
s = amplify({if file.exists("/r/desk-quiet") then 0. else 1. end}, sine(1000.))
output.srt(mode="listener", bind_address="127.0.0.1", port=$SRT_PORT,
  %ffmpeg(format="mpegts", %audio(codec="mp2", b="256k", ac=2, ar=48000)), s)
LIQ

cleanup() {
  [ -n "${restarter:-}" ] && kill "$restarter" 2>/dev/null
  for c in playout listeners desk pg; do
    podman logs "$prefix-$c" > "$run/$c.log" 2>&1 || true
    podman rm -f "$prefix-$c" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT
cleanup

cargo build --release -q -p weavesignal-control
common=(--network host --security-opt label=disable)
podman run -d --name "$prefix-pg" -p "127.0.0.1:$PG_PORT:5432" -e POSTGRES_PASSWORD=test-only \
  -e POSTGRES_DB=weavesignal "$PG_IMAGE" >/dev/null
liq=("${common[@]}" --user 0:0 -v "$root/playout:/p:ro" -v "$run:/r:ro" -v "$run/sock:/sock"
     -v "$run/library:/media/library:ro" -v "$run/carts:/media/carts:ro" -v "$run/fallback:/media/fallback:ro"
     -v "$run/recordings:/media/recordings" -e WEAVESIGNAL_PLAYOUT_DATA=/r/playout.json
     -e "WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD=$SOURCE_PASSWORD" --entrypoint /usr/bin/liquidsoap "$LIQ_IMAGE")
podman run -d --name "$prefix-listeners" "${liq[@]}" /p/listeners.liq >/dev/null
podman run -d --name "$prefix-desk" "${liq[@]}" /r/desk.liq >/dev/null
podman run -d --name "$prefix-playout" "${liq[@]}" /p/weavesignal.liq >/dev/null

for i in $(seq 1 60); do
  podman exec "$prefix-pg" pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 \
    && [ -S "$run/sock/playout.sock" ] && [ -S "$run/sock/listeners.sock" ] && break
  sleep 0.5
  [ "$i" = 60 ] && { echo "the stack did not come up" >&2; exit 1; }
done
echo "stack up: listener process :$LISTENERS_PORT (feed :$FEED_PORT), PostgreSQL :$PG_PORT, desk SRT :$SRT_PORT"

(
  while :; do
    if [ -e "$run/restart-playout" ]; then
      rm -f "$run/restart-playout"
      echo "stopping playout"; podman stop -t 0 "$prefix-playout" >/dev/null
      sleep 5
      echo "starting playout"; podman start "$prefix-playout" >/dev/null
      touch "$run/playout-restarted"
    fi
    sleep 0.2
  done
) &
restarter=$!

WEAVESIGNAL_PLAYOUT_STACK=1 \
WEAVESIGNAL_CONTROL_BIN="$root/target/release/weavesignal-control" \
WEAVESIGNAL_TEST_DATABASE_URL="postgres://postgres:test-only@127.0.0.1:$PG_PORT/weavesignal" \
WEAVESIGNAL_PLAYOUT_RUN="$run" \
WEAVESIGNAL_PLAYOUT_CONTROL_PORT="$CONTROL_PORT" \
WEAVESIGNAL_PLAYOUT_LISTENERS_PORT="$LISTENERS_PORT" \
WEAVESIGNAL_PLAYOUT_REPORT="$run/report.json" \
  cargo test -p weavesignal-control --test playout_stack -- --ignored --nocapture --test-threads=1
