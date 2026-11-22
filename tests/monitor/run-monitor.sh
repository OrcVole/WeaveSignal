#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The monitor link's sound (ADR-0280, design 4.5) end to end on loopback: PostgreSQL, LiveKit
# 1.13.10, the control plane, the ingest and the real desk in its image, with ffmpeg as two
# hosts' own software sending over Icecast (host A a 1 kHz tone, host B a 2 kHz tone), and a
# headless Chromium as host A's monitor page (tests/monitor/monitor.mjs). It checks that:
#
#   1. host A's monitor token is a listener's for `studio-monitor-<key id>`, two browsers may
#      listen and a third is refused;
#   2. with both hosts on air the monitor hears host B and not host A's own tone; with B off air
#      it hears B cued, and B talking to A; and silence otherwise;
#   3. over 30 s of listening the receiver's concealment (`concealedSamples` minus
#      `silentConcealedSamples`, over `totalSamplesReceived`, as tests/m3/return-stats.mjs reads
#      a caller's return) stays near 0 %, the sign that each block of the monitor mix is
#      stamped from its place in the mix and not from the clock at encode time;
#   4. a stranger in the monitor room (a third seat, signed with LiveKit's key and no recorder
#      grant) receives nothing (ADR-0118);
#   5. the desk makes the monitor mix only while someone listens: `monitor_open` in the host's
#      view is true while they do and false some seconds after they leave;
#   6. the monitor link's controls (design 3.3, 3.8): host A goes on air from the link (Go on air
#      now); the link's event stream carries the view; A brings host B on air beside them and
#      hands over (the desk shows A off air and B on); B ends their show from the link; and when a
#      key ends, its page is taken out of the monitor room.
#
# Needs the desk built (`just desk-build`, with WEAVESIGNAL_DESK_IMAGE for its image) and
# PLAYWRIGHT_CORE (an installed playwright-core with its Chromium); `pnpm install` in apps/web
# gives the page's livekit-client. Ports in 26700-26799 (or set them: 27xxx for the monitor
# controls' own runs), loopback only; every one is a setting.
# Not in `ci`: it needs a browser outside the repository.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/monitor/run"
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-monitor
CONTROL_PORT=${CONTROL_PORT:-26710}
PG_PORT=${PG_PORT:-26742}
LK_PORT=${LK_PORT:-26780}
LK_UDP_PORT=${LK_UDP_PORT:-26782}
DESK_SOURCES_PORT=${DESK_SOURCES_PORT:-26718}
DESK_SRT_PORT=${DESK_SRT_PORT:-26713}
ICE_PORT=${ICE_PORT:-26705}
SRT_PORT=${SRT_PORT:-26771}
PG_IMAGE=${PG_IMAGE:-docker.io/library/postgres:18}
LK_IMAGE=${LK_IMAGE:-docker.io/livekit/livekit-server:v1.13.10}
DESK_IMAGE=${DESK_IMAGE:-${WEAVESIGNAL_DESK_IMAGE:-localhost/weavesignal-desk-dev:current}}
API_KEY=monitortest
API_SECRET=monitor-test-only-secret-not-for-use-0
ADMIN=monitor-admin-test-only
INGEST_TOKEN=monitor-ingest-test-only-monitor-ingest-0
DESK_PASSWORD=monitor-desk-test-only
: "${PLAYWRIGHT_CORE:?needs PLAYWRIGHT_CORE: an installed playwright-core with its Chromium}"
[ -f "$root/apps/web/node_modules/livekit-client/dist/livekit-client.umd.js" ] || { echo "run pnpm install in apps/web (livekit-client for the page)" >&2; exit 1; }
[ -x "$root/target/container/debug/weavesignal-desk" ] || { echo "build the desk first: just desk-build" >&2; exit 1; }

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$CONTROL_PORT" "$PG_PORT" "$LK_PORT" "$DESK_SOURCES_PORT" "$ICE_PORT"; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done
for p in "$LK_UDP_PORT" "$SRT_PORT" "$DESK_SRT_PORT"; do taken -u "$p" && { echo "port $p/udp is in use; set another" >&2; exit 1; }; done

