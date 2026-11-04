#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The Compose bundle up on test ports with throwaway secrets: the control plane answers with
# its database, a room token is issued, the desk and playout run; the listener process's feed
# buffer behind the real desk, the MP3 compatibility stream (ADR-0002), and the default loop
# from playout and then from the listener process with playout stopped (ADR-0130); then the
# port scan (only LiveKit's media port may be off loopback), and the bundle down again with
# its volumes.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/room/run"; mkdir -p "$run"
env="$run/bundle.env"
project=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-bundle
sed -e 's/^STATION_ID=.*/STATION_ID=3d0b7a52-1f7e-4a8b-9a51-0d2b6a4e9c11/' \
    -e 's/^LIVEKIT_API_SECRET=.*/LIVEKIT_API_SECRET=bundle-test-only-secret-not-for-use-0/' \
    -e 's/^LISTENERS_SOURCE_PASSWORD=.*/LISTENERS_SOURCE_PASSWORD=bundle-test-only/' \
    -e 's/^POSTGRES_PASSWORD=.*/POSTGRES_PASSWORD=bundle-test-only/' \
    -e 's/^CONTROL_ADMIN_TOKEN=.*/CONTROL_ADMIN_TOKEN=bundle-admin-test/' \
    -e 's/^CHAT_EMAIL_KEY=.*/CHAT_EMAIL_KEY=bundle-test-only-chat-email-key-0123456789/' \
    -e "s|^SERVER_IMAGE=.*|SERVER_IMAGE=${WEAVESIGNAL_SERVER_IMAGE:-localhost/weavesignal-server:m1}|" \
    -e 's/^COMPAT_STREAM=.*/COMPAT_STREAM=true/' \
    -e "s/^CONTROL_PORT=.*/CONTROL_PORT=${CONTROL_PORT:-17310}/" \
    -e "s/^CHAT_PORT=.*/CHAT_PORT=${CHAT_PORT:-17311}/" \
    -e "s/^LISTENERS_PORT=.*/LISTENERS_PORT=${LISTENERS_PORT:-17312}/" \
    -e "s/^PLAYOUT_FEED_PORT=.*/PLAYOUT_FEED_PORT=${FEED_PORT:-17315}/" \
    -e "s/^SRT_INGEST_PORT=.*/SRT_INGEST_PORT=${SRT_PORT:-17313}/" \
    -e "s/^LIVEKIT_HTTP_PORT=.*/LIVEKIT_HTTP_PORT=${LK_PORT:-17880}/" \
    -e "s/^LIVEKIT_MEDIA_PORT=.*/LIVEKIT_MEDIA_PORT=${LK_UDP_PORT:-17882}/" \
    "$root/deploy/compose/.env.example" > "$env"
. <(grep -E '^[A-Z_]+_PORT=' "$env")
compose() { (cd "$root/deploy/compose" && podman compose --env-file "$env" -p "$project" "$@"); }
down() { compose down -v >/dev/null 2>&1 || true; rm -f "$env"; }
trap down EXIT
compose up -d >/dev/null 2>&1
for i in $(seq 1 60); do
  curl -fs "http://127.0.0.1:$CONTROL_PORT/health" 2>/dev/null | grep -q '"database":"ok"' && break
  sleep 1; [ "$i" = 60 ] && { echo "the control plane did not come up with its database" >&2; compose logs control | tail -20; exit 1; }
done
echo "control plane: $(curl -fs "http://127.0.0.1:$CONTROL_PORT/health")"
curl -fs -X POST -H "Authorization: Bearer bundle-admin-test" -H "Content-Type: application/json" \
  -d '{"identity": "hamish", "role": "presenter"}' "http://127.0.0.1:$CONTROL_PORT/api/v1/room/tokens" | grep -q '"token"'
echo "room token issued"
# The public pages (M3), from the image: the player with its CSP, an immutable asset, and
# call-in's challenge.
page=$(curl -fsi "http://127.0.0.1:$CONTROL_PORT/")
grep -qi "^content-security-policy: default-src 'none'; script-src 'self';" <<<"$page" \
  && grep -qi '^cache-control: no-cache' <<<"$page" && grep -q '<div id="app">' <<<"$page" \
  || { echo "the control plane does not serve the player with its headers" >&2; exit 1; }
