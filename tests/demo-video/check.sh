#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Listens to the finished demo video: every line is cut from the video's own soundtrack where
# the edit put it and given to VoiceLab's Whisper (127.0.0.1:8181), and what it heard is set
# against the script (a lagging or clipped line, or one drowned by a cart, reads as a miss);
# the swear must not be heard anywhere, neither in the video (bleeped) nor in the listener
# stream around the dump (dumped); and the loudness of each file is measured (-16 LUFS
# integrated is the aim). It also writes a frame of each moment to target/demo-video/edit/check/.
#
#   tests/demo-video/check.sh [video]     # default target/demo-video/out/demo.mp4
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
script="$here/script.json"
. "$here/words.sh"
video=${1:-$root/target/demo-video/out/demo.mp4}
tv="$root/target/demo-video/edit/timeline-video.json"
STT=${STT:-http://127.0.0.1:8181}
work="$root/target/demo-video/edit/check"
mkdir -p "$work"
hear() { curl -fsS --max-time 120 -F "file=@$1" -F response_format=json -F temperature=0 "$STT/v1/audio/transcriptions" | jq -r .text | tr '\n' ' ' | sed 's/^ *//; s/ *$//'; }

misses=0
while IFS= read -r l; do
  id=$(jq -r .id <<<"$l"); at=$(jq -r .at <<<"$l"); len=$(jq -r .len <<<"$l"); text=$(jq -r .text <<<"$l"); swear=$(jq -r '.swear // ""' <<<"$l")
  ffmpeg -v error -y -ss "$(echo "$at - 0.15" | bc)" -t "$(echo "$len + 0.4" | bc)" -i "$video" -vn -ac 1 -ar 16000 "$work/$id.wav"
  heard=$(hear "$work/$id.wav")
  want=$text
  if [ -n "$swear" ]; then
    if grep -qiw "$swear" <<<"$heard"; then echo "SWEAR HEARD in the video: $id: $heard"; misses=$((misses + 1)); fi
    # The words are checked up to the bleep (Whisper invents words for a tone), less the word
    # before the swear, whose last sound the bleep's edge may take.
    upto=$(jq -r .swear_from <<<"$l")
    ffmpeg -v error -y -ss "$(echo "$at - 0.15" | bc)" -t "$(echo "$upto + 0.1" | bc)" -i "$video" -vn -ac 1 -ar 16000 "$work/$id-before.wav"
    heard=$(hear "$work/$id-before.wav")
    want=$(sed -E "s/[[:alnum:]']+ +$swear\\b.*$//" <<<"$text")
  fi
  a=$(norm "$want"); b=$(norm "$heard")
  # Up to the bleep, Whisper may stop a word or two short of it: compare as many words as it heard.
  [ -n "$swear" ] && a=$(cut -d' ' -f1-"$(wc -w <<<"$b")" <<<"$a")
  d=$(distance "$a" "$b")
  # A word or one in ten may go to Whisper's own slips (a name split, a soft last consonant).
  words=$(wc -w <<<"$a"); allowed=$(( words / 10 > 1 ? words / 10 : 1 ))
  mark=ok; [ "$d" -gt 0 ] && mark="ok ($d)"; [ "$d" -gt "$allowed" ] && { mark="MISS ($d)"; misses=$((misses + 1)); }
  printf '%-14s %7.2fs  %-9s %s\n' "$id" "$at" "$mark" "$heard"
done < <(jq -c '.lines[]' "$tv")

# The listener stream around the dump: the swear must not be in it.
swear=$(jq -r '[.moments[].steps[] | select(.swear) | .swear][0]' "$script")
sfile=$(jq -r .stream.file "$tv"); sfrom=$(jq -r .stream.dump.from "$tv"); slen=$(jq -r .stream.dump.len "$tv")
ffmpeg -v error -y -ss "$sfrom" -t "$slen" -i "$sfile" -ac 1 -ar 16000 "$work/stream-dump.wav"
heard=$(hear "$work/stream-dump.wav")
if grep -qiw "$swear" <<<"$heard"; then echo "SWEAR HEARD on the listener stream: $heard"; misses=$((misses + 1)); else echo "listener stream around the dump (no \"$swear\"): $heard"; fi

for f in "$video" "$root"/target/demo-video/out/demo.webm "$root"/target/demo-video/out/clips/*.webm; do
  [ -f "$f" ] || continue
  i=$(ffmpeg -hide_banner -i "$f" -map 0:a -af ebur128=peak=true:framelog=quiet -f null - 2>&1 | awk '/Integrated loudness/{getline; print $2} /True peak/{getline; print "peak " $2}' | tr '\n' ' ')
  printf '%-40s %s s  %s LUFS\n' "$(basename "$f")" "$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$f")" "$i"
done
while IFS= read -r m; do
  id=$(jq -r .id <<<"$m"); at=$(jq -r .at <<<"$m")
  ffmpeg -v error -y -ss "$(echo "$at + 3" | bc)" -i "$video" -frames:v 1 -vf scale=960:540 "$work/moment-$id.jpg"
done < <(jq -c '.moments[]' "$tv")
echo "$misses misses"
[ "$misses" = 0 ]
