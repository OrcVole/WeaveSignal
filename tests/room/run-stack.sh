#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The stack tests: LiveKit 1.13.10 and Liquidsoap 2.5.0-rc1 (playout and icecast.server, each
# in its own process) on loopback, then one of the desk's ignored stack tests in the desk image:
#   TEST=room     (default) three people in the room, found by role; tests/room.rs
#   TEST=churn    people join and leave in cycles; the desk returns to its baseline; tests/churn.rs
#   TEST=empty    the listener stream's byte rate with the room empty; tests/empty.rs
#   TEST=load     the desk's CPU with 4, 8 and 13 people, then with 2 carts sounding (a release
#                 build); tests/load.rs
#   TEST=contain  one person's failure stays theirs, and twelve join at once; tests/contain.rs
#   TEST=dump     the profanity delay dumped: the listener never hears what was held; tests/dump.rs
#   TEST=listen   a listener's connection lifetime over LISTEN_SECONDS (600); tests/listen.rs
#   TEST=cameras  cameras through the room to the desk's HLS, the room picture, 4 cameras' cost;
#                 tests/cameras.rs (M5)
#   TEST=rejoin   a person rejoining at once under the same identity is heard again; tests/rejoin.rs
#   TEST=quiet    a digitally silent microphone stays linked and is never subscribed again
#                 (issue 13); tests/quiet.rs
#   TEST=feed     video on, the real playout's output back to the desk as the video's sound while
#                 nobody is live (ADR-0172, review 0028 A1); tests/feed.rs
#   TEST=browsercam  video on, headless Chromium publishing a camera as the join page does (AV1;
#                 the talk station's Brave fault, 2026-11-11): the desk holds that person's one
#                 subscription and shows their camera; tests/browsercam.rs, with
#                 tests/room/browser-camera.mjs on the host (needs PLAYWRIGHT_CORE)
# WEAVESIGNAL_ROOM_VIDEO=1 runs any of them with video on and a camera beside each voice (the
# room's figures with video on); WEAVESIGNAL_DESK_GPU=1 hands the desk's container the GPU.
#   TEST=rehearsal  the rehearsal studio's canary (1.3): a live desk and a rehearsal desk, each the
#                 real binary in a process of its own; 1,733 Hz in the rehearsal room never reaches
#                 the mount, the rehearsal desk owns no listening port, moved people stand by
#                 until a press; the rehearsal desk's cost at rest and with six (a release build);
#                 tests/rehearsal.rs. TEST=load with WEAVESIGNAL_LOAD_MODE=rehearsal measures a
#                 rehearsal desk instead of a live one. TEST=rehearsal also runs the move through
#                 the real control plane (built in the desk image) on a throwaway PostgreSQL 18
#                 (PG_PORT, loopback) and its own CONTROL_PORT: rehearsal links, Move to the live
#                 studio, the pages' live passes, standing by, and the digit with the opening.
#   TEST=playout  playout, then the desk, killed and restarted; tests/playout.rs. When the
#                 test writes run/restart-playout, this script stops the playout container,
#                 waits 5 s, starts it again and writes run/playout-restarted.
# Every port is a setting; the defaults here sit clear of the station defaults (7310-7314,
# 7880-7882) and of spike 01's rehearsal ports (7880, 9000, 8010, 50000-50100).
#
#   LK_PORT=17880 LK_UDP_PORT=17882 SRT_PORT=19000 LISTENERS_PORT=18010 FEED_PORT=18015 ROOM_SECONDS=80 tests/room/run-stack.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/room/run"
LK_PORT=${LK_PORT:-17880}
LK_UDP_PORT=${LK_UDP_PORT:-17882}
SRT_PORT=${SRT_PORT:-19000}
LISTENERS_PORT=${LISTENERS_PORT:-18010}
# Playout's programme to the listener process, which holds the mount (listeners.liq).
FEED_PORT=${FEED_PORT:-18015}
DESK_FEED_PORT=${DESK_FEED_PORT:-18017}
# The live desk's hosts' sources listener in TEST=rehearsal (the rehearsal desk must hold none).
SOURCES_PORT=${SOURCES_PORT:-18018}
# TEST=rehearsal's control plane and its database (the move, R9).
CONTROL_PORT=${CONTROL_PORT:-18310}
PG_PORT=${PG_PORT:-18433}
PG_IMAGE=${PG_IMAGE:-docker.io/library/postgres:18}
ROOM_SECONDS=${ROOM_SECONDS:-80}
TEST=${TEST:-room}
[ "$TEST" = load ] && RELEASE=${RELEASE-1}
[ "$TEST" = rehearsal ] && RELEASE=${RELEASE-1}
case $TEST in room|playout|churn|contain|empty|load|dump|listen|cameras|rejoin|quiet|feed|browsercam|rehearsal) ;; *) echo "TEST must be room, playout, churn, contain, empty, load, dump, listen, cameras, rejoin, quiet, feed, browsercam or rehearsal" >&2; exit 2 ;; esac
if [ "$TEST" = browsercam ]; then
  : "${PLAYWRIGHT_CORE:?TEST=browsercam needs PLAYWRIGHT_CORE: an installed playwright-core with its Chromium}"
  WEAVESIGNAL_ROOM_VIDEO=1
