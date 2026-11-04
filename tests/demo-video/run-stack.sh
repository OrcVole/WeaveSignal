#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The demo video's stack on loopback: the M3 join stack (LiveKit 1.13.10, Liquidsoap 2.5.0-rc1
# playout and icecast.server, the desk in its image, the control plane on the host, the public
# pages on Vite) with the M2 stack's PostgreSQL (staff sign-in, ADR-0127) and the demo cart
# pack, and playout's fallback set to the default loop so auto mode has something to play
# without a restart mid-show. Nobody is in the room: every person in the video joins from a
# browser page driven by tests/demo-video/run.mjs. The studio app's dev server adds no bearer:
# each studio page signs in as its own member of staff (`?token=`).
#
#   tests/demo-video/run-stack.sh        # up, and wait (Ctrl+C ends it)
#   tests/demo-video/run-stack.sh down   # stop playout (PLAYOUT DOWN)
#   tests/demo-video/run-stack.sh up     # start playout again
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/target/demo-video/run"
prefix=${DEMO_PREFIX:-weavesignal-demo}
LK_PORT=${LK_PORT:-28880}
LK_UDP_PORT=${LK_UDP_PORT:-28882}
SRT_PORT=${SRT_PORT:-28913}
LISTENERS_PORT=${LISTENERS_PORT:-28812}
FEED_PORT=${FEED_PORT:-28815}
CONTROL_PORT=${CONTROL_PORT:-28310}
CHAT_PORT=${CHAT_PORT:-28311}
STUDIO_PORT=${STUDIO_PORT:-28420}
WEB_PORT=${WEB_PORT:-28430}
PG_PORT=${PG_PORT:-28432}
LIQ_IMAGE=${LIQ_IMAGE:-docker.io/savonet/liquidsoap:v2.5.0-rc1}
LK_IMAGE=${LK_IMAGE:-docker.io/livekit/livekit-server:v1.13.10}
DESK_IMAGE=${DESK_IMAGE:-${WEAVESIGNAL_DESK_IMAGE:-localhost/weavesignal-desk-dev:m5}}
API_KEY=demovideo
API_SECRET=demo-video-test-only-secret-not-for-use
ADMIN_TOKEN=demo-admin-test-only
SOURCE_PASSWORD=studio-test-only
common=(--network host --security-opt label=disable)

case ${1:-} in
  down) podman stop -t 2 "$prefix-playout" >/dev/null && echo "playout stopped"; exit 0 ;;
  up) podman start "$prefix-playout" >/dev/null && echo "playout started"; exit 0 ;;
  '') ;;
  *) echo "usage: $0 [down|up]" >&2; exit 2 ;;
esac

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
mkdir -p "$run/sock" "$run/carts" "$run/fallback"
rm -f "$run/desk.sock" "$run/sock/playout.sock"
pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
  for c in desk playout listeners livekit pg; do
    podman logs "$prefix-$c" > "$run/$c.log" 2>&1 || true
    podman rm -f "$prefix-$c" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT
trap 'exit 130' INT TERM
cleanup
for p in "$LK_PORT" "$LISTENERS_PORT" "$FEED_PORT" "$CONTROL_PORT" "$STUDIO_PORT" "$WEB_PORT" "$PG_PORT" "$CHAT_PORT"; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done
for p in "$LK_UDP_PORT" "$SRT_PORT"; do taken -u "$p" && { echo "port $p/udp is in use; set another" >&2; exit 1; }; done

cat > "$run/livekit.yaml" <<YAML
port: $LK_PORT
bind_addresses: ["127.0.0.1"]
rtc:
  udp_port: $LK_UDP_PORT
  tcp_port: 0
  use_external_ip: false
  node_ip: 127.0.0.1
keys:
  $API_KEY: $API_SECRET
logging:
  level: ${LK_LOG:-warn}
  pion_level: error
