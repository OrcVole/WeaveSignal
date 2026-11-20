#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The congestion-control measurement (issue 1): tests/cc/run-cc.sh over a set of links, each with
# the desk's congestion control on (rtpgccbwe present, as in the images since 1.0.1) and off (the
# plugin hidden: the images before that), then a table. Not in `ci`: about two hours in all, and
# the figures are a measurement, not a pass mark. A group is about 10 minutes; to share the
# machine run each under the stack lock:  flock "$LOCK" tests/cc/matrix.sh audio1
#
#   tests/cc/matrix.sh [cc-audio|cc-video|cc-busy|cc-cost|cc-all|audio1|audio2|audio3|video1|video2|video3|cost|images|throttle|throttle2|busy|repeat|repeat2|threshold|all|table]
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
run="$here/run"
mkdir -p "$run"
one() { # name, then VAR=value pairs
  local name=$1; shift
  echo "== $name" >&2
  env NAME="$name" "$@" "$here/run-cc.sh" | tail -1 >> "$run/matrix.jsonl"
}
JITTER='delay 80ms 40ms distribution normal'
MILD='delay 15ms 10ms distribution normal'
audio1() { for g in on off; do
  one "audio-$g-clean" GCC=$g; one "audio-$g-desk-loss5" GCC=$g DESK_NETEM='loss 5%'; one "audio-$g-guest-loss5" GCC=$g GUEST_NETEM='loss 5%'
done; }
audio2() { for g in on off; do
  one "audio-$g-desk-loss2" GCC=$g DESK_NETEM='loss 2%'; one "audio-$g-desk-rate96k" GCC=$g DESK_RATE=96kbit
  one "audio-$g-guest-rate96k" GCC=$g GUEST_RATE=96kbit; one "audio-$g-guest-loss2" GCC=$g GUEST_NETEM='loss 2%'
done; }
audio3() { for g in on off; do
  one "audio-$g-desk-jitter" GCC=$g DESK_NETEM="$JITTER"; one "audio-$g-desk-mild" GCC=$g DESK_NETEM="$MILD"
  one "audio-$g-desk-loss15" GCC=$g DESK_NETEM='loss 15%'
done; }
video1() { for g in on off; do # the picture needs a minute and a half to show a trend
  one "video-$g-clean" GCC=$g VIDEO=1 SECONDS_TOTAL=90; one "video-$g-desk-loss5" GCC=$g VIDEO=1 SECONDS_TOTAL=90 DESK_NETEM='loss 5%'
  one "video-$g-desk-rate600k" GCC=$g VIDEO=1 SECONDS_TOTAL=90 DESK_RATE=600kbit
done; }
video2() { for g in on off; do
  one "video-$g-desk-loss2" GCC=$g VIDEO=1 SECONDS_TOTAL=90 DESK_NETEM='loss 2%'
  one "video-$g-desk-rate250k" GCC=$g VIDEO=1 SECONDS_TOTAL=90 DESK_RATE=250kbit
  one "video-$g-desk-rate150k" GCC=$g VIDEO=1 SECONDS_TOTAL=90 DESK_RATE=150kbit
  one "video-$g-guest-loss5" GCC=$g VIDEO=1 SECONDS_TOTAL=90 GUEST_NETEM='loss 5%'
done; }
video3() { for g in on off; do
  one "video-$g-desk-jitter" GCC=$g VIDEO=1 SECONDS_TOTAL=90 DESK_NETEM="$JITTER"; one "video-$g-desk-mild" GCC=$g VIDEO=1 SECONDS_TOTAL=90 DESK_NETEM="$MILD"
done; }
cost() { for g in on off; do one "cost-$g-8guests" GCC=$g GUESTS=8 SECONDS_TOTAL=70; done; } # eight more guests
throttle() { for g in on off; do # the desk's container held to 40 ms of CPU in each 100 ms
  one "video-$g-desk-cpu40" GCC=$g VIDEO=1 SECONDS_TOTAL=90 DESK_CPU_QUOTA=40
  one "audio-$g-desk-cpu25" GCC=$g SECONDS_TOTAL=60 DESK_CPU_QUOTA=25
done; }
throttle2() { for g in on off; do # just under what the desk needs (about 20 ms in each 100 ms with the picture on): it is held and released in turn
  one "video-$g-desk-cpu22" GCC=$g VIDEO=1 SECONDS_TOTAL=90 DESK_CPU_QUOTA=22
  one "video-$g-desk-cpu15" GCC=$g VIDEO=1 SECONDS_TOTAL=90 DESK_CPU_QUOTA=15
done; }
busy() { for g in on off; do # a picture that wants more than the 1.2 Mbit cap the desk sets on it
  one "busy-$g-clean" GCC=$g VIDEO=1 BUSY=1 SECONDS_TOTAL=90
  one "busy-$g-desk-loss5" GCC=$g VIDEO=1 BUSY=1 SECONDS_TOTAL=90 DESK_NETEM='loss 5%'
  one "busy-$g-desk-rate1m" GCC=$g VIDEO=1 BUSY=1 SECONDS_TOTAL=90 DESK_RATE=1000kbit
done; }
repeat() { for r in r2 r3; do for g in on off; do # the throttled desk again: how much of the difference is run-to-run
  one "video-$g-desk-cpu22-$r" GCC=$g VIDEO=1 SECONDS_TOTAL=90 DESK_CPU_QUOTA=22
done; done; }
repeat2() { for r in r1 r2 r3; do for g in on off; do # audio only, the desk held to 9 ms of CPU in each 100 ms (it needs about 7)
  one "audio-$g-desk-cpu9-$r" GCC=$g SECONDS_TOTAL=60 DESK_CPU_QUOTA=9
done; done; }
threshold() { for g in on off; do # between the mild and the heavy jitter, and LiveKit (not the desk) short of CPU
  one "video-$g-desk-j40" GCC=$g VIDEO=1 SECONDS_TOTAL=90 DESK_NETEM='delay 40ms 20ms distribution normal'
  one "video-$g-livekit-cpu20" GCC=$g VIDEO=1 SECONDS_TOTAL=90 LK_CPU_QUOTA=20
  one "video-$g-livekit-cpu10" GCC=$g VIDEO=1 SECONDS_TOTAL=90 LK_CPU_QUOTA=10
done; }
images() { # each image's own desk binary and GStreamer: the server image's and the Cloudron image's
  : "${WEAVESIGNAL_SERVER_IMAGE:?set WEAVESIGNAL_SERVER_IMAGE and WEAVESIGNAL_CLOUDRON_IMAGE: the images whose own desk binary is run}"
  : "${WEAVESIGNAL_CLOUDRON_IMAGE:?set WEAVESIGNAL_CLOUDRON_IMAGE}"
  for spec in "server|$WEAVESIGNAL_SERVER_IMAGE|/usr/local/bin/weavesignal-desk" "cloudron|$WEAVESIGNAL_CLOUDRON_IMAGE|/app/code/bin/weavesignal-desk"; do
    IFS='|' read -r n img bin <<<"$spec"
    one "image-$n-clean" GCC=on PROBE=1 SECONDS_TOTAL=30 SETTLE=10 DESK_RUN_IMAGE="$img" DESK_RUN_BIN="$bin"
    one "image-$n-video-rate600k" GCC=on PROBE=1 VIDEO=1 SECONDS_TOTAL=60 SETTLE=15 DESK_RATE=600kbit DESK_RUN_IMAGE="$img" DESK_RUN_BIN="$bin"
  done
}
# 1.2.0 (Phase 16c): the three congestion controls, `desk.congestion_control` = gcc, homegrown, disabled,
# under the same links. The names are cc-<mode>-<link>; `tests/cc/matrix.sh cc-table` prints only them.
CCMODES="gcc homegrown disabled"
cc-audio() { for m in $CCMODES; do # 60 s, the guest and a presenter's returns
  one "cc-$m-audio-clean" CC=$m; one "cc-$m-audio-desk-loss5" CC=$m DESK_NETEM='loss 5%'; one "cc-$m-audio-desk-loss15" CC=$m DESK_NETEM='loss 15%'
  one "cc-$m-audio-guest-loss5" CC=$m GUEST_NETEM='loss 5%'; one "cc-$m-audio-desk-jitter" CC=$m DESK_NETEM="$JITTER"
done; }
cc-video() { for m in $CCMODES; do # the programme picture from a plain moving square; 90 s
  one "cc-$m-video-clean" CC=$m VIDEO=1 SECONDS_TOTAL=90; one "cc-$m-video-desk-loss5" CC=$m VIDEO=1 SECONDS_TOTAL=90 DESK_NETEM='loss 5%'
  one "cc-$m-video-desk-rate600k" CC=$m VIDEO=1 SECONDS_TOTAL=90 DESK_RATE=600kbit
  one "cc-$m-video-desk-jitter" CC=$m VIDEO=1 SECONDS_TOTAL=90 DESK_NETEM="$JITTER"; one "cc-$m-video-desk-mild" CC=$m VIDEO=1 SECONDS_TOTAL=90 DESK_NETEM="$MILD"
done; }
cc-busy() { for m in $CCMODES; do # a picture that wants more than the desk's 1.2 Mbit/s cap
  one "cc-$m-busy-clean" CC=$m VIDEO=1 BUSY=1 SECONDS_TOTAL=90; one "cc-$m-busy-desk-loss5" CC=$m VIDEO=1 BUSY=1 SECONDS_TOTAL=90 DESK_NETEM='loss 5%'
  one "cc-$m-busy-desk-rate1m" CC=$m VIDEO=1 BUSY=1 SECONDS_TOTAL=90 DESK_RATE=1000kbit
done; }
cc-cost() { for m in $CCMODES; do one "cc-$m-cost-8guests" CC=$m GUESTS=8 SECONDS_TOTAL=70; done; }
case ${1:-all} in
  cc-audio|cc-video|cc-busy|cc-cost) "$1" ;;
  cc-all) cc-audio; cc-video; cc-busy; cc-cost ;;
  cc-table) node "$here/summarise.mjs" table "$run/matrix.jsonl" | awk 'NR<=2||/^cc-/'; exit 0 ;;
  audio1|audio2|audio3|video1|video2|video3|cost|images|throttle|throttle2|busy|repeat|repeat2|threshold) "$1" ;;
  all) audio1; audio2; audio3; video1; video2; video3; cost; images; throttle; throttle2; busy; repeat; repeat2; threshold ;;
  table) ;;
  *) echo "audio1, audio2, audio3, video1, video2, video3, cost, all or table" >&2; exit 2 ;;
esac
node "$here/summarise.mjs" table "$run/matrix.jsonl"
