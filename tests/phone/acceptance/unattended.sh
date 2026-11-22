#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The phone line's timed acceptance run with nobody at the phone (17f): a local SIP client
# (baresip in a Debian container, as the spike's) rings a station's SIP port, plays the timed run's
# caller as its microphone (timed.mojo --caller-wav: each answer for 10 s, then 5 s of silence),
# presses the key at the key cue, and records what it hears. The studio's acts (screen, send, on
# air, drop) go through the control plane's API at their seconds. The cue runner prints the same
# schedule beside it, so a person watching can follow.
#
#   tests/phone/acceptance/unattended.sh [--sip 127.0.0.1:5060] [--to +441314960000]
#       [--from +447700900123] [--out DIR] [--dry-run]
#
#   WEAVESIGNAL_CONTROL_URL    the control plane (http://127.0.0.1:7310); unset: no studio acts and
#                              no API checks, the call only
#   WEAVESIGNAL_CONTROL_TOKEN  an admin's bearer, from the environment only (the station key from
#                              `secret`, or a staff session); never written into a file here
#
# The station's trunk must take calls to --to from the client's address (127.0.0.1/32 for a
# station on this machine) with no digest password; --from is from Ofcom's drama range, never a
# real line. --dry-run makes the caller's microphone, prints the schedule and stops: no container.
#
# Checked afterwards (RESULT.txt in --out):
#   - the call was answered (baresip's log says Call established) and the consent words were
#     heard: what the caller heard before the key matches the shipped recording's level contour
#     (words_match.mojo; a ringback or other sound does not), or is quiet with the words off;
#   - with the API: the station's trunk is made in LiveKit before the call (GET /api/v1/phone's
#     `trunk`; 1.2.1); a phone caller (phone-…) reached the queue after the key, its card shows the
#     last digits and no name from LiveKit, and after the drop no phone caller is left;
#   - the call was ended by the studio (baresip's log says closed) within a few seconds of the drop;
#   - the From number appears in no file the control plane wrote (its log is the operator's to add).
# Runs a container: never while a soak or another stack holds the machine.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
sip=127.0.0.1:5060
to=+441314960000
from=+447700900123
out=''
dry=false
while [ $# -gt 0 ]; do
  case $1 in
    --sip) sip=$2; shift ;;
    --to) to=$2; shift ;;
    --from) from=$2; shift ;;
    --out) out=$2; shift ;;
    --dry-run) dry=true ;;
    *) echo "unattended.sh: unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
out=${out:-$(mktemp -d "${TMPDIR:-/tmp}/ws-phone-accept-XXXXXX")}
mkdir -p "$out"
CALLER_IMG=${CALLER_IMG:-docker.io/library/debian:13-slim}
P=ws-phone-accept-$$
note() { printf '%s  %s\n' "$(date -u +%H:%M:%S)" "$*" | tee -a "$out/RESULT.txt"; }
fail=0
check() { if [ "$1" = ok ]; then note "PASS  $2"; else note "FAIL  $2"; fail=1; fi; }

# The caller's microphone and the schedule, from the same file the operator's run reads.
mojo run "$here/timed.mojo" "$here/timed.json" --route sip --caller-wav "$out/caller.wav" >/dev/null
mojo run "$here/timed.mojo" "$here/timed.json" --route sip --schedule > "$out/schedule.tsv"
at() { awk -F'\t' -v k="$1" '$2 == k { print int($1); exit }' "$out/schedule.tsv"; }
key_at=$(at key); key=$(awk -F'\t' '$2 == "key" { print $4; exit }' "$out/schedule.tsv")
drop_at=$(at drop); done_at=$(at "done")
note "schedule: key $key at ${key_at}s, drop at ${drop_at}s, end at ${done_at}s; microphone $out/caller.wav"
if [ "$dry" = true ]; then note "dry run: no call made"; exit 0; fi

