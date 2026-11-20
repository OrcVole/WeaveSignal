#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The desk's video encode on this machine's NVIDIA card, measured (Phase 17e, issue #8): the
# server image's own GStreamer runs the desk's AV1 chain (`nvav1enc` with the desk's settings, from
# crates/desk/src/video.rs `encoder_chain`) on the card handed over by its CDI name, as gpu.yaml
# hands it to the desk, then the same picture with SVT-AV1 on the CPU for comparison. Each run
# writes how fast it encoded (frames per second against the rung's own) and the card's encoder use.
#
# The card's driver reports a fault as an Xid in the kernel log (this machine's crashed on one,
# Xid 79, on 2026-11-17). The kernel log is followed from the moment the script starts, and the
# first Xid stops everything: the encode is killed, the Xid line is written to the results, and
# the script exits 3. At the end the kernel log since the start is read again, whole, so an Xid the
# follower missed still fails the run. Xids from before the start (earlier boots included) are
# counted and said, never taken as this run's.
#
#   tests/gpu/measure.sh                     the compose probe, then every rung (480p20, 720p30,
#                                            1080p30), 3 encodes at once at 720p30, SVT-AV1 at 720p30
#   tests/gpu/measure.sh --live --sustain 30 the same in real time, then 30 minutes of 720p30 on the card
#   tests/gpu/measure.sh --rungs 480p20 --seconds 20 --parallel 0 --no-svt --no-compose
#   WEAVESIGNAL_SERVER_IMAGE=localhost/weavesignal-server:r122 tests/gpu/measure.sh
#
# Options: --image IMAGE (default $WEAVESIGNAL_SERVER_IMAGE, else the 1.2.0 server image),
# --device CDI-NAME (default $GPU_DEVICE, else nvidia.com/gpu=all), --rungs "A B" (480p20, 360p20,
# 720p30, 720p25, 1080p30), --seconds N (seconds of picture per run, default 60), --live (each run
# in real time, so N seconds takes N seconds; without it the encoder runs flat out and a run takes
# a few seconds), --sustain MIN (then MIN minutes of 720p30 on the card in real time; 0, the
# default, skips it), --parallel N (encodes at once at 720p30; 0 skips it; NVENC's session limit is
# the card driver's), --no-svt, --no-compose, --force, --out DIR (default
# target/gpu-measure/<UTC time>; target/ is not kept, so copy RESULT.md somewhere durable).
#
# The compose probe (first, unless --no-compose) checks the Compose route's own hand-over: a
# one-service project shaped like gpu.yaml's desk (the device by its CDI name, labelling off) is
# run once with `podman compose run`, and must find `nvav1enc` with the card. Every other step is
# one short-lived container (`podman run --rm`, no network); no stack, no station. Do not run it
# while another job uses the card (a soak's VoiceLab, NVENC elsewhere): it refuses to start while
# nvidia-smi lists a compute process, unless --force.
# Needs Podman (with `podman compose` for the probe), an NVIDIA CDI spec (`sudo nvidia-ctk cdi
# generate --output=/etc/cdi/nvidia.yaml`; WEAVESIGNAL_CDI_DIRS names where to look), nvidia-smi,
# and a user who may read the kernel log (journalctl -k). Not in `ci`; tests/gpu/measure-check.sh
# runs its control flow with every command stubbed, touching no card.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
image=${WEAVESIGNAL_SERVER_IMAGE:-forgejo.weavesignal.top/weavesignal/weavesignal-server:1.2.0}
device=${GPU_DEVICE:-nvidia.com/gpu=all}
cdi_dirs=${WEAVESIGNAL_CDI_DIRS:-/etc/cdi /var/run/cdi}
rungs="480p20 720p30 1080p30"
seconds=60
live=false
sustain=0
parallel=3
svt=true
compose_probe=true
force=false
out=''
while [ $# -gt 0 ]; do
  case $1 in
    --image) image=${2:?}; shift ;;
    --device) device=${2:?}; shift ;;
    --rungs) rungs=${2:?}; shift ;;
    --seconds) seconds=${2:?}; shift ;;
    --live) live=true ;;
    --sustain) sustain=${2:?}; shift ;;
    --parallel) parallel=${2:?}; shift ;;
    --no-svt) svt=false ;;
    --no-compose) compose_probe=false ;;
    --force) force=true ;;
    --out) out=${2:?}; shift ;;
    -h|--help) sed -n '5,42p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "measure.sh: unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
