#!/usr/bin/env bash
# Synthesise every speech cue of show.json through VoiceLab's tts-serve (127.0.0.1:7500), one WAV per cue.
# usage: synth.sh [cue-index ...]   (no arguments: every speech cue not yet made)
# Voices: voices/<who>/ref.wav + ref.txt (VoiceLab persona references; VCTK speakers, two sentences joined, see README). Output: out/cues/NNN-<who>.wav, 48 kHz stereo.
# The text sent to the model has say_as respellings applied (Gaelic names); show.json keeps the real spelling.
set -euo pipefail
cd "$(dirname "$0")"
TTS=http://127.0.0.1:7500
mkdir -p out/cues

n=$(jq '.cues | length' show.json)
idx=("$@"); [[ ${#idx[@]} -gt 0 ]] || mapfile -t idx < <(seq 0 $((n - 1)))
for i in "${idx[@]}"; do
  kind=$(jq -r ".cues[$i].kind" show.json); [[ $kind == speech ]] || continue
  who=$(jq -r ".cues[$i].who" show.json)
  out=$(printf 'out/cues/%03d-%s.wav' "$i" "$who")
  [[ -f $out && $# -eq 0 ]] && continue
  # A per-cue seed in show.json's "seeds" wins (chosen where Whisper heard a name wrong), then SEED, then 11.
  seed=$(jq -r --arg i "$i" --arg d "${SEED:-11}" '.seeds[$i] // (if env.SEED then $d else .seeds.default end) // 11' show.json)
  text=$(jq -r --argjson i "$i" '.say_as as $m | .cues[$i].text
          | reduce ($m | to_entries[]) as $e (.; gsub("\\b" + $e.key + "\\b"; $e.value))' show.json)
  jq -n --arg text "$text" --rawfile ref_text "voices/$who/ref.txt" --rawfile audio_base64 <(base64 -w0 "voices/$who/ref.wav") \
        --argjson seed "$seed" \
        '{text:$text, audio_base64:$audio_base64, reference_text:$ref_text, language:"en", seed:$seed}' \
    | curl -fsS --max-time 300 -H 'Content-Type: application/json' --data-binary @- "$TTS/synthesize" \
    | jq -r .audio_base64 | base64 -d > out/cues/tmp.wav
  ffmpeg -v error -y -i out/cues/tmp.wav -ac 2 -ar 48000 "$out"
  printf '%3d %-8s %6.2fs  %s\n' "$i" "$who" "$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$out")" "${text:0:60}"
done
rm -f out/cues/tmp.wav
