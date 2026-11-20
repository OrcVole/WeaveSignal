#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# WHIP for remote DJs (17d), end to end on loopback with a WHIP client that is not ours:
# PostgreSQL, the control plane, the ingest, and a stand-in desk (as `test-sources-probe`), with
# GStreamer's `whipclientsink` (gst-plugins-rs, in the desk image) sending a live tone to the
# control plane's public route `/whip/<key id>` with the key's secret as its bearer. It checks:
#
#   1. a wrong bearer is refused (401) and counted against the key as a wrong key;
#   2. the WHIP source in its slot stands by, with way `whip` and `rtp/opus 48 kHz stereo`, and
#      its sound reaches the desk with a level;
#   3. stopping the client ends the source (its DELETE, or its silence).
#
# The post, the grace, LINK LOST and the monitor link follow from the same admit/report loop
# as Icecast and SRT (`test-sources` checks them with DESK=real); this script does not repeat
# them. Ports in 22xxx, loopback only. The client runs in its own network namespace (rootless
# Podman's pasta) and reaches this machine's loopback at an address pasta maps to it
# (`--map-host-loopback`), which is also the ingest's one ICE candidate: so the client is
# behind a NAT, as a DJ is, and its packets arrive from an address it never offered. (On the host
# network it cannot be tried: libnice offers no loopback candidate, so its checks never reach a
# station on loopback.) Needs the desk image (`just desk-image`) for `whipclientsink`.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/sources/run-whip"
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-whip
target=${CARGO_TARGET_DIR:-$root/target}
CONTROL_PORT=${CONTROL_PORT:-22310}
PG_PORT=${PG_PORT:-22432}
DESK_SOURCES_PORT=${DESK_SOURCES_PORT:-22318}
INGEST_WHIP_PORT=${INGEST_WHIP_PORT:-22319}
WHIP_PORT=${WHIP_PORT:-22712}
PG_IMAGE=${PG_IMAGE:-docker.io/library/postgres:18}
DESK_IMAGE=${DESK_IMAGE:-${WEAVESIGNAL_DESK_IMAGE:-localhost/weavesignal-desk-dev:m5}}
ADMIN=whip-admin-test-only
INGEST_TOKEN=whip-ingest-test-only-whip-ingest-test-0
DESK_PASSWORD=whip-desk-test-only

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$CONTROL_PORT" "$PG_PORT" "$DESK_SOURCES_PORT" "$INGEST_WHIP_PORT"; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done
taken -u "$WHIP_PORT" && { echo "port $WHIP_PORT/udp is in use; set WHIP_PORT" >&2; exit 1; }
podman image exists "$DESK_IMAGE" || { echo "no $DESK_IMAGE: build the desk image first (just desk-image)" >&2; exit 1; }
podman run --rm "$DESK_IMAGE" gst-inspect-1.0 whipclientsink >/dev/null 2>&1 \
  || { echo "$DESK_IMAGE has no whipclientsink (gst-plugins-rs's webrtc plugin with whip)" >&2; exit 1; }

# The address the client's namespace reaches this machine's loopback at (pasta's
# --map-host-loopback): TEST-NET-1 (RFC 5737), nobody's; str0m refuses a link-local candidate.
station_ip=${STATION_IP:-192.0.2.9}

rm -rf "$run"; mkdir -p "$run/stub"
pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
  podman logs "$prefix-pg" > "$run/pg.log" 2>&1 || true
  podman logs "$prefix-client" > "$run/client.log" 2>&1 || true
  podman rm -f "$prefix-pg" "$prefix-client" >/dev/null 2>&1 || true
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
wait_for() { # what seconds command...
  local what=$1 secs=$2; shift 2
  for _ in $(seq 1 $((secs * 2))); do "$@" && return 0; sleep 0.5; done
  fail "$what"
}

cat > "$run/control.toml" <<TOML
[station]
id = "7a2d0c55-1e6b-4f0e-8d0b-2b9e5c1a7f30"
name = "Kettle FM"
[ports]
control = $CONTROL_PORT
ingest_whip = $INGEST_WHIP_PORT
source_whip = $WHIP_PORT
[desk]
socket = "$run/desk.sock"
poll_ms = 250
[publishing]
public_url = "http://127.0.0.1:$CONTROL_PORT"
zone = "Europe/London"
cards_dir = "$run/cards"
episodes_dir = "$run/episodes"
[join]
stream_url = "http://127.0.0.1/programme.opus"
[log]
format = "pretty"
filter = "info"
TOML

