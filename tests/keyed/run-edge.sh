#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Keyed shows, the edge half of the spike (PLAN 19a): the HLS edge image (deploy/images/edge,
# with keyed.js) in front of a stand-in origin (nginx serving a keyed and a public HLS show,
# AV1 video and AAC sound, with the same file names), and a stand-in listener mount (an endless
# body). Tokens are made by crates/keyed (bin keyed-token), so the Rust signer and the edge's
# njs check are tried against each other.
# It fails if any keyed file is served without a valid token (none, wrong show, wrong media,
# expired, another station's signature, revoked, a path that climbs out of its show), for GET,
# HEAD or a range, after the cache holds it; if a keyed file reaches a public path or a public
# file a keyed one; if a keyed answer may be kept by a shared cache; if the cache does not
# serve one copy to every holder; or if a stream outlives its token or its key's revocation.
# It also plays the keyed show through the edge with ffmpeg (the playlist's relative addresses
# must carry the token to every segment) and measures what the check costs on a cache hit.
#
# Ports 26501 (origin), 26502 (edge), 26503 (the mount). Run it inside the stack lock:
#   flock ~/Code/WeaveSignal/tools/1.1.1/stack.lock tests/keyed/run-edge.sh
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
run="$here/run-edge"
prefix=${WEAVESIGNAL_TEST_PREFIX:-p132-keyed-spike}-keyed-edge
image=${EDGE_IMAGE:-localhost/weavesignal-edge:p132-keyed-spike}
ORIGIN_PORT=${ORIGIN_PORT:-26501} EDGE_PORT=${EDGE_PORT:-26502} MOUNT_PORT=${MOUNT_PORT:-26503}
edge="http://127.0.0.1:$EDGE_PORT"

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$ORIGIN_PORT" "$EDGE_PORT" "$MOUNT_PORT"; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done

rm -rf "$run"; mkdir -p "$run/www/keyed/v0" "$run/www/keyed/audio" "$run/www/public/v0" "$run/www/public/audio" "$run/www/keyed-list" "$run/log"
mountpid=''
cleanup() {
  podman logs "$prefix-edge" > "$run/edge.log" 2>&1 || true
  podman rm -f "$prefix-edge" "$prefix-origin" >/dev/null 2>&1 || true
  [ -z "$mountpid" ] || kill "$mountpid" 2>/dev/null || true
}
trap cleanup EXIT
cleanup

( cd "$root" && CARGO_BUILD_JOBS=4 nice -n 19 cargo build -q -p weavesignal-keyed --bin keyed-token )
tok() { "$root/target/debug/keyed-token" "$@"; }
seed="$run/station.pk8"; other="$run/other-station.pk8"
tok new "$seed"; tok new "$other"
[ "${SKIP_BUILD:-0}" = 1 ] || nice -n 19 podman build -q --target hls -t "$image" "$root/deploy/images/edge" >/dev/null

# Two shows with the same file names: keyed (red, 1 kHz) and public (blue, 440 Hz), 12 s each in
# 2 s fMP4 segments, AV1 video with AAC sound, and an audio-only rendition.
hls() { # dir colour freq
  ( cd "$run/www/$1"
    ffmpeg -v error -f lavfi -i "color=$2:s=320x180:r=25" -f lavfi -i "sine=frequency=$3:sample_rate=48000" -t 12 \
      -c:v libsvtav1 -preset 12 -g 50 -pix_fmt yuv420p -c:a aac -b:a 96k -f hls -hls_time 2 -hls_playlist_type vod \
      -hls_segment_type fmp4 -hls_fmp4_init_filename init.mp4 -hls_segment_filename 'v0/seg%05d.m4s' v0/index.m3u8
    ffmpeg -v error -f lavfi -i "sine=frequency=$3:sample_rate=48000" -t 12 -c:a aac -b:a 96k -f hls -hls_time 2 \
      -hls_playlist_type vod -hls_segment_type fmp4 -hls_fmp4_init_filename init.mp4 \
      -hls_segment_filename 'audio/seg%05d.m4s' audio/index.m3u8
    printf '#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=600000,CODECS="av01.0.04M.08,mp4a.40.2",RESOLUTION=320x180\nv0/index.m3u8\n' > live.m3u8
    printf '#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=100000,CODECS="mp4a.40.2"\naudio/index.m3u8\n' > audio.m3u8 )
}
hls keyed red 1000
hls public blue 440
echo '{"revoked": []}' > "$run/www/keyed-list/revoked.json"
chmod -R a+rX "$run/www"