die() { echo "measure.sh: $*" >&2; exit 2; }
[[ $seconds =~ ^[0-9]+$ ]] && [ "$seconds" -ge 5 ] && [ "$seconds" -le 3600 ] || die "--seconds is 5 to 3600"
[[ $sustain =~ ^[0-9]+$ ]] && [ "$sustain" -le 720 ] || die "--sustain is 0 to 720 minutes"
[[ $parallel =~ ^[0-9]+$ ]] && [ "$parallel" -le 16 ] || die "--parallel is 0 to 16"
[[ $device =~ ^nvidia\.com/gpu=[A-Za-z0-9][A-Za-z0-9_.:-]*$ ]] || die "--device is a CDI name such as nvidia.com/gpu=all"

# The desk's rungs (crates/desk/src/config.rs, Rung::PRESETS and CANVASES): width height fps kbps.
rung() {
  case $1 in
    480p20) echo "854 480 20 600" ;;
    360p20) echo "640 360 20 350" ;;
    720p30) echo "1280 720 30 1500" ;;
    720p25) echo "1280 720 25 1300" ;;
    1080p30) echo "1920 1080 30 2700" ;;
    *) return 1 ;;
  esac
}
for r in $rungs; do rung "$r" >/dev/null || die "unknown rung $r"; done