asset=$(grep -o '/assets/player-[^"]*\.js' <<<"$page" | head -1)
curl -fsI "http://127.0.0.1:$CONTROL_PORT$asset" | grep -qi 'max-age=31536000, immutable' \
  || { echo "the player's script is not served immutable" >&2; exit 1; }
curl -fs "http://127.0.0.1:$CONTROL_PORT/api/v1/call-in/challenge" | grep -q '"bits":18' \
  || { echo "call-in's challenge is not served" >&2; exit 1; }
echo "public pages: the player, its assets and call-in's challenge"
# The chat (M6): up on its database, following the control plane, its legal facts served.
for i in $(seq 1 60); do
  curl -fs "http://127.0.0.1:$CHAT_PORT/api/v1/chat/info" 2>/dev/null | grep -q '"reasons"' && break
  sleep 1; [ "$i" = 60 ] && { echo "the chat did not come up" >&2; compose logs chat | tail -20; exit 1; }
done
echo "chat: answers on $CHAT_PORT"
# The cart wall: the control plane reaches the desk, which plays the carts (ADR-0117), and the
# demo pack (in the image) starts loading through the media worker.
for i in $(seq 1 60); do
  carts=$(curl -fs -H "Authorization: Bearer bundle-admin-test" "http://127.0.0.1:$CONTROL_PORT/api/v1/studio" \
    | python3 -I -c 'import json,sys; c=json.load(sys.stdin)["carts"]; print("ok" if c["desk"] and c["version"] > 0 else "")' 2>/dev/null || true)
  [ "$carts" = ok ] && break
  sleep 1; [ "$i" = 60 ] && { echo "the cart wall did not reach the desk, or loaded no carts" >&2; compose logs control desk | tail -20; exit 1; }
done
echo "cart wall: the desk answers, carts loading: $(curl -fs -H "Authorization: Bearer bundle-admin-test" "http://127.0.0.1:$CONTROL_PORT/api/v1/carts" | python3 -I -c 'import json,sys; print(len(json.load(sys.stdin)["carts"]), "carts so far")')"
# A cart fired over HTTP sounds at the desk: the desk reads the media worker's file from the
# shared volume.
# The demo pack imports one cart at a time (about 70 s for all 50): wait for the first.
for i in $(seq 1 120); do
  cart=$(curl -fs -H "Authorization: Bearer bundle-admin-test" "http://127.0.0.1:$CONTROL_PORT/api/v1/carts" \
    | python3 -I -c 'import json,sys; print(next((c["id"] for c in json.load(sys.stdin)["carts"] if not c["missing"]), ""))' 2>/dev/null || true)
  [ -n "$cart" ] && break
  sleep 1; [ "$i" = 120 ] && { echo "no demo cart was imported" >&2; compose logs control | tail -20; exit 1; }
done
curl -fs -X POST -H "Authorization: Bearer bundle-admin-test" -H "Content-Type: application/json" \
  -d "{\"op\": \"fire\", \"cart\": \"$cart\"}" "http://127.0.0.1:$CONTROL_PORT/api/v1/carts/commands" \
  | python3 -I -c 'import json,sys; s=json.load(sys.stdin)["carts"]["sounding"]; assert s, "nothing sounding"' \
  || { echo "a cart fired over HTTP did not sound at the desk" >&2; compose logs control desk | tail -20; exit 1; }
echo "cart wall: a cart fired at the desk"
# The listener process's feed buffer, behind the real desk: what it holds stays as listener
# delay for as long as playout is connected. FFmpeg probing the desk's Matroska added about
# 3 s to it at every playout connection until playout named the container (2026-11-03).
# One command on the listener process's socket, from inside its volume (the volume's full path
# is too long for a Unix socket's address); prints the answer's first line.
listeners_cmd() {
  podman unshare python3 -I -c '
import os, socket, sys
os.chdir(sys.argv[1])
s = socket.socket(socket.AF_UNIX); s.settimeout(5); s.connect("listeners.sock")
s.sendall(sys.argv[2].encode() + b"\n"); out = b""
while b"END" not in out:
    chunk = s.recv(256)
    if not chunk: break
    out += chunk
print(out.decode().split("\r\n")[0])' \
    "$(podman volume inspect "${project}_listeners-control" --format '{{.Mountpoint}}')" "$1"
}
# The highest of five readings a second apart: a backlog lasts, a low reading can be a moment.
buffer=$(for i in 1 2 3 4 5; do listeners_cmd feed.buffer_length; sleep 1; done | sort -g | tail -1)
# Under 0.2 s, playout is not feeding it at all.
python3 -I -c 'import sys; b = float(sys.argv[1]); sys.exit(0 if 0.2 < b < 1.5 else 1)' "$buffer" \
  || { echo "the listener process holds $buffer s of playout's feed (0.5 s set): playout fed it a backlog, or nothing" >&2; compose logs playout | tail -20; exit 1; }
