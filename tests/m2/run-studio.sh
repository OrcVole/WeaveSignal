#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The studio app against the real desk on loopback (M2): LiveKit 1.13.10, Liquidsoap 2.5.0-rc1
# (playout and icecast.server), the desk in its image with its control socket, PostgreSQL, the
# control plane on the host with the demo pack as its cart banks (the desk plays them from the
# media worker's files), synthetic people in the room (the desk B and screener pages' names,
# ADR-0071), and the interface on Vite's dev server, which passes /api to the control plane
# with the admin bearer. Then it waits: open http://127.0.0.1:$STUDIO_PORT/ (or drive it with
# tests/m2/drive.mjs), and stop it with Ctrl+C. Every port is clear of the station defaults and
# of spike 01's (7880, 9000, 8010, 8090), and of the room test's.
#
#   tests/m2/run-studio.sh            # up, and wait
#   tests/m2/run-studio.sh down       # stop playout (the PLAYOUT DOWN state) while it runs
#   tests/m2/run-studio.sh up         # start playout again
#   STEPS=1 tests/m2/run-studio.sh    # also a public address and a takedown contact, so shows can be
#                                     # saved and call-in opens: the beginner steps on a real station
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/m2/run"
prefix=${M2_PREFIX:-weavesignal-m2}
LK_PORT=${LK_PORT:-27880}
LK_UDP_PORT=${LK_UDP_PORT:-27882}
SRT_PORT=${SRT_PORT:-27913}
LISTENERS_PORT=${LISTENERS_PORT:-27812}
FEED_PORT=${FEED_PORT:-27815}
CONTROL_PORT=${CONTROL_PORT:-27310}
STUDIO_PORT=${STUDIO_PORT:-27420}
PG_PORT=${PG_PORT:-27432}
LIQ_IMAGE=${LIQ_IMAGE:-docker.io/savonet/liquidsoap:v2.5.0-rc1}
LK_IMAGE=${LK_IMAGE:-docker.io/livekit/livekit-server:v1.13.10}
DESK_IMAGE=${DESK_IMAGE:-${WEAVESIGNAL_DESK_IMAGE:-localhost/weavesignal-desk-dev:m5}}
API_KEY=m2studio
API_SECRET=m2-studio-test-only-secret-not-for-use-0
ADMIN_TOKEN=m2-admin-test-only
SOURCE_PASSWORD=studio-test-only
common=(--network host --security-opt label=disable)

case ${1:-} in
  down) podman stop -t 2 "$prefix-playout" >/dev/null && echo "playout stopped"; exit 0 ;;
  up) podman start "$prefix-playout" >/dev/null && echo "playout started"; exit 0 ;;
  '') ;;
  *) echo "usage: $0 [down|up]" >&2; exit 2 ;;
esac

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$LK_PORT" "$LISTENERS_PORT" "$FEED_PORT" "$CONTROL_PORT" "$STUDIO_PORT" "$PG_PORT"; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done
for p in "$LK_UDP_PORT" "$SRT_PORT"; do taken -u "$p" && { echo "port $p/udp is in use; set another" >&2; exit 1; }; done

mkdir -p "$run/sock" "$run/carts"
rm -f "$run/desk.sock" "$run/sock/playout.sock"
pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
  for c in people desk playout listeners livekit pg; do
    podman logs "$prefix-$c" > "$run/$c.log" 2>&1 || true
    podman rm -f "$prefix-$c" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT
