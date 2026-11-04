#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Makes assets/default-loop/default-loop.flac, the loop a new station plays until it adds its own
# fallback files (ADR-0130). The sound is synthesised here from sine tones, so it is the
# project's own work and carries no third party's licence (the recording is CC0-1.0, see
# REUSE.toml; this script is AGPL-3.0-only).
#
#   assets/default-loop/make.sh [OUT.flac]     needs ffmpeg 6 or later
#
# What it is: 32 seconds of a soft D major add 9 pad that breathes slowly, with a few quiet bell
# notes on the pentatonic scale. Every tone has a whole number of cycles in the 32 seconds, and
# every slow swell and every bell's envelope repeats every 32 seconds, so the last sample runs
# straight into the first: the loop has no join, no fade and no click, in any player that loops
# it gaplessly. It is then given a constant gain to reach the station's loudness target,
# -23 LUFS integrated (`library_lufs`, `carts.loudness_lufs`, the desk's tile), with its true
# peak well under the limiter's -1.5 dBFS. The script checks both and fails if either is off.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
out=${1:-$here/default-loop.flac}
L=32            # seconds
TARGET=-23      # LUFS integrated
PEAK_MAX=-6     # dBTP: far under the limiter's -1.5, so the limiter never touches it

# Cycles in L seconds for each tone (frequency x 32, rounded): D3 A3 D4 E4 F#4 A4, and the
# bells D5 E5 F#5 A5 B5.
pad_l=(4699 7040 9397 10548 11840 14080)
pad_r=(4700 7040 9398 10548 11839 14080)
bell_n=(18795 21096 23680 28160 31609)
# The bells: the note (an index into bell_n), the second it starts, and its side (-1 left to 1).
bells=("0 1.0 -0.3" "2 5.5 0.35" "3 9.0 -0.15" "1 13.5 0.4" "2 17.0 -0.4" "0 21.0 0.2" "3 24.5 -0.25" "4 28.0 0.3")

pad() { # side (l|r)
  local side=$1 i expr='' n a b
  local -a cycles
  if [ "$side" = l ]; then cycles=("${pad_l[@]}"); else cycles=("${pad_r[@]}"); fi
  for i in "${!cycles[@]}"; do
    n=${cycles[$i]}
    a=$((i + 1)); b=$((i % 3 + 1))
    # a slow swell: 1 to 3 cycles in L, a different phase for each tone and side
    expr+="+(0.55+0.45*sin(2*PI*$b*t/$L+$i*1.7+$([ "$side" = r ] && echo 0.9 || echo 0)))*sin(2*PI*$n*t/$L+$a)*$(awk -v i="$i" 'BEGIN{printf "%.3f", 0.20/(1+i*0.25)}')"
  done
  printf '%s' "$expr"
}

bell_terms() { # side (l|r)
  local side=$1 spec idx t0 pan n expr='' g
  for spec in "${bells[@]}"; do
    read -r idx t0 pan <<< "$spec"
    n=${bell_n[$idx]}
    if [ "$side" = l ]; then g=$(awk -v p="$pan" 'BEGIN{printf "%.3f", 0.5*(1-p)}'); else g=$(awk -v p="$pan" 'BEGIN{printf "%.3f", 0.5*(1+p)}'); fi
    # u: seconds since the bell started, wrapped into one loop; a 6 ms rise, a 1.2 s decay
    expr+="+0.16*$g*(1-exp(-u_(t,$t0)*400))*exp(-u_(t,$t0)*1.25)*sin(2*PI*$n*t/$L)"
  done
  # ffmpeg expressions have no user functions: write u_ out in full
  printf '%s' "$expr" | sed -E 's/u_\(t,([0-9.]+)\)/mod(t-\1+'"$L"',32)/g'
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
left="0$(pad l)$(bell_terms l)"
right="0$(pad r)$(bell_terms r)"
ffmpeg -v error -y -f lavfi -i "aevalsrc='${left}|${right}':s=48000:d=$L:c=stereo" -c:a pcm_f32le "$tmp/raw.wav"

# Measure, then apply one constant gain (no limiter or filter, which would break the loop).
measure() { ffmpeg -nostats -i "$1" -af ebur128=peak=true -f null - 2>&1 | awk '/Summary:/{s=1} s&&/ I:/{i=$2} s&&/Peak:/{p=$2} END{print i, p}'; }
read -r lufs _ <<< "$(measure "$tmp/raw.wav")"
gain=$(awk -v t="$TARGET" -v l="$lufs" 'BEGIN{printf "%.3f", t-l}')
ffmpeg -v error -y -i "$tmp/raw.wav" -af "volume=${gain}dB,aresample=osr=48000:osf=s16:dither_method=triangular" \
  -c:a flac -compression_level 12 -metadata title="WeaveSignal default loop" -metadata comment="Synthesised by assets/default-loop/make.sh. CC0-1.0." "$out"

read -r lufs peak <<< "$(measure "$out")"
echo "default loop: $out, ${L} s, ${lufs} LUFS integrated, true peak ${peak} dBTP"
awk -v l="$lufs" -v p="$peak" -v t="$TARGET" -v m="$PEAK_MAX" 'BEGIN{ if (l < t-0.5 || l > t+0.5 || p > m) { print "make.sh: outside the loudness target or the peak ceiling" > "/dev/stderr"; exit 1 } }'