YAML
cp "$root/assets/default-loop/default-loop.flac" "$run/default-loop.flac"
cat > "$run/playout.json" <<JSON
{
  "station_id": "00000000-0000-0000-0000-000000000000",
  "srt": { "host": "127.0.0.1", "port": $SRT_PORT },
  "listeners": { "bind": "127.0.0.1", "host": "127.0.0.1", "port": $LISTENERS_PORT, "mount": "programme.opus", "feed_port": $FEED_PORT },
  "control_socket": "/sock/playout.sock",
  "carts": { "dir": "/carts" },
  "fallback": { "dir": "/r/fallback", "default_file": "/r/default-loop.flac" }
}
JSON
cat > "$run/desk.toml" <<TOML
[livekit]
url = "ws://127.0.0.1:$LK_PORT"
api_key = "$API_KEY"
room = "studio"
[programme]
srt_port = $SRT_PORT
[control]
socket = "/work/target/demo-video/run/desk.sock"
[carts]
dir = "/work/target/demo-video/run/carts"
TOML
at() { date -d "@$(( $(date +%s) + $1 ))" +%H:%M; }
cat > "$run/control.toml" <<TOML
[station]
id = "6f1c1f43-7d0e-4d3e-9b39-6c8f3f8d2a10"
name = "Kettle FM"
[ports]
control = $CONTROL_PORT
chat = $CHAT_PORT
[room]
public_url = "ws://127.0.0.1:$LK_PORT"
api_url = "http://127.0.0.1:$LK_PORT"
api_key = "$API_KEY"
name = "studio"
[desk]
socket = "$run/desk.sock"
poll_ms = 250
[playout]
socket = "$run/sock/playout.sock"
[carts]
media_dir = "$run/carts"
demo_pack = "$root/assets/carts-demo"
worker = "$root/target/release/weavesignal-control"
[studio]
show = "The Morning Kettle"
[[studio.rundown]]
at = "$(at -120)"
title = "Leith in the 1950s"
detail = "Gregor Stewart, in the studio by link"
[[studio.rundown]]
at = "$(at 120)"
title = "Phone-in"
detail = "callers through Shona"
[[studio.rundown]]
at = "$(at 600)"
title = "Steeped · Finlay Grant"
detail = "handover"
[join]
stream_url = "http://127.0.0.1:$LISTENERS_PORT/programme.opus"
consent_contact = "calls@kettle.example"
[log]
format = "pretty"
filter = "info"
TOML

podman run -d --name "$prefix-livekit" "${common[@]}" -v "$run/livekit.yaml:/etc/livekit.yaml:ro" \
  "$LK_IMAGE" --config /etc/livekit.yaml >/dev/null
podman run -d --name "$prefix-pg" -p "127.0.0.1:$PG_PORT:5432" -e POSTGRES_PASSWORD=test-only \
  -e POSTGRES_DB=weavesignal docker.io/library/postgres:18 >/dev/null
liq=("${common[@]}" --user 0:0 -v "$root/playout:/p:ro" -v "$run:/r:ro" -v "$run/sock:/sock"
     -v "$run/carts:/carts:ro" -e WEAVESIGNAL_PLAYOUT_DATA=/r/playout.json
     -e "WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD=$SOURCE_PASSWORD" --entrypoint /usr/bin/liquidsoap "$LIQ_IMAGE")
podman run -d --name "$prefix-listeners" "${liq[@]}" /p/listeners.liq >/dev/null
podman run -d --name "$prefix-playout" "${liq[@]}" /p/weavesignal.liq >/dev/null
for i in $(seq 1 60); do
  curl -fs "http://127.0.0.1:$LK_PORT" >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && { echo "LiveKit did not come up" >&2; exit 1; }
done

[ -x "$root/target/container/debug/weavesignal-desk" ] || { echo "build the desk first: just desk-build" >&2; exit 1; }
podman run -d --name "$prefix-desk" "${common[@]}" -v "$root:/work" -w /work \
  -e "WEAVESIGNAL_LIVEKIT_API_SECRET=$API_SECRET" -e RUST_LOG=weavesignal_desk=info \
  "$DESK_IMAGE" /work/target/container/debug/weavesignal-desk --config /work/target/demo-video/run/desk.toml >/dev/null

