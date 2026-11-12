#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The Compose bundle up on test ports with throwaway secrets: the control plane answers with
# its database, a room token is issued, the desk and playout run; the listener process's feed
# buffer behind the real desk, the Opus stream at a rate set (ADR-0233), both MP3 streams at
# their rates (ADR-0002, ADR-0232) and the player offering all three, the chat read-only between
# shows (CHAT_HOURS=shows, ADR-0234), and the default loop
# from playout and then from the listener process with playout stopped (ADR-0130), a listener
# on each mount held through playout's restart without a break or a new Ogg stream; then the
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
    -e 's/^COMPAT_HIGH_STREAM=.*/COMPAT_HIGH_STREAM=true/' \
    -e 's/^CHAT_HOURS=.*/CHAT_HOURS=shows/' \
    -e "s/^OPUS_BITRATE=.*/OPUS_BITRATE=${OPUS_RATE:=128}/" \
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
# The chat's hours (ADR-0234), set to shows above: with nothing scheduled the room is read-only,
# and the page is told so, with no time to give.
curl -fs "http://127.0.0.1:$CHAT_PORT/api/v1/chat/info" | python3 -I -c '
import json, sys
h = json.load(sys.stdin)["rules"]["hours"]
sys.exit(0 if h["shows_only"] and not h["open"] and h["opens_at_ms"] is None else f"the chat hours are {h}")'
echo "chat: read-only between shows (CHAT_HOURS=shows, nothing scheduled)"
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
# The MP3 streams (ADR-0002, ADR-0232), turned on above: MP3 from LAME at each rate set
# (128 and 320 kbit/s, 44.1 kHz), each on its own mount of the listener port.
# The mounts answer a while after the stack starts: about 110 s here with the demo carts being
# imported at the same time (2026-11-03), the Opus mount as late as the MP3 one. Try for 150 s.
# A frame header's third byte: the rate's index (128: 9, 320: 14), 44.1 kHz, padded or not.
mp3_ok() { # mount, kbit/s, rate index
  curl -s --max-time 4 "http://127.0.0.1:$LISTENERS_PORT/$1" -o "$run/compat.mp3" || true
  python3 -I -c '
import sys
b = open(sys.argv[1], "rb").read()
i = int(sys.argv[2])
# MPEG-1 Layer III, no CRC, at this rate and 44.1 kHz, padded or not
frames = b.count(bytes([0xff, 0xfb, i << 4])) + b.count(bytes([0xff, 0xfb, (i << 4) | 2]))
print(f"{frames} frames in {len(b)} bytes") if frames else None
sys.exit(0 if frames >= 50 else 1)' "$run/compat.mp3" "$3"
}
for spec in "programme.mp3 128 9" "programme-high.mp3 320 14"; do
  read -r mount kbps index <<<"$spec"
  for i in $(seq 1 30); do
    mp3_ok "$mount" "$kbps" "$index" && break
    sleep 1; [ "$i" = 30 ] && { echo "the MP3 stream sent no $kbps kbit/s MP3 on $mount: $(head -c 200 "$run/compat.mp3")" >&2; compose logs listeners > "$run/bundle-listeners.log" 2>&1; compose logs listeners | grep -E 'compat|mp3' | tail -20; rm -f "$run/compat.mp3"; exit 1; }
  done
  echo "MP3 stream: $kbps kbit/s on $mount"