rm -rf "$run"; mkdir -p "$run"
pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
  for c in pg livekit desk; do
    podman logs "$prefix-$c" > "$run/$c.log" 2>&1 || true
    podman rm -f "$prefix-$c" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT
trap 'exit 130' INT TERM
cleanup

fail() { echo "FAIL: $*" >&2; echo "logs in $run" >&2; exit 1; }
api() { # method path [json]
  local args=(-s -X "$1" -H "Authorization: Bearer $ADMIN" -H 'Content-Type: application/json')
  [ $# -ge 3 ] && args+=(--data "$3")
  curl "${args[@]}" "http://127.0.0.1:$CONTROL_PORT$2"
}

cat > "$run/livekit.yaml" <<YAML
# Loopback only, test keys, one UDP port (ADR-0023), ICE-TCP and TURN off.
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
keys:
  $API_KEY: $API_SECRET
logging:
  level: info
YAML
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
[publishing]
public_url = "https://kettle.example"
zone = "Europe/London"
cards_dir = "$run/cards"
episodes_dir = "$run/episodes"
[join]
stream_url = "http://127.0.0.1/programme.opus"
[log]
format = "pretty"
filter = "info"
TOML
cat > "$run/desk.toml" <<TOML
[livekit]
url = "ws://127.0.0.1:$LK_PORT"
api_key = "$API_KEY"
room = "studio"
[programme]
srt_port = $DESK_SRT_PORT
[control]
socket = "/work/tests/monitor/run/desk.sock"
[sources]
port = $DESK_SOURCES_PORT
TOML

podman run -d --name "$prefix-pg" -p "127.0.0.1:$PG_PORT:5432" -e POSTGRES_PASSWORD=test-only \
  -e POSTGRES_DB=weavesignal "$PG_IMAGE" >/dev/null
podman run -d --name "$prefix-livekit" --network host --security-opt label=disable \
  -v "$run/livekit.yaml:/etc/livekit.yaml:ro" "$LK_IMAGE" --config /etc/livekit.yaml >/dev/null
cargo build -q -p weavesignal-control -p weavesignal-ingest
for i in $(seq 1 60); do
  podman exec "$prefix-pg" pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 && curl -fs "http://127.0.0.1:$LK_PORT" >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && fail "PostgreSQL or LiveKit did not come up"
done

podman run -d --name "$prefix-desk" --network host --security-opt label=disable -v "$root:/work" -w /work \
  -e "WEAVESIGNAL_LIVEKIT_API_SECRET=$API_SECRET" \
  -e "WEAVESIGNAL_DESK_SOURCES_PASSWORD=$DESK_PASSWORD" -e RUST_LOG=weavesignal_desk=info \
  "$DESK_IMAGE" /work/target/container/debug/weavesignal-desk --config /work/tests/monitor/run/desk.toml >/dev/null

WEAVESIGNAL_DATABASE_URL="postgres://postgres:test-only@127.0.0.1:$PG_PORT/weavesignal" \
WEAVESIGNAL_LIVEKIT_API_SECRET=$API_SECRET \
WEAVESIGNAL_CONTROL_ADMIN_TOKEN=$ADMIN WEAVESIGNAL_INGEST_TOKEN=$INGEST_TOKEN \
  "$root/target/debug/weavesignal-control" --config "$run/control.toml" > "$run/control.log" 2>&1 &
pids+=($!)
for i in $(seq 1 60); do
  curl -fs "http://127.0.0.1:$CONTROL_PORT/health" >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && fail "the control plane did not come up"
done

# One show on air now (Steeped, host A) and the next show twenty minutes on (Kettle Drive, host B),
# both at Full, so the handover between them is on offer.
air() { echo "$(TZ=Europe/London date -d "@$(( $(date +%s) + $1 ))" +%Y-%m-%dT%H:%M:%S%:z)[Europe/London]"; }
episode() { # slug name seconds-from-now minutes
  api PUT "/api/v1/shows/$1" "{\"name\":\"$2\",\"colour_slot\":3,\"step\":\"full\"}" >/dev/null
  api PUT "/api/v1/shows/$1/series/1" '{}' >/dev/null
  api POST "/api/v1/shows/$1/series/1/episodes" '{"count":1}' >/dev/null
  r=$(api PATCH "/api/v1/shows/$1/series/1/episodes/1" "{\"air_at\":\"$(air "$3")\",\"minutes\":$4}")
  echo "$r" | jq -e '.episode.air_at' >/dev/null || fail "the episode of $1: $r"
}
episode steeped Steeped -600 120
episode kettle "Kettle Drive" 1200 60
key() { api POST "/api/v1/shows/$1/keys" "{\"label\":\"$2\",\"hint\":\"icecast\"}"; }
A=$(key steeped "Finlay Grant"); B=$(key kettle "Heather Douglas")
for k in "$A" "$B"; do echo "$k" | jq -e '.monitor_secret' >/dev/null || fail "a key: $k"; done
jq -n --argjson a "$A" --argjson b "$B" \
  '{a: {key_id: $a.key.key_id, password: $a.details.icecast.password, monitor: $a.monitor_secret},
    b: {key_id: $b.key.key_id, password: $b.details.icecast.password, monitor: $b.monitor_secret}}' > "$run/keys.json"