podman run -d --name "$prefix-pg" -p "127.0.0.1:$PG_PORT:5432" -e POSTGRES_PASSWORD=test-only \
  -e POSTGRES_DB=weavesignal "$PG_IMAGE" >/dev/null
nice cargo build -q -p weavesignal-control -p weavesignal-ingest
# This run's own copies: a target directory shared with other worktrees may be rebuilt meanwhile.
mkdir -p "$run/bin" && cp "$target/debug/weavesignal-control" "$target/debug/weavesignal-ingest" "$run/bin/"
for i in $(seq 1 60); do
  podman exec "$prefix-pg" pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && fail "PostgreSQL did not come up"
done

# The stand-in desk: each connection's head to a file, 200 OK, then the WAV and its samples.
cat > "$run/stub/one.sh" <<'SH'
#!/bin/bash
n=$(date +%s%N)
head=""
while IFS= read -r line; do head+="$line"$'\n'; [ "$line" = $'\r' ] && break; done
printf '%s' "$head" > "$STUB_DIR/$n.head"
printf 'HTTP/1.0 200 OK\r\n\r\n'
cat > "$STUB_DIR/$n.pcm"
SH
chmod +x "$run/stub/one.sh"
STUB_DIR="$run/stub" socat "TCP-LISTEN:$DESK_SOURCES_PORT,bind=127.0.0.1,reuseaddr,fork" "EXEC:$run/stub/one.sh" &
pids+=($!)

WEAVESIGNAL_DATABASE_URL="postgres://postgres:test-only@127.0.0.1:$PG_PORT/weavesignal" \
WEAVESIGNAL_CONTROL_ADMIN_TOKEN=$ADMIN WEAVESIGNAL_INGEST_TOKEN=$INGEST_TOKEN \
  "$run/bin/weavesignal-control" --config "$run/control.toml" > "$run/control.log" 2>&1 &
pids+=($!)
for i in $(seq 1 60); do
  curl -fs "http://127.0.0.1:$CONTROL_PORT/health" >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && fail "the control plane did not come up"
done

# A show on air now (an hour from ten minutes ago), and a WHIP key for its host.
air() { echo "$(TZ=Europe/London date -d "@$(( $(date +%s) + $1 ))" +%Y-%m-%dT%H:%M:%S%:z)[Europe/London]"; }
api PUT /api/v1/shows/steeped '{"name":"Steeped","colour_slot":3}' >/dev/null
api PUT /api/v1/shows/steeped/series/1 '{}' >/dev/null
api POST /api/v1/shows/steeped/series/1/episodes '{"count":1}' >/dev/null
r=$(api PATCH /api/v1/shows/steeped/series/1/episodes/1 "{\"air_at\":\"$(air -600)\",\"minutes\":60}")
jq -e '.episode.air_at' <<<"$r" >/dev/null || fail "the episode: $r"
KEY=$(api POST /api/v1/shows/steeped/keys '{"label":"Morag Campbell","hint":"whip"}')
key_id=$(jq -r .key.key_id <<<"$KEY"); secret=$(jq -r .secret <<<"$KEY")
url=$(jq -r .details.whip.url <<<"$KEY")
[ "$url" = "http://127.0.0.1:$CONTROL_PORT/whip/$key_id" ] || fail "the key's WHIP address: $KEY"
[ "$(jq -r .details.whip.port <<<"$KEY")" = "$WHIP_PORT" ] || fail "the key's WHIP port: $KEY"

# The ingest, on loopback, with the address the client reaches loopback at as WHIP's one candidate.
WEAVESIGNAL_INGEST_TOKEN=$INGEST_TOKEN WEAVESIGNAL_DESK_SOURCES_PASSWORD=$DESK_PASSWORD RUST_LOG=info NO_COLOR=1 \
  "$run/bin/weavesignal-ingest" run --control "http://127.0.0.1:$CONTROL_PORT" \
  --desk "127.0.0.1:$DESK_SOURCES_PORT" --bind 127.0.0.1 \
  --whip-http "127.0.0.1:$INGEST_WHIP_PORT" --public-ip "$station_ip" > "$run/ingest.log" 2>&1 &
