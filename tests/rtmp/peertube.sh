#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# A throwaway PeerTube (the official image, with its PostgreSQL and Redis) in one pod on
# loopback, for RTMP out's stack test (ADR-0230; `rtmp-out`):
#   peertube.sh up     start it and wait for its API
#   peertube.sh test   up, then for live transcoding on and then off: two permanent lives, the
#                      desk sends `compatible` to one and `open` to the other, and what PeerTube
#                      made of each is read from its API and its HLS (tests/rtmp/run/peertube-*)
#   peertube.sh down   remove it
# Every port is on 127.0.0.1: PT_HTTP (24900) for its web and API, PT_RTMP (24937) for RTMP.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/rtmp/run"
PT_HTTP=${PT_HTTP:-24900}
PT_RTMP=${PT_RTMP:-24937}
PT_IMAGE=${PT_IMAGE:-docker.io/chocobozzz/peertube:v8.3.1}
PG_IMAGE=${PG_IMAGE:-docker.io/library/postgres:17-alpine}
REDIS_IMAGE=${REDIS_IMAGE:-docker.io/library/redis:8-alpine}
DESK_IMAGE=${DESK_IMAGE:-${WEAVESIGNAL_DESK_IMAGE:-localhost/weavesignal-desk-dev:m5}}
pod=${PREFIX:-${WEAVESIGNAL_TEST_PREFIX:-weavesignal-rtmp-out}}-peertube
api="http://127.0.0.1:$PT_HTTP/api/v1"
password=rtmp-test-only-root-password
mkdir -p "$run"

down() { podman pod rm -f "$pod" >/dev/null 2>&1 || true; }

up() {
  down
  podman pod create --name "$pod" -p "127.0.0.1:$PT_HTTP:9000" -p "127.0.0.1:$PT_RTMP:1935" >/dev/null
  podman run -d --pod "$pod" --name "$pod-pg" -e POSTGRES_USER=peertube -e POSTGRES_PASSWORD=test-only \
    -e POSTGRES_DB=peertube "$PG_IMAGE" >/dev/null
  podman run -d --pod "$pod" --name "$pod-redis" "$REDIS_IMAGE" >/dev/null
  for i in $(seq 1 60); do podman exec "$pod-pg" pg_isready -U peertube >/dev/null 2>&1 && break; sleep 0.5; done
  podman run -d --pod "$pod" --name "$pod-app" \
    -e PEERTUBE_DB_USERNAME=peertube -e PEERTUBE_DB_PASSWORD=test-only -e PEERTUBE_DB_HOSTNAME=127.0.0.1 \
    -e PEERTUBE_DB_SSL=false -e PEERTUBE_REDIS_HOSTNAME=127.0.0.1 \
    -e PEERTUBE_WEBSERVER_HOSTNAME=127.0.0.1 -e PEERTUBE_WEBSERVER_PORT="$PT_HTTP" -e PEERTUBE_WEBSERVER_HTTPS=false \
    -e PEERTUBE_TRUST_PROXY='["127.0.0.1", "loopback"]' -e PEERTUBE_SECRET=rtmp-test-only-secret-0123456789abcdef \
    -e PEERTUBE_ADMIN_EMAIL=admin@example.invalid -e PT_INITIAL_ROOT_PASSWORD="$password" \
    -e PEERTUBE_SMTP_HOSTNAME=127.0.0.1 -e PEERTUBE_SMTP_PORT=25 -e PEERTUBE_SMTP_FROM=noreply@example.invalid \
    "$PT_IMAGE" >/dev/null
  for i in $(seq 1 240); do curl -fs "$api/config" >/dev/null 2>&1 && return 0; sleep 1; done
  podman logs --tail 40 "$pod-app" >&2
  echo "PeerTube never answered" >&2
  return 1
}

token() {
  local c
  c=$(curl -fs "$api/oauth-clients/local")
  curl -fs "$api/users/token" \
    --data-urlencode "client_id=$(jq -r .client_id <<<"$c")" --data-urlencode "client_secret=$(jq -r .client_secret <<<"$c")" \
    --data-urlencode grant_type=password --data-urlencode username=root --data-urlencode "password=$password" | jq -r .access_token
}