api PUT /api/v1/settings/sources "$(api GET /api/v1/settings/sources | jq --argjson i "$ICE_PORT" --argjson s "$SRT_PORT" '.port_icecast=$i | .port_srt=$s')" | jq -e .port_srt >/dev/null

WEAVESIGNAL_INGEST_TOKEN=$INGEST_TOKEN WEAVESIGNAL_DESK_SOURCES_PASSWORD=$DESK_PASSWORD RUST_LOG=info \
  "$root/target/debug/weavesignal-ingest" run --control "http://127.0.0.1:$CONTROL_PORT" \
  --desk "127.0.0.1:$DESK_SOURCES_PORT" --bind 127.0.0.1 > "$run/ingest.log" 2>&1 &
pids+=($!)
for i in $(seq 1 40); do
  taken -t "$ICE_PORT" && break
  sleep 0.5; [ "$i" = 40 ] && fail "the ingest did not listen on $ICE_PORT"
done

# Two hosts' software: A sends 1 kHz, B 2 kHz, as 44.1 kHz MP3 over Icecast (the resampler's work).
for who in a b; do
  hz=1000; [ $who = b ] && hz=2000
  ffmpeg -hide_banner -loglevel error -y -f lavfi -i "sine=f=$hz:sample_rate=44100:d=600,volume=0.3" -ac 2 -c:a libmp3lame -b:a 128k "$run/tone-$who.mp3"
  pass=$(jq -r ".$who.password" "$run/keys.json")
  ffmpeg -hide_banner -loglevel error -re -i "$run/tone-$who.mp3" -c:a copy -content_type audio/mpeg -f mp3 \
    "icecast://source:$pass@127.0.0.1:$ICE_PORT/live" > "$run/ffmpeg-$who.log" 2>&1 &
  pids+=($!)
done

CONTROL="http://127.0.0.1:$CONTROL_PORT" ADMIN_TOKEN=$ADMIN RUN="$run" \
  LK_URL="ws://127.0.0.1:$LK_PORT" LK_API_KEY=$API_KEY LK_API_SECRET=$API_SECRET \
  MONITOR_SECONDS=${MONITOR_SECONDS:-30} \
  PLAYWRIGHT_CORE="$PLAYWRIGHT_CORE" node "$root/tests/monitor/monitor.mjs" 2>&1 | tee "$run/monitor.log"
status=${PIPESTATUS[0]}
[ "$status" = 0 ] && echo "PASS" || { echo "FAIL (see $run)" >&2; exit 1; }