api() { # method path [json]
  curl -fsS -X "$1" -H "Authorization: Bearer $WEAVESIGNAL_CONTROL_TOKEN" \
    ${3:+-H 'Content-Type: application/json' --data "$3"} "$WEAVESIGNAL_CONTROL_URL$2"
}
phone_caller() { # the first phone caller in the studio's view, as `identity<TAB>card`
  api GET /api/v1/studio | python3 -I -c '
import json, sys
for c in json.load(sys.stdin).get("callers", []):
    if c.get("identity", "").startswith("phone-"):
        print(c["identity"] + "\t" + ((c.get("phone") or {}).get("number") or "") + "\t" + json.dumps(c.get("name", "")))
        break'
}
with_api=false
[ -n "${WEAVESIGNAL_CONTROL_URL:-}" ] && [ -n "${WEAVESIGNAL_CONTROL_TOKEN:-}" ] && with_api=true
[ "$with_api" = true ] || note "no WEAVESIGNAL_CONTROL_URL and token: the call only, no studio acts or API checks"
# The station makes its own trunk and dispatch rule in LiveKit (1.2.1); until it has, LiveKit
# refuses every call with a 404. Wait for it rather than ring too soon.
if [ "$with_api" = true ]; then
  trunk=''
  for _ in $(seq 1 60); do
    trunk=$(api GET /api/v1/phone 2>/dev/null | python3 -I -c 'import json,sys; t=json.load(sys.stdin).get("trunk") or {}; print(t.get("state","none"))' || true)
    [ "$trunk" = made ] && break
    sleep 2
  done
  [ "$trunk" = made ] && check ok "the station's trunk is made in LiveKit" \
    || check no "the station's trunk is not made (trunk: ${trunk:-no answer}); the call is made anyway"
fi
# Which words the caller should hear: the shipped recordings (default), the station's own, or none.
words_kind=default
if [ "$with_api" = true ]; then
  words_kind=$(api GET /api/v1/phone 2>/dev/null | python3 -I -c 'import json,sys; print((json.load(sys.stdin).get("words") or {}).get("kind","default"))' || echo default)
fi

cleanup() { exec 8>&- 2>/dev/null || true; podman rm -f "$P" >/dev/null 2>&1 || true; }
trap cleanup EXIT
mkdir -p "$out/baresip"; rm -f "$out/caller.in"; mkfifo "$out/caller.in"
cat > "$out/baresip/config" <<CFG
sip_listen 127.0.0.1:${CALLER_SIP_PORT:-25062}
audio_source aufile,/w/caller.wav
audio_player aufile,/w/heard.wav
module_path /usr/lib/baresip/modules
module stdio.so
module g711.so
module aufile.so
module_app account.so
module_app menu.so
CFG
echo "<sip:$from@127.0.0.1>;regint=0;audio_codecs=PCMU,PCMA" > "$out/baresip/accounts"
nice -n 10 podman run -d --name "$P" --network host -i -v "$out:/w:z" "$CALLER_IMG" sh -c \
  "apt-get -qq update >/dev/null && DEBIAN_FRONTEND=noninteractive apt-get -qq install -y baresip-core >/dev/null && \
   exec baresip -f /w/baresip < /w/caller.in" >/dev/null
exec 8<>"$out/caller.in"
for _ in $(seq 1 180); do podman logs "$P" 2>&1 | grep -qi "baresip is ready\|ua: " && break; sleep 1; done
sleep 2

# T0: the call. Then each act at its second.
t0=$(date +%s)
mojo run "$here/timed.mojo" "$here/timed.json" --route sip --t0 "$t0" > "$out/cues.txt" 2>&1 &
cues=$!
note "T0: dialling sip:$to@$sip"
echo "/dial sip:$to@$sip" >&8
wait_until() { local s=$(( t0 + $1 - $(date +%s) )); [ "$s" -le 0 ] || sleep "$s"; }
wait_until "$key_at"
answered=false
podman logs "$P" 2>&1 | grep -q "Call established" && answered=true
[ "$answered" = true ] && check ok "the call was answered by ${key_at}s" \
  || check no "the call was not answered by ${key_at}s"
