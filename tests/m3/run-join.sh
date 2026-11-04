#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The join pages against the real desk on loopback (M3): the M2 stack (LiveKit 1.13.10,
# Liquidsoap 2.5.0-rc1 playout and icecast.server, the desk in its image, the control plane on
# the host) on ports 32000-32999, with a presenter and a screener in the room as synthetic
# people (ADR-0071's names), the studio app on Vite (admin bearer added by its proxy), and the
# public pages on Vite (no bearer: only the public routes and passes). Then it waits: open the
# player at http://127.0.0.1:$WEB_PORT/ (or drive it with tests/m3/join.mjs), stop with Ctrl+C.
#
# Two settings take it nearer a station:
# - TURN_TLS_PORT=<port>: LiveKit's own TURN on TLS there (ADR-0059, ADR-0080), with a
#   self-signed certificate for `turn.localhost` (LiveKit wants a dotted name; Chromium resolves *.localhost to loopback) made in tests/m3/run/ (its SPKI hash in
#   turn-spki.txt, for Chromium's --ignore-certificate-errors-spki-list), relay ports in
#   RELAY_START-RELAY_END, and no TURN over UDP: the only way through UDP-less is TLS.
# - SERVE_WEB=1: the public pages from the control plane (apps/web/dist, `web.dir`) with their
#   caching and CSP, at http://127.0.0.1:$CONTROL_PORT/, instead of from Vite.
#
#   tests/m3/run-join.sh            # up, and wait
#   TURN_TLS_PORT=39349 SERVE_WEB=1 tests/m3/run-join.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/m3/run"
prefix=${M3_PREFIX:-weavesignal-m3}
LK_PORT=${LK_PORT:-32880}
LK_UDP_PORT=${LK_UDP_PORT:-32882}
SRT_PORT=${SRT_PORT:-32913}
LISTENERS_PORT=${LISTENERS_PORT:-32812}
FEED_PORT=${FEED_PORT:-32815}
CONTROL_PORT=${CONTROL_PORT:-32310}
STUDIO_PORT=${STUDIO_PORT:-32420}
WEB_PORT=${WEB_PORT:-32430}
LIQ_IMAGE=${LIQ_IMAGE:-docker.io/savonet/liquidsoap:v2.5.0-rc1}
LK_IMAGE=${LK_IMAGE:-docker.io/livekit/livekit-server:v1.13.10}
DESK_IMAGE=${DESK_IMAGE:-${WEAVESIGNAL_DESK_IMAGE:-localhost/weavesignal-desk-dev:m5}}
API_KEY=m3join
API_SECRET=m3-join-test-only-secret-not-for-use-00
ADMIN_TOKEN=m3-admin-test-only
SOURCE_PASSWORD=studio-test-only
common=(--network host --security-opt label=disable)
TURN_TLS_PORT=${TURN_TLS_PORT:-}
RELAY_START=${RELAY_START:-39600}
RELAY_END=${RELAY_END:-39699}
SERVE_WEB=${SERVE_WEB:-}


taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }

mkdir -p "$run"
rm -f "$run/desk.sock"
pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
  for c in people desk playout listeners livekit; do
    podman logs "$prefix-$c" > "$run/$c.log" 2>&1 || true
    podman rm -f "$prefix-$c" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT
trap 'exit 130' INT TERM
cleanup
# After the cleanup, so this stack's own leftovers from a killed run do not count as clashes.
for p in "$LK_PORT" "$LISTENERS_PORT" "$FEED_PORT" "$CONTROL_PORT" "$STUDIO_PORT" "$WEB_PORT" $TURN_TLS_PORT; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done
for p in "$LK_UDP_PORT" "$SRT_PORT"; do taken -u "$p" && { echo "port $p/udp is in use; set another" >&2; exit 1; }; done

