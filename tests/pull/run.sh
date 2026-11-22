#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Pulled sources (1.2, issue #20; the how-to *Pull a partner's stream into a show*), end to end in
# containers on loopback: PostgreSQL, and the control plane, the ingest and the real desk from the
# server image, with WEAVESIGNAL_PULL_ALLOW_PRIVATE=1 on the control plane (as the docs say) so a
# pull may open this machine's own addresses. Two partners stand in for other stations:
#
#   - Icecast 2.5 (the relay image) with a mount fed by Liquidsoap (MP3, 440 Hz);
#   - an SRT listener: Liquidsoap's output.srt (libsrt) in listener mode, AAC in MPEG-TS (1 kHz);
#     SRT_SENDER=gstreamer uses GStreamer's srtsink (libsrt) from the server image instead.
#
# Each is a show key with `hint: pull` and the partner's address, for two shows one after the
# other (the first in about 40 s, the second when it ends), with short rules. It checks that:
#
#   1. both pulls are opened from before their shows' times and stand by (STANDBY, `pull: true`,
#      way `pull`), each heard at the desk (a level, not on air);
#   2. at the first show's post the Icecast pull goes on air (automation held the air);
#   3. the Icecast partner's source dropped: LINK LOST, then the station opens it again on its
#      own and the pull goes back on air in its kept place;
#   4. at the second show's post, after the grace, the SRT pull takes the air, and the first
#      pull, its door closed, is closed (a pull is never a private test);
#   5. the SRT partner dropped and back: LINK LOST, reopened, on air again.
#
# Ports in 21xxx on loopback, each a setting. About four minutes. Needs the server image
# (`just server-image`), the relay image (`just relay-image`) and Liquidsoap's image.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/pull/run"
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-pull
CONTROL_PORT=${CONTROL_PORT:-21310}
PG_PORT=${PG_PORT:-21432}
DESK_SOURCES_PORT=${DESK_SOURCES_PORT:-21318}
DESK_SRT_PORT=${DESK_SRT_PORT:-21313}
PARTNER_ICECAST_PORT=${PARTNER_ICECAST_PORT:-21800}
PARTNER_SRT_PORT=${PARTNER_SRT_PORT:-21900}
PG_IMAGE=${PG_IMAGE:-docker.io/library/postgres:18}
SERVER_IMAGE=${WEAVESIGNAL_SERVER_IMAGE:-localhost/weavesignal-server:m1}
RELAY_IMAGE=${RELAY_IMAGE:-localhost/weavesignal-relay:m1}
LIQ_IMAGE=${LIQ_IMAGE:-docker.io/savonet/liquidsoap:v2.5.0-rc1}
ADMIN=pull-admin-test-only
INGEST_TOKEN=pull-ingest-test-only-pull-ingest-test-0
DESK_PASSWORD=pull-desk-test-only
PARTNER_PASSWORD=partner-test-only
SRT_PAYLOAD=${SRT_PAYLOAD:-1316}

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$CONTROL_PORT" "$PG_PORT" "$DESK_SOURCES_PORT" "$PARTNER_ICECAST_PORT"; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done
for p in "$DESK_SRT_PORT" "$PARTNER_SRT_PORT"; do taken -u "$p" && { echo "port $p/udp is in use; set another" >&2; exit 1; }; done
for i in "$SERVER_IMAGE" "$RELAY_IMAGE" "$LIQ_IMAGE"; do
  podman image exists "$i" || { echo "no image $i: run 'just server-image relay-image' (or set WEAVESIGNAL_SERVER_IMAGE, RELAY_IMAGE)" >&2; exit 1; }
done

