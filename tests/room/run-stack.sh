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
#   TEST=feed     video on, the real playout's output back to the desk as the video's sound while
#                 nobody is live (ADR-0172, review 0028 A1); tests/feed.rs
# WEAVESIGNAL_ROOM_VIDEO=1 runs any of them with video on and a camera beside each voice (the
# room's figures with video on); WEAVESIGNAL_DESK_GPU=1 hands the desk's container the GPU.
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
ROOM_SECONDS=${ROOM_SECONDS:-80}
TEST=${TEST:-room}
[ "$TEST" = load ] && RELEASE=${RELEASE-1}
case $TEST in room|playout|churn|contain|empty|load|dump|listen|cameras|rejoin|feed) ;; *) echo "TEST must be room, playout, churn, contain, empty, load, dump, listen, cameras, rejoin or feed" >&2; exit 2 ;; esac
LIQ_IMAGE=${LIQ_IMAGE:-docker.io/savonet/liquidsoap:v2.5.0-rc1}
LK_IMAGE=${LK_IMAGE:-docker.io/livekit/livekit-server:v1.13.10}
DESK_IMAGE=${DESK_IMAGE:-${WEAVESIGNAL_DESK_IMAGE:-localhost/weavesignal-desk-dev:m5}}
API_KEY=m1test
API_SECRET=m1-room-test-only-secret-not-for-use-0
SOURCE_PASSWORD=room-test-only
prefix=${PREFIX:-${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-room}

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$LK_PORT" "$LISTENERS_PORT" "$FEED_PORT"; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done
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
  "listeners": { "bind": "127.0.0.1", "host": "127.0.0.1", "port": $LISTENERS_PORT, "mount": "programme.opus", "feed_port": $FEED_PORT },
  "control_socket": "/tmp/weavesignal-playout.sock",
  "carts": { "dir": "/var/lib/weavesignal/carts" }$feed_extra
}
JSON

cleanup() {
  [ -n "${restarter:-}" ] && kill "$restarter" 2>/dev/null
  rm -f "$run/restart-playout" "$run/playout-restarted"
  for c in livekit playout listeners; do
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

gpu=()
[ "${WEAVESIGNAL_DESK_GPU:-}" = 1 ] && gpu=(--device nvidia.com/gpu=all)
podman run --rm "${common[@]}" "${gpu[@]}" -v "$root:/work" -v "$HOME/.cargo/registry:/root/.cargo/registry" -w /work \
  -e CARGO_TARGET_DIR=/work/target/container \
  -e "WEAVESIGNAL_ROOM_LIVEKIT_URL=ws://127.0.0.1:$LK_PORT" \
  -e "WEAVESIGNAL_ROOM_API_KEY=$API_KEY" -e "WEAVESIGNAL_ROOM_API_SECRET=$API_SECRET" \
  -e "WEAVESIGNAL_ROOM_SRT_PORT=$SRT_PORT" \
  -e "WEAVESIGNAL_ROOM_DESK_FEED_PORT=$DESK_FEED_PORT" -e "WEAVESIGNAL_PLAYOUT_FEED_PASSWORD=$SOURCE_PASSWORD" \
  -e "WEAVESIGNAL_ROOM_LISTENER_URL=http://127.0.0.1:$LISTENERS_PORT/programme.opus" \
  -e "WEAVESIGNAL_ROOM_SECONDS=$ROOM_SECONDS" \
  -e "WEAVESIGNAL_CHURN_CYCLES=${CHURN_CYCLES:-4}" \
  -e "WEAVESIGNAL_LOAD_MONITORS=${LOAD_MONITORS:-0}" -e "WEAVESIGNAL_LOAD_MONITOR_ROLE=${LOAD_MONITOR_ROLE:-producer}" \
  -e "WEAVESIGNAL_LOAD_STEPS=${LOAD_STEPS:-4,8,13}" \
  -e "WEAVESIGNAL_LOAD_CARTS=${LOAD_CARTS:-2}" \
  -e "WEAVESIGNAL_LISTEN_SECONDS=${LISTEN_SECONDS:-600}" \
  -e "WEAVESIGNAL_ROOM_REPORT=/work/tests/room/run/report-$TEST${WEAVESIGNAL_ROOM_VIDEO:+-video}.json" \
  -e "WEAVESIGNAL_ROOM_VIDEO=${WEAVESIGNAL_ROOM_VIDEO:-}" \
  -e WEAVESIGNAL_STACK_DIR=/work/tests/room/run \
  -e RUST_LOG="${RUST_LOG:-weavesignal_desk=info}" \
  "$DESK_IMAGE" cargo test ${RELEASE:+--release} -p weavesignal-desk --test "$TEST" -- --ignored --nocapture --test-threads=1
