#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Keyed shows end to end (1.3.2, PLAN 19b): the real playout/weavesignal.liq (its processing
# chain on) and playout/listeners.liq in Liquidsoap 2.5.0-rc1, a stand-in desk (660 Hz, or
# 1000 Hz while run/desk-keyed exists: the keyed show), a throwaway PostgreSQL 18 and the control
# plane (a release build), which crates/control/tests/keyed_stack.rs drives as staff and
# listeners would: a show set to `key`, a one-minute episode, a key redeemed, a public listener
# and a key holder at once, the key revoked halfway. Nothing here sends playout a command: the
# control plane's driver does, from the schedule. The public listener must hear no 1000 Hz.
#
# Then, with the edge image (EDGE_IMAGE, built by `just edge-image`), the token the control plane
# made is tried against the edge's njs check: accepted on its show's path, refused on another
# show's, refused garbled.
#
# With PLAYWRIGHT_CORE set (a playwright-core directory), the player is tried in a real browser too
# (tests/keyed/player.mjs): the public pages are built and served by the control plane, a second
# key's link is opened as the window opens, the page plays the keyed stream, and when that key is
# revoked the page must say so in words. Without it, that check is skipped and said to be.
#
# NEGATIVE=1 plays 1000 Hz outside any keyed window too; the run must then find it on the public
# mount (it fails if it does not), so the test is shown to see what it is there to catch.
#
# Ports below 32768: 26410 control, 26412 listeners (and 26413), 26415 feed, 26414/udp desk,
# 26433 PostgreSQL, 26490 the edge. Run it inside the stack lock:
#   flock ~/Code/WeaveSignal/tools/1.1.1/stack.lock tests/keyed/run-stack.sh
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
run="$here/run-stack"
CONTROL_PORT=${CONTROL_PORT:-26410}
LISTENERS_PORT=${LISTENERS_PORT:-26412}
FEED_PORT=${FEED_PORT:-26415}
SRT_PORT=${SRT_PORT:-26414}
PG_PORT=${PG_PORT:-26433}
EDGE_PORT=${EDGE_PORT:-26490}
LIQ_IMAGE=${LIQ_IMAGE:-docker.io/savonet/liquidsoap:v2.5.0-rc1}
PG_IMAGE=${PG_IMAGE:-docker.io/library/postgres:18}
EDGE_IMAGE=${EDGE_IMAGE:-}
NEGATIVE=${NEGATIVE:-0}
SOURCE_PASSWORD=keyed-stack-test-only
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-keyedstack

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$CONTROL_PORT" "$LISTENERS_PORT" "$((LISTENERS_PORT + 1))" "$FEED_PORT" "$PG_PORT" "$EDGE_PORT"; do
  taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }
done
taken -u "$SRT_PORT" && { echo "port $SRT_PORT/udp is in use; set another" >&2; exit 1; }

rm -rf "$run"; mkdir -p "$run/library" "$run/fallback" "$run/sock" "$run/recordings/logger"
# The station's fallback files are the slate here (no slate file is set): 440 Hz.
ffmpeg -v error -f lavfi -i "sine=frequency=440:duration=20:sample_rate=48000" -ac 2 "$run/fallback/fallback-440.flac"
cat > "$run/playout.json" <<JSON
{
  "station_id": "5b1e7c2a-3d4f-4a6b-8c9d-0e1f2a3b4c5d",
  "srt": { "host": "127.0.0.1", "port": $SRT_PORT, "format": "mpegts" },
  "listeners": { "bind": "127.0.0.1", "host": "127.0.0.1", "port": $LISTENERS_PORT, "mount": "programme.opus", "feed_port": $FEED_PORT, "feed_buffer_s": 0.5,
                 "keyed": { "enabled": true, "mount": "programme-keyed.opus", "compat_mount": "programme-keyed.mp3" },
                 "control_socket": "/sock/listeners.sock" },
  "control_socket": "/sock/playout.sock",
  "library": { "dir": "/media/library" },
  "carts": { "dir": "/media/library" },
  "fallback": { "dir": "/media/fallback" },
  "logger": { "dir": "/media/recordings/logger", "bitrate": 48 },
  "processing": { "enabled": true },
  "programme": { "mode": "live", "dead_air_s": 4.0, "dead_air_db": -50.0 },
  "keyed": { "enabled": true, "window_file": "/sock/keyed-window", "lead_s": 0.2, "hold_s": 0.2 }
}
JSON
# The stand-in desk, the SRT listener as the desk is: 660 Hz, or 1000 Hz (the keyed show) while
# run/desk-keyed exists. Both at the same level, so the processing chain treats them alike.
cat > "$run/desk.liq" <<LIQ
keyed = {file.exists("/r/desk-keyed")}
s = add(normalize=false, [amplify({if keyed() then 0. else 0.3 end}, sine(660.)),
                          amplify({if keyed() then 0.3 else 0. end}, sine(1000.))])
output.srt(mode="listener", bind_address="127.0.0.1", port=$SRT_PORT,
  %ffmpeg(format="mpegts", %audio(codec="mp2", b="256k", ac=2, ar=48000)), s)
LIQ