# With TURN on, LiveKit offers one ICE address, this machine's own (TURN_NODE_IP, by default
# the source address of its default route), and answers only from it: the relay forwards to that
# address and drops answers from any other for want of a permission. Loopback will not do, since
# LiveKit and the people's libnice both leave loopback candidates out. Every port stays as
# exposed as without TURN: LiveKit's media and TURN ports listen on every interface anyway.
node_ip=127.0.0.1
rtc_extra=""
if [ -n "$TURN_TLS_PORT" ]; then
  node_ip=${TURN_NODE_IP:-$(ip -4 route get 192.0.2.1 2>/dev/null | sed -n 's/.* src \([0-9.]*\).*/\1/p')}
  [ -n "$node_ip" ] || { echo "no IPv4 address to give LiveKit: set TURN_NODE_IP" >&2; exit 1; }
  rtc_extra="  ips:
    includes: [\"$node_ip/32\"]"
fi
cat > "$run/livekit.yaml" <<YAML
port: $LK_PORT
bind_addresses: ["127.0.0.1"]
rtc:
  udp_port: $LK_UDP_PORT
  tcp_port: 0
  use_external_ip: false
  node_ip: $node_ip
$rtc_extra
keys:
  $API_KEY: $API_SECRET
logging:
  level: ${LK_LOG:-warn}
  pion_level: ${LK_PION_LOG:-error}
YAML
if [ -n "$TURN_TLS_PORT" ]; then
  # A throwaway certificate for localhost, as a station's own would be for its TURN name.
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 2 \
    -subj /CN=turn.localhost -addext subjectAltName=DNS:turn.localhost,IP:127.0.0.1 \
    -keyout "$run/turn.key" -out "$run/turn.crt" 2>/dev/null
  chmod 0644 "$run/turn.key"
  openssl x509 -in "$run/turn.crt" -pubkey -noout | openssl pkey -pubin -outform der \
    | openssl dgst -sha256 -binary | base64 > "$run/turn-spki.txt"
  cat >> "$run/livekit.yaml" <<YAML
turn:
  enabled: true
  domain: turn.localhost
  tls_port: $TURN_TLS_PORT
  udp_port: 0
  external_tls: false
  cert_file: /run/turn/turn.crt
  key_file: /run/turn/turn.key
  relay_range_start: $RELAY_START
  relay_range_end: $RELAY_END
  # LiveKit's TURN refuses to relay to private and loopback addresses, its own included, unless
  # told: a station whose node IP is private (behind NAT) needs the same line for that address.
  allow_restricted_peer_cidrs: ["$node_ip/32"]
YAML
fi
cat > "$run/playout.json" <<JSON
{
  "station_id": "00000000-0000-0000-0000-000000000000",
  "srt": { "host": "127.0.0.1", "port": $SRT_PORT },
  "listeners": { "bind": "127.0.0.1", "host": "127.0.0.1", "port": $LISTENERS_PORT, "mount": "programme.opus", "feed_port": $FEED_PORT },
  "control_socket": "/tmp/$prefix-playout.sock",
  "carts": { "dir": "/var/lib/weavesignal/carts" }
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
socket = "/work/tests/m3/run/desk.sock"
TOML
# The rundown: the next hour around now, so the clock has a hard post to back-time to.
at() { date -d "@$(( $(date +%s) + $1 ))" +%H:%M; }
cat > "$run/control.toml" <<TOML
[station]
id = "6f1c1f43-7d0e-4d3e-9b39-6c8f3f8d2a10"
name = "Kettle FM"
[ports]
control = $CONTROL_PORT
${TURN_TLS_PORT:+turn_tls = $TURN_TLS_PORT}
[room]
public_url = "ws://127.0.0.1:$LK_PORT"
api_url = "http://127.0.0.1:$LK_PORT"
api_key = "$API_KEY"
name = "studio"
[desk]
socket = "$run/desk.sock"
poll_ms = 250
[studio]
show = "The Morning Kettle"
[[studio.rundown]]
at = "$(at -240)"
title = "Phone-in"
detail = "callers through Elspeth"
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
[join]
stream_url = "http://127.0.0.1:$LISTENERS_PORT/programme.opus"
consent_contact = "calls@kettle.example"
TOML
[ -n "$SERVE_WEB" ] && printf '[web]\ndir = "%s"\n' "$root/apps/web/dist" >> "$run/control.toml"
cat >> "$run/control.toml" <<TOML
[log]
format = "pretty"
filter = "info"
TOML

turn_mount=()
[ -n "$TURN_TLS_PORT" ] && turn_mount=(-v "$run:/run/turn:ro")
podman run -d --name "$prefix-livekit" "${common[@]}" -v "$run/livekit.yaml:/etc/livekit.yaml:ro" "${turn_mount[@]}" \
  "$LK_IMAGE" --config /etc/livekit.yaml >/dev/null
liq=("${common[@]}" -v "$root/playout:/p:ro" -v "$run:/r:ro" -e WEAVESIGNAL_PLAYOUT_DATA=/r/playout.json
     -e "WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD=$SOURCE_PASSWORD" --entrypoint /usr/bin/liquidsoap "$LIQ_IMAGE")
podman run -d --name "$prefix-listeners" "${liq[@]}" /p/listeners.liq >/dev/null
podman run -d --name "$prefix-playout" "${liq[@]}" /p/weavesignal.liq >/dev/null
for i in $(seq 1 60); do
  curl -fs "http://127.0.0.1:$LK_PORT" >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && { echo "LiveKit did not come up" >&2; exit 1; }
done

# The desk, built in its image (`just desk-build`), its control socket in tests/m3/run/.
[ -x "$root/target/container/debug/weavesignal-desk" ] || { echo "build the desk first: just desk-build" >&2; exit 1; }
podman run -d --name "$prefix-desk" "${common[@]}" -v "$root:/work" -w /work \
  -e "WEAVESIGNAL_LIVEKIT_API_SECRET=$API_SECRET" -e RUST_LOG=weavesignal_desk=info \
  "$DESK_IMAGE" /work/target/container/debug/weavesignal-desk --config /work/tests/m3/run/desk.toml >/dev/null

# The control plane on the host.
cargo build -q -p weavesignal-control
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
  "rory|Rory Bruce|presenter|0.10"
  "elspeth|Elspeth Gordon|screener|0.05"
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
# The public pages: from the control plane (SERVE_WEB), or on Vite with /api passed to the
# control plane and no bearer.
if [ -n "$SERVE_WEB" ]; then
  web="http://127.0.0.1:$CONTROL_PORT"
  curl -fs "$web/" | grep -q '<div id="app">' || { echo "the control plane does not serve the pages: build them (just web-check)" >&2; exit 1; }
else
  web="http://127.0.0.1:$WEB_PORT"
  (
    cd "$root/apps/web"
    WEAVESIGNAL_CONTROL_URL="http://127.0.0.1:$CONTROL_PORT" WEAVESIGNAL_WEB_PORT=$WEB_PORT exec pnpm exec vite --logLevel warn
  ) > "$run/web.log" 2>&1 &
  pids+=($!)
  for i in $(seq 1 60); do
    curl -fs "$web/" >/dev/null 2>&1 && break
    sleep 0.5; [ "$i" = 60 ] && { echo "the public pages did not come up" >&2; exit 1; }
  done
fi
echo "up: the player at $web/ and the studio app at http://127.0.0.1:$STUDIO_PORT/ (LiveKit :$LK_PORT, control :$CONTROL_PORT${TURN_TLS_PORT:+, TURN over TLS :$TURN_TLS_PORT})"
wait "${pids[0]}"