fi
LIQ_IMAGE=${LIQ_IMAGE:-docker.io/savonet/liquidsoap:v2.5.0-rc1}
LK_IMAGE=${LK_IMAGE:-docker.io/livekit/livekit-server:v1.13.10}
DESK_IMAGE=${DESK_IMAGE:-${WEAVESIGNAL_DESK_IMAGE:-localhost/weavesignal-desk-dev:current}}
API_KEY=m1test
API_SECRET=m1-room-test-only-secret-not-for-use-0
SOURCE_PASSWORD=room-test-only
prefix=${PREFIX:-${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-room}

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$LK_PORT" "$LISTENERS_PORT" "$FEED_PORT" "$SOURCES_PORT"; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done
if [ "$TEST" = rehearsal ]; then
  for p in "$CONTROL_PORT" "$PG_PORT"; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done
fi
for p in "$LK_UDP_PORT" "$SRT_PORT"; do taken -u "$p" && { echo "port $p/udp is in use; set another" >&2; exit 1; }; done

mkdir -p "$run"
cat > "$run/livekit.yaml" <<YAML
# Room test: loopback only, test keys, one UDP port (ADR-0023), ICE-TCP and TURN off.
port: $LK_PORT
bind_addresses: ["127.0.0.1"]
rtc:
  udp_port: $LK_UDP_PORT
  tcp_port: 0
  use_external_ip: false
  node_ip: 127.0.0.1
room:
  # TEST=quiet unmutes a voice through the server API, as it muted it.
  enable_remote_unmute: true
  enabled_codecs:
    - mime: audio/opus
    - mime: audio/red
    - mime: video/vp8
    - mime: video/vp9
    - mime: video/av1
keys:
  $API_KEY: $API_SECRET
logging:
  level: info
YAML
# TEST=feed: playout sends its output back to the desk (ADR-0172) and plays a 3 kHz fallback
# once the desk has been quiet for 4 s, so the desk can tell playout's sound from its own.
# Every run starts without a fallback folder: one left by TEST=feed's 3 kHz tone is heard by
# the next test's playout (it made TEST=dump hear its "dumped marker", 2026-11-04).
rm -rf "$run/fallback"
feed_extra=
if [ "$TEST" = feed ]; then
  mkdir -p "$run/fallback"
  ffmpeg -v error -f lavfi -i "sine=frequency=3000:duration=30" -ac 2 -ar 48000 "$run/fallback/fallback-3k.flac"
  feed_extra=", \"desk_feed\": { \"host\": \"127.0.0.1\", \"port\": $DESK_FEED_PORT }, \"fallback\": { \"dir\": \"/r/fallback\" }, \"programme\": { \"mode\": \"live\", \"dead_air_s\": 4.0 }"
fi
cat > "$run/playout.json" <<JSON
{
  "station_id": "00000000-0000-0000-0000-000000000000",
  "srt": { "host": "127.0.0.1", "port": $SRT_PORT },
  "listeners": { "bind": "127.0.0.1", "host": "127.0.0.1", "port": $LISTENERS_PORT, "mount": "programme.opus", "feed_port": $FEED_PORT, "bitrate": ${OPUS_RATE:-96} },
  "control_socket": "/tmp/weavesignal-playout.sock",
  "carts": { "dir": "/var/lib/weavesignal/carts" }$feed_extra
}
JSON

cleanup() {
  [ -n "${restarter:-}" ] && kill "$restarter" 2>/dev/null
  if [ -n "${browser:-}" ]; then kill "$browser" 2>/dev/null || true; fi
  rm -f "$run/restart-playout" "$run/playout-restarted"
  for c in livekit playout listeners pg; do
    podman logs "$prefix-$c" > "$run/$c.log" 2>&1 || true
    podman rm -f "$prefix-$c" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT
cleanup

common=(--network host --security-opt label=disable)
podman run -d --name "$prefix-livekit" "${common[@]}" -v "$run/livekit.yaml:/etc/livekit.yaml:ro" \
  "$LK_IMAGE" --config /etc/livekit.yaml >/dev/null
liq=("${common[@]}" -v "$root/playout:/p:ro" -v "$run:/r:ro" -e WEAVESIGNAL_PLAYOUT_DATA=/r/playout.json
     -e "WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD=$SOURCE_PASSWORD" --entrypoint /usr/bin/liquidsoap "$LIQ_IMAGE")
podman run -d --name "$prefix-listeners" "${liq[@]}" /p/listeners.liq >/dev/null
podman run -d --name "$prefix-playout" "${liq[@]}" /p/weavesignal.liq >/dev/null
if [ "$TEST" = rehearsal ]; then
  podman run -d --name "$prefix-pg" -p "127.0.0.1:$PG_PORT:5432" -e POSTGRES_PASSWORD=test-only \
    -e POSTGRES_DB=weavesignal "$PG_IMAGE" >/dev/null
  for i in $(seq 1 60); do
    podman exec "$prefix-pg" pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 && break
    sleep 0.5; [ "$i" = 60 ] && { echo "PostgreSQL did not come up" >&2; exit 1; }
  done
fi

for i in $(seq 1 60); do
  curl -fs "http://127.0.0.1:$LK_PORT" >/dev/null 2>&1 && taken -t "$LISTENERS_PORT" && break
  sleep 0.5
  [ "$i" = 60 ] && { echo "the stack did not come up" >&2; exit 1; }
done
echo "stack up: LiveKit :$LK_PORT (udp $LK_UDP_PORT), SRT :$SRT_PORT, icecast.server :$LISTENERS_PORT"

if [ "$TEST" = playout ]; then
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
fi

if [ "$TEST" = browsercam ]; then
  RUN="$run" PLAYWRIGHT_CORE="$PLAYWRIGHT_CORE" node "$root/tests/room/browser-camera.mjs" > "$run/browser.log" 2>&1 &
  browser=$!
fi

gpu=()
[ "${WEAVESIGNAL_DESK_GPU:-}" = 1 ] && gpu=(--device nvidia.com/gpu=all)
# TEST=rehearsal's control plane, built in the desk image into the same target directory.
if [ "$TEST" = rehearsal ]; then
  podman run --rm "${common[@]}" -v "$root:/work" -v "$HOME/.cargo/registry:/root/.cargo/registry" -w /work \
    -e CARGO_TARGET_DIR=/work/target/container -e "CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-4}" \
    "$DESK_IMAGE" cargo build --release -q -p weavesignal-control
fi
podman run --rm "${common[@]}" "${gpu[@]}" -v "$root:/work" -v "$HOME/.cargo/registry:/root/.cargo/registry" -w /work \
  -e CARGO_TARGET_DIR=/work/target/container -e "CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-4}" \
  -e "WEAVESIGNAL_ROOM_LIVEKIT_URL=ws://127.0.0.1:$LK_PORT" \
  -e "WEAVESIGNAL_ROOM_API_KEY=$API_KEY" -e "WEAVESIGNAL_ROOM_API_SECRET=$API_SECRET" \
  -e "WEAVESIGNAL_ROOM_SRT_PORT=$SRT_PORT" \
  -e "WEAVESIGNAL_ROOM_SOURCES_PORT=$SOURCES_PORT" -e "WEAVESIGNAL_LOAD_MODE=${WEAVESIGNAL_LOAD_MODE:-live}" \
  -e "WEAVESIGNAL_ROOM_DESK_FEED_PORT=$DESK_FEED_PORT" -e "WEAVESIGNAL_PLAYOUT_FEED_PASSWORD=$SOURCE_PASSWORD" \
  -e "WEAVESIGNAL_ROOM_LISTENER_URL=http://127.0.0.1:$LISTENERS_PORT/programme.opus" \
  -e "WEAVESIGNAL_ROOM_OPUS_KBPS=${OPUS_RATE:-96}" \
  -e "WEAVESIGNAL_ROOM_SECONDS=$ROOM_SECONDS" \
  -e "WEAVESIGNAL_CHURN_CYCLES=${CHURN_CYCLES:-4}" \
  -e "WEAVESIGNAL_LOAD_MONITORS=${LOAD_MONITORS:-0}" -e "WEAVESIGNAL_LOAD_MONITOR_ROLE=${LOAD_MONITOR_ROLE:-producer}" \
  -e "WEAVESIGNAL_LOAD_STEPS=${LOAD_STEPS:-4,8,13}" \
  -e "WEAVESIGNAL_LOAD_CARTS=${LOAD_CARTS:-2}" \
  -e "WEAVESIGNAL_LISTEN_SECONDS=${LISTEN_SECONDS:-600}" \
  -e "WEAVESIGNAL_ROOM_REPORT=/work/tests/room/run/report-$TEST${WEAVESIGNAL_ROOM_VIDEO:+-video}.json" \
  -e "WEAVESIGNAL_ROOM_VIDEO=${WEAVESIGNAL_ROOM_VIDEO:-}" \
  -e WEAVESIGNAL_STACK_DIR=/work/tests/room/run \
  -e "WEAVESIGNAL_ROOM_CONTROL_PORT=$CONTROL_PORT" \
  -e "WEAVESIGNAL_ROOM_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:$PG_PORT/weavesignal" \
  -e WEAVESIGNAL_CONTROL_BIN=/work/target/container/release/weavesignal-control \
  -e RUST_LOG="${RUST_LOG:-weavesignal_desk=info}" \
  ${GST_DEBUG:+-e "GST_DEBUG=$GST_DEBUG"} \
  "$DESK_IMAGE" cargo test ${RELEASE:+--release} -p weavesignal-desk --test "$TEST" -- --ignored --nocapture --test-threads=1
