#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Congestion control at the desk (issue 1): one scenario of the measurement, rootless.
#
# The real desk (the binary built by `just desk-build`, in its image), LiveKit 1.13.10 and the
# people's tones each in a container on a podman network of their own (10.89.77.0/24, internal:
# no route out). A headless Chromium joins as a guest from inside podman's rootless network
# namespace, so it reaches the containers over the bridge, and reads its return's and (VIDEO=1)
# the programme picture's `getStats` every few seconds (tests/cc/receiver.mjs).
#
# The links are shaped with `tc` (netem for loss and delay, tbf for a rate) in the containers'
# network namespaces, entered with `podman unshare nsenter`: no root, no sudo.
#   DESK_NETEM / DESK_RATE    the desk's own uplink (desk to LiveKit): where the desk's congestion
#                             control looks
#   GUEST_NETEM / GUEST_RATE  LiveKit's downlink to the guest's browser (the desk never sees it)
# They are applied SETTLE seconds after the guest first hears its return, so a run shows the
# clean link first and then the impaired one (or, with neither set, a clean run for comparison).
#
#   GCC=on|off   off hides gst-plugins-rs's rtp plugin (rtpgccbwe) from the desk: the images
#                before 1.0.1, which sent with no congestion control at all
#   VIDEO=1      the desk's video path on, the programme picture published to the room (VP8)
#   GUESTS=N     N more tone guests, each with a return of its own (the desk's cost per return)
#   BIN=release|debug   the desk binary: release, as shipped (default), or the debug build
#   DESK_RUN_IMAGE=... DESK_RUN_BIN=...   another image's own desk binary (its own GStreamer and plugins)
#   DESK_CPU_QUOTA=N   the desk's container may use N ms of CPU in each 100 ms (cgroup cpu.max): a busy
#                      host's stalls, for the delay-based half of the estimator
#   BUSY=1       the guest's camera is a noisy test picture that VP8 cannot squeeze (a made file in run/),
#                not Chromium's plain moving square: a programme picture that wants the whole cap
#   LK_CPU_QUOTA=N     the same for LiveKit's container (the receiving end of the desk's packets)
#   PROBE=1      GST_DEBUG on webrtcsink and rtpgccbwe, and pipeline dumps, kept in run/
#
#   GCC=on DESK_NETEM='loss 5%' SECONDS_TOTAL=60 tests/cc/run-cc.sh
#   SECONDS_TOTAL=60 SETTLE=20   the run's length and the clean part of it, in seconds
# Results: JSON lines on stdout and in run/<NAME>.json; the desk's log in run/<NAME>.desk.log.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
here="$root/tests/cc"
run="$here/run"
NAME=${NAME:-scenario}
prefix=${WEAVESIGNAL_TEST_PREFIX:-ws-net}-cc
GCC=${GCC:-on}
VIDEO=${VIDEO:-0}
GUESTS=${GUESTS:-0}
BIN=${BIN:-release}
SECONDS_TOTAL=${SECONDS_TOTAL:-60}
SETTLE=${SETTLE:-20}
DESK_NETEM=${DESK_NETEM:-}
DESK_RATE=${DESK_RATE:-}
GUEST_NETEM=${GUEST_NETEM:-}
GUEST_RATE=${GUEST_RATE:-}
PROBE=${PROBE:-}
BUSY=${BUSY:-}
DESK_CPU_QUOTA=${DESK_CPU_QUOTA:-}
LK_CPU_QUOTA=${LK_CPU_QUOTA:-}
DESK_IMAGE=${WEAVESIGNAL_DESK_IMAGE:-localhost/weavesignal-desk-dev:m5}
# The desk to run: by default the worktree's build in the desk image; DESK_RUN_IMAGE and
# DESK_RUN_BIN run another image's own binary instead (the server image's, the Cloudron image's).
DESK_RUN_IMAGE=${DESK_RUN_IMAGE:-$DESK_IMAGE}
DESK_RUN_BIN=${DESK_RUN_BIN:-}
LK_IMAGE=${LK_IMAGE:-docker.io/livekit/livekit-server:v1.13.10}
LIVEKIT_UMD=${LIVEKIT_UMD:-$root/apps/web/node_modules/livekit-client/dist/livekit-client.umd.js}
[ -f "$LIVEKIT_UMD" ] || { echo "no $LIVEKIT_UMD: run pnpm install in apps/web, or set LIVEKIT_UMD" >&2; exit 1; }
: "${PLAYWRIGHT_CORE:?set PLAYWRIGHT_CORE to an installed playwright-core (with its Chromium)}"
NET=$prefix-net
SUBNET=10.89.77
LK_IP=$SUBNET.2
DESK_IP=$SUBNET.3
PEOPLE_IP=$SUBNET.4
API_KEY=cckey
API_SECRET=cc-test-only-secret-not-for-use-00000
ROOM=cc
[ -n "$DESK_RUN_BIN" ] || [ -x "$root/target/container/$BIN/weavesignal-desk" ] || { echo "build the desk first: just desk-build (or cargo build --release in its image)" >&2; exit 1; }

