#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Keyed shows, the sound half of the spike (PLAN 19a): playout's gate and the listener process's
# keyed mounts, with the real playout/weavesignal.liq and playout/listeners.liq in Liquidsoap
# 2.5.0-rc1, a stand-in desk (a 1 kHz tone over SRT) and no control plane: this script sends
# the control plane's commands itself. The tones:
#   1000 Hz  keyed content: the desk, and automation items pushed with `keyed`
#    660 Hz  public automation items
#    440 Hz  the station's fallback files (public)
#   2500 Hz  the slate
# One connection is held on each mount for the whole run (public Opus and MP3, keyed Opus and
# MP3), and a late listener joins the public Opus mount in the middle of the keyed live show.
# The timeline: automation alternating public and keyed items (the gate follows each keyed item,
# its look-ahead covers the start); then a keyed live show (window on, then the desk); then the
# keyed mounts taken down (their listeners must be dropped, the public ones kept).
# It fails if any public capture holds a sample of 1 kHz tone, if the keyed mount lost the keyed
# content (the shared-frame trap), if a public stream restarted (a second Opus header), stalled,
# or went silent for more than 60 ms, or if the keyed listeners were not dropped.
#
# NEGATIVE=1 runs the same with one fault on purpose: the desk goes live 100 ms before the
# window opens, as a control plane that sent its commands in the wrong order would. NEGATIVE=2
# takes the gate's look-ahead and hold away (lead_s 0, hold_s 0: one frame), which lets the
# first and last few milliseconds of each keyed item through. Either run must then find the
# leak (it fails if it does not), so the test is shown to see what it is there to catch.
#
# Ports below 32768 (26812 listeners, 26815 feed, 26913/udp desk); set LISTENERS_PORT and the
# others to move them. Run it inside the stack lock:
#   flock ~/Code/WeaveSignal/tools/1.1.1/stack.lock tests/keyed/run-switch.sh
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
run="$here/run"
LISTENERS_PORT=${LISTENERS_PORT:-26812}
FEED_PORT=${FEED_PORT:-26815}
SRT_PORT=${SRT_PORT:-26913}
LIQ_IMAGE=${LIQ_IMAGE:-docker.io/savonet/liquidsoap:v2.5.0-rc1}
NEGATIVE=${NEGATIVE:-0}
SOURCE_PASSWORD=keyed-test-only
prefix=${WEAVESIGNAL_TEST_PREFIX:-p132-keyed-spike}-keyed
base="http://127.0.0.1:$LISTENERS_PORT"

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$LISTENERS_PORT" "$FEED_PORT"; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done
taken -u "$SRT_PORT" && { echo "port $SRT_PORT/udp is in use; set another" >&2; exit 1; }

lead=0.2 hold=0.2
[ "$NEGATIVE" = 2 ] && lead=0 hold=0
rm -rf "$run"; mkdir -p "$run/library" "$run/fallback" "$run/sock" "$run/cap"
pub_id=11111111-1111-4111-8111-111111111111
key_id=22222222-2222-4222-8222-222222222222
# Odd lengths, so item boundaries fall inside a frame, not on one.
ffmpeg -v error -f lavfi -i "sine=frequency=660:duration=3.013:sample_rate=48000" -ac 2 "$run/library/$pub_id.flac"
ffmpeg -v error -f lavfi -i "sine=frequency=1000:duration=3.007:sample_rate=48000" -ac 2 "$run/library/$key_id.flac"
ffmpeg -v error -f lavfi -i "sine=frequency=440:duration=20:sample_rate=48000" -ac 2 "$run/fallback/fallback-440.flac"
ffmpeg -v error -f lavfi -i "sine=frequency=2500:duration=7:sample_rate=48000" -ac 2 "$run/slate.flac"

cat > "$run/playout.json" <<JSON
{
  "station_id": "00000000-0000-0000-0000-000000000000",
  "srt": { "host": "127.0.0.1", "port": $SRT_PORT, "format": "mpegts" },
  "listeners": { "bind": "127.0.0.1", "host": "127.0.0.1", "port": $LISTENERS_PORT, "mount": "programme.opus", "feed_port": $FEED_PORT, "feed_buffer_s": 0.5,
                 "compat": { "enabled": true, "mount": "programme.mp3", "bitrate": 128 },
                 "keyed": { "enabled": true, "mount": "programme-keyed.opus", "compat_mount": "programme-keyed.mp3" },
                 "control_socket": "/sock/listeners.sock" },
  "control_socket": "/sock/playout.sock",
  "library": { "dir": "/media/library" },
  "carts": { "dir": "/media/library" },
  "fallback": { "dir": "/media/fallback" },
  "processing": { "enabled": false },
  "programme": { "mode": "auto", "dead_air_s": 4.0, "dead_air_db": -50.0 },
  "keyed": { "enabled": true, "window": false, "slate": "/r/slate.flac", "lead_s": $lead, "hold_s": $hold }
}
JSON
cat > "$run/desk.liq" <<LIQ
output.srt(mode="listener", bind_address="127.0.0.1", port=$SRT_PORT,
  %ffmpeg(format="mpegts", %audio(codec="mp2", b="256k", ac=2, ar=48000)), sine(1000.))
