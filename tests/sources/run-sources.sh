#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Hosts in their own studios (ADR-0280), end to end on loopback: PostgreSQL, the control plane,
# the ingest, and the desk, with real host software standing in for a host's: ffmpeg's
# `icecast://` output sending an MP3 of a 44.1 kHz tone, and ffmpeg's `srt://` (libsrt) sending
# AAC in MPEG-TS with a stream id and a passphrase. It checks that:
#
#   1. an Icecast source in its show's slot is admitted and stands by (STANDBY: at the desk in
#      slot 5, not on air), and its sound arrives at the desk;
#   2. a wrong key is refused (ffmpeg fails; the key's row says wrong_key);
#   3. an SRT source outside its slot is a private test, and never reaches the desk;
#   4. an SRT source in its slot stands by, and its sound arrives at the desk;
#   5. a wrong SRT passphrase is refused by the handshake and counted as a wrong key;
#   6. ending a key closes its connection;
#   7. (DESK=real) when a slot's door closes, a source still standing by becomes a private test
#      (off the desk, its connection kept); with `test_outside` off its connection is closed
#      instead; a source on air is never touched (ADR-0280, the operator's yes of 2026-11-16);
#   8. (DESK=real) the post's rule with short settings: a late host goes on air after the count; at a
#      show's post a host on air keeps the air for the grace and then the schedule hands over (the
#      outgoing host becomes a test); a producer's hold stops one handover; End show passes the air
#      to the next host standing by; a dropped link keeps its place and the return goes back on air.
#
# DESK=real (the default) runs the desk in its image (`just desk-build` first) and reads the
# studio's view: the source's slot, state and level. DESK=stub runs a stand-in that keeps what
# the ingest hands the desk, and checks the request and that sound arrived: a quick probe that
# needs no desk image. Ports in 25xxx, loopback only; every one is a setting.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/sources/run"
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-sources
DESK=${DESK:-real}
CONTROL_PORT=${CONTROL_PORT:-25310}
PG_PORT=${PG_PORT:-25432}
DESK_SOURCES_PORT=${DESK_SOURCES_PORT:-25318}
DESK_SRT_PORT=${DESK_SRT_PORT:-25313}
ICE_PORT=${ICE_PORT:-25005}
SRT_PORT=${SRT_PORT:-25710}
PG_IMAGE=${PG_IMAGE:-docker.io/library/postgres:18}
DESK_IMAGE=${DESK_IMAGE:-${WEAVESIGNAL_DESK_IMAGE:-localhost/weavesignal-desk-dev:m5}}
ADMIN=sources-admin-test-only
INGEST_TOKEN=sources-ingest-test-only-sources-ingest-0
DESK_PASSWORD=sources-desk-test-only
case $DESK in real|stub) ;; *) echo "DESK must be real or stub" >&2; exit 2 ;; esac

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$CONTROL_PORT" "$PG_PORT" "$DESK_SOURCES_PORT" "$ICE_PORT"; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done
for p in "$SRT_PORT" "$DESK_SRT_PORT"; do taken -u "$p" && { echo "port $p/udp is in use; set another" >&2; exit 1; }; done

rm -rf "$run"; mkdir -p "$run/stub"
pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
  podman logs "$prefix-pg" > "$run/pg.log" 2>&1 || true
  podman logs "$prefix-desk" > "$run/desk.log" 2>&1 || true
  podman rm -f "$prefix-pg" "$prefix-desk" >/dev/null 2>&1 || true
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

# The control plane, with the desk's socket where the desk will make it.
cat > "$run/control.toml" <<TOML
[station]
id = "6f1c1f43-7d0e-4d3e-9b39-6c8f3f8d2a10"
name = "Kettle FM"
[ports]
control = $CONTROL_PORT
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

podman run -d --name "$prefix-pg" -p "127.0.0.1:$PG_PORT:5432" -e POSTGRES_PASSWORD=test-only \
  -e POSTGRES_DB=weavesignal "$PG_IMAGE" >/dev/null
cargo build -q -p weavesignal-control -p weavesignal-ingest
for i in $(seq 1 60); do
  podman exec "$prefix-pg" pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && fail "PostgreSQL did not come up"