pids+=($!)
wait_for "the ingest to take WHIP on $WHIP_PORT/udp and $INGEST_WHIP_PORT/tcp" 20 bash -c "ss -Hlun 'sport = :$WHIP_PORT' | grep -q . && ss -Hltn 'sport = :$INGEST_WHIP_PORT' | grep -q ."

source_json() { api GET /api/v1/sources | jq -c --arg k "$key_id" '.sources[] | select(.key_id==$k)'; }

echo "1. a wrong bearer: refused, counted as a wrong key"
code=$(curl -s -o "$run/wrong.txt" -w '%{http_code}' -X POST -H 'Content-Type: application/sdp' \
  -H "Authorization: Bearer WRONGWRONGWRONGWRONGWRON" --data-binary $'v=0\r\no=- 1 1 IN IP4 0.0.0.0\r\ns=-\r\nt=0 0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=rtpmap:111 opus/48000/2\r\n' "$url")
[ "$code" = 401 ] || fail "a wrong bearer got $code: $(cat "$run/wrong.txt")"
api GET /api/v1/shows/steeped/keys | jq -e --arg k "$key_id" '.keys[] | select(.key_id==$k) | .last_refusal.reason == "wrong_key" and .last_refusal.way == "whip"' >/dev/null \
  || fail "the key's last refusal: $(api GET /api/v1/shows/steeped/keys)"
echo "   ok"

echo "2. whipclientsink in the slot: standing by, heard at the desk"
# webrtcsink's own default STUN server is Google's: point it at a port on loopback that nothing
# answers, so the test asks no outside server (ADR-0290).
client_url=${url/127.0.0.1/$station_ip}
podman run -d --name "$prefix-client" --network "pasta:--map-host-loopback,$station_ip" -e "GST_DEBUG=${CLIENT_GST_DEBUG:-2}" "$DESK_IMAGE" \
  gst-launch-1.0 -e audiotestsrc is-live=true wave=sine freq=1000 volume=0.3 ! audioconvert ! audioresample \
  ! whipclientsink name=ws "signaller::whip-endpoint=$client_url" "signaller::auth-token=$secret" stun-server=stun://127.0.0.1:9 >/dev/null
wait_for "the WHIP source to stand by (the client's log: $run/client.log)" 30 \
  bash -c "curl -s -H 'Authorization: Bearer $ADMIN' http://127.0.0.1:$CONTROL_PORT/api/v1/sources | jq -e --arg k '$key_id' '.sources[] | select(.key_id==\$k) | .state == \"standby\"' >/dev/null"
heard() {
  local h; h=$(grep -l "PUT /sources/source-$key_id " "$run"/stub/*.head 2>/dev/null | head -1) || return 1
  [ -n "$h" ] && grep -q "X-Source-Way: whip" "$h" || return 1
  local pcm=${h%.head}.pcm
  [ "$(head -c 4 "$pcm")" = RIFF ] && [ "$(stat -c %s "$pcm")" -gt 400000 ] \
    && source_json | jq -e '.level != null and .level > -40' >/dev/null
}
wait_for "the WHIP source's sound at the desk" 20 heard
source_json | jq -e '.way == "whip" and .format == "rtp/opus 48 kHz stereo"' >/dev/null || fail "the source's way and format: $(source_json)"
echo "   ok: $(source_json | jq -c '{state, slot, way, format, level}')"

echo "3. the client stops (Ctrl-C: gst-launch -e ends the stream, and whipclientsink its session): the source ends"
podman kill -s INT "$prefix-client" >/dev/null
for _ in $(seq 1 20); do [ "$(podman inspect -f '{{.State.Running}}' "$prefix-client" 2>/dev/null)" = true ] || break; sleep 0.5; done
wait_for "the source to end" 20 bash -c "[ -z \"\$(curl -s -H 'Authorization: Bearer $ADMIN' http://127.0.0.1:$CONTROL_PORT/api/v1/sources | jq -c --arg k '$key_id' '.sources[] | select(.key_id==\$k and .state != \"due\" and .state != \"not_connected\")')\" ]"
grep -q "a host's own studio ended" "$run/ingest.log" || fail "the ingest never ended the source"
grep -q "why=ended by the host's software" "$run/ingest.log" || fail "the client's DELETE did not end its session (the ingest's log: $(grep 'WHIP session ended' "$run/ingest.log" | tail -1))"
echo "   ok: its DELETE ended the session; the source ended: $(grep "a host's own studio ended" "$run/ingest.log" | tail -1 | sed 's/.*why=//')"
echo "PASS"