echo "feed buffer: $buffer s (0.5 s set)"
# The compatibility stream (ADR-0002), turned on above: MP3 from LAME at the bitrate set
# (128 kbit/s, 44.1 kHz), on its own mount of the listener port.
# The mounts answer a while after the stack starts: about 110 s here with the demo carts being
# imported at the same time (2026-11-03), the Opus mount as late as the MP3 one. Try for 150 s.
mp3_ok() {
  curl -s --max-time 4 "http://127.0.0.1:$LISTENERS_PORT/programme.mp3" -o "$run/compat.mp3" || true
  python3 -I -c '
import sys
b = open(sys.argv[1], "rb").read()
# MPEG-1 Layer III, no CRC, 128 kbit/s at 44.1 kHz, padded or not
frames = b.count(b"\xff\xfb\x90") + b.count(b"\xff\xfb\x92")
print(f"{frames} frames in {len(b)} bytes") if frames else None
sys.exit(0 if frames >= 50 else 1)' "$run/compat.mp3"
}
for i in $(seq 1 30); do
  mp3_ok && break
  sleep 1; [ "$i" = 30 ] && { echo "the MP3 compatibility stream sent no 128 kbit/s MP3: $(head -c 200 "$run/compat.mp3")" >&2; compose logs listeners > "$run/bundle-listeners.log" 2>&1; compose logs listeners | grep -E 'compat|mp3' | tail -20; rm -f "$run/compat.mp3"; exit 1; }
done
rm -f "$run/compat.mp3"
echo "compatibility stream: MP3 at 128 kbit/s on programme.mp3"
# The default loop (ADR-0130). The bundle's fallback folder is empty, so once the desk falls
# quiet playout plays the loop WeaveSignal ships, and the control plane says so; with playout
# away the listener process plays it itself, and the control plane still says so.
playout_status() {
  curl -fs -H "Authorization: Bearer bundle-admin-test" "http://127.0.0.1:$CONTROL_PORT/api/v1/playout" \
    | python3 -I -c 'import json,sys; s=json.load(sys.stdin); p=s["playout"]; print((p["on_air"] if p else "away") + " " + str(s["default_fallback"]).lower())'
}
for i in $(seq 1 90); do
  [ "$(playout_status 2>/dev/null)" = "default true" ] && break
  sleep 1; [ "$i" = 90 ] && { echo "playout did not play the default loop, or the control plane did not say so: $(playout_status)" >&2; compose logs playout control | tail -20; exit 1; }
done
echo "default loop: playout plays it, the studio is told"
compose stop playout >/dev/null 2>&1
for i in $(seq 1 60); do
  [ "$(playout_status 2>/dev/null)" = "away true" ] \
    && [ "$(listeners_cmd fallback.status 2>/dev/null)" = '{"on_air":"default","default":true}' ] && break
  sleep 1; [ "$i" = 60 ] && { echo "with playout away the listener process did not play the default loop, or the control plane did not say so: $(playout_status), $(listeners_cmd fallback.status)" >&2; compose logs listeners control | tail -20; exit 1; }
done
echo "default loop: the listener process plays it with playout away, the studio is told"
compose start playout >/dev/null 2>&1
for i in $(seq 1 60); do
  [ "$(listeners_cmd fallback.status 2>/dev/null)" = '{"on_air":"feed","default":false}' ] \
    && [ "$(playout_status 2>/dev/null)" = "default true" ] && break
  # A playout container restarted on Podman can find its socket volume root-owned (AGENTS.md's traps).
  sleep 1; [ "$i" = 60 ] && { echo "playout did not come back to the listener process: $(playout_status), $(listeners_cmd fallback.status)" >&2; compose logs playout | tail -20; exit 1; }
done
echo "default loop: playout back on the feed"
sleep 5
containers=$(compose ps -q | xargs podman inspect -f '{{.Name}}')
podman unshare python3 -I "$root/tests/ports/scan.py" --public "$LIVEKIT_MEDIA_PORT/udp" $containers | tail -4