# --- Before anything runs ---------------------------------------------------------------------
command -v podman >/dev/null || die "Podman is needed"
command -v nvidia-smi >/dev/null || die "nvidia-smi is needed (the NVIDIA driver's tools)"
journalctl -k -n 1 --no-pager >/dev/null 2>&1 || die "this user cannot read the kernel log (journalctl -k): the Xid watch is the safety, so nothing runs without it"
spec_found=false
for d in $cdi_dirs; do
  for f in "$d"/*.yaml "$d"/*.yml "$d"/*.json; do
    [ -f "$f" ] && grep -q 'nvidia\.com/gpu' "$f" && spec_found=true
  done
done
[ "$spec_found" = true ] || die "no NVIDIA CDI spec in $cdi_dirs: run 'sudo nvidia-ctk cdi generate --output=/etc/cdi/nvidia.yaml'"
busy=$(nvidia-smi --query-compute-apps=pid,process_name --format=csv,noheader 2>/dev/null || true)
if [ -n "$busy" ] && [ "$force" = false ]; then
  die "the card is in use (nvidia-smi lists: $(tr '\n' ';' <<<"$busy")); wait for it to finish, or pass --force"
fi
podman image exists "$image" || die "the server image $image is not here: build it ('just desk-image server-image') or pull it, or give --image"

[ -n "$out" ] || out="$root/target/gpu-measure/$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$out"
log="$out/RESULT.md"
say() { printf '%s\n' "$*" | tee -a "$log"; }
# A line for the results that never lands in a captured result (encode's stdout is its figures).
note() { printf '%s\n' "$*" >> "$log"; printf '%s\n' "$*" >&2; }
start=$(date '+%Y-%m-%d %H:%M:%S')
xids_since_start() { journalctl -k --no-pager --since "$start" 2>/dev/null | grep -c 'NVRM: Xid' || true; }
before=$(journalctl -k --no-pager -b 2>/dev/null | grep -c 'NVRM: Xid' || true)
earlier=$(journalctl -k --no-pager 2>/dev/null | grep -c 'NVRM: Xid' || true)
pace="flat out (a run takes as long as the encoder needs)"
[ "$live" = false ] || pace="in real time"
{
  echo "# GPU encode, $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo
  echo "- Image: \`$image\`; device \`$device\`; rungs $rungs; $seconds s of picture a run, $pace; parallel $parallel; SVT-AV1 $svt; sustained $sustain min"
  echo "- Card: $(nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader | head -1)"
  echo "- Xids in the kernel log before the start: $before this boot, $earlier in every boot kept (not this run's)"
  echo
} > "$log"
cat "$log"

# --- The Xid watch: from the start, the first one stops everything ----------------------------
xid_file="$out/xid.txt"
: > "$xid_file"
containers="$out/containers"
: > "$containers"
stop_all() {
  while read -r c; do [ -z "$c" ] || podman kill "$c" >/dev/null 2>&1 || true; done < "$containers"
}
# The follower and its reader are separate jobs through a FIFO, so each can be stopped by its own
# process id and none outlives the script.
fifo="$out/kernel.fifo"
rm -f "$fifo"; mkfifo "$fifo"
journalctl -k -f --no-pager --since "$start" > "$fifo" 2>/dev/null &
follow_pid=$!
grep --line-buffered -m 1 'NVRM: Xid' < "$fifo" > "$xid_file" &
watch_pid=$!
dmon_pid=''
probe_dir=''
cleanup() {
  stop_all
  [ -z "$dmon_pid" ] || kill "$dmon_pid" 2>/dev/null || true
  kill "$follow_pid" "$watch_pid" 2>/dev/null || true
  rm -f "$fifo"
  [ -z "$probe_dir" ] || rm -rf -- "$probe_dir"
}
trap cleanup EXIT
xid_seen() { [ -s "$xid_file" ]; }
stopped_by_xid() {
  stop_all
  # Said once, though an encode's own subshell may have said it first.
  [ ! -e "$out/stopped" ] || exit 3
  : > "$out/stopped"
  [ -s "$xid_file" ] || journalctl -k --no-pager --since "$start" 2>/dev/null | grep -m 1 'NVRM: Xid' > "$xid_file" || true
  say ""
  say "**STOPPED at the first Xid:** \`$(head -1 "$xid_file")\`"
  say "Nothing after this ran. Read \`journalctl -k --since '$start'\` before trying again."
  exit 3
}

# The card's encoder use, once a second, for the whole run.
nvidia-smi dmon -s u -d 1 > "$out/dmon.txt" 2>/dev/null &
dmon_pid=$!

# One encode in the server image: the desk's chain from a test picture (a moving ball over a
# coloured ground, so the encoder has motion to work on), frames counted at a fakesink. Prints
# "frames seconds" and nothing else; a failure is said in the results and on stderr.
encode() { # name encoder(nvenc|svt) rung seconds live
  local name=$1 enc=$2 w h fps kbps gop chain frames
  read -r w h fps kbps <<<"$(rung "$3")"
  frames=$((fps * $4))
  gop=$((fps * 2))
  if [ "$enc" = nvenc ]; then
    chain="videoconvert ! video/x-raw,format=NV12 ! cudaupload ! nvav1enc bitrate=$kbps gop-size=$gop rc-mode=cbr preset=p4 tune=ultra-low-latency bframes=0 zerolatency=true vbv-buffer-size=$((kbps / 2))"
  else
    chain="svtav1enc target-bitrate=$kbps intra-period-length=$((gop - 1)) preset=10 parameters-string=hierarchical-levels=2"
  fi
  local cname="ws-gpu-measure-$name-$$"
  echo "$cname" >> "$containers"
  local args=(run --rm --name "$cname" --network none --security-opt label=disable)
  [ "$enc" = svt ] || args+=(--device "$device")
  # The registry is rebuilt in the container, so a GPU run finds nvcodec (the image drops it).
  podman "${args[@]}" "$image" sh -c "rm -rf \"\${XDG_CACHE_HOME:-\$HOME/.cache}/gstreamer-1.0\"; \
    gst-launch-1.0 -e videotestsrc num-buffers=$frames pattern=ball background-color=0xff202040 is-live=$5 \
      ! video/x-raw,width=$w,height=$h,framerate=$fps/1 ! $chain ! av1parse ! fakesink sync=false" \
    > "$out/$name.log" 2>&1 &
  local pid=$! status=0
  while kill -0 "$pid" 2>/dev/null; do
    if xid_seen; then stopped_by_xid; fi
    sleep 1
  done
  wait "$pid" || status=$?
  if [ "$status" != 0 ]; then note "- $name: the encode failed (exit $status); its log is $name.log"; return 1; fi
  # gst-launch says "Execution ended after H:MM:SS.nnnnnnnnn".
  local took
  took=$(sed -n 's/.*Execution ended after \([0-9:.]*\).*/\1/p' "$out/$name.log" | tail -1 | awk -F: 'NF == 3 { print $1 * 3600 + $2 * 60 + $3 }')
  if [ -z "$took" ] || ! awk -v t="$took" 'BEGIN { exit !(t > 0) }'; then note "- $name: no running time in its log"; return 1; fi
  echo "$frames $took"
}
report() { # label rung frames seconds
  local w h fps kbps
  read -r w h fps kbps <<<"$(rung "$2")"
  awk -v n="$1" -v f="$3" -v s="$4" -v want="$fps" 'BEGIN { r = f / s; printf "- %s: %d frames in %.1f s, %.1f frames a second (%.1f times the rung'"'"'s %d)\n", n, f, s, r, r / want, want }' | tee -a "$log"
}

say "## Runs"
say ""

# --- 0. The Compose route's own hand-over of the card ----------------------------------------
if [ "$compose_probe" = true ]; then
  probe_dir=$(mktemp -d)
  cat > "$probe_dir/compose.yaml" <<YAML
# gpu.yaml's desk, alone: the card by its CDI name, labelling off.
services:
  probe:
    image: $image
    network_mode: none
    devices:
      - $device
    security_opt: [label=disable]
    entrypoint: ["sh", "-c", "rm -rf \"\$\${XDG_CACHE_HOME:-\$\$HOME/.cache}/gstreamer-1.0\"; gst-inspect-1.0 nvav1enc >/dev/null && gst-inspect-1.0 cudaupload >/dev/null && echo nvenc-found"]
