#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The outgoing Icecast source on the real stack (ADR-0226): Liquidsoap 2.5.0-rc1 (playout and the
# listener process), a stand-in desk (a 1 kHz tone over SRT, silenced while run/desk-quiet
# exists), a throwaway PostgreSQL 18, the control plane (a release build), and Icecast 2.5 (the
# relay image's) standing in for a DJ's AzuraCast: one mount that takes a source only with a user
# name and password.
#
# It checks that
#   - the programme reaches the remote as MP3 once the station is live with the desk on air, and
#     a client decodes it (1 kHz, 44.1 kHz, 128 kbit/s);
#   - the control plane follows the desk: the source goes when the desk falls silent for the
#     dead-air time and comes back when it sounds;
#   - the manual choice works (off, on, auto) and the status says what is happening;
#   - with the remote killed our own listener stream does not stop (a second listener reads it
#     the whole time), the status says what failed, and the source returns when the remote does;
#   - a wrong password is one clear error in the status, with tries that back off (a handful in
#     45 s, not one every few seconds), and the right one is taken as soon as it is given.
# Ports sit in 22300-22399 by default, clear of the station defaults and the other stack tests.
#
#   RELAY_IMAGE=localhost/weavesignal-relay:m1 tests/playout/run-icecast-out.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/playout/run-icecast-out"
CONTROL_PORT=${CONTROL_PORT:-22310}
LISTENERS_PORT=${LISTENERS_PORT:-22312}
FEED_PORT=${FEED_PORT:-22315}
SRT_PORT=${SRT_PORT:-22313}
PG_PORT=${PG_PORT:-22333}
REMOTE_PORT=${REMOTE_PORT:-22300}
# The Opus stream's constant rate (ADR-0233); the byte counts below follow it.
OPUS_RATE=${OPUS_RATE:-96}
LIQ_IMAGE=${LIQ_IMAGE:-docker.io/savonet/liquidsoap:v2.5.0-rc1}
PG_IMAGE=${PG_IMAGE:-docker.io/library/postgres:18}
RELAY_IMAGE=${RELAY_IMAGE:-localhost/weavesignal-relay:m1}
SOURCE_PASSWORD=icecast-out-test-only
ADMIN=icecast-out-test-admin
REMOTE_PASSWORD=azura-streamer-test-only
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-icecastout
station=8d0c2f4e-6a1b-4c3d-9e5f-7a8b9c0d1e2f

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$CONTROL_PORT" "$LISTENERS_PORT" "$FEED_PORT" "$PG_PORT" "$REMOTE_PORT"; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done
taken -u "$SRT_PORT" && { echo "port $SRT_PORT/udp is in use; set another" >&2; exit 1; }
podman image exists "$RELAY_IMAGE" || { echo "no image $RELAY_IMAGE: run 'just relay-image'" >&2; exit 1; }

fail() { echo "FAIL: $*" >&2; exit 1; }
ok() { echo "ok: $*"; }

rm -rf "$run"; mkdir -p "$run/library" "$run/fallback" "$run/recordings/logger" "$run/carts" "$run/sock"
chmod 777 "$run"
ffmpeg -v error -f lavfi -i "sine=frequency=440:duration=20" -ac 2 -ar 48000 "$run/fallback/fallback-440.flac"

# The remote: the relay image's Icecast with this test's own configuration.
remote_conf() {   # password
  sed -e "s/@PORT@/$REMOTE_PORT/" -e "s/@PASSWORD@/$1/" "$root/tests/playout/icecast-out-icecast.xml" > "$run/icecast.xml"
  chmod 644 "$run/icecast.xml"
}
remote_conf "$REMOTE_PASSWORD"

cat > "$run/playout.json" <<JSON
{
  "station_id": "00000000-0000-0000-0000-000000000000",
  "srt": { "host": "127.0.0.1", "port": $SRT_PORT, "format": "mpegts" },
  "listeners": { "bind": "127.0.0.1", "host": "127.0.0.1", "port": $LISTENERS_PORT, "mount": "programme.opus", "feed_port": $FEED_PORT, "bitrate": $OPUS_RATE, "control_socket": "/sock/listeners.sock" },
  "icecast_out": { "enabled": true, "host": "127.0.0.1", "port": $REMOTE_PORT, "mount": "/dj", "user": "dj", "tls": false, "bitrate": 128, "name": "WeaveSignal test" },
  "control_socket": "/sock/playout.sock",
  "carts": { "dir": "/media/carts" },
  "library": { "dir": "/media/library" },
  "fallback": { "dir": "/media/fallback" },
  "logger": { "dir": "/media/recordings/logger", "bitrate": 48 },
  "programme": { "mode": "live", "dead_air_s": 4.0, "dead_air_db": -50.0 }
}
JSON
# The stand-in desk, as in run-playout.sh: MPEG-TS, a tone, silence while desk-quiet exists.
cat > "$run/desk.liq" <<LIQ
s = amplify({if file.exists("/r/desk-quiet") then 0. else 1. end}, sine(1000.))
output.srt(mode="listener", bind_address="127.0.0.1", port=$SRT_PORT,
  %ffmpeg(format="mpegts", %audio(codec="mp2", b="256k", ac=2, ar=48000)), s)