LIQ

caps=()
cleanup() {
  for p in "${caps[@]}"; do kill "$p" 2>/dev/null || true; done
  for c in playout listeners desk; do
    podman logs "$prefix-$c" > "$run/$c.log" 2>&1 || true
    podman rm -f "$prefix-$c" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT
cleanup

liq=(--network host --security-opt label=disable --user 0:0 -v "$root/playout:/p:ro" -v "$run:/r:ro"
     -v "$run/sock:/sock" -v "$run/library:/media/library:ro" -v "$run/fallback:/media/fallback:ro"
     -e WEAVESIGNAL_PLAYOUT_DATA=/r/playout.json -e "WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD=$SOURCE_PASSWORD"
     --entrypoint /usr/bin/liquidsoap "$LIQ_IMAGE")
podman run -d --name "$prefix-listeners" "${liq[@]}" /p/listeners.liq >/dev/null
podman run -d --name "$prefix-desk" "${liq[@]}" /r/desk.liq >/dev/null
podman run -d --name "$prefix-playout" "${liq[@]}" /p/weavesignal.liq >/dev/null

say() { printf '%s\nquit\n' "$2" | socat -t 3 - "UNIX-CONNECT:$run/sock/$1.sock" | sed -n '1p'; }
now() { date +%s.%N; }
log() { printf '%8.3f  %s\n' "$(echo "$(now) - $t0" | bc)" "$*" | tee -a "$run/timeline.txt"; }

# Up: both sockets, the desk connected, and every mount answering (14 to 60 s after a start;
# Liquidsoap's type check of playout's script alone took minutes on a loaded host once).
for i in $(seq 1 1200); do
  if [ -S "$run/sock/playout.sock" ] && [ -S "$run/sock/listeners.sock" ]; then
    ok=1; st=""
    for m in programme.opus programme.mp3 programme-keyed.opus programme-keyed.mp3; do
      code=$(curl -s -o /dev/null --max-time 1 -w '%{http_code}' "$base/$m" || true)
      st="$st $m:$code"
      [ "$code" = 200 ] || ok=0
    done
    [ "$ok" = 1 ] && say playout programme.status | grep -q '"desk_connected":true' && break
  fi
  sleep 0.5
  [ "$i" = 1200 ] && { echo "the stack did not come up: $st / $(say playout programme.status)" >&2; exit 1; }
done
echo "stack up (listeners :$LISTENERS_PORT, feed :$FEED_PORT, desk :$SRT_PORT/udp)"

t0=$(now)
: > "$run/timeline.txt"
span=80
for m in programme.opus programme.mp3 programme-keyed.opus programme-keyed.mp3; do
  curl -s -N --max-time "$span" -o "$run/cap/$m" -w '%{http_code} %{time_total}\n' "$base/$m" > "$run/cap/$m.curl" 2>&1 &
  caps+=($!)
done
sleep 2.5
log "automation: public, keyed, public, keyed, public"
n=0
for kind in pub key pub key pub; do
  n=$((n + 1))
  if [ "$kind" = key ]; then say playout "autodj.push $n track $key_id $key_id.flac 0 keyed" >/dev/null
  else say playout "autodj.push $n track $pub_id $pub_id.flac 0" >/dev/null; fi
done
sleep 16.5
if [ "$NEGATIVE" = 1 ]; then
  log "NEGATIVE: desk live first, window 100 ms later"
  say playout "programme.mode live" >/dev/null; sleep 0.1; say playout "keyed.window on" >/dev/null
else
  log "keyed live show: window on, then the desk"
  say playout "keyed.window on" >/dev/null; sleep 0.3; say playout "programme.mode live" >/dev/null
fi
sleep 2
log "a late listener joins the public mount (its first seconds are the mount's burst: the keyed items)"
curl -s -N --max-time 6 -o "$run/cap/late.opus" "$base/programme.opus" || true
sleep 1
log "the keyed live show ends: automation, then the window closes"
say playout "programme.mode auto" >/dev/null; sleep 0.5; say playout "keyed.window off" >/dev/null
# The mounts run about 13 s behind (icecast.server's 12 s burst and the feed's cushion): wait
# until the live show has reached the keyed listeners before dropping them.
sleep 22
log "keyed mounts taken down (the show's end), then up again"
say listeners "keyed.stop" >/dev/null
sleep 3
say listeners "keyed.start" >/dev/null
log "waiting for the captures to end"
wait "${caps[@]}" 2>/dev/null || true
caps=()
say playout "events.since 0" > "$run/events.json" || true

# ---- Analysis ----
fail=0
check() { if eval "$2"; then echo "ok    $1"; else echo "FAIL  $1"; fail=1; fi; }
an() { ffmpeg -v error -y -i "$1" -ac 1 -ar 16000 -f s16le "$1.s16" && python3 -I "$here/tones.py" "$1.s16" 16000 440,660,1000,2500; }
for m in programme.opus programme.mp3 programme-keyed.opus programme-keyed.mp3 late.opus; do
  an "$run/cap/$m" > "$run/cap/$m.json"
done
j() { python3 -I -c "import json,sys; d=json.load(open(sys.argv[1])); print(eval(sys.argv[2]))" "$run/cap/$1.json" "$2"; }
for m in programme.opus programme.mp3 late.opus; do
  echo "$m: 1 kHz windows $(j "$m" 'd["hits"]["1000"]'), first at $(j "$m" 'd["first"]["1000"]'); slate $(j "$m" 'd["tone_s"]["2500"]') s; length $(j "$m" 'd["seconds"]') s; longest gap $(j "$m" 'd["longest_gap_s"]') s"
done
for m in programme-keyed.opus programme-keyed.mp3; do
  echo "$m: 1 kHz $(j "$m" 'd["tone_s"]["1000"]') s, 660 Hz $(j "$m" 'd["tone_s"]["660"]') s, slate $(j "$m" 'd["tone_s"]["2500"]') s; length $(j "$m" 'd["seconds"]') s"
done
leak=$(( $(j programme.opus 'd["hits"]["1000"]') + $(j programme.mp3 'd["hits"]["1000"]') + $(j late.opus 'd["hits"]["1000"]') ))
if [ "$NEGATIVE" != 0 ]; then
  check "the fault (NEGATIVE=$NEGATIVE) is caught: 1 kHz on the public mounts ($leak windows of 5 ms)" '[ "$leak" -gt 0 ]'
  [ "$fail" = 0 ] && echo "NEGATIVE=$NEGATIVE control passed: the test sees the leak" && exit 0
  exit 1
fi
check "no 1 kHz on the public Opus, the public MP3 or the late listener" '[ "$leak" = 0 ]'
check "the public mounts carried the slate during the keyed show" "python3 -c 'import sys; sys.exit(0 if $(j programme.opus 'd["tone_s"]["2500"]') > 8 else 1)'"
check "the late listener heard the slate" "python3 -c 'import sys; sys.exit(0 if $(j late.opus 'd["tone_s"]["2500"]') > 2 else 1)'"
check "the keyed Opus mount carried the keyed items and the live show (1 kHz, over 13 s)" "python3 -c 'import sys; sys.exit(0 if $(j programme-keyed.opus 'd["tone_s"]["1000"]') > 13 else 1)'"
check "the keyed MP3 mount too" "python3 -c 'import sys; sys.exit(0 if $(j programme-keyed.mp3 'd["tone_s"]["1000"]') > 13 else 1)'"
heads=$(grep -c -a OpusHead "$run/cap/programme.opus" || true)
check "one Opus stream on the public mount, never restarted (OpusHead x$heads)" '[ "$heads" = 1 ]'
check "the public Opus kept time with the clock (decoded $(j programme.opus 'd["seconds"]') s of $span)" "python3 -c 'import sys; sys.exit(0 if abs($(j programme.opus 'd["seconds"]') - $span) < 2.5 else 1)'"
check "no gap over 60 ms on the public Opus" "python3 -c 'import sys; sys.exit(0 if $(j programme.opus 'd["longest_gap_s"]') <= 0.06 else 1)'"
check "keyed listeners were dropped when the keyed mounts went down (Opus after $(cut -d' ' -f2 "$run/cap/programme-keyed.opus.curl") s, MP3 after $(cut -d' ' -f2 "$run/cap/programme-keyed.mp3.curl") s)" "python3 -c 'import sys; sys.exit(0 if max(float(\"$(cut -d' ' -f2 "$run/cap/programme-keyed.opus.curl")\"), float(\"$(cut -d' ' -f2 "$run/cap/programme-keyed.mp3.curl")\")) < $span - 5 else 1)'"
check "the public listeners were kept (Opus $(cut -d' ' -f2 "$run/cap/programme.opus.curl") s)" "python3 -c 'import sys; sys.exit(0 if float(\"$(cut -d' ' -f2 "$run/cap/programme.opus.curl")\") > $span - 1 else 1)'"
grep -o '"kind":"keyed"[^}]*' "$run/events.json" | head -20 || true
exit "$fail"