YAML
  if (cd "$probe_dir" && podman compose -p "ws-gpu-probe-$$" -f compose.yaml run --rm probe) > "$out/compose-probe.log" 2>&1 \
      && grep -q nvenc-found "$out/compose-probe.log"; then
    say "- \`podman compose\` hands the card to a service by its CDI name, and \`nvav1enc\` loads there"
  else
    say "- **\`podman compose\` did not hand the card over** (compose-probe.log): the Quadlets (native Podman) may still work; the Compose route does not until this passes"
  fi
  (cd "$probe_dir" && podman compose -p "ws-gpu-probe-$$" -f compose.yaml down >/dev/null 2>&1) || true
  xid_seen && stopped_by_xid
fi

# --- 1. NVENC's AV1 is in the image and reaches the card ---------------------------------------
echo "ws-gpu-measure-inspect-$$" >> "$containers"
if ! podman run --rm --name "ws-gpu-measure-inspect-$$" --network none --security-opt label=disable --device "$device" "$image" \
    sh -c 'rm -rf "${XDG_CACHE_HOME:-$HOME/.cache}/gstreamer-1.0"; gst-inspect-1.0 nvav1enc && gst-inspect-1.0 cudaupload' > "$out/inspect.log" 2>&1; then
  say "- \`nvav1enc\` or \`cudaupload\` is missing in the image with the card (inspect.log): a card before NVIDIA's Ada generation has no AV1 encoder, and the desk uses SVT-AV1 there"
  exit 1
fi
xid_seen && stopped_by_xid
say "- \`nvav1enc\` and \`cudaupload\` load with the card"

# --- 2. One encode per rung on the card --------------------------------------------------------
for r in $rungs; do
  if res=$(encode "nvenc-$r" nvenc "$r" "$seconds" "$live"); then report "NVENC AV1 $r" "$r" $res
  else xid_seen && stopped_by_xid; fi
done

# --- 3. Several at once (the desk opens one session per rung, and RTMP out one more) -----------
if [ "$parallel" -gt 0 ]; then
  pids=()
  for i in $(seq 1 "$parallel"); do
    ( encode "nvenc-720p30-x$i" nvenc 720p30 "$seconds" "$live" > "$out/parallel-$i.txt" ) &
    pids+=($!)
  done
  for p in "${pids[@]}"; do wait "$p" || true; done
  xid_seen && stopped_by_xid
  for i in $(seq 1 "$parallel"); do
    if [ -s "$out/parallel-$i.txt" ]; then report "NVENC AV1 720p30, $i of $parallel at once" 720p30 $(cat "$out/parallel-$i.txt")
    else say "- NVENC AV1 720p30, $i of $parallel at once: failed (nvenc-720p30-x$i.log; past the driver's session limit?)"; fi
  done
fi

# --- 4. The same picture on the CPU --------------------------------------------------------------
if [ "$svt" = true ]; then
  xid_seen && stopped_by_xid
  if res=$(encode svt-720p30 svt 720p30 "$seconds" "$live"); then report "SVT-AV1 720p30 (CPU)" 720p30 $res; fi
fi

# --- 5. Sustained, in real time ----------------------------------------------------------------
if [ "$sustain" -gt 0 ]; then
  xid_seen && stopped_by_xid
  if res=$(encode sustain-720p30 nvenc 720p30 $((sustain * 60)) true); then report "NVENC AV1 720p30 for $sustain min, real time" 720p30 $res; fi
fi

# --- The end: the follower alive, and the whole kernel log since the start read again ----------
xid_seen && stopped_by_xid
kill -0 "$follow_pid" 2>/dev/null || { say ""; say "**The Xid follower stopped during the run**, so the run is not proved; read \`journalctl -k --since '$start'\`."; exit 4; }
after=$(xids_since_start)
[ "${after:-0}" = 0 ] || stopped_by_xid
kill "$dmon_pid" 2>/dev/null || true; dmon_pid=''
peak=$(awk '!/^#/ && NF >= 4 { if ($4 + 0 > m) m = $4 + 0 } END { print m + 0 }' "$out/dmon.txt")
say ""
say "## Result"
say ""
say "- The card's encoder at most $peak % busy (nvidia-smi dmon, dmon.txt)"
say "- No Xid in the kernel log since $start (followed through the run, and read again at the end)"
say ""
say "A run at 1.0 times the rung's rate or more keeps up live; the desk's own encode also waits on"
say "the vision mixer, so treat these as the encoder's ceiling, not the desk's."
echo "results: $out"