done

# The desk: the real one in its image, or the stand-in.
if [ "$DESK" = real ]; then
  [ -x "$root/target/container/debug/weavesignal-desk" ] || { echo "build the desk first: just desk-build" >&2; exit 1; }
  cat > "$run/desk.toml" <<TOML
[livekit]
url = "ws://127.0.0.1:1"
api_key = "unused"
room = "studio"
[programme]
srt_port = $DESK_SRT_PORT
[control]
socket = "/work/tests/sources/run/desk.sock"
[sources]
port = $DESK_SOURCES_PORT
TOML
  podman run -d --name "$prefix-desk" --network host --security-opt label=disable -v "$root:/work" -w /work \
    -e WEAVESIGNAL_LIVEKIT_API_SECRET=sources-test-only-not-for-use-0000 \
    -e "WEAVESIGNAL_DESK_SOURCES_PASSWORD=$DESK_PASSWORD" -e RUST_LOG=weavesignal_desk=info \
    "$DESK_IMAGE" /work/target/container/debug/weavesignal-desk --config /work/tests/sources/run/desk.toml >/dev/null
else
  # Each connection: the head to a file, 200 OK, then the WAV and its samples to another.
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
fi

WEAVESIGNAL_DATABASE_URL="postgres://postgres:test-only@127.0.0.1:$PG_PORT/weavesignal" \
WEAVESIGNAL_CONTROL_ADMIN_TOKEN=$ADMIN WEAVESIGNAL_INGEST_TOKEN=$INGEST_TOKEN \
  "$root/target/debug/weavesignal-control" --config "$run/control.toml" > "$run/control.log" 2>&1 &
pids+=($!)
for i in $(seq 1 60); do
  curl -fs "http://127.0.0.1:$CONTROL_PORT/health" >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && fail "the control plane did not come up"
done

# Two shows: Steeped on air now (an hour from ten minutes ago), Kettle Drive in three days.
air() { echo "$(TZ=Europe/London date -d "@$(( $(date +%s) + $1 ))" +%Y-%m-%dT%H:%M:%S%:z)[Europe/London]"; }
show() { # slug name from-now-s [minutes]
  api PUT "/api/v1/shows/$1" "{\"name\":\"$2\",\"colour_slot\":3}" >/dev/null
  api PUT "/api/v1/shows/$1/series/1" '{}' >/dev/null
  api POST "/api/v1/shows/$1/series/1/episodes" '{"count":1}' >/dev/null
  local r; r=$(api PATCH "/api/v1/shows/$1/series/1/episodes/1" "{\"air_at\":\"$(air "$3")\",\"minutes\":${4:-60}}")
  echo "$r" | jq -e '.episode.air_at' >/dev/null || fail "episode for $1: $r"
}
show steeped Steeped -600
show kettle-drive "Kettle Drive" $((3 * 86400))
key() { api POST "/api/v1/shows/$1/keys" "{\"label\":\"$2\",\"hint\":\"$3\"}"; }
ICE=$(key steeped "Finlay Grant" icecast)
SRT_IN=$(key steeped "Heather Douglas" srt)
SRT_OUT=$(key kettle-drive "Shona Campbell" srt)
for k in "$ICE" "$SRT_IN" "$SRT_OUT"; do echo "$k" | jq -e '.secret' >/dev/null || fail "a key: $k"; done
ice_id=$(jq -r .key.key_id <<<"$ICE"); ice_pass=$(jq -r .details.icecast.password <<<"$ICE")
in_id=$(jq -r .key.key_id <<<"$SRT_IN"); in_secret=$(jq -r .secret <<<"$SRT_IN")
out_id=$(jq -r .key.key_id <<<"$SRT_OUT"); out_secret=$(jq -r .secret <<<"$SRT_OUT")
api PUT /api/v1/settings/sources "$(api GET /api/v1/settings/sources | jq --argjson i "$ICE_PORT" --argjson s "$SRT_PORT" '.port_icecast=$i | .port_srt=$s')" | jq -e .port_srt >/dev/null