done
rm -f "$run/compat.mp3"
# The Opus stream at the rate set (ADR-0233; OPUS_RATE, 128 here, not the default 96): constant
# bit rate, so 8 s of it after the burst icecast.server sends a new listener carries the rate's
# bytes, within 10 % (Ogg's pages add about 1 %).
curl -s --max-time 12 "http://127.0.0.1:$LISTENERS_PORT/programme.opus" -o "$run/rate.opus" -w '%{size_download}\n' >/dev/null || true
python3 -I -c '
import subprocess, sys
want = int(sys.argv[2])
out = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "packet=pts_time,size", "-of", "csv=p=0", sys.argv[1]], capture_output=True, text=True).stdout
pk = [(float(t), int(n)) for t, n in (l.split(",")[:2] for l in out.split() if l.count(",") >= 1)]
# The last 8 s of audio received: bytes over time, from the packets themselves.
end = pk[-1][0]
tail = [n for t, n in pk if t > end - 8]
kbps = sum(tail) * 8 / 8 / 1000
print(f"Opus stream: {kbps:.1f} kbit/s of audio in its last 8 s ({want} set)")
sys.exit(0 if abs(kbps - want) <= want * 0.1 else 1)' "$run/rate.opus" "$OPUS_RATE" \
  || { echo "the Opus stream is not at the $OPUS_RATE kbit/s set" >&2; rm -f "$run/rate.opus"; exit 1; }
rm -f "$run/rate.opus"
# The player offers the three, Opus first, each at its own mount beside the Opus one.
curl -fs "http://127.0.0.1:$CONTROL_PORT/api/v1/player" | python3 -I -c '
import json, sys
s = [(x["codec"], x["kbps"], x["url"].rsplit("/", 1)[1]) for x in json.load(sys.stdin)["streams"]]
want = [("opus", int(sys.argv[1]), "programme.opus"), ("mp3", 128, "programme.mp3"), ("mp3", 320, "programme-high.mp3")]
sys.exit(0 if s == want else f"the player offers {s}, not {want}")' "$OPUS_RATE"
echo "player: offers Opus $OPUS_RATE, MP3 128 and MP3 320"
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
# A listener on each mount, held through playout's stop and start: the listener process keeps
# every mount (M4), so none may end, stall or (Opus) start a new chained Ogg stream, which
# icecast.server would drop every listener for (the third fault).
held=()
for mount in programme.opus programme.mp3 programme-high.mp3; do
  curl -s -N --max-time 600 "http://127.0.0.1:$LISTENERS_PORT/$mount" -o "$run/held-$mount" &
  held+=("$!")
done
sleep 3
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
# Each held listener still connected, its file still growing, and the Opus one a single Ogg
# stream: one beginning-of-stream page.
for k in 0 1 2; do
  mount=$(echo programme.opus programme.mp3 programme-high.mp3 | cut -d' ' -f$((k + 1)))
  kill -0 "${held[$k]}" 2>/dev/null || { echo "the listener held on $mount was dropped while playout restarted" >&2; compose logs listeners | tail -20; exit 1; }
  before=$(stat -c %s "$run/held-$mount"); sleep 2; after=$(stat -c %s "$run/held-$mount")
  [ "$after" -gt "$before" ] || { echo "the listener held on $mount stopped receiving after playout's restart" >&2; exit 1; }
done
for p in "${held[@]}"; do kill "$p" 2>/dev/null || true; done
wait "${held[@]}" 2>/dev/null || true
python3 -I -c '
import sys
b = open(sys.argv[1], "rb").read()
# An Ogg page: "OggS", version 0, then its header type; 2 marks a beginning of stream.
bos = sum(1 for i in range(len(b) - 5) if b[i:i + 4] == b"OggS" and b[i + 5] & 2)
print(f"held Opus listener: {len(b)} bytes, {bos} Ogg stream(s)")
sys.exit(0 if bos == 1 else 1)' "$run/held-programme.opus" \
  || { echo "the held Opus listener got a chained Ogg stream across playout's restart" >&2; exit 1; }
echo "held listeners: all three mounts unbroken through playout's restart ($(for m in programme.opus programme.mp3 programme-high.mp3; do printf '%s %s B; ' "$m" "$(stat -c %s "$run/held-$m")"; done))"
rm -f "$run"/held-*
containers=$(compose ps -q | xargs podman inspect -f '{{.Name}}')
podman unshare python3 -I "$root/tests/ports/scan.py" --public "$LIVEKIT_MEDIA_PORT/udp" $containers | tail -4
