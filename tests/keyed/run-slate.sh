#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# What the public hears while a keyed show is on (1.3.2, 19f): the slate must be real sound,
# whatever the station has. Found on the test station: a station with no slate file and an empty
# fallback folder (every Cloudron install makes the folder) gave the public digital silence
# (-91 dB) for the whole keyed show, because the slate chose the fallback folder for existing and
# never fell through to the default loop.
#
# The real playout/weavesignal.liq and playout/listeners.liq in Liquidsoap 2.5.0-rc1 and a
# stand-in desk (1 kHz over SRT), no control plane: this script sends playout's commands. One
# listener process for the whole run; playout is started again for each station:
#   loop    no slate file, an empty fallback folder, the default loop (the shipped file): default_loop
#   junk    no slate file, a fallback folder holding only a file that does not play: default_loop
#   files   no slate file, a fallback file (440 Hz) and the default loop: fallback
#   slate   a slate file (2500 Hz), a fallback file and the default loop: slate_file
# For each: the desk live, the public mount followed for 48 s, the window on for 16 s in the
# middle. It fails if playout names the wrong slate, if the public heard the slate quieter than
# -60 dBFS or for more than 0.5 s below it, if the desk's tone was in it, or if the public heard
# a quiet run longer than 1 s at the window's start or end. The click figure at each change is
# printed, not judged (the gate cuts between the programme and the slate within one sample).
#
# NEGATIVE=1 runs one station that has nothing to play as a slate (no slate file, an empty
# fallback folder, no default loop: silence, by design); the run must then fail its level check
# (it exits 0 only if it does), so the test is shown to hear silence.
#
# Ports below 32768 (26612 listeners and 26613, 26615 feed, 26614/udp desk). Run it inside the
# stack lock:
#   flock ~/Code/WeaveSignal/tools/1.1.1/stack.lock tests/keyed/run-slate.sh
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
run="$here/run-slate"
LISTENERS_PORT=${LISTENERS_PORT:-26612}
FEED_PORT=${FEED_PORT:-26615}
SRT_PORT=${SRT_PORT:-26614}
LIQ_IMAGE=${LIQ_IMAGE:-docker.io/savonet/liquidsoap:v2.5.0-rc1}
NEGATIVE=${NEGATIVE:-0}
STATIONS=${STATIONS:-loop junk files slate}
[ "$NEGATIVE" = 1 ] && STATIONS=none
SOURCE_PASSWORD=keyed-slate-test-only
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-keyedslate
base="http://127.0.0.1:$LISTENERS_PORT"

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$LISTENERS_PORT" "$((LISTENERS_PORT + 1))" "$FEED_PORT"; do
  taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }
done
taken -u "$SRT_PORT" && { echo "port $SRT_PORT/udp is in use; set another" >&2; exit 1; }

rm -rf "$run"; mkdir -p "$run/sock" "$run/cap" "$run/empty"
cp "$root/assets/default-loop/default-loop.flac" "$run/default-loop.flac"
ffmpeg -v error -f lavfi -i "sine=frequency=2500:duration=7:sample_rate=48000" -ac 2 "$run/slate.flac"
for s in loop junk files slate none; do mkdir -p "$run/$s/fallback"; done
echo "not a sound" > "$run/junk/fallback/notes.txt"
for s in files slate; do
  ffmpeg -v error -f lavfi -i "sine=frequency=440:duration=20:sample_rate=48000" -ac 2 "$run/$s/fallback/fallback-440.flac"
done
data() { # slate_or_empty default_file
  local slate=""
  [ -n "$1" ] && slate="\"slate\": \"$1\", "
  cat <<JSON
{
  "station_id": "00000000-0000-0000-0000-000000000000",
  "srt": { "host": "127.0.0.1", "port": $SRT_PORT, "format": "mpegts" },
  "listeners": { "bind": "127.0.0.1", "host": "127.0.0.1", "port": $LISTENERS_PORT, "mount": "programme.opus", "feed_port": $FEED_PORT, "feed_buffer_s": 0.5,
                 "keyed": { "enabled": true, "mount": "programme-keyed.opus", "compat_mount": "programme-keyed.mp3" },
                 "control_socket": "/sock/listeners.sock" },
  "control_socket": "/sock/playout.sock",
  "library": { "dir": "/media/library" },
  "carts": { "dir": "/media/library" },
  "fallback": { "dir": "/media/fallback", "default_file": "$2" },
  "processing": { "enabled": true },
  "programme": { "mode": "auto", "dead_air_s": 4.0, "dead_air_db": -50.0 },
  "keyed": { ${slate}"enabled": true, "window": false, "lead_s": 0.2, "hold_s": 0.2 }
}
JSON
}
for s in loop junk files; do data "" /r/default-loop.flac > "$run/$s/playout.json"; done
data /r/slate.flac /r/default-loop.flac > "$run/slate/playout.json"
data "" /r/no-such-loop.flac > "$run/none/playout.json"
cat > "$run/desk.liq" <<LIQ
output.srt(mode="listener", bind_address="127.0.0.1", port=$SRT_PORT,
  %ffmpeg(format="mpegts", %audio(codec="mp2", b="256k", ac=2, ar=48000)), amplify(0.3, sine(1000.)))
LIQ