# Live on, transcoding on (one rendition, 480p, the original kept) or off.
configure() {
  local transcode=$1 t=$2 cfg
  cfg=$(curl -fs -H "Authorization: Bearer $t" "$api/config/custom")
  cfg=$(jq --argjson on "$transcode" '
    .live.enabled = true | .live.allowReplay = false | .live.maxDuration = -1 | .live.maxUserLives = 10
    | .live.transcoding.enabled = $on
    | .live.transcoding.resolutions = (.live.transcoding.resolutions | with_entries(.value = false))
    | .live.transcoding.resolutions["480p"] = $on
    | .live.transcoding.alwaysTranscodeOriginalResolution = $on
    | .live.transcoding.threads = 2' <<<"$cfg")
  curl -fs -X PUT -H "Authorization: Bearer $t" -H 'Content-Type: application/json' -d "$cfg" "$api/config/custom" >/dev/null
}

new_live() {
  local name=$1 t=$2 channel id
  channel=$(curl -fs "$api/accounts/root/video-channels" | jq -r '.data[0].id')
  id=$(curl -fs -X POST -H "Authorization: Bearer $t" -H 'Content-Type: application/json' \
    -d "{\"channelId\": $channel, \"name\": \"$name\", \"privacy\": 1, \"permanentLive\": true, \"saveReplay\": false}" \
    "$api/videos/live" | jq -r .video.uuid)
  echo "$id"
}

# What PeerTube made of a live: its state, and each HLS rendition's codecs as ffprobe reads them.
inspect() {
  local id=$1 out=$2 v master
  v=$(curl -fs "$api/videos/$id")
  {
    echo "state: $(jq -c '.state' <<<"$v") isLive=$(jq -r .isLive <<<"$v")"
    master=$(jq -r '.streamingPlaylists[0].playlistUrl // empty' <<<"$v")
    if [ -n "$master" ]; then
      echo "master: $master"
      curl -fs "$master" | sed 's/^/  /' || true
      # Each variant, and each audio rendition named in an EXT-X-MEDIA line (with transcoding
      # on, PeerTube puts the sound in a rendition of its own).
      for p in $(curl -fs "$master" | { grep -v '^#' || true; } | grep .) $(curl -fs "$master" | grep -o 'URI="[^"]*"' | cut -d'"' -f2); do
        echo "rendition $p:"
        ffprobe -v error -show_entries stream=codec_type,codec_name,profile,width,height,sample_rate -of compact \
          "${master%/*}/$p" 2>&1 | sed 's/^/  /' || true
      done
    else
      echo "no HLS playlist: PeerTube made nothing to watch"
    fi
  } > "$out"
  cat "$out"
}

test_peertube() {
  up
  local t
  t=$(token)
  for transcode in true false; do
    echo "-- PeerTube, live transcoding $transcode"
    configure "$transcode" "$t"
    local a b ka kb
    a=$(new_live "compatible-$transcode" "$t")
    b=$(new_live "open-$transcode" "$t")
    ka=$(curl -fs -H "Authorization: Bearer $t" "$api/videos/live/$a" | jq -r .streamKey)
    kb=$(curl -fs -H "Authorization: Bearer $t" "$api/videos/live/$b" | jq -r .streamKey)
    local since
    since=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    podman run --rm --network host --security-opt label=disable -v "$root:/work" \
      -v "$HOME/.cargo/registry:/root/.cargo/registry" -w /work -e CARGO_TARGET_DIR=/work/target/container \
      -e RUST_LOG=warn -e WEAVESIGNAL_RTMP_RUN=/work/tests/rtmp/run -e WEAVESIGNAL_RTMP_SECONDS=45 \
      -e WEAVESIGNAL_RTMP_TARGETS="pt-compatible=compatible=rtmp://127.0.0.1:$PT_RTMP/live=$ka
pt-open=open=rtmp://127.0.0.1:$PT_RTMP/live=$kb" \
      "$DESK_IMAGE" cargo test -p weavesignal-desk --test rtmp_stack -- --ignored --nocapture send_to_the_named_servers \
      > "$run/peertube-$transcode-desk.log" 2>&1 &
    local desk=$!
    sleep 40
    inspect "$a" "$run/peertube-$transcode-compatible.txt"
    inspect "$b" "$run/peertube-$transcode-open.txt"
    wait "$desk" || { tail -30 "$run/peertube-$transcode-desk.log" >&2; return 1; }
    grep -E '^pt-' "$run/peertube-$transcode-desk.log" || true
    podman logs --since "$since" "$pod-app" > "$run/peertube-$transcode-app.log" 2>&1 || true
    grep -i -E 'probing|original resolution|publish and federate|transmuxing|error' "$run/peertube-$transcode-app.log" \
      | grep -v -i -E 'GET /|SMTP|stack' | tail -12 | cut -c1-200
    # compatible must be watchable, as H.264 and AAC. What open gets is recorded, not required
    # (PeerTube 8.3.1 found no picture in it, 2026-11-06).
    grep -q 'codec_name=h264' "$run/peertube-$transcode-compatible.txt" \
      && grep -q '"Published"' "$run/peertube-$transcode-compatible.txt" \
      || { echo "PeerTube (transcoding $transcode) did not publish compatible as H.264" >&2; return 1; }
    grep -q 'codec_name=aac' "$run/peertube-$transcode-compatible.txt" \
      || { echo "PeerTube (transcoding $transcode) has no AAC from compatible" >&2; return 1; }
  done
  echo "peertube: compatible published with transcoding on and off; open: see run/peertube-*-open.txt"
}

case "${1:-test}" in
  up) up ;;
  down) down ;;
  test) trap down EXIT; test_peertube ;;
  *) echo "up, test or down" >&2; exit 2 ;;
esac