mkdir -p "$run"
rm -rf "$run/hls" "$run/dot"; mkdir -p "$run/hls" "$run/dot"; rm -f "$run/$NAME.shaped"
# Another image runs its desk as an unprivileged user: it must be able to write here.
chmod 0777 "$run/hls" "$run/dot"
cleanup() {
  for c in desk livekit people; do
    podman logs "$prefix-$c" > "$run/$NAME.$c.log" 2>&1 || true
    podman rm -f -t 1 "$prefix-$c" >/dev/null 2>&1 || true
  done
  podman network rm -f "$NET" >/dev/null 2>&1 || true
  [ -n "${recv_pid:-}" ] && kill "$recv_pid" 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM
cleanup
camera=
if [ -n "$BUSY" ]; then
  camera="$run/busy.y4m"
  [ -s "$camera" ] || ffmpeg -v error -f lavfi -i "testsrc2=size=640x360:rate=25,noise=alls=35:allf=t+u" -t 8 -pix_fmt yuv420p -y "$camera"
fi
podman network create --internal --subnet $SUBNET.0/24 "$NET" >/dev/null

cat > "$run/livekit.yaml" <<YAML
port: 7880
bind_addresses: ["0.0.0.0"]
rtc:
  udp_port: 7882
  tcp_port: 0
  use_external_ip: false
  node_ip: $LK_IP
  ips:
    includes: ["$SUBNET.0/24"]
room:
  enabled_codecs:
    - mime: audio/opus
    - mime: audio/red
    - mime: video/vp8
keys:
  $API_KEY: $API_SECRET
logging:
  level: warn
YAML
cat > "$run/desk.toml" <<TOML
[livekit]
url = "ws://$LK_IP:7880"
api_key = "$API_KEY"
room = "$ROOM"
[programme]
srt_port = 19100
[control]
socket = "/tmp/desk.sock"
TOML
if [ "$VIDEO" = 1 ]; then
  cat >> "$run/desk.toml" <<TOML
[video]
enabled = true
[video.hls]
dir = "/work/tests/cc/run/hls"
TOML
fi

common=(--network "$NET" --security-opt label=disable)
lkcpu=()
[ -n "$LK_CPU_QUOTA" ] && lkcpu=(--cpu-period=100000 "--cpu-quota=$((LK_CPU_QUOTA * 1000))")
podman run -d --name "$prefix-livekit" "${common[@]}" "${lkcpu[@]}" --ip $LK_IP -v "$run/livekit.yaml:/etc/livekit.yaml:ro" \
  "$LK_IMAGE" --config /etc/livekit.yaml >/dev/null
lk_up() { podman unshare --rootless-netns curl -fs "http://$LK_IP:7880" >/dev/null 2>&1; }
for i in $(seq 1 60); do lk_up && break; sleep 0.5; [ "$i" = 60 ] && { echo "LiveKit did not come up" >&2; exit 1; }; done

mask=()
[ "$GCC" = off ] && mask=(-v /dev/null:/usr/lib64/gstreamer-1.0/libgstrsrtp.so:ro)
cpu=()
[ -n "$DESK_CPU_QUOTA" ] && cpu=(--cpu-period=100000 "--cpu-quota=$((DESK_CPU_QUOTA * 1000))")
dbg=()
[ -n "$PROBE" ] && dbg=(-e "GST_DEBUG=webrtcsink:5,rtpgccbwe:6,webrtcbin:3" -e GST_DEBUG_DUMP_DOT_DIR=/work/tests/cc/run/dot -e GST_DEBUG_NO_COLOR=1)
podman run -d --name "$prefix-desk" "${common[@]}" --ip $DESK_IP -v "$root:/work" -w /work "${mask[@]}" "${cpu[@]}" "${dbg[@]}" \
  -e "WEAVESIGNAL_LIVEKIT_API_SECRET=$API_SECRET" -e RUST_LOG=weavesignal_desk=info \
  "$DESK_RUN_IMAGE" "${DESK_RUN_BIN:-/work/target/container/$BIN/weavesignal-desk}" --config /work/tests/cc/run/desk.toml >/dev/null

tok() { node "$here/token.mjs" "$API_KEY" "$API_SECRET" "$ROOM" "$@"; }
# The presenter, and GUESTS tone guests, each a process of their own in one container.
launch=""
add() { # identity role volume
  launch+="gst-launch-1.0 -q audiotestsrc is-live=true wave=pink-noise volume=$3 ! audio/x-raw,format=S16LE,rate=48000,channels=2 ! queue ! livekitwebrtcsink name=pub-$1 signaller::ws-url=ws://$LK_IP:7880 signaller::auth-token=$(tok "$1" "$2") signaller::room-name=$ROOM & "
}
add rory presenter 0.10
for n in $(seq 1 "$GUESTS"); do add "guest$n" guest 0.05; done
podman run -d --name "$prefix-people" "${common[@]}" --ip $PEOPLE_IP "$DESK_IMAGE" sh -c "${launch}wait" >/dev/null

tcn() { local c=$1; shift; podman unshare nsenter --net="$(podman inspect -f '{{.NetworkSettings.SandboxKey}}' "$prefix-$c")" tc "$@"; }
qstats() { # container: bytes, packets, dropped of the root-most qdisc with traffic
  tcn "$1" -s qdisc show dev eth0 | awk '/^qdisc (netem|prio) /{f=1} f&&/Sent/{print $2, $4, $7; exit}' | tr -d ',)'
}
desk_cpu_ticks() { podman exec "$prefix-desk" sh -c 'read -r _ _ _ _ _ _ _ _ _ _ _ _ _ u s _ < /proc/1/stat; echo $((u+s))'; }
desk_threads() { podman exec "$prefix-desk" sh -c 'ls /proc/1/task | wc -l'; }

( # The guest: Chromium in podman's rootless network namespace.
  exec env CAMERA_FILE="$camera" PLAYWRIGHT_CORE="$PLAYWRIGHT_CORE" LIVEKIT_UMD="$LIVEKIT_UMD" URL="ws://$LK_IP:7880" TOKEN="$(tok gregor guest subscribe)" \
    IDENTITY=gregor SECONDS="$SECONDS_TOTAL" VIDEO="$VIDEO" PHASE_FILE="$run/$NAME.shaped" \
    podman unshare --rootless-netns node "$here/receiver.mjs"
) > "$run/$NAME.recv" 2> "$run/$NAME.recv.err" &
recv_pid=$!
for i in $(seq 1 180); do grep -q '"ready"' "$run/$NAME.recv" 2>/dev/null && break; kill -0 "$recv_pid" 2>/dev/null || { echo "the guest's browser stopped:"; cat "$run/$NAME.recv" "$run/$NAME.recv.err"; exit 1; }; sleep 0.5; [ "$i" = 180 ] && { echo "the guest never heard its return" >&2; exit 1; }; done

# Settled: the baseline, then the impairment.
sleep "$SETTLE"
cpu0=$(desk_cpu_ticks); thr0=$(desk_threads); t0=$(date +%s.%N)
shape_desk() {
  tcn desk qdisc replace dev eth0 root handle 1: netem ${DESK_NETEM:-delay 0ms}
  [ -n "$DESK_RATE" ] && tcn desk qdisc add dev eth0 parent 1:1 handle 10: tbf rate "$DESK_RATE" burst 16kb latency 300ms
  return 0
}
shape_guest() {
  tcn livekit qdisc replace dev eth0 root handle 1: prio bands 3 priomap 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
  tcn livekit qdisc add dev eth0 parent 1:1 handle 10: netem ${GUEST_NETEM:-delay 0ms}
  [ -n "$GUEST_RATE" ] && tcn livekit qdisc add dev eth0 parent 10:1 handle 20: tbf rate "$GUEST_RATE" burst 16kb latency 300ms
  # Only what goes to the browser: the desk and the people's tones are left alone.
  for ip in $DESK_IP $PEOPLE_IP; do tcn livekit filter add dev eth0 protocol ip parent 1:0 prio 1 u32 match ip dst "$ip/32" flowid 1:3; done
  return 0
}
shape_desk
shape_guest
touch "$run/$NAME.shaped"
d0=$(qstats desk); g0=$(qstats livekit)
wait "$recv_pid"; recv_pid=
t1=$(date +%s.%N)
cpu1=$(desk_cpu_ticks); thr1=$(desk_threads)
d1=$(qstats desk); g1=$(qstats livekit)
dt=$(echo "$t1 - $t0" | bc -l)
read -r db dp dd <<<"$d1"
read -r gb gp gd <<<"$g1"
hz=$(getconf CLK_TCK)
podman logs "$prefix-desk" > "$run/$NAME.desk.log" 2>&1 || true
lines=$(grep -c 'rtpgccbwe.*not available' "$run/$NAME.desk.log" 2>/dev/null || true)
lines=${lines:-0}
# With PROBE=1: how many of the desk's own session pipelines (webrtcsink's dumps) hold the estimator.
dumps=0
[ -n "$PROBE" ] && dumps=$(grep -l 'GstRtpGCCBwE' "$run"/dot/*peer-unique-remote-description-set.dot 2>/dev/null | wc -l)
node "$here/summarise.mjs" run "$run/$NAME.recv" name="$NAME" gcc="$GCC" video="$VIDEO" guests="$GUESTS" bin="$BIN" \
  desk_netem="$DESK_NETEM" desk_rate="$DESK_RATE" guest_netem="$GUEST_NETEM" guest_rate="$GUEST_RATE" dt="$dt" \
  desk_bytes="$db" desk_packets="$dp" desk_dropped="$dd" lk_packets="$gp" lk_dropped="$gd" cpu0="$cpu0" cpu1="$cpu1" hz="$hz" \
  desk_quota="$DESK_CPU_QUOTA" lk_quota="$LK_CPU_QUOTA" busy="$BUSY" thr0="$thr0" thr1="$thr1" warn_lines="$lines" dumps="$dumps" \
  | tee "$run/$NAME.json"
echo "(the desk's log has $lines lines saying rtpgccbwe is missing; GCC=$GCC)" >&2
# The control: the warning is there exactly when the plugin was hidden.
if { [ "$GCC" = off ] && [ "$lines" = 0 ]; } || { [ "$GCC" = on ] && [ "$lines" != 0 ]; }; then echo "CONTROL FAILED: the desk's warning count ($lines) does not match GCC=$GCC" >&2; exit 3; fi
