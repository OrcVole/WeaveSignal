#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# RTMP out against real servers on loopback (ADR-0230; `rtmp-out`), never a real platform:
#   MediaMTX 1.21.3   both profiles read back over RTSP, a refused key, the server going away,
#                     a dump (crates/desk/tests/rtmp_stack.rs)
#   OvenMediaEngine   v0.21.0 with Enhanced RTMP on: both profiles; what it made of each is
#                     read from its log
#   PeerTube          (PEERTUBE=1, the default; PEERTUBE=0 skips it) live on, transcoding on
#                     and then off: both profiles; what it accepted is read from its API
# then the desk's ignored stack tests in the desk image. Every port is 24xxx on loopback.
#
#   MTX_RTMP=24935 MTX_RTSP=24554 OME_RTMP=24936 PT_HTTP=24900 PT_RTMP=24937 tests/rtmp/run.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/rtmp/run"
MTX_RTMP=${MTX_RTMP:-24935}
MTX_RTSP=${MTX_RTSP:-24554}
OME_RTMP=${OME_RTMP:-24936}
PT_HTTP=${PT_HTTP:-24900}
PT_RTMP=${PT_RTMP:-24937}
PEERTUBE=${PEERTUBE:-1}
MTX_IMAGE=${MTX_IMAGE:-docker.io/bluenviron/mediamtx:1.21.3}
OME_IMAGE=${OME_IMAGE:-docker.io/airensoft/ovenmediaengine:v0.21.0}
DESK_IMAGE=${DESK_IMAGE:-${WEAVESIGNAL_DESK_IMAGE:-localhost/weavesignal-desk-dev:m5}}
prefix=${PREFIX:-${WEAVESIGNAL_TEST_PREFIX:-weavesignal-rtmp-out}}

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$MTX_RTMP" "$MTX_RTSP" "$OME_RTMP"; do taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }; done

mkdir -p "$run"
rm -f "$run"/restart-mediamtx "$run"/mediamtx-restarted "$run"/servers-status.json

# MediaMTX: RTMP and RTSP on loopback, every other listener off. Only the paths below may be
# published to, so any other key is refused.
cat > "$run/mediamtx.yml" <<YAML
logLevel: info
api: no
metrics: no
pprof: no
playback: no
webrtc: no
srt: no
hls: no
moq: no
rtsp: yes
rtspTransports: [tcp]
rtspEncryption: "no"
rtspAddress: 127.0.0.1:$MTX_RTSP
rtmp: yes
rtmpEncryption: "no"
rtmpAddress: 127.0.0.1:$MTX_RTMP
paths:
  live/compat:
  live/open:
  live/compat2:
  live/dump:
YAML

# OvenMediaEngine: its own default settings, bound to loopback, with Enhanced RTMP on.
podman run --rm --entrypoint cat "$OME_IMAGE" /opt/ovenmediaengine/bin/origin_conf/Server.xml > "$run/Server.default.xml"
sed -e 's#<IP>\*</IP>#<IP>127.0.0.1</IP>#' \
    -e 's#<StunServer>[^<]*</StunServer>##' \
    -e 's#</Modules>#<ERTMP><Enable>true</Enable></ERTMP></Modules>#' \
    "$run/Server.default.xml" > "$run/Server.xml"
grep -q '<ERTMP><Enable>true' "$run/Server.xml"

cleanup() {
  [ -n "${restarter:-}" ] && kill "$restarter" 2>/dev/null
  for c in mediamtx ome; do
    podman logs "$prefix-$c" > "$run/$c.log" 2>&1 || true
    podman rm -f "$prefix-$c" >/dev/null 2>&1 || true
  done
  [ "$PEERTUBE" = 1 ] && "$root/tests/rtmp/peertube.sh" down >/dev/null 2>&1 || true
}
trap cleanup EXIT
cleanup

common=(--network host --security-opt label=disable)
start_mtx() {
  podman run -d --name "$prefix-mediamtx" "${common[@]}" -v "$run/mediamtx.yml:/mediamtx.yml:ro" "$MTX_IMAGE" >/dev/null
}
start_mtx
podman run -d --name "$prefix-ome" "${common[@]}" -v "$run/Server.xml:/opt/ovenmediaengine/bin/origin_conf/Server.xml:ro" \
  -e OME_HOST_IP=127.0.0.1 -e OME_RTMP_PROV_PORT="$OME_RTMP" -e OME_SRT_PROV_PORT=24998 -e OME_MPEGTS_PROV_PORT=24940-24945/udp \
  -e OME_WEBRTC_SIGNALLING_PORT=24333 -e OME_WEBRTC_SIGNALLING_TLS_PORT=24334 -e OME_WEBRTC_CANDIDATE_PORT=24010-24013/udp \
  -e OME_WEBRTC_TCP_ICE_PORT=24014/tcp -e OME_WEBRTC_TCP_RELAY_PORT=24478 -e OME_ORIGIN_PORT=24909 \
  -e OME_LLHLS_STREAM_PORT=24333 -e OME_LLHLS_STREAM_TLS_PORT=24334 "$OME_IMAGE" >/dev/null