# The stand-in origin answers a keyed path from the keyed show whatever the token (the control
# plane checks it there too; that is 19b's), and logs every request it gets.
cat > "$run/origin.conf" <<CONF
events {}
http {
  types { application/vnd.apple.mpegurl m3u8; video/mp4 mp4 m4s; application/json json; }
  log_format uri '\$request_method \$request_uri';
  server {
    listen 127.0.0.1:$ORIGIN_PORT;
    access_log /log/origin.log uri;
    location ~ ^/hls/k/show-a/[^/]+/(.+)\$ { alias /www/keyed/\$1; }
    location /hls/ { alias /www/public/; }
    location /keyed/ { alias /www/keyed-list/; }
  }
}
CONF
podman run -d --name "$prefix-origin" --network host --security-opt label=disable \
  -v "$run/origin.conf:/etc/nginx/nginx.conf:ro" -v "$run/www:/www:ro" -v "$run/log:/log" \
  docker.io/library/nginx:1.27-alpine >/dev/null
python3 -I "$here/stream.py" "$MOUNT_PORT" & mountpid=$!
pub=$(tok public "$seed")
podman run -d --name "$prefix-edge" --network host --security-opt label=disable \
  -e EDGE_ORIGIN="http://127.0.0.1:$ORIGIN_PORT" -e EDGE_HTTP_PORT="$EDGE_PORT" -e EDGE_BIND=127.0.0.1 \
  -e EDGE_RELAY="127.0.0.1:$MOUNT_PORT" -e EDGE_KEYED_PUBLIC_KEY="$pub" -e EDGE_KEYED_SKEW=1 \
  -e EDGE_KEYED_REVOKED_URL="http://127.0.0.1:$ORIGIN_PORT/keyed/revoked.json" -e EDGE_KEYED_REVOKED_EVERY=1s \
  "$image" >/dev/null
for i in $(seq 1 40); do curl -s -o /dev/null "$edge/hls/live.m3u8" && break; sleep 0.25; done

now=$(date +%s)
good=$(tok mint "$seed" k-ishbel show-a av $((now - 10)) $((now + 600)))
good2=$(tok mint "$seed" k-ruairidh show-a av $((now - 10)) $((now + 600)))
sound=$(tok mint "$seed" k-morag show-a a $((now - 10)) $((now + 600)))
expired=$(tok mint "$seed" k-ishbel show-a av $((now - 600)) $((now - 60)))
early=$(tok mint "$seed" k-ishbel show-a av $((now + 600)) $((now + 900)))
wrong=$(tok mint "$seed" k-ishbel show-b av $((now - 10)) $((now + 600)))
forged=$(tok mint "$other" k-ishbel show-a av $((now - 10)) $((now + 600)))
later=$(tok mint "$seed" k-later show-a av $((now - 10)) $((now + 600)))
echo "a token is ${#good} characters"

fail=0
check() { if eval "$2"; then echo "ok    $1"; else echo "FAIL  $1"; fail=1; fi; }
code() { curl -s -o /dev/null -w '%{http_code}' "$@"; }
hdr() { curl -s -D - -o /dev/null "$2" | tr -d '\r' | sed -n "s/^$1: //Ip" | head -1; }
k() { echo "$edge/hls/k/show-a/$1/$2"; }