containers=(pg desk control ingest partner-icecast partner-icecast-source partner-srt)
rm -rf "$run"; mkdir -p "$run"
cleanup() {
  for c in "${containers[@]}"; do
    podman logs "$prefix-$c" > "$run/$c.log" 2>&1 || true
    podman rm -f "$prefix-$c" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT
trap 'exit 130' INT TERM
for c in "${containers[@]}"; do podman rm -f "$prefix-$c" >/dev/null 2>&1 || true; done

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
# Every container on the host's network, as the Compose bundle runs; root in the container is
# this user, so the run folder stays this user's.
host=(--network host --security-opt label=disable --user 0:0)

cat > "$run/control.toml" <<TOML
[station]
id = "4a7e2c19-8b3d-4f60-9d1e-5c2b7a8f0e31"
name = "Kettle FM"
[ports]
control = $CONTROL_PORT
[desk]
socket = "/r/desk.sock"
poll_ms = 250
[publishing]
public_url = "https://kettle.example"
zone = "Europe/London"
cards_dir = "/r/cards"
episodes_dir = "/r/episodes"
[join]
stream_url = "http://127.0.0.1/programme.opus"
[log]
format = "pretty"
filter = "info"
TOML
cat > "$run/desk.toml" <<TOML
[livekit]
url = "ws://127.0.0.1:1"
api_key = "unused"
room = "studio"
[programme]
srt_port = $DESK_SRT_PORT
[control]
socket = "/r/desk.sock"
[sources]
port = $DESK_SOURCES_PORT
TOML
# The partner's Icecast: one mount, /live, that takes a source with a password.
cat > "$run/icecast.xml" <<XML
<icecast>
  <limits><clients>20</clients><sources>2</sources></limits>
  <authentication>
    <source-password>$PARTNER_PASSWORD</source-password>
    <relay-password>never-used-by-the-test</relay-password>
    <admin-user>admin</admin-user>
    <admin-password>admin-test-only</admin-password>
  </authentication>
  <hostname>localhost</hostname>
  <listen-socket><port>$PARTNER_ICECAST_PORT</port><bind-address>127.0.0.1</bind-address></listen-socket>
  <paths>
    <basedir>/usr/share/icecast</basedir>
    <logdir>/tmp</logdir>
    <webroot>/usr/share/icecast/web</webroot>
    <adminroot>/usr/share/icecast/admin</adminroot>
  </paths>
  <logging><accesslog>access.log</accesslog><errorlog>-</errorlog><loglevel>3</loglevel></logging>
</icecast>
XML
chmod 644 "$run/icecast.xml"

podman run -d --name "$prefix-pg" -p "127.0.0.1:$PG_PORT:5432" -e POSTGRES_PASSWORD=test-only \
  -e POSTGRES_DB=weavesignal "$PG_IMAGE" >/dev/null
podman run -d --name "$prefix-partner-icecast" --network host --security-opt label=disable \
  -v "$run/icecast.xml:/etc/test-icecast.xml:ro" --entrypoint icecast "$RELAY_IMAGE" -c /etc/test-icecast.xml >/dev/null
for i in $(seq 1 60); do
  podman exec "$prefix-pg" pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && fail "PostgreSQL did not come up"
done
podman run -d --name "$prefix-desk" "${host[@]}" -v "$run:/r" \
  -e WEAVESIGNAL_LIVEKIT_API_SECRET=pull-test-only-not-for-use-00000000 \
  -e "WEAVESIGNAL_DESK_SOURCES_PASSWORD=$DESK_PASSWORD" -e RUST_LOG=weavesignal_desk=info \
  --entrypoint weavesignal-desk "$SERVER_IMAGE" --config /r/desk.toml >/dev/null
podman run -d --name "$prefix-control" "${host[@]}" -v "$run:/r" \
  -e "WEAVESIGNAL_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:$PG_PORT/weavesignal" \
  -e "WEAVESIGNAL_CONTROL_ADMIN_TOKEN=$ADMIN" -e "WEAVESIGNAL_INGEST_TOKEN=$INGEST_TOKEN" \
  -e WEAVESIGNAL_PULL_ALLOW_PRIVATE=1 \
  --entrypoint weavesignal-control "$SERVER_IMAGE" --config /r/control.toml serve >/dev/null
wait_for "the control plane to come up" 60 curl -fs -o /dev/null "http://127.0.0.1:$CONTROL_PORT/health"
grep -q "pulls may open addresses inside the station's own network" <(podman logs "$prefix-control" 2>&1) \
  || fail "the control plane did not take WEAVESIGNAL_PULL_ALLOW_PRIVATE"
podman run -d --name "$prefix-ingest" "${host[@]}" \
  -e "WEAVESIGNAL_INGEST_TOKEN=$INGEST_TOKEN" -e "WEAVESIGNAL_DESK_SOURCES_PASSWORD=$DESK_PASSWORD" -e RUST_LOG=info -e NO_COLOR=1 \
  --entrypoint weavesignal-ingest "$SERVER_IMAGE" run --control "http://127.0.0.1:$CONTROL_PORT" \
  --desk "127.0.0.1:$DESK_SOURCES_PORT" --bind 127.0.0.1 >/dev/null

# The partners' senders, a tone each, live: Liquidsoap (its Icecast output, as test-icecast-out uses it,
# and its libsrt listener). SRT_SENDER=gstreamer sends SRT with GStreamer's srtsink (libsrt) instead.
# SRT_PAYLOAD is the partner's SRT message size: 1316 (seven MPEG-TS packets), what ffmpeg, OBS and
# GStreamer send. Found 2026-11-21: a libsrt listener (Liquidsoap, GStreamer) sends the ingest's
# caller nothing at 1316 ("Message too large to send" in Liquidsoap's log), and every check passes
# at 1128 (six packets). Likely (inferred from the code, not proven): srt-tokio 0.4.4's caller gives
# its payload size, 1316, as the handshake's largest packet, and libsrt keeps 1316 - 44 = 1272 bytes
# a message. Why ffmpeg's 1316-byte messages into the ingest's own listener work is not explained.
cat > "$run/partner-icecast.liq" <<LIQ
output.icecast(%mp3(bitrate=128, samplerate=44100), host="127.0.0.1", port=$PARTNER_ICECAST_PORT,
  password="$PARTNER_PASSWORD", mount="/live", amplify(0.3, sine(440.)))
LIQ
cat > "$run/partner-srt.liq" <<LIQ
output.srt(mode="listener", bind_address="127.0.0.1", port=$PARTNER_SRT_PORT, payload_size=$SRT_PAYLOAD,
  %ffmpeg(format="mpegts", %audio(codec="aac", b="128k", ac=2, ar=48000)), amplify(0.3, sine(1000.)))
LIQ
liq=(--network host --security-opt label=disable -v "$run:/r:ro" --entrypoint /usr/bin/liquidsoap)
icecast_source() {
  podman rm -f "$prefix-partner-icecast-source" >/dev/null 2>&1 || true
  podman run -d --name "$prefix-partner-icecast-source" "${liq[@]}" "$LIQ_IMAGE" /r/partner-icecast.liq >/dev/null
}
srt_source() {
  podman rm -f "$prefix-partner-srt" >/dev/null 2>&1 || true
  if [ "${SRT_SENDER:-liquidsoap}" = gstreamer ]; then
    podman run -d --name "$prefix-partner-srt" --network host --security-opt label=disable -e GST_DEBUG=srt*:5 \
      --entrypoint gst-launch-1.0 "$SERVER_IMAGE" \
      audiotestsrc is-live=true wave=sine freq=1000 volume=0.3 ! audioconvert ! audioresample \
      ! audio/x-raw,rate=48000,channels=2 ! voaacenc bitrate=128000 ! aacparse ! mpegtsmux "alignment=$((SRT_PAYLOAD / 188))" \
      ! srtsink "uri=srt://127.0.0.1:$PARTNER_SRT_PORT?mode=listener" >/dev/null
  else
    podman run -d --name "$prefix-partner-srt" "${liq[@]}" "$LIQ_IMAGE" /r/partner-srt.liq >/dev/null
  fi
}
icecast_source
srt_source
wait_for "the partner's Icecast mount to answer" 20 bash -c "curl -s --max-time 2 -o /dev/null -w '%{http_code}' http://127.0.0.1:$PARTNER_ICECAST_PORT/live | grep -q 200"

# Short rules: the grace, the late count, a steady link and the silence that ends one.
api PUT /api/v1/settings/sources "$(api GET /api/v1/settings/sources | jq '.grace_s=4 | .late_count_s=3 | .steady_s=2 | .silence_s=2 | .place_kept_s=40')" \
  | jq -e '.grace_s == 4' >/dev/null || fail "the short rules: $(api GET /api/v1/settings/sources)"

# Two shows: the Icecast partner's in 40 s for two minutes, the SRT partner's when it ends.
air() { echo "$(TZ=Europe/London date -d "@$(( $1 ))" +%Y-%m-%dT%H:%M:%S%:z)[Europe/London]"; }
show() { # slug name at-epoch minutes
  api PUT "/api/v1/shows/$1" "{\"name\":\"$2\",\"colour_slot\":3}" >/dev/null
  api PUT "/api/v1/shows/$1/series/1" '{}' >/dev/null
  api POST "/api/v1/shows/$1/series/1/episodes" '{"count":1}' >/dev/null
  local r; r=$(api PATCH "/api/v1/shows/$1/series/1/episodes/1" "{\"air_at\":\"$(air "$3")\",\"minutes\":$4}")
  jq -e '.episode.air_at' <<<"$r" >/dev/null || fail "episode for $1: $r"
}
t0=$(date +%s)
post_a=$((t0 + 40)); post_b=$((post_a + 120))
show partner-hour "Partner Hour" "$post_a" 2
show relay-hour "Relay Hour" "$post_b" 2
pull() { # show label url
  api POST "/api/v1/shows/$1/keys" "{\"label\":\"$2\",\"hint\":\"pull\",\"url\":\"$3\"}"
}
ICE=$(pull partner-hour "Isla Fraser" "http://127.0.0.1:$PARTNER_ICECAST_PORT/live")
SRT=$(pull relay-hour "Callum Reid" "srt://127.0.0.1:$PARTNER_SRT_PORT")
ice=$(jq -r '.key.key_id // empty' <<<"$ICE"); srt=$(jq -r '.key.key_id // empty' <<<"$SRT")
[ -n "$ice" ] || fail "the Icecast pull was not made: $ICE"
[ -n "$srt" ] || fail "the SRT pull was not made: $SRT"

source_of() { api GET /api/v1/sources | jq -c --arg k "$1" '.sources[] | select(.key_id==$k)'; }
state_of() { api GET /api/v1/sources | jq -r --arg k "$1" '(.sources[] | select(.key_id==$k) | .state) // "none"'; }
is_state() { [ "$(state_of "$1")" = "$2" ]; }
studio_source() { api GET /api/v1/studio | jq -c --arg i "source-$1" '.sources[] | select(.identity==$i)'; }
heard() { studio_source "$1" | jq -e '.lufs != null and .slot != null' >/dev/null; }
is_on_air() { studio_source "$1" | jq -e '.on_air == true and .lufs != null' >/dev/null; }
is_off_air() { studio_source "$1" | jq -e '.on_air == false' >/dev/null; }
gone() { is_state "$1" none && [ -z "$(studio_source "$1")" ]; }
show_it() { source_of "$1" | jq -c '{state, way, pull, format, level, slot}'; }

echo "1. both pulls opened before their shows: standing by, heard at the desk"
for k in "$ice" "$srt"; do
  wait_for "pull $k to stand by" 30 is_state "$k" standby
  wait_for "pull $k heard at the desk" 20 heard "$k"
  # The way and the format come with the ingest's next report after the sound does.
  wait_for "pull $k reported as a pull: $(source_of "$k")" 10 bash -c "curl -s -H 'Authorization: Bearer $ADMIN' http://127.0.0.1:$CONTROL_PORT/api/v1/sources | jq -e --arg k '$k' '.sources[] | select(.key_id==\$k) | .pull == true and .way == \"pull\" and .format != null' >/dev/null"
  is_off_air "$k" || fail "pull $k is on air before its post: $(studio_source "$k")"
done
source_of "$ice" | jq -e '.format | startswith("mp3")' >/dev/null || fail "the Icecast pull's format: $(source_of "$ice")"
source_of "$srt" | jq -e '.format | startswith("mpegts/aac")' >/dev/null || fail "the SRT pull's format: $(source_of "$srt")"
[ "$(date +%s)" -lt "$post_a" ] || fail "too slow: the first show's post came before both stood by (started $t0)"
grep -q 'a pull is due: opening it' <(podman logs "$prefix-ingest" 2>&1) || fail "the ingest never said it opened a pull"
echo "   ok: Icecast $(show_it "$ice")"
echo "       SRT     $(show_it "$srt")"

echo "2. the first show's post: the Icecast pull goes on air"
wait_for "the Icecast pull on air at its post" $(( post_a - $(date +%s) + 15 )) is_on_air "$ice"
[ "$(date +%s)" -ge "$post_a" ] || fail "the pull went on air before its post"
is_state "$ice" on_air || fail "GET /sources says $(state_of "$ice")"
is_off_air "$srt" || fail "the second show's pull went on air at the first show's post"
echo "   ok: on air $(( $(date +%s) - post_a )) s after the post"

echo "3. the Icecast partner drops and comes back: LINK LOST, reopened, back on air"
podman rm -f "$prefix-partner-icecast-source" >/dev/null
wait_for "LINK LOST for the Icecast pull" 25 is_state "$ice" link_lost
source_of "$ice" | jq -e '.place_kept_until_ms != null' >/dev/null || fail "no place kept: $(source_of "$ice")"
lost=$(date +%s)
sleep 3
icecast_source
wait_for "the Icecast pull back on air" 35 is_on_air "$ice"
grep -q 'cannot open a pull; trying again' <(podman logs "$prefix-ingest" 2>&1) \
  && echo "   (the ingest tried while the partner was away, and said so)"
echo "   ok: back on air $(( $(date +%s) - lost )) s after the loss"

echo "4. the second show's post: after the grace the SRT pull takes the air; the first is closed"
wait_for "the second show's post" $(( post_b - $(date +%s) + 5 )) bash -c "[ \$(date +%s) -ge $post_b ]"
wait_for "the SRT pull on air after the grace" 25 is_on_air "$srt"
handover=$(( $(date +%s) - post_b ))
[ "$handover" -ge 3 ] || fail "the SRT pull took the air $handover s after the post: no grace (4 s)"
wait_for "the first pull to come off the air" 10 bash -c "! curl -s -H 'Authorization: Bearer $ADMIN' http://127.0.0.1:$CONTROL_PORT/api/v1/studio | jq -e '.sources[] | select(.identity==\"source-$ice\") | .on_air' | grep -q true"
wait_for "the first pull, its door closed, to be closed (never a test)" 30 gone "$ice"
echo "   ok: on air $handover s after the post; the first pull closed"

echo "5. the SRT partner drops and comes back: LINK LOST, reopened, back on air"
podman rm -f -t 0 "$prefix-partner-srt" >/dev/null
wait_for "LINK LOST for the SRT pull" 30 is_state "$srt" link_lost
lost=$(date +%s)
sleep 3
srt_source
wait_for "the SRT pull back on air" 40 is_on_air "$srt"
echo "   ok: back on air $(( $(date +%s) - lost )) s after the loss"
echo "PASS: pulls from Icecast and SRT stand by, go on air at the post, come back after a drop, and are heard at the desk"