trap 'exit 130' INT TERM
cleanup

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
YAML
cat > "$run/playout.json" <<JSON
{
  "station_id": "00000000-0000-0000-0000-000000000000",
  "srt": { "host": "127.0.0.1", "port": $SRT_PORT },
  "listeners": { "bind": "127.0.0.1", "host": "127.0.0.1", "port": $LISTENERS_PORT, "mount": "programme.opus", "feed_port": $FEED_PORT },
  "control_socket": "/sock/playout.sock",
  "carts": { "dir": "/carts" }
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
socket = "/work/tests/m2/run/desk.sock"
# The desk plays carts from the files the control plane's media worker writes (carts.media_dir).
[carts]
dir = "/work/tests/m2/run/carts"
TOML
# The rundown: the next hour around now, so the clock has a hard post to back-time to.
at() { date -d "@$(( $(date +%s) + $1 ))" +%H:%M; }
cat > "$run/control.toml" <<TOML
[station]
id = "6f1c1f43-7d0e-4d3e-9b39-6c8f3f8d2a10"
name = "Kettle FM"
[ports]
control = $CONTROL_PORT
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
$([ -n "${STEPS:-}" ] && printf '[publishing]\npublic_url = "http://127.0.0.1:%s"\n[join]\nconsent_contact = "calls@kettle.example"\n' "$STUDIO_PORT")
[studio]
show = "The Morning Kettle"
[[studio.rundown]]
at = "$(at -240)"
title = "Phone-in"
detail = "callers through Moira"
[[studio.rundown]]
at = "$(at 180)"
title = "News · 3:00"
detail = "hard post · playout fires it"
hard_post = true
key = "F2"
[[studio.rundown]]
at = "$(at 360)"
title = "Weather"
detail = "Eilidh reads · Weather bed"
key = "F4"
[[studio.rundown]]
at = "$(at 480)"
title = "Steeped · Finlay Grant"
detail = "handover"
[[studio.banned]]
identity = "tavish"
date = "2026-09-12"
reason = "Abuse on air"
[log]
format = "pretty"
filter = "info"
TOML

podman run -d --name "$prefix-livekit" "${common[@]}" -v "$run/livekit.yaml:/etc/livekit.yaml:ro" \
  "$LK_IMAGE" --config /etc/livekit.yaml >/dev/null
podman run -d --name "$prefix-pg" -p "127.0.0.1:$PG_PORT:5432" -e POSTGRES_PASSWORD=test-only \
  -e POSTGRES_DB=weavesignal docker.io/library/postgres:18 >/dev/null
# Rootless Podman maps the container's root to this user: playout's socket is ours to open.
liq=("${common[@]}" --user 0:0 -v "$root/playout:/p:ro" -v "$run:/r:ro" -v "$run/sock:/sock"
     -v "$run/carts:/carts:ro" -e WEAVESIGNAL_PLAYOUT_DATA=/r/playout.json
     -e "WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD=$SOURCE_PASSWORD" --entrypoint /usr/bin/liquidsoap "$LIQ_IMAGE")
podman run -d --name "$prefix-listeners" "${liq[@]}" /p/listeners.liq >/dev/null
podman run -d --name "$prefix-playout" "${liq[@]}" /p/weavesignal.liq >/dev/null
for i in $(seq 1 60); do
  curl -fs "http://127.0.0.1:$LK_PORT" >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && { echo "LiveKit did not come up" >&2; exit 1; }
done

# The desk, built in its image (`just desk-build`), its control socket in tests/m2/run/.
[ -x "$root/target/container/debug/weavesignal-desk" ] || { echo "build the desk first: just desk-build" >&2; exit 1; }
podman run -d --name "$prefix-desk" "${common[@]}" -v "$root:/work" -w /work \
  -e "WEAVESIGNAL_LIVEKIT_API_SECRET=$API_SECRET" -e RUST_LOG=weavesignal_desk=info \
  "$DESK_IMAGE" /work/target/container/debug/weavesignal-desk --config /work/tests/m2/run/desk.toml >/dev/null

# The control plane on the host; its release build is the media worker, so the demo pack loads
# in about a minute.
cargo build -q -p weavesignal-control
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

# The people: publish-only tokens from the control plane (ADR-0090), one livekitwebrtcsink each,
# a quiet noise each at its own level so the meters differ. Identity, name, role, volume.
people=(
  "eilidh|Eilidh Fraser-MacLeod|presenter|0.10"
  "gregor|Gregor Stewart|guest|0.08"
  "kirsty|Kirsty Wilson|guest|0.06"
  "finlay|Finlay Grant|dj|0.04"
  "moira|Moira Campbell|screener|0.05"
  "niall|Niall Anderson|caller|0.07"
  "dougal|Dougal Thomson|caller|0.06"
  "jessie|Jessie Brown|caller|0.05"
  "tavish|Tavish|caller|0.05"
)
# A caller's token needs their consent to record (M3): the synthetic callers' is given on their
# behalf by the station, against the words the player page shows now.
consent_version=$(curl -fs "http://127.0.0.1:$CONTROL_PORT/api/v1/player" | python3 -c 'import json,sys; print(json.load(sys.stdin)["call_in"]["consent_version"])')
launch=""
for p in "${people[@]}"; do
  IFS='|' read -r id name role vol <<<"$p"
  consent=""
  [ "$role" = caller ] && consent=",\"consent\":{\"accepted\":true,\"version\":\"$consent_version\"}"
  token=$(curl -fs -X POST "http://127.0.0.1:$CONTROL_PORT/api/v1/room/tokens" \
    -H "Authorization: Bearer $ADMIN_TOKEN" -H 'Content-Type: application/json' \
    -d "{\"identity\":\"$id\",\"name\":\"$name\",\"role\":\"$role\",\"subscribe\":false$consent}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])')
  # One process each, so a person dropped from the room (⌃D ⌃D) takes nobody else with them.
  launch+="gst-launch-1.0 -q audiotestsrc is-live=true wave=pink-noise volume=$vol ! audio/x-raw,format=S16LE,rate=48000,channels=2 ! queue ! livekitwebrtcsink name=pub-$id signaller::ws-url=ws://127.0.0.1:$LK_PORT signaller::auth-token=$token signaller::room-name=studio & "
done
podman run -d --name "$prefix-people" "${common[@]}" "$DESK_IMAGE" sh -c "${launch}wait" >/dev/null

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
echo "up: the studio app at http://127.0.0.1:$STUDIO_PORT/ (LiveKit :$LK_PORT, control :$CONTROL_PORT, SRT :$SRT_PORT)"
echo "    stop playout with: $0 down   start it again: $0 up   end: Ctrl+C"
wait "${pids[0]}"