check "no token: 403" '[ "$(code "$edge/hls/k/show-a/nothing/live.m3u8")" = 403 ]'
check "a valid token: the playlist (MISS)" '[ "$(hdr x-cache "$(k "$good" v0/index.m3u8)")" = MISS ]'
check "a valid token: a segment (MISS, 200)" '[ "$(hdr x-cache "$(k "$good" v0/seg00001.m4s)")" = MISS ]'
check "keyed answers are private" '[ "$(hdr cache-control "$(k "$good" v0/seg00001.m4s)")" = "private, max-age=60" ] && [ "$(code "$(k "$good" v0/seg00001.m4s)")" = 200 ]'
check "another holder's token: the same segment from the cache (HIT)" '[ "$(hdr x-cache "$(k "$good2" v0/seg00001.m4s)")" = HIT ]'
for t in expired early wrong forged; do
  check "$t token, the cached segment: 403 (GET)" '[ "$(code "$(k "${!t}" v0/seg00001.m4s)")" = 403 ]'
  check "$t token, the cached segment: 403 (HEAD)" '[ "$(code -I "$(k "${!t}" v0/seg00001.m4s)")" = 403 ]'
  check "$t token, the cached segment: 403 (range)" '[ "$(code -r 0-99 "$(k "${!t}" v0/seg00001.m4s)")" = 403 ]'
done
check "a garbled signature: 403" '[ "$(code "$(k "${good%?????}AAAAA" v0/seg00001.m4s)")" = 403 ]'
check "a sound-only token opens the audio rendition" '[ "$(code "$(k "$sound" audio/seg00001.m4s)")" = 200 ] && [ "$(code "$(k "$sound" audio.m3u8)")" = 200 ]'
check "a sound-only token does not open video" '[ "$(code "$(k "$sound" v0/seg00001.m4s)")" = 403 ] && [ "$(code "$(k "$sound" live.m3u8)")" = 403 ]'
check "a path that climbs out of its show: 403" '[ "$(curl -s --path-as-is -o /dev/null -w "%{http_code}" "$edge/hls/k/show-b/$good/../../show-a/$good/v0/seg00001.m4s")" = 403 ]'
check "a path that climbs into another show: 403" '[ "$(curl -s --path-as-is -o /dev/null -w "%{http_code}" "$edge/hls/k/show-a/$good/../../show-b/$good/v0/seg00001.m4s")" = 403 ]'
check "an escaped path: 403" '[ "$(code "$edge/hls/k/show-a/$good/v0%2fseg00001.m4s")" = 403 ]'
keyed_md5=$(curl -s "$(k "$good" v0/seg00001.m4s)" | md5sum | cut -c1-32)
public_md5=$(curl -s "$edge/hls/v0/seg00001.m4s" | md5sum | cut -c1-32)
check "the public path serves the public file, not the cached keyed one" '[ "$public_md5" = "$(md5sum < "$run/www/public/v0/seg00001.m4s" | cut -c1-32)" ] && [ "$public_md5" != "$keyed_md5" ]'
check "the keyed path serves the keyed file" '[ "$keyed_md5" = "$(md5sum < "$run/www/keyed/v0/seg00001.m4s" | cut -c1-32)" ]'
public2=$(curl -s "$(k "$good" v0/seg00002.m4s)" >/dev/null; curl -s "$edge/hls/v0/seg00002.m4s" | md5sum | cut -c1-32)
check "a keyed segment cached first does not answer the public path" '[ "$public2" = "$(md5sum < "$run/www/public/v0/seg00002.m4s" | cut -c1-32)" ]'