# The ingest, on loopback.
WEAVESIGNAL_INGEST_TOKEN=$INGEST_TOKEN WEAVESIGNAL_DESK_SOURCES_PASSWORD=$DESK_PASSWORD RUST_LOG=info \
  "$root/target/debug/weavesignal-ingest" run --control "http://127.0.0.1:$CONTROL_PORT" \
  --desk "127.0.0.1:$DESK_SOURCES_PORT" --bind 127.0.0.1 > "$run/ingest.log" 2>&1 &
pids+=($!)
for i in $(seq 1 40); do
  taken -t "$ICE_PORT" && taken -u "$SRT_PORT" && break
  sleep 0.5; [ "$i" = 40 ] && fail "the ingest did not listen on $ICE_PORT and $SRT_PORT"
done

# A 44.1 kHz tone (the resampler's work), long enough to read while it plays.
ffmpeg -hide_banner -loglevel error -y -f lavfi -i "sine=f=1000:sample_rate=44100:d=30" -ac 2 -c:a libmp3lame -b:a 128k "$run/tone.mp3"

source_state() { # key-id: its state in GET /sources, or none
  api GET /api/v1/sources | jq -r --arg k "$1" '(.sources[] | select(.key_id==$k) | .state) // "none"'
}
studio_source() { # identity: its entry among the studio's sources
  api GET /api/v1/studio | jq -c --arg i "$1" '.sources[] | select(.identity==$i)'
}
wait_for() { # what seconds command...
  local what=$1 secs=$2; shift 2
  for _ in $(seq 1 $((secs * 2))); do "$@" && return 0; sleep 0.5; done
  fail "$what"
}
heard_at_desk() { # identity
  if [ "$DESK" = real ]; then
    studio_source "$1" | jq -e '.lufs != null and .on_air == false and .slot != null' >/dev/null
  else
    local h; h=$(grep -l "PUT /sources/$1 " "$run"/stub/*.head 2>/dev/null | head -1) || return 1
    [ -n "$h" ] || return 1
    local pcm=${h%.head}.pcm
    # The float WAV header, then about a second of 48 kHz stereo float (384 KB) at least, and
    # the ingest's own reading of the decoded sound: a level, not silence.
    [ "$(head -c 4 "$pcm")" = RIFF ] && [ "$(stat -c %s "$pcm")" -gt 400000 ] \
      && api GET /api/v1/sources | jq -e --arg k "${1#source-}" '.sources[] | select(.key_id==$k) | .level != null and .level > -40' >/dev/null
  fi
}
never_at_desk() { # identity
  if [ "$DESK" = real ]; then
    [ -z "$(studio_source "$1")" ] && ! api GET /api/v1/studio | jq -e --arg i "$1" '.. | objects | select(.identity? == $i)' >/dev/null
  else
    ! grep -q "PUT /sources/$1 " "$run"/stub/*.head 2>/dev/null
  fi
}

echo "1. Icecast in its slot: standing by, heard at the desk"
ffmpeg -hide_banner -loglevel error -re -i "$run/tone.mp3" -c:a copy -content_type audio/mpeg -f mp3 \
  "icecast://source:$ice_pass@127.0.0.1:$ICE_PORT/live" > "$run/ffmpeg-ice.log" 2>&1 &
ff_ice=$!; pids+=($ff_ice)
wait_for "the Icecast source to stand by" 15 bash -c "[ \"\$(curl -s -H 'Authorization: Bearer $ADMIN' http://127.0.0.1:$CONTROL_PORT/api/v1/sources | jq -r '.sources[] | select(.key_id==\"$ice_id\") | .state')\" = standby ]"
wait_for "the Icecast source's sound at the desk" 15 heard_at_desk "source-$ice_id"
api GET /api/v1/sources | jq -e --arg k "$ice_id" '.sources[] | select(.key_id==$k) | .way == "icecast" and (.format | startswith("mp3 44.1 kHz"))' >/dev/null \
  || fail "the source's way and format: $(api GET /api/v1/sources)"
if [ "$DESK" = real ]; then
  studio_source "source-$ice_id" | jq -e '.slot == 5 and .role == "source" and .source.way == "icecast"' >/dev/null \
    || fail "slot 5, role source, way icecast: $(studio_source "source-$ice_id")"
fi
echo "   ok: $(api GET /api/v1/sources | jq -c --arg k "$ice_id" '.sources[] | select(.key_id==$k) | {state, slot, way, format, level, steady_ms}')"

echo "2. a wrong key: refused"
if ffmpeg -hide_banner -loglevel error -t 3 -i "$run/tone.mp3" -c:a copy -content_type audio/mpeg -f mp3 \
  "icecast://source:${ice_id}-wrongwrongwrongwrongwron@127.0.0.1:$ICE_PORT/live" > "$run/ffmpeg-wrong.log" 2>&1; then
  fail "ffmpeg was taken with a wrong key"
fi
api GET /api/v1/shows/steeped/keys | jq -e --arg k "$ice_id" '.keys[] | select(.key_id==$k) | .last_refusal.reason == "wrong_key"' >/dev/null \
  || fail "the key's last refusal: $(api GET /api/v1/shows/steeped/keys)"
[ "$(source_state "$ice_id")" = standby ] || fail "the wrong key disturbed the connected source"
echo "   ok"

echo "3. SRT outside its slot: a private test, never on the desk"
ffmpeg -hide_banner -loglevel error -re -t 12 -i "$run/tone.mp3" -c:a aac -b:a 128k -f mpegts \
  "srt://127.0.0.1:$SRT_PORT?streamid=$out_id&passphrase=$out_secret&pbkeylen=16" > "$run/ffmpeg-srt-test.log" 2>&1 &
pids+=($!)
wait_for "the SRT test" 15 bash -c "[ \"\$(curl -s -H 'Authorization: Bearer $ADMIN' http://127.0.0.1:$CONTROL_PORT/api/v1/sources | jq -r '.sources[] | select(.key_id==\"$out_id\") | .state')\" = test ]"
sleep 3
never_at_desk "source-$out_id" || fail "a test reached the desk"
api GET /api/v1/sources | jq -e --arg k "$out_id" '.sources[] | select(.key_id==$k) | .way == "srt" and (.format | startswith("mpegts/aac "))' >/dev/null \
  || fail "the test's way and format: $(api GET /api/v1/sources)"
echo "   ok: $(api GET /api/v1/sources | jq -c --arg k "$out_id" '.sources[] | select(.key_id==$k) | {state, way, format, level}')"

echo "4. SRT in its slot, with its passphrase: standing by, heard at the desk"
ffmpeg -hide_banner -loglevel error -re -t 20 -i "$run/tone.mp3" -c:a aac -b:a 128k -f mpegts \
  "srt://127.0.0.1:$SRT_PORT?streamid=$in_id&passphrase=$in_secret&pbkeylen=16" > "$run/ffmpeg-srt.log" 2>&1 &
pids+=($!)
wait_for "the SRT source's sound at the desk" 20 heard_at_desk "source-$in_id"
[ "$(source_state "$in_id")" = standby ] || fail "the SRT source is $(source_state "$in_id")"
if [ "$DESK" = real ]; then
  studio_source "source-$in_id" | jq -e '.slot == 1' >/dev/null || fail "the second source in slot 1: $(studio_source "source-$in_id")"
fi
echo "   ok: $(api GET /api/v1/sources | jq -c --arg k "$in_id" '.sources[] | select(.key_id==$k) | {state, slot, way, format, level}')"

echo "5. a wrong SRT passphrase: refused by the handshake, counted as a wrong key"
if timeout 8 ffmpeg -hide_banner -loglevel error -t 2 -i "$run/tone.mp3" -c:a aac -f mpegts \
  "srt://127.0.0.1:$SRT_PORT?streamid=$in_id&passphrase=wrongwrongwrongwrongwrong&pbkeylen=16" > "$run/ffmpeg-srt-wrong.log" 2>&1; then
  fail "ffmpeg was taken with a wrong passphrase"
fi
sleep 1
api GET /api/v1/shows/steeped/keys | jq -e --arg k "$in_id" '.keys[] | select(.key_id==$k) | .last_refusal.reason == "wrong_key"' >/dev/null \
  || fail "the SRT key's last refusal: $(api GET /api/v1/shows/steeped/keys)"
echo "   ok"

echo "6. ending a key closes its connection"
api DELETE "/api/v1/shows/steeped/keys/$ice_id" | jq -e '.ended' >/dev/null
wait_for "the ended key's source to go" 10 bash -c "[ \"\$(curl -s -H 'Authorization: Bearer $ADMIN' http://127.0.0.1:$CONTROL_PORT/api/v1/sources | jq -r '.sources[] | select(.key_id==\"$ice_id\") | .state')\" = '' ]"
echo "   ok"
if [ "$DESK" = real ]; then
echo "7. the slot's door closes: standing by becomes a private test; on air is never touched; with tests off the connection is closed"
# The post's rule is not under test here: no grace, no late count.
api PUT /api/v1/settings/sources "$(api GET /api/v1/settings/sources | jq '.grace_s=0 | .late_count_s=0 | .test_outside=true')" | jq -e '.test_outside' >/dev/null
# Door-a airs for a minute from just now; door-b takes the air when it ends, door-c after door-b:
# a show's door closes when the next show takes the air.
t0=$(date +%s)
show door-a "Door A" -10 1
show door-b "Door B" 50 1
show door-c "Door C" 110 1
A1=$(key door-a "Ada Stand" icecast); A2=$(key door-a "Ben Air" srt); B1=$(key door-b "Cora Close" srt)
a1_id=$(jq -r .key.key_id <<<"$A1"); a1_pass=$(jq -r .details.icecast.password <<<"$A1")
a2_id=$(jq -r .key.key_id <<<"$A2"); a2_secret=$(jq -r .secret <<<"$A2")
b1_id=$(jq -r .key.key_id <<<"$B1"); b1_secret=$(jq -r .secret <<<"$B1")
ffmpeg -hide_banner -loglevel error -y -f lavfi -i "sine=f=1000:sample_rate=44100:d=480" -ac 2 -c:a libmp3lame -b:a 128k "$run/tone-long.mp3"
ffmpeg -hide_banner -loglevel error -re -i "$run/tone-long.mp3" -c:a copy -content_type audio/mpeg -f mp3 \
  "icecast://source:$a1_pass@127.0.0.1:$ICE_PORT/live" > "$run/ffmpeg-a1.log" 2>&1 &
ff_a1=$!; pids+=($ff_a1)
ffmpeg -hide_banner -loglevel error -re -t 200 -i "$run/tone-long.mp3" -c:a aac -b:a 128k -f mpegts \
  "srt://127.0.0.1:$SRT_PORT?streamid=$a2_id&passphrase=$a2_secret&pbkeylen=16" > "$run/ffmpeg-a2.log" 2>&1 &
ff_a2=$!; pids+=($ff_a2)
ffmpeg -hide_banner -loglevel error -re -t 200 -i "$run/tone-long.mp3" -c:a aac -b:a 128k -f mpegts \
  "srt://127.0.0.1:$SRT_PORT?streamid=$b1_id&passphrase=$b1_secret&pbkeylen=16" > "$run/ffmpeg-b1.log" 2>&1 &
ff_b1=$!; pids+=($ff_b1)
for id in "$a1_id" "$a2_id" "$b1_id"; do
  wait_for "source $id at the desk" 25 heard_at_desk "source-$id"
done
[ "$(( $(date +%s) - t0 ))" -lt 40 ] || fail "too slow to reach the desk before door-a's door closes (started at $t0)"
# door-a's second host goes on air, as a person's On does.
api POST /api/v1/desk/commands "{\"op\":\"on_air\",\"identity\":\"source-$a2_id\",\"on\":true}" | jq -e . >/dev/null || fail "On for $a2_id"
wait_for "the second host on air" 10 bash -c "[ \"\$(curl -s -H 'Authorization: Bearer $ADMIN' http://127.0.0.1:$CONTROL_PORT/api/v1/sources | jq -r '.sources[] | select(.key_id==\"$a2_id\") | .state')\" = on_air ]"
[ "$(source_state "$a1_id")" = standby ] || fail "door-a's first host is $(source_state "$a1_id") before its door closed"
echo "   door-a open: $(source_state "$a1_id") and $(source_state "$a2_id"); door-b's host $(source_state "$b1_id")"
# door-a's door closes when door-b takes the air.
wait_for "door-a's host standing by to become a test" 90 bash -c "[ \"\$(curl -s -H 'Authorization: Bearer $ADMIN' http://127.0.0.1:$CONTROL_PORT/api/v1/sources | jq -r '.sources[] | select(.key_id==\"$a1_id\") | .state')\" = test ]"
wait_for "it off the desk" 10 never_at_desk "source-$a1_id"
kill -0 "$ff_a1" 2>/dev/null || fail "the test's connection was closed, not kept: $(cat "$run/ffmpeg-a1.log")"
grep -q "the slot's door closed: now a private test" "$run/ingest.log" || fail "the ingest did not say the door closed"
[ "$(source_state "$a2_id")" = on_air ] || fail "the host on air was touched: $(source_state "$a2_id")"
studio_source "source-$a2_id" | jq -e '.on_air == true' >/dev/null || fail "the host on air left the desk: $(studio_source "source-$a2_id")"
kill -0 "$ff_a2" 2>/dev/null || fail "the host on air was disconnected"
echo "   door-a closed: $(source_state "$a1_id") (kept), $(source_state "$a2_id") (on air, untouched)"
# With tests outside the slot off, door-b's host (still standing by) is disconnected when its door closes.
api PUT /api/v1/settings/sources "$(api GET /api/v1/settings/sources | jq '.test_outside=false')" | jq -e '.test_outside == false' >/dev/null
[ "$(source_state "$b1_id")" = standby ] || fail "door-b's host is $(source_state "$b1_id") before its door closed"
wait_for "door-b's host closed with tests off" 90 bash -c "[ \"\$(curl -s -H 'Authorization: Bearer $ADMIN' http://127.0.0.1:$CONTROL_PORT/api/v1/sources | jq -r '.sources[] | select(.key_id==\"$b1_id\") | .state')\" = '' ]"
wait_for "its software to see the connection closed" 10 bash -c "! kill -0 $ff_b1 2>/dev/null"
never_at_desk "source-$b1_id" || fail "door-b's host stayed at the desk"
[ "$(source_state "$a2_id")" = on_air ] || fail "the host on air was touched by door-b's door: $(source_state "$a2_id")"
kill -0 "$ff_a2" 2>/dev/null || fail "the host on air was disconnected by door-b's door"
echo "   ok: door-b's host closed (tests off); the host on air untouched"

echo "8. the post's rule: the late count, the grace, a hold, End show, a dropped link (short rules)"
# Section 7's hosts go: its connections close and the slots they held are free for these.
api DELETE "/api/v1/shows/door-a/keys/$a1_id" >/dev/null
api DELETE "/api/v1/shows/door-a/keys/$a2_id" >/dev/null
api DELETE "/api/v1/shows/door-b/keys/$b1_id" >/dev/null
kill "$ff_a1" "$ff_a2" "$ff_b1" 2>/dev/null || true
api PUT /api/v1/settings/sources "$(api GET /api/v1/settings/sources | jq '.grace_s=6 | .late_count_s=3 | .steady_s=2 | .silence_s=2 | .place_kept_s=20 | .test_outside=true')" | jq -e '.grace_s == 6' >/dev/null || fail "the short rules"
is_on_air() { studio_source "source-$1" | jq -e '.on_air == true' >/dev/null; }
is_off_air() { studio_source "source-$1" | jq -e '.on_air == false' >/dev/null; }
is_state() { [ "$(source_state "$1")" = "$2" ]; }
connect_srt() { # id secret seconds log
  ffmpeg -hide_banner -loglevel error -re -t "$3" -i "$run/tone-long.mp3" -c:a aac -b:a 128k -f mpegts \
    "srt://127.0.0.1:$SRT_PORT?streamid=$1&passphrase=$2&pbkeylen=16" > "$run/$4" 2>&1 &
  last_ff=$!; pids+=($last_ff)
}
# Four shows a minute long, a minute apart, the first starting in two seconds (a show's door closes when the
# next takes the air); each host connects now, after the first show's post (door-c, from section 7, is the
# show on air until then).
t0=$(date +%s)
show post-a "Post A" 2 1
show post-b "Post B" 65 1
show post-c "Post C" 125 1
show post-d "Post D" 185 1
PA=$(key post-a "Ada Late" srt); PB=$(key post-b "Ben Grace" srt); PC=$(key post-c "Cora Hold" srt); PD=$(key post-d "Dan End" srt)
pa=$(jq -r .key.key_id <<<"$PA"); pa_s=$(jq -r .secret <<<"$PA")
pb=$(jq -r .key.key_id <<<"$PB"); pb_s=$(jq -r .secret <<<"$PB")
pc=$(jq -r .key.key_id <<<"$PC"); pc_s=$(jq -r .secret <<<"$PC")
pd=$(jq -r .key.key_id <<<"$PD"); pd_s=$(jq -r .secret <<<"$PD")
connect_srt "$pa" "$pa_s" 400 ff-pa.log; ff_pa=$last_ff
connect_srt "$pb" "$pb_s" 400 ff-pb.log; ff_pb=$last_ff
connect_srt "$pc" "$pc_s" 400 ff-pc.log; ff_pc=$last_ff
connect_srt "$pd" "$pd_s" 400 ff-pd.log; ff_pd=$last_ff

echo "   8a. a host who arrives after the post: standing by, then the late count puts them on air"
wait_for "the late count to put the first host on air" 25 is_on_air "$pa"
studio_post() { api GET /api/v1/studio | jq -e "$1" >/dev/null; } # the post's view is worked out once a second
wait_for "the post to be post-b's" 10 studio_post '.post.show.slug == "post-b"' 
for k in "$pb" "$pc" "$pd"; do wait_for "host $k at the desk" 20 heard_at_desk "source-$k"; done
[ "$(( $(date +%s) - t0 ))" -lt 60 ] || fail "too slow before post-b's post (started at $t0)"
is_off_air "$pb" || fail "the next host is on air before its post"

echo "   8b. the post, with a host on air: the grace counts, and at its end the schedule hands over"
post_started() { api GET /api/v1/studio | jq -e --arg s "$1" '.post.show.slug == $s and .post.show.started == true' >/dev/null; }
wait_for "post-b's post" 70 post_started post-b
wait_for "the grace to be counting" 5 studio_post '.post.show.slug == "post-b" and .post.state == "standing_by" and (.post.grace_ends_ms != null) and (.post.holders | length == 1)' 
is_on_air "$pa" || fail "the host on air lost the air at the post"
wait_for "the grace's end to hand the air to the next host" 20 is_on_air "$pb"
# Its door is shut, so the ingest may close it within a report of its coming off air: gone counts.
is_off_or_gone() { is_off_air "$1" || never_at_desk "source-$1"; }
wait_for "the outgoing host to come off air (the handover's second step)" 5 is_off_or_gone "$pa"
# The outgoing host's door is shut and it is off air now: the next report makes it a private test.
wait_for "the outgoing host to become a test" 15 is_state "$pa" test
never_at_desk "source-$pa" || fail "the outgoing host stayed at the desk"
echo "      ok: $(api GET /api/v1/sources | jq -c '.sources[] | {key_id, state}' | tr '\n' ' ')"

echo "   8c. a producer's hold stops the grace for one handover"
wait_for "the next post to be post-c's" 10 studio_post '.post.show.slug == "post-c"' 
api POST /api/v1/air/post/hold | jq -e '.held' >/dev/null || fail "hold"
wait_for "post-c's post" 70 post_started post-c
sleep 9 # the grace is 6 s
is_on_air "$pb" || fail "the hold did not keep the air"
is_off_air "$pc" || fail "the held handover happened"
wait_for "the hold to be shown" 5 studio_post '.post.held_by != null' 
api DELETE /api/v1/air/post/hold | jq -e '.held == false' >/dev/null
wait_for "the released handover to happen" 15 is_on_air "$pc"
wait_for "the outgoing host to come off air (the handover's second step)" 5 is_off_or_gone "$pb"
echo "      ok"

echo "   8d. End show passes the air to the next host standing by"
r=$(api POST /api/v1/show/end)
echo "$r" | jq -e '.passed_to == "Dan End" and .automation_in_ms == null' >/dev/null || fail "End show: $r"
wait_for "the next host on air" 10 is_on_air "$pd"
wait_for "the outgoing host to come off air (the handover's second step)" 5 is_off_or_gone "$pc"
echo "      ok"

echo "   8e. a dropped link: the place is kept, the return goes back on air"
wait_for "post-d's time to pass" 90 bash -c "[ \$(date +%s) -ge $((t0 + 198)) ]"
show post-f "Post F" -2 60
PF=$(key post-f "Fay Drop" srt)
pf=$(jq -r .key.key_id <<<"$PF"); pf_s=$(jq -r .secret <<<"$PF")
api POST /api/v1/show/end | jq -e '.passed_to == null' >/dev/null || fail "End show with nobody to pass to"
wait_for "the air to be empty" 10 is_off_air "$pd"
connect_srt "$pf" "$pf_s" 300 ff-pf.log; ff_pf=$last_ff
wait_for "the late count to put the host on air" 30 is_on_air "$pf"
kill "$ff_pf"
wait_for "the lost link to show" 20 is_state "$pf" link_lost
wait_for "the lost link in the studio's view" 5 studio_post ".post.link_lost.key_id == \"$pf\" and (.post.link_lost.place_kept_until_ms - .post.link_lost.lost_ms == 20000)"
api GET /api/v1/sources | jq -e --arg k "$pf" '.sources[] | select(.key_id==$k) | .place_kept_until_ms != null' >/dev/null || fail "the place kept in GET /sources"
connect_srt "$pf" "$pf_s" 200 ff-pf2.log; ff_pf=$last_ff
wait_for "the returning host to go back on air" 25 is_on_air "$pf"
# Off air (design 3.6): the place kept for a lost link goes at once, by a press, and is logged.
# The first loss's place may still be in the view for up to a second after the return (the view is worked
# out once a second): wait for the NEW loss, whose place is kept 20 s from a time after the kill.
killed_ms=$(date +%s%3N)
kill "$ff_pf"
wait_for "the new loss to show, with a place kept from after the kill" 20 bash -c "curl -s -H 'Authorization: Bearer $ADMIN' http://127.0.0.1:$CONTROL_PORT/api/v1/sources | jq -e --arg k '$pf' --argjson t $killed_ms '.sources[] | select(.key_id==\$k) | .state == \"link_lost\" and .place_kept_until_ms >= (\$t + 20000)' >/dev/null"
out=$(api POST "/api/v1/sources/$pf/off-air")
jq -e '.let_go == true' <<<"$out" >/dev/null || fail "Off air: $out; the list: $(api GET /api/v1/sources | jq -c --arg k "$pf" '.sources[] | select(.key_id==$k)')"
wait_for "the place to be let go" 5 studio_post '(.post == null) or (.post.link_lost == null)'
api POST "/api/v1/sources/$pf/off-air" | jq -e '.error | test("no place")' >/dev/null || fail "a second Off air has nothing to let go"
# The schedule's moves fade over fade_ms (2 s): the real desk took the field (the control plane
# would have said so, and switched at once, had it refused), and says it faded.
! grep -q "would not fade" "$run/control.log" || fail "the desk would not fade: $(grep 'would not fade' "$run/control.log" | head -1)"
if [ "$DESK" = real ]; then
  desk_log=$(podman logs "$prefix-desk" 2>&1 || true) # (grep -q in a pipe would end podman early: pipefail)
  grep -q "on air switched, faded" <<<"$desk_log" || fail "the desk never faded a move of the schedule's"
fi
echo "      ok"
fi
echo "PASS (DESK=$DESK)"