cleanup() {
  for c in playout listeners desk pg edge; do
    podman logs "$prefix-$c" > "$run/$c.log" 2>&1 || true
    podman rm -f "$prefix-$c" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT
cleanup

nice -n 19 cargo build --release -q -p weavesignal-control
browser_env=()
if [ -n "${PLAYWRIGHT_CORE:-}" ] && [ -d "$PLAYWRIGHT_CORE" ] && [ "$NEGATIVE" = 0 ]; then
  (cd "$root/apps/web" && nice -n 19 pnpm install --frozen-lockfile >/dev/null && nice -n 19 pnpm build >/dev/null)
  browser_env=(WEAVESIGNAL_KEYED_BROWSER="$here/player.mjs" WEAVESIGNAL_KEYED_WEB_DIR="$root/apps/web/dist" PLAYWRIGHT_CORE="$PLAYWRIGHT_CORE")
else
  echo "PLAYWRIGHT_CORE not set: the player in a browser is not tried"
fi
podman run -d --name "$prefix-pg" -p "127.0.0.1:$PG_PORT:5432" -e POSTGRES_PASSWORD=test-only \
  -e POSTGRES_DB=weavesignal "$PG_IMAGE" >/dev/null
liq=(--network host --security-opt label=disable --user 0:0 -v "$root/playout:/p:ro" -v "$run:/r:ro"
     -v "$run/sock:/sock" -v "$run/library:/media/library:ro" -v "$run/fallback:/media/fallback:ro"
     -v "$run/recordings:/media/recordings"
     -e WEAVESIGNAL_PLAYOUT_DATA=/r/playout.json -e "WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD=$SOURCE_PASSWORD"
     --entrypoint /usr/bin/liquidsoap "$LIQ_IMAGE")
podman run -d --name "$prefix-listeners" "${liq[@]}" /p/listeners.liq >/dev/null
podman run -d --name "$prefix-desk" "${liq[@]}" /r/desk.liq >/dev/null
podman run -d --name "$prefix-playout" "${liq[@]}" /p/weavesignal.liq >/dev/null

for i in $(seq 1 240); do
  podman exec "$prefix-pg" pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 \
    && [ -S "$run/sock/playout.sock" ] && [ -S "$run/sock/listeners.sock" ] && break
  sleep 0.5
  [ "$i" = 240 ] && { echo "the stack did not come up" >&2; exit 1; }
done
echo "stack up: listener process :$LISTENERS_PORT (feed :$FEED_PORT), PostgreSQL :$PG_PORT, desk SRT :$SRT_PORT"

status=0
env "${browser_env[@]}" \
WEAVESIGNAL_KEYED_STACK=1 \
WEAVESIGNAL_KEYED_NEGATIVE="$NEGATIVE" \
WEAVESIGNAL_CONTROL_BIN="$root/target/release/weavesignal-control" \
WEAVESIGNAL_TEST_DATABASE_URL="postgres://postgres:test-only@127.0.0.1:$PG_PORT/weavesignal" \
WEAVESIGNAL_KEYED_RUN="$run" \
WEAVESIGNAL_KEYED_CONTROL_PORT="$CONTROL_PORT" \
WEAVESIGNAL_KEYED_LISTENERS_PORT="$LISTENERS_PORT" \
  nice -n 19 cargo test -p weavesignal-control --test keyed_stack -- --ignored --nocapture --test-threads=1 || status=$?
printf 'keyed.status (playout): '; printf 'keyed.status\nquit\n' | socat -t 3 - "UNIX-CONNECT:$run/sock/playout.sock" 2>/dev/null | head -1 || true
grep -o '"message":"keyed shows[^"]*"[^}]*' "$run/control.log" | head -20 || true
[ "$status" = 0 ] || exit "$status"
[ "$NEGATIVE" = 0 ] || exit 0

# ---- The edge's njs against the control plane's real token ----
if [ -z "$EDGE_IMAGE" ]; then
  echo "EDGE_IMAGE not set: the edge's check of the real token was not tried"
  exit 0
fi
token=$(cat "$run/token.txt"); show=$(cat "$run/show.txt"); public_key=$(cat "$run/public_key.txt")
# The address the redeem handed the sound key (`urls.hls`, /hls/k/<show>/<token>/audio.m3u8).
hls=$(cat "$run/hls.txt")
# The origin is gone (the control plane stopped with the test): a token the edge accepts goes on
# to it and fails there (502), one it refuses never leaves the edge (403). The token is still
# within its time: the test gives tokens a long HLS lag (keys.hls_lag_s = 120).
podman run -d --name "$prefix-edge" --network host \
  -e EDGE_ORIGIN="http://127.0.0.1:$CONTROL_PORT" -e EDGE_HTTP_PORT="$EDGE_PORT" -e EDGE_BIND=127.0.0.1 \
  -e EDGE_KEYED_PUBLIC_KEY="$public_key" -e EDGE_KEYED_SKEW=5 \
  --tmpfs /var/cache/weavesignal-edge "$EDGE_IMAGE" >/dev/null
for i in $(seq 1 50); do curl -s -o /dev/null "http://127.0.0.1:$EDGE_PORT/hls/k/x/y/z" && break; sleep 0.2; done
code() { curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$EDGE_PORT$1"; }
fail=0
check() { if [ "$2" = "$3" ]; then echo "ok    $1 ($2)"; else echo "FAIL  $1: $2, wanted $3"; fail=1; fi; }
check "the redeem's own HLS address for the sound key passes the edge's njs check" "$(code "$hls")" 502
check "the same token for the picture (a sound key)" "$(code "${hls%/audio.m3u8}/live.m3u8")" 403
check "another show's path" "$(code "/hls/k/$(cat /proc/sys/kernel/random/uuid)/$token/audio.m3u8")" 403
# Change a character ten from the end, never the last: the last character of a 64-byte signature
# in base64 has two unused bits, so replacing it can decode to the same bytes and the "garbled"
# token is then the real one (a flake of about one run in sixteen, seen in r132f, 2026-11-24).
garbled="${token:0:${#token}-10}A${token: -9}"; [ "$garbled" = "$token" ] && garbled="${token:0:${#token}-10}B${token: -9}"
check "a garbled signature" "$(code "/hls/k/$show/$garbled/audio.m3u8")" 403
exit "$fail"