echo "$key" >&8
note "pressed $key"
caller=''
if [ "$with_api" = true ]; then
  wait_until 16
  row=$(phone_caller || true)
  caller=${row%%$'\t'*}
  if [ -n "$caller" ]; then
    check ok "a phone caller is in the queue: $caller"
    card=$(cut -f2 <<<"$row"); name=$(cut -f3 <<<"$row")
    # The card says `···NNNN` (CallerView.phone.number): the last four digits only.
    [ "$card" = "···${from: -4}" ] && check ok "its card shows the last four digits" || check no "its card: $card"
    [ "$name" = '""' ] && check ok "no name from LiveKit" || check no "a name came with the call: $name"
  else
    check no "no phone caller in the queue after the key"
  fi
fi
act() { # second op path json
  wait_until "$1"
  [ "$with_api" = true ] && [ -n "$caller" ] || return 0
  api POST "$2" "$3" >/dev/null && note "act at ${1}s: $3" || check no "act at ${1}s refused: $3"
}
act "$(at act)" /api/v1/screener/commands "{\"op\": \"screen\", \"identity\": \"$caller\"}"
act "$(at send)" /api/v1/screener/commands "{\"op\": \"send\", \"identity\": \"$caller\"}"
act "$(at on_air)" /api/v1/desk/commands "{\"op\": \"on_air\", \"identity\": \"$caller\", \"on\": true}"
act "$drop_at" /api/v1/screener/commands "{\"op\": \"drop\", \"identity\": \"$caller\"}"
wait_until $(( drop_at + 6 ))
if [ "$with_api" = true ]; then
  podman logs "$P" 2>&1 | grep -qiE "closed|terminated|bye" && check ok "the studio's drop ended the call" \
    || check no "the call was still up 6 s after the drop"
  [ -z "$(phone_caller || true)" ] && check ok "no phone caller left in the queue" || check no "a phone caller is left after the drop"
fi
wait_until "$done_at"
echo "/hangup" >&8 || true
sleep 2
kill "$cues" 2>/dev/null || true
podman logs "$P" > "$out/caller.log" 2>&1 || true

# What the caller heard before the key. Any sound is not enough: baresip plays its own ringback
# after 180 Ringing, and on a call LiveKit refused that passed as the words (the real-kit tries,
# 2026-11-21). The call must have been answered, and what was heard must match the words'
# recording (words_match.mojo, the level contour against the shipped voices).
if [ ! -s "$out/heard.wav" ]; then
  check no "the caller recorded nothing"
elif [ "$answered" != true ]; then
  check no "the words: the call was never answered, so none were heard (a ringback is not the words)"
else
  lead=$(ffmpeg -hide_banner -t "$key_at" -i "$out/heard.wav" -af volumedetect -f null - 2>&1 | sed -n 's/.*mean_volume: \([-0-9.]*\) dB/\1/p')
  note "what the caller heard before the key: mean ${lead:-?} dB"
  root=$(cd "$here/../../.." && pwd)
  case $words_kind in
    off)
      awk -v v="${lead:--99}" 'BEGIN { exit !(v < -50) }' && check ok "no words were played, as the line is set" \
        || check no "the words are off, yet the caller heard sound before the key (${lead} dB)" ;;
    station)
      # The station's own recording is not in this repository: by level only, and said so.
      awk -v v="${lead:--99}" 'BEGIN { exit !(v > -50) }' && note "CHECK the station's own words: sound was heard before the key (by level only; listen to heard.wav)" \
        || check no "the station's own words were not heard before the key" ;;
    *)
      if m=$(mojo run "$here/words_match.mojo" "$out/heard.wav" --within "$key_at" \
          "$root/assets/phone/phone-words-alba.opus" "$root/assets/phone/phone-words-edinburgh.opus" 2>&1); then
        check ok "the consent words were heard before the key ($(tail -1 <<<"$m" | awk '{ n = split($3, p, "/"); printf "%s, %.2f", p[n], $2 }'))"
      else
        check no "what the caller heard before the key is not the consent words ($(tail -1 <<<"$m"))"
      fi ;;
  esac
fi
digits=${from#+}
if grep -rqs -- "$digits" "$out"/*.txt "$out"/*.tsv 2>/dev/null; then check no "the number is in a file of this run"; fi
note "files: $out (caller.wav, heard.wav, caller.log, cues.txt)"
[ "$fail" = 0 ] && note "every check passed" || note "some checks failed"
exit "$fail"