caps=()
stop_playout() {
  podman logs "$prefix-playout" > "$run/playout-$1.log" 2>&1 || true
  podman rm -f "$prefix-playout" >/dev/null 2>&1 || true
}
cleanup() {
  for p in "${caps[@]}"; do kill "$p" 2>/dev/null || true; done
  stop_playout last
  for c in listeners desk; do
    podman logs "$prefix-$c" > "$run/$c.log" 2>&1 || true
    podman rm -f "$prefix-$c" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT
cleanup

liq() { # station: the podman arguments (in L) for a Liquidsoap container with its folders
  L=(--network host --security-opt label=disable --user 0:0 -v "$root/playout:/p:ro" -v "$run:/r:ro"
     -v "$run/sock:/sock" -v "$run/empty:/media/library:ro" -v "$run/$1/fallback:/media/fallback:ro"
     -e "WEAVESIGNAL_PLAYOUT_DATA=/r/$1/playout.json" -e "WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD=$SOURCE_PASSWORD"
     --entrypoint /usr/bin/liquidsoap "$LIQ_IMAGE")
}
liq loop
podman run -d --name "$prefix-listeners" "${L[@]}" /p/listeners.liq >/dev/null
podman run -d --name "$prefix-desk" "${L[@]}" /r/desk.liq >/dev/null

say() { printf '%s\nquit\n' "$2" | socat -t 3 - "UNIX-CONNECT:$run/sock/$1.sock" | sed -n '1p'; }
fail=0
check() { if eval "$2"; then echo "ok    $1"; else echo "FAIL  $1"; fail=1; fi; }
py() { python3 -I -c "import json,sys; d=json.load(open(sys.argv[1])); print(eval(sys.argv[2]))" "$1" "$2"; }

declare -A want=([loop]=default_loop [junk]=default_loop [files]=fallback [slate]=slate_file [none]=silence)
for s in $STATIONS; do
  rm -f "$run/sock/playout.sock"
  liq "$s"
  podman run -d --name "$prefix-playout" "${L[@]}" /p/weavesignal.liq >/dev/null
  for i in $(seq 1 1200); do
    if [ -S "$run/sock/playout.sock" ] && [ -S "$run/sock/listeners.sock" ]; then
      code=$(curl -s -o /dev/null --max-time 1 -w '%{http_code}' "$base/programme.opus" || true)
      [ "$code" = 200 ] && say playout programme.status | grep -q '"desk_connected":true' && break
    fi
    sleep 0.5
    [ "$i" = 1200 ] && { echo "station $s: the stack did not come up ($(say playout programme.status 2>&1))" >&2; exit 1; }
  done
  say playout "programme.mode live" >/dev/null
  # The desk on the public mount before the capture starts (the mount runs about 13 s behind).
  sleep 15
  cap="$run/cap/$s.opus"
  curl -s -N --max-time 48 -o "$cap" "$base/programme.opus" || true &
  caps=($!)
  sleep 4
  say playout "keyed.window on" >/dev/null
  sleep 1
  status=$(say playout keyed.status)
  sleep 15
  say playout "keyed.window off" >/dev/null
  wait "${caps[@]}" 2>/dev/null || true
  caps=()
  ffmpeg -v error -y -i "$cap" -ac 1 -ar 48000 -f s16le "$cap.s16"
  python3 -I "$here/level.py" "$cap.s16" 48000 > "$cap.json"
  echo "station $s: keyed.status $status"
  echo "station $s: $(python3 -I -c "import json,sys; d=json.load(open(sys.argv[1])); print({k: d.get(k) for k in ('shut','shut_s','shut_db','shut_quiet_s','shut_1khz','longest_quiet_s','edges','click_median')})" "$cap.json")"
  kind=$(printf '%s' "$status" | python3 -I -c "import json,sys; print(json.load(sys.stdin).get('slate'))" 2>/dev/null || echo "?")
  if [ "$NEGATIVE" = 1 ]; then
    if py "$cap.json" 'd["shut"] is not None and d["shut_db"] > -60 and d["shut_quiet_s"] <= 0.5' | grep -q True; then
      echo "FAIL  NEGATIVE=1: a station with nothing to play as a slate was not caught (the test is deaf)"; exit 1
    fi
    echo "NEGATIVE=1 control passed: the test hears the silent slate"; exit 0
  fi
  check "$s: playout names the slate ${want[$s]} ($kind)" '[ "$kind" = "${want[$s]}" ]'
  check "$s: the window is found on the public mount (shut for $(py "$cap.json" 'd.get("shut_s")') s of 16)" \
    "py '$cap.json' 'd[\"shut\"] is not None and 13 < d[\"shut_s\"] < 19' | grep -q True"
  check "$s: the public heard the slate as sound ($(py "$cap.json" 'd.get("shut_db")') dBFS, over -60)" \
    "py '$cap.json' 'd.get(\"shut_db\", -120) > -60' | grep -q True"
  check "$s: never quiet in it for long ($(py "$cap.json" 'd.get("shut_quiet_s")') s below -60 dBFS)" \
    "py '$cap.json' 'd.get(\"shut_quiet_s\", 99) <= 0.5' | grep -q True"
  check "$s: the desk's 1 kHz is not in the slate ($(py "$cap.json" 'd.get("shut_1khz")') windows)" \
    "py '$cap.json' 'd.get(\"shut_1khz\") == 0' | grep -q True"
  check "$s: no gap over 1 s at the window's start or end ($(py "$cap.json" '[e["quiet_s"] for e in d.get("edges", [])]') s)" \
    "py '$cap.json' 'all(e[\"quiet_s\"] <= 1.0 for e in d[\"edges\"])' | grep -q True"
  stop_playout "$s"
done
exit "$fail"