cargo build -q -p weavesignal-control -p weavesignal-chat
cargo build -q --release -p weavesignal-control
for i in $(seq 1 60); do
  podman exec "$prefix-pg" pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && { echo "PostgreSQL did not come up" >&2; exit 1; }
done
WEAVESIGNAL_DATABASE_URL="postgres://postgres:test-only@127.0.0.1:$PG_PORT/weavesignal" \
WEAVESIGNAL_LIVEKIT_API_SECRET=$API_SECRET WEAVESIGNAL_CONTROL_ADMIN_TOKEN=$ADMIN_TOKEN \
  "$root/target/debug/weavesignal-control" --config "$run/control.toml" > "$run/control.log" 2>&1 &
pids+=($!)
for i in $(seq 1 60); do
  curl -fs "http://127.0.0.1:$CONTROL_PORT/health" >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && { echo "the control plane did not come up" >&2; exit 1; }
done
# The chat, so the screener's chat panels and the player's chat are as a station's are (an
# empty room); its sign-in codes would go to an outbox, unused here.
mkdir -p "$run/outbox"
cat > "$run/chat.toml" <<TOML
[station]
id = "6f1c1f43-7d0e-4d3e-9b39-6c8f3f8d2a10"
name = "Kettle FM"
[listen]
port = $CHAT_PORT
[control]
url = "http://127.0.0.1:$CONTROL_PORT"
[mail]
mode = "outbox"
from = "chat@kettle.example"
outbox_dir = "$run/outbox"
[defaults]
terms = "Be kind to the callers and to each other. No abuse, no spam, nothing unlawful."
accountable_person = "Shona Campbell, station manager"
contact = "chat@kettle.example"
[log]
format = "pretty"
filter = "info"
TOML
WEAVESIGNAL_DATABASE_URL="postgres://postgres:test-only@127.0.0.1:$PG_PORT/weavesignal" WEAVESIGNAL_CONTROL_ADMIN_TOKEN=$ADMIN_TOKEN \
WEAVESIGNAL_CHAT_EMAIL_KEY=demo-video-email-key-0123456789abcdef \
  "$root/target/debug/weavesignal-chat" --config "$run/chat.toml" > "$run/chat.log" 2>&1 &
pids+=($!)
for i in $(seq 1 60); do
  curl -fs "http://127.0.0.1:$CHAT_PORT/health" >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && { echo "the chat did not come up" >&2; exit 1; }
done

(
  cd "$root/apps/studio"
  WEAVESIGNAL_CONTROL_URL="http://127.0.0.1:$CONTROL_PORT" WEAVESIGNAL_CHAT_URL="http://127.0.0.1:$CHAT_PORT" WEAVESIGNAL_STUDIO_PORT=$STUDIO_PORT exec pnpm exec vite --logLevel warn
) > "$run/vite.log" 2>&1 &
pids+=($!)
(
  cd "$root/apps/web"
  WEAVESIGNAL_CONTROL_URL="http://127.0.0.1:$CONTROL_PORT" WEAVESIGNAL_CHAT_URL="http://127.0.0.1:$CHAT_PORT" WEAVESIGNAL_WEB_PORT=$WEB_PORT exec pnpm exec vite --logLevel warn
) > "$run/web.log" 2>&1 &
pids+=($!)
for u in "http://127.0.0.1:$STUDIO_PORT/" "http://127.0.0.1:$WEB_PORT/"; do
  for i in $(seq 1 60); do
    curl -fs "$u" >/dev/null 2>&1 && break
    sleep 0.5; [ "$i" = 60 ] && { echo "$u did not come up" >&2; exit 1; }
  done
done
echo "up: studio http://127.0.0.1:$STUDIO_PORT/ · player http://127.0.0.1:$WEB_PORT/ · control :$CONTROL_PORT · LiveKit :$LK_PORT · stream :$LISTENERS_PORT"
wait "${pids[0]}"