# ffmpeg as a player: the multivariant playlist, its media playlist and every segment by
# relative address, all through the token in the path.
ffmpeg -v error -y -i "$(k "$good" live.m3u8)" -map 0 -c copy -f matroska "$run/played.mkv"
streams=$(ffprobe -v error -show_entries stream=codec_name -of csv=p=0 "$run/played.mkv" | sort | tr '\n' ' ')
check "ffmpeg played the keyed show through the edge (streams: $streams)" '[ "$streams" = "aac av1 " ]'
ffmpeg -v error -y -i "$run/played.mkv" -map 0:a -ac 1 -ar 16000 -f s16le "$run/played.s16"
check "and heard the keyed sound (1 kHz)" "python3 -I '$here/tones.py' '$run/played.s16' 16000 440,1000 | grep -q '\"1000\": 1[0-9]\\.'"
bad=$(grep ' /hls/k/' "$run/log/origin.log" | grep -vc " /hls/k/show-a/ks1\." || true)
check "every keyed request that reached the origin carried a token ($(grep -c ' /hls/k/' "$run/log/origin.log") requests)" '[ "$bad" = 0 ]'

# Revocation: refused within the list's interval (1 s here), and a stream with it is ended.
check "a key not yet revoked: 200" '[ "$(code "$(k "$later" v0/seg00001.m4s)")" = 200 ]'
curl -s -N --max-time 20 -o "$run/revoked-stream.bin" -w '%{time_total}' "$edge/listen/k/show-a/$later/programme-keyed.opus" > "$run/revoked-stream.t" &
revstream=$!
sleep 2
echo '{"revoked": ["k-later"]}' > "$run/www/keyed-list/revoked.json"
sleep 2.5
check "the revoked key: 403" '[ "$(code "$(k "$later" v0/seg00001.m4s)")" = 403 ]'
wait "$revstream" || true
check "its stream was ended after the revoke ($(cat "$run/revoked-stream.t") s)" "python3 -c 'import sys; sys.exit(0 if float(open(\"$run/revoked-stream.t\").read()) < 8 else 1)'"

# Streams: refused without a token, ended when the token ends (4 s and the 1 s skew), kept with one.
check "a stream without a token: 403" '[ "$(code --max-time 3 "$edge/listen/k/show-a/nothing/programme-keyed.opus")" = 403 ]'
check "a stream with a video-only token: 403" '[ "$(code --max-time 3 "$edge/listen/k/show-a/$(tok mint "$seed" k-ishbel show-a v $((now - 10)) $((now + 600)))/programme-keyed.opus")" = 403 ]'
short=$(tok mint "$seed" k-ishbel show-a a $(($(date +%s) - 10)) $(($(date +%s) + 4)))
t_short=$(curl -s -N --max-time 20 -o "$run/short.bin" -w '%{time_total}' "$edge/listen/k/show-a/$short/programme-keyed.opus" || true)
t_good=$(curl -s -N --max-time 8 -o "$run/good.bin" -w '%{time_total}' "$edge/listen/k/show-a/$good/programme-keyed.opus" || true)
check "a stream is ended when its token ends ($t_short s, $(stat -c %s "$run/short.bin") bytes)" "python3 -c 'import sys; sys.exit(0 if 2 < float(\"$t_short\") < 9 else 1)'"
check "a stream with a valid token keeps going ($t_good s, $(stat -c %s "$run/good.bin") bytes)" "python3 -c 'import sys; sys.exit(0 if float(\"$t_good\") >= 7.9 and $(stat -c %s "$run/good.bin") > 100000 else 1)'"

# What the check costs: a cache hit on the keyed path against one on the public path.
curl -s -o /dev/null "$(k "$good" v0/seg00003.m4s)"; curl -s -o /dev/null "$edge/hls/v0/seg00003.m4s"
kb=$(python3 -I "$here/bench.py" 127.0.0.1 "$EDGE_PORT" "/hls/k/show-a/$good/v0/seg00003.m4s" 2000)
pb=$(python3 -I "$here/bench.py" 127.0.0.1 "$EDGE_PORT" "/hls/v0/seg00003.m4s" 2000)
echo "cost: keyed $kb"
echo "cost: public $pb"
check "the keyed benchmark was all 200s" "grep -q '\"codes\": {\"200\": 2000}' <<<'$kb'"
exit "$fail"