LIQ
cat > "$run/control.toml" <<TOML
[station]
id = "$station"
name = "Kettle FM"

[ports]
control = $CONTROL_PORT
listeners = $LISTENERS_PORT

[playout]
socket = "$run/sock/playout.sock"
listeners_socket = "$run/sock/listeners.sock"
library_dir = "$run/library"
poll_ms = 100

[studio]
listener_delay_ms = 0

[retention]
recordings_dir = "$run/recordings"

[log]
format = "json"
TOML

# A reader of our own listener stream: counts bytes in one-second bins and the longest wait
# between reads, and prints one line at the end.
cat > "$run/monitor.py" <<'PY'
import http.client, json, signal, sys, time
port = int(sys.argv[1])
class Stop(Exception):
    pass
def stop(*_):
    raise Stop()
signal.signal(signal.SIGTERM, stop)
t0 = last = time.time(); gap = 0.0; bins = {}; eof = False
try:
    while True:
        try:
            c = http.client.HTTPConnection("127.0.0.1", port, timeout=15)
            c.request("GET", "/programme.opus")
            r = c.getresponse()
            if r.status == 200:
                break
        except OSError:
            pass
        time.sleep(0.5)
    t0 = last = time.time()
    while True:
        d = r.read1(65536)
        if not d:
            eof = True
            break
        now = time.time(); gap = max(gap, now - last); last = now
        k = int(now - t0); bins[k] = bins.get(k, 0) + len(d)
except Stop:
    pass
n = int(time.time() - t0)
print(json.dumps({"eof": eof, "max_gap_s": round(gap, 2), "seconds": n,
                  "bins": [bins.get(i, 0) for i in range(n)]}))