for i in $(seq 1 60); do taken -t "$MTX_RTMP" && taken -t "$OME_RTMP" && break; sleep 0.5; done
taken -t "$MTX_RTMP" && taken -t "$OME_RTMP" || { echo "MediaMTX or OvenMediaEngine never listened" >&2; exit 1; }
# Nothing off loopback.
if ss -H -lntup | grep -E 'mediamtx|OvenMediaEngine' | grep -v -E '127\.0\.0\.1:' ; then
  echo "a test server listens off loopback" >&2; exit 1
fi

# The test asks for MediaMTX to go away (run/restart-mediamtx): it is stopped, kept away 5 s,
# started again, and the test is told (run/mediamtx-restarted).
(
  while true; do
    if [ -e "$run/restart-mediamtx" ]; then
      rm -f "$run/restart-mediamtx"
      podman rm -f "$prefix-mediamtx" >/dev/null 2>&1 || true
      sleep 5
      start_mtx
      for i in $(seq 1 40); do taken -t "$MTX_RTMP" && break; sleep 0.25; done
      touch "$run/mediamtx-restarted"
    fi
    sleep 0.25
  done
) &
restarter=$!

in_desk=(podman run --rm --network host --security-opt label=disable -v "$root:/work"
  -v "$HOME/.cargo/registry:/root/.cargo/registry" -w /work -e CARGO_TARGET_DIR=/work/target/container
  -e RUST_LOG="${RUST_LOG:-warn}" -e WEAVESIGNAL_RTMP_RUN=/work/tests/rtmp/run
  -e WEAVESIGNAL_RTMP_MEDIAMTX="rtmp://127.0.0.1:$MTX_RTMP/live" -e WEAVESIGNAL_RTMP_RTSP="rtsp://127.0.0.1:$MTX_RTSP/live")

echo "== MediaMTX"
"${in_desk[@]}" "$DESK_IMAGE" cargo test -p weavesignal-desk --test rtmp_stack -- --ignored --test-threads=1 --nocapture \
  mediamtx_takes_both_profiles a_refused_key_is_tried_again the_platform_never_gets 2>&1 | tee "$run/mediamtx-tests.log" | grep -v -E 'Svt\[' 
grep -q 'test result: ok. 3 passed' "$run/mediamtx-tests.log"

echo "== OvenMediaEngine"
"${in_desk[@]}" -e WEAVESIGNAL_RTMP_SECONDS=30 \
  -e WEAVESIGNAL_RTMP_TARGETS="ome-compatible=compatible=rtmp://127.0.0.1:$OME_RTMP/app=ome-compat
ome-open=open=rtmp://127.0.0.1:$OME_RTMP/app=ome-open" \
  "$DESK_IMAGE" cargo test -p weavesignal-desk --test rtmp_stack -- --ignored --nocapture send_to_the_named_servers 2>&1 \
  | tee "$run/ome-tests.log" | grep -E '^(ome-|test result)'
podman logs "$prefix-ome" 2>&1 | sed 's/\x1b\[[0-9;]*m//g' > "$run/ome.log"
echo "-- what OvenMediaEngine made of each stream:"
grep -E 'Track #|Not supported|E-RTMP\)|has been created' "$run/ome.log" | sed 's/^.*| //' | sort | uniq -c | sed 's/^/   /'
grep -q 'Codec(H264)' "$run/ome.log" && grep -q 'Codec(AAC)' "$run/ome.log" || { echo "OvenMediaEngine kept no H.264 and AAC from compatible" >&2; exit 1; }
grep -q 'Codec(AV1)' "$run/ome.log" || { echo "OvenMediaEngine kept no AV1 from open" >&2; exit 1; }

if [ "$PEERTUBE" = 1 ]; then
  echo "== PeerTube"
  PT_HTTP=$PT_HTTP PT_RTMP=$PT_RTMP DESK_IMAGE=$DESK_IMAGE "$root/tests/rtmp/peertube.sh" test
fi
echo "rtmp: all done"
