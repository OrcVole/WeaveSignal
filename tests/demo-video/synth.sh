#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Reads every line of tests/demo-video/script.json (the introduction, the show and the outro) in
# its character's designed voice through VoiceLab's tts-serve (127.0.0.1:7500), with the
# reference design.sh made (target/demo-video/voices/<who>.wav) and the character's seed, then
# checks it with VoiceLab's Whisper (127.0.0.1:8181).
#
#   tests/demo-video/synth.sh            # every line not yet made
#   tests/demo-video/synth.sh l-open     # these lines again (a new take)
#   SEED_OFFSET=7 tests/demo-video/synth.sh l-open   # a new take at the character's seed + 7
#                                                     (keep a take that passes in `reseeded`)
#
# Out, in target/demo-video/stems/:
#   raw/<id>.wav   the model's take, 48 kHz stereo
#   <id>.wav       what is played: callers coloured as a phone line (the filter of the talk-show
#                  fixture's fx.sh: 8 kHz, 300-3,400 Hz, a firm compressor, a little line noise;
#                  the desk band-limits callers again on its own, ADR-0194, which does no harm),
#                  every line set to -23 LUFS (the desk's leveller does the rest)
#   check.tsv      id, who, the words Whisper heard against the script's, and the text heard
# The text sent to the model has `say_as` respellings applied; the script keeps the real
# spelling, and `heard_as` lists what Whisper may write for a name. A line Whisper mishears is
# printed with MISHEARD: make it again with another SEED_OFFSET, or re-word it in the script.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
script="$here/script.json"
voices="$root/target/demo-video/voices"
out="$root/target/demo-video/stems"
TTS=${TTS:-http://127.0.0.1:7500}
STT=${STT:-http://127.0.0.1:8181}
mkdir -p "$out/raw"
PHONE="aresample=8000,highpass=f=300:poles=2,lowpass=f=3400:poles=2,acompressor=threshold=-24dB:ratio=6:attack=5:release=80:makeup=6dB,aresample=48000"

# Every line: id, who, text.
lines=$(jq -c '[.intro[], .outro, (.moments[].steps[] | (., (.background // [])[]) | select(.line) | {id: .line, who, text})] | .[]' "$script")
want=("$@")
[ -f "$out/check.tsv" ] || : > "$out/check.tsv"

. "$here/words.sh"

while IFS= read -r l; do
  id=$(jq -r .id <<<"$l"); who=$(jq -r .who <<<"$l"); text=$(jq -r .text <<<"$l")
  if [ ${#want[@]} -gt 0 ]; then
    printf '%s\n' "${want[@]}" | grep -qx "$id" || continue
  elif [ -f "$out/$id.wav" ]; then
    continue
  fi
  seed=$(jq -r --arg w "$who" --arg i "$id" --argjson o "${SEED_OFFSET:-0}" \
    'if env.SEED_OFFSET then .cast[$w].seed + $o else (.reseeded[$i] // .cast[$w].seed) end' "$script")
  effect=$(jq -r --arg w "$who" '.cast[$w].effect // ""' "$script")
  said=$(jq -rn --arg t "$text" --slurpfile s "$script" '$s[0].say_as as $m | $t | reduce ($m | to_entries[]) as $e (.; gsub("\\b" + $e.key + "\\b"; $e.value))')
  jq -n --arg text "$said" --rawfile ref_text "$voices/$who.txt" --rawfile audio_base64 <(base64 -w0 "$voices/$who.wav") \
        --argjson seed "$seed" '{text: $text, audio_base64: $audio_base64, reference_text: $ref_text, language: "en", seed: $seed}' \
    | curl -fsS --max-time 300 -H 'Content-Type: application/json' --data-binary @- "$TTS/synthesize" \
    | jq -r .audio_base64 | base64 -d > "$out/raw/$id.tmp.wav"
  ffmpeg -v error -y -i "$out/raw/$id.tmp.wav" -ac 2 -ar 48000 "$out/raw/$id.wav"; rm -f "$out/raw/$id.tmp.wav"
  if [ "$effect" = phone ]; then
    ffmpeg -v error -y -i "$out/raw/$id.wav" -f lavfi -i "anoisesrc=color=pink:amplitude=0.004:r=48000" \
      -filter_complex "[0:a]$PHONE,aformat=channel_layouts=stereo[v];[1:a]highpass=f=300,lowpass=f=3400,aformat=channel_layouts=stereo[n];[v][n]amix=inputs=2:duration=first:normalize=0,loudnorm=I=-23:TP=-2:LRA=11" \
      -ar 48000 -ac 2 "$out/$id.wav"
  else
    ffmpeg -v error -y -i "$out/raw/$id.wav" -af "loudnorm=I=-23:TP=-2:LRA=11" -ar 48000 -ac 2 "$out/$id.wav"
  fi
  heard=$(curl -fsS --max-time 120 -F "file=@$out/$id.wav" -F response_format=json -F temperature=0 "$STT/v1/audio/transcriptions" | jq -r .text | tr '\n' ' ' | sed 's/^ *//; s/ *$//')
  a=$(norm "$text"); b=$(norm "$heard")
  d=$(distance "$a" "$b")
  dur=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$out/$id.wav")
  mark=ok; [ "$d" -gt 0 ] && mark=MISHEARD
  printf '%-14s %-7s %5.2fs %-8s %s\n' "$id" "$who" "$dur" "$mark$( [ "$d" -gt 0 ] && echo " ($d)")" "$heard"
  grep -v "^$id	" "$out/check.tsv" > "$out/check.tsv.new" || true
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$id" "$who" "$seed" "$dur" "$d" "$heard" >> "$out/check.tsv.new"
  mv "$out/check.tsv.new" "$out/check.tsv"
done <<<"$lines"