PY
# How much of a decoded MP3 (mono, 8 kHz) is a tone at the given frequency, 0 to 1.
cat > "$run/tone.py" <<'PY'
import math, struct, sys
data = sys.stdin.buffer.read()
x = struct.unpack("<%dh" % (len(data) // 2), data[: len(data) // 2 * 2])
f, rate = float(sys.argv[1]), 8000.0
w = 2 * math.pi * f / rate
s1 = s2 = 0.0; energy = 0.0
for v in x:
    energy += v * v
    s0 = v + 2 * math.cos(w) * s1 - s2
    s2, s1 = s1, s0
power = s1 * s1 + s2 * s2 - 2 * math.cos(w) * s1 * s2
print("%.3f" % (min(1.0, 2 * power / (len(x) * energy)) if energy else 0.0))
PY

cleanup() {
  [ -n "${monitor_pid:-}" ] && kill "$monitor_pid" 2>/dev/null || true
  [ -n "${control_pid:-}" ] && kill "$control_pid" 2>/dev/null || true
  for c in playout listeners desk pg remote; do
    podman logs "$prefix-$c" > "$run/$c.log" 2>&1 || true
    podman rm -f "$prefix-$c" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT
for c in playout listeners desk pg remote; do podman rm -f "$prefix-$c" >/dev/null 2>&1 || true; done

cargo build --release -q -p weavesignal-control
common=(--network host --security-opt label=disable)
podman run -d --name "$prefix-pg" -p "127.0.0.1:$PG_PORT:5432" -e POSTGRES_PASSWORD=test-only \
  -e POSTGRES_DB=weavesignal "$PG_IMAGE" >/dev/null
podman run -d --name "$prefix-remote" "${common[@]}" -v "$run/icecast.xml:/etc/test-icecast.xml:ro" \
  --entrypoint icecast "$RELAY_IMAGE" -c /etc/test-icecast.xml >/dev/null
liq=("${common[@]}" --user 0:0 -v "$root/playout:/p:ro" -v "$run:/r:ro" -v "$run/sock:/sock"
     -v "$run/library:/media/library:ro" -v "$run/carts:/media/carts:ro" -v "$run/fallback:/media/fallback:ro"
     -v "$run/recordings:/media/recordings" -e WEAVESIGNAL_PLAYOUT_DATA=/r/playout.json
     -e "WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD=$SOURCE_PASSWORD")
podman run -d --name "$prefix-listeners" "${liq[@]}" -e "WEAVESIGNAL_ICECAST_OUT_PASSWORD=$REMOTE_PASSWORD" \
  --entrypoint /usr/bin/liquidsoap "$LIQ_IMAGE" /p/listeners.liq >/dev/null
podman run -d --name "$prefix-desk" "${liq[@]}" --entrypoint /usr/bin/liquidsoap "$LIQ_IMAGE" /r/desk.liq >/dev/null
podman run -d --name "$prefix-playout" "${liq[@]}" --entrypoint /usr/bin/liquidsoap "$LIQ_IMAGE" /p/weavesignal.liq >/dev/null

for i in $(seq 1 60); do
  podman exec "$prefix-pg" pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 \
    && [ -S "$run/sock/playout.sock" ] && [ -S "$run/sock/listeners.sock" ] && break
  sleep 0.5
  [ "$i" = 60 ] && fail "the stack did not come up"
done
WEAVESIGNAL_DATABASE_URL="postgres://postgres:test-only@127.0.0.1:$PG_PORT/weavesignal" \
WEAVESIGNAL_CONTROL_ADMIN_TOKEN="$ADMIN" \
  "$root/target/release/weavesignal-control" --config "$run/control.toml" serve > "$run/control.log" 2>&1 &
control_pid=$!
echo "stack up: control :$CONTROL_PORT, listener process :$LISTENERS_PORT, remote Icecast :$REMOTE_PORT"

api() {   # method path [json]
  curl -fsS -X "$1" -H "Authorization: Bearer $ADMIN" ${3:+-H 'Content-Type: application/json' -d "$3"} "http://127.0.0.1:$CONTROL_PORT$2"
}
out() { api GET /api/v1/icecast-out; }
# wait SECONDS "what" jq-filter-that-is-true-on-/icecast-out
wait_for() {
  local secs=$1 what=$2 filter=$3 end=$((SECONDS + $1))
  while ((SECONDS < end)); do
    out 2>/dev/null | jq -e "$filter" >/dev/null 2>&1 && { ok "$what"; return 0; }
    sleep 1
  done
  echo "--- last status:"; out | jq -c . || true
  fail "timed out (${secs}s) waiting for: $what"
}
remote_sources() { curl -fsS -u admin:admin-test-only "http://127.0.0.1:$REMOTE_PORT/admin/stats.xml" | grep -c '<source mount="/dj"' || true; }
tone_of() {   # file frequency
  ffmpeg -v error -i "$1" -ac 1 -ar 8000 -f s16le - | python3 -I "$run/tone.py" "$2"
}

# ---- the control plane is up and the station is live: the programme goes out ----------------
for i in $(seq 1 60); do api GET /api/v1/playout >/dev/null 2>&1 && break; sleep 1; [ "$i" = 60 ] && fail "the control plane did not answer"; done
s=$(out)
echo "$s" | jq -e '.configured == true and .mode == "auto" and .host == "127.0.0.1" and .mount == "/dj" and .bitrate == 128' >/dev/null || fail "status at the start: $s"
echo "$s" | grep -q "$REMOTE_PASSWORD" && fail "the password is in the status"
grep -rq "$REMOTE_PASSWORD" "$run/control.log" "$run/control.toml" "$run/playout.json" && fail "the password is in a log or a settings file"
ok "status at the start (configured, auto, no password in it)"

# A second listener reads our own stream for the whole of the failure tests below.
python3 -I "$run/monitor.py" "$LISTENERS_PORT" > "$run/monitor.json" &
monitor_pid=$!

wait_for 90 "live with the desk on air: wanted and connected" '.wanted == true and .connected == true and .reason == "live with the desk on air"'
sleep 4
curl -s -m 6 "http://127.0.0.1:$REMOTE_PORT/dj" -o "$run/rx.mp3" || true
info=$(ffprobe -v error -show_entries stream=codec_name,sample_rate,channels,bit_rate -of default=nw=1 "$run/rx.mp3" | tr '\n' ' ')
echo "a client of the remote server decodes: $info"
[[ $info == *"codec_name=mp3"* && $info == *"sample_rate=44100"* && $info == *"channels=2"* && $info == *"bit_rate=128000"* ]] || fail "the remote stream is not MP3 128 kbit/s 44.1 kHz stereo: $info"
t=$(tone_of "$run/rx.mp3" 1000)
awk -v t="$t" 'BEGIN{exit !(t>0.6)}' || fail "the remote stream is not the desk's 1 kHz tone (share $t)"
ok "the remote carries the programme as MP3 (1 kHz share $t)"
b=$(out | jq .bytes_sent); [ "$b" -gt 20000 ] || fail "bytes_sent is $b"
ok "bytes sent: $b"
[ "$(remote_sources)" = 1 ] || fail "the remote shows no source on /dj"

# ---- gating: the desk falls silent, automation takes over, the source goes ------------------
touch "$run/desk-quiet"
wait_for 40 "the desk is quiet: the source is let go" '.wanted == false and .connected == false and .reason == "the desk is not on air"'
sleep 2; [ "$(remote_sources)" = 0 ] || fail "the remote still shows a source after the desk fell silent"
ok "the remote's mount is empty again (AzuraCast would return to its AutoDJ)"
rm -f "$run/desk-quiet"
wait_for 60 "the desk sounds again: the source comes back" '.wanted == true and .connected == true'

# ---- the manual choice ------------------------------------------------------------------------
api PUT /api/v1/icecast-out '{"mode":"off"}' | jq -e '.mode == "off" and .wanted == false' >/dev/null || fail "PUT off"
wait_for 20 "off: not sending although the desk is on air" '.mode == "off" and .connected == false and .reason == "turned off"'
touch "$run/desk-quiet"
api PUT /api/v1/icecast-out '{"mode":"on"}' | jq -e '.mode == "on"' >/dev/null || fail "PUT on"
wait_for 40 "on: sending even though the desk is quiet" '.mode == "on" and .connected == true and .wanted == true'
rm -f "$run/desk-quiet"
api PUT /api/v1/icecast-out '{"mode":"auto"}' >/dev/null
wait_for 40 "auto again: sending with the desk on air" '.mode == "auto" and .connected == true'
# The choice is kept in the database, not only in memory.
n=$(podman exec "$prefix-pg" psql -U postgres -d weavesignal -tA -c "SELECT mode FROM icecast_out_setting")
[ "$n" = auto ] || fail "the database holds '$n', not 'auto'"
ok "the choice is kept in the database"
code=$(curl -s -o /dev/null -w '%{http_code}' -X PUT -H "Authorization: Bearer $ADMIN" -H 'Content-Type: application/json' -d '{"mode":"sideways"}' "http://127.0.0.1:$CONTROL_PORT/api/v1/icecast-out")
[ "$code" = 422 ] || [ "$code" = 400 ] || fail "an unknown mode answered $code"
ok "an unknown mode is refused ($code)"
code=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$CONTROL_PORT/api/v1/icecast-out")
[ "$code" = 401 ] || fail "without a bearer the route answered $code"
ok "no bearer: 401"

# ---- the remote is killed: our listeners hear nothing different; it returns ------------------
echo "killing the remote server"
podman stop -t 0 "$prefix-remote" >/dev/null
wait_for 30 "the remote is gone: not connected, the error says so" '.connected == false and (.last_error | test("connection to the server was lost|nothing is listening"))'
out | jq -r '"last error: " + .last_error'
sleep 8
echo "starting the remote server again"
podman start "$prefix-remote" >/dev/null
wait_for 60 "the remote is back: the source reconnects by itself" '.connected == true'

# ---- a wrong password: one clear error, tries that back off ----------------------------------
echo "changing the streamer's password on the remote"
remote_conf "a-different-password"
podman restart -t 0 "$prefix-remote" >/dev/null
f0=$(out | jq .failures)
wait_for 30 "the login is refused: the error says so" '.connected == false and (.last_error | test("user name or password"))'
out | jq -r '"last error: " + .last_error'
sleep 45
f1=$(out | jq .failures)
tries=$((f1 - f0))
echo "failed tries in about 75 s: $tries"
[ "$tries" -le 7 ] || fail "$tries refused logins: the retries are not backing off"
ok "a wrong password: $tries tries in about 75 s (backing off), not a storm"
echo "putting the right password back"
remote_conf "$REMOTE_PASSWORD"
podman restart -t 0 "$prefix-remote" >/dev/null
wait_for 60 "the right password: connected again" '.connected == true'

# ---- our own listener stream was unbroken throughout ----------------------------------------
kill -TERM "$monitor_pid"; wait "$monitor_pid" || true
monitor_pid=
cat "$run/monitor.json"
jq -e '.eof == false and .max_gap_s < 2.5' "$run/monitor.json" >/dev/null || fail "our own listener stream stalled or ended: $(cat "$run/monitor.json")"
# Opus at a constant OPUS_RATE kbit/s (96 by default: 12000 B/s): no second after the first two
# may be short of half.
half=$((OPUS_RATE * 125 / 2))
jq -e --argjson half "$half" '[.bins[2:][] | select(. < $half)] | length == 0' "$run/monitor.json" >/dev/null || fail "a second of our own listener stream was short of $half B: $(cat "$run/monitor.json")"
ok "our own listener stream never stalled (longest wait $(jq .max_gap_s "$run/monitor.json") s, every second full) through the remote's death and return and the wrong password"
grep -E "clock.to-listeners.*tick took" <(podman logs "$prefix-listeners" 2>&1) | tail -3 || true
podman logs "$prefix-listeners" 2>&1 | grep -qF "$REMOTE_PASSWORD" && fail "the password is in the listener process's log"
ok "the password is in no log"
echo "icecast-out: all checks passed"
