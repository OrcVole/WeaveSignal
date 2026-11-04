#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The cart wall's stack test: the desk (a release build in the desk image, its profanity delay
# at 3 s), Liquidsoap 2.5.0-rc1 (playout and icecast.server) and a throwaway PostgreSQL 18 on
# loopback, the control plane (a release build, which is also the media worker) with the demo
# pack as its banks; then crates/control/tests/carts_stack.rs fires carts over HTTP and finds
# them in the listener stream. Carts sound in the desk (ADR-0117). No LiveKit and nobody in
# the room: the programme is the desk's silence, so anything heard is a cart.
# Ports sit in 31000-31999, clear of the station defaults and the other stack tests.
#
#   tests/carts/run-carts.sh
#   CONTROL_PORT=31310 LISTENERS_PORT=31812 FEED_PORT=31815 SRT_PORT=31913 PG_PORT=31433 tests/carts/run-carts.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/carts/run"
CONTROL_PORT=${CONTROL_PORT:-31310}
LISTENERS_PORT=${LISTENERS_PORT:-31812}
FEED_PORT=${FEED_PORT:-31815}
SRT_PORT=${SRT_PORT:-31913}
PG_PORT=${PG_PORT:-31433}
LIQ_IMAGE=${LIQ_IMAGE:-docker.io/savonet/liquidsoap:v2.5.0-rc1}
PG_IMAGE=${PG_IMAGE:-docker.io/library/postgres:18}
DESK_IMAGE=${DESK_IMAGE:-${WEAVESIGNAL_DESK_IMAGE:-localhost/weavesignal-desk-dev:m5}}
DELAY_MS=${DELAY_MS:-3000}
SOURCE_PASSWORD=carts-test-only
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-carts

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$CONTROL_PORT" "$LISTENERS_PORT" "$FEED_PORT" "$PG_PORT"; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done
taken -u "$SRT_PORT" && { echo "port $SRT_PORT/udp is in use; set another" >&2; exit 1; }

rm -rf "$run"; mkdir -p "$run/carts" "$run/sock"
cat > "$run/playout.json" <<JSON
{
  "station_id": "00000000-0000-0000-0000-000000000000",
  "srt": { "host": "127.0.0.1", "port": $SRT_PORT },
  "listeners": { "bind": "127.0.0.1", "host": "127.0.0.1", "port": $LISTENERS_PORT, "mount": "programme.opus", "feed_port": $FEED_PORT },
  "control_socket": "/sock/playout.sock",
  "carts": { "dir": "/carts" }
}
JSON
# The desk: no room to join (the room watch says so and keeps mixing), its SRT listener for
# playout, its control socket beside playout's, and the cart files the media worker writes.
cat > "$run/desk.toml" <<TOML
[livekit]
url = "ws://127.0.0.1:1"
api_key = "carts-test-only"
room = "carts"

[desk]
realtime = false

[programme]
srt_port = $SRT_PORT

[programme.delay]
target_ms = $DELAY_MS

[control]
socket = "/r/sock/desk.sock"

[carts]
dir = "/r/carts"
TOML

cleanup() {
  for c in desk playout listeners pg; do
    podman logs "$prefix-$c" > "$run/$c.log" 2>&1 || true
    podman rm -f "$prefix-$c" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT
cleanup

cargo build --release -q -p weavesignal-control
common=(--network host --security-opt label=disable)
# The desk is built in its image (GStreamer), into the target directory the stack tests use.
podman run --rm "${common[@]}" -v "$root:/work" -v "$HOME/.cargo/registry:/root/.cargo/registry" -w /work \
  -e CARGO_TARGET_DIR=/work/target/container "$DESK_IMAGE" cargo build --release -q -p weavesignal-desk
podman run -d --name "$prefix-desk" "${common[@]}" --user 0:0 -v "$root/target/container/release:/bin-desk:ro" \
  -v "$run:/r" -e WEAVESIGNAL_LIVEKIT_API_SECRET=carts-test-only -e RUST_LOG=weavesignal_desk=info \
  "$DESK_IMAGE" /bin-desk/weavesignal-desk --config /r/desk.toml >/dev/null
podman run -d --name "$prefix-pg" -p "127.0.0.1:$PG_PORT:5432" -e POSTGRES_PASSWORD=test-only \
  -e POSTGRES_DB=weavesignal "$PG_IMAGE" >/dev/null
# Rootless Podman maps the container's root to this user, so the socket playout makes is ours
# to connect to. Carts sound in the desk, so playout reads no cart files.
liq=("${common[@]}" --user 0:0 -v "$root/playout:/p:ro" -v "$run:/r:ro" -v "$run/sock:/sock"
     -e WEAVESIGNAL_PLAYOUT_DATA=/r/playout.json
     -e "WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD=$SOURCE_PASSWORD" --entrypoint /usr/bin/liquidsoap "$LIQ_IMAGE")
podman run -d --name "$prefix-listeners" "${liq[@]}" /p/listeners.liq >/dev/null
podman run -d --name "$prefix-playout" "${liq[@]}" /p/weavesignal.liq >/dev/null

for i in $(seq 1 60); do
  podman exec "$prefix-pg" pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 && [ -S "$run/sock/playout.sock" ] && [ -S "$run/sock/desk.sock" ] && break
  sleep 0.5
  [ "$i" = 60 ] && { echo "the stack did not come up" >&2; exit 1; }
done
echo "stack up: the desk (SRT :$SRT_PORT, delay $DELAY_MS ms, socket $run/sock/desk.sock), icecast.server :$LISTENERS_PORT, PostgreSQL :$PG_PORT"

WEAVESIGNAL_CARTS_STACK=1 \
WEAVESIGNAL_CONTROL_BIN="$root/target/release/weavesignal-control" \
WEAVESIGNAL_TEST_DATABASE_URL="postgres://postgres:test-only@127.0.0.1:$PG_PORT/weavesignal" \
WEAVESIGNAL_CARTS_RUN="$run" \
WEAVESIGNAL_CARTS_CONTROL_PORT="$CONTROL_PORT" \
WEAVESIGNAL_CARTS_LISTENER_URL="http://127.0.0.1:$LISTENERS_PORT/programme.opus" \
WEAVESIGNAL_CARTS_REPORT="$run/report.json" \
  cargo test -p weavesignal-control --test carts_stack -- --ignored --nocapture --test-threads=1
