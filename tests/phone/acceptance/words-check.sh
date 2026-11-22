#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The acceptance run's words check (words_match.mojo) on its own, with no station: what a caller
# heard on a real station (Alba's words, their first second missed) must match, and a ringback
# like baresip's, silence and the other voice's words played to nobody must not. Needs mojo and
# ffmpeg; no container. `just phone-words-check`.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
work=$(mktemp -d); trap 'rm -rf -- "$work"' EXIT
alba="$root/assets/phone/phone-words-alba.opus"; edinburgh="$root/assets/phone/phone-words-edinburgh.opus"
match() { mojo run "$here/words_match.mojo" "$1" --within 10 "$alba" "$edinburgh" 2>&1 | tail -1; }
fail=0
expect() { # pass|fail file what
  local m; if m=$(match "$2"); then got=pass; else got=fail; fi
  if [ "$got" = "$1" ]; then echo "pass  $3: $m"; else echo "FAIL  $3: expected $1, got $got ($m)"; fail=1; fi
}
expect pass "$here/fixtures/heard-words-alba.wav" "the words as a caller heard them"
# A UK ringback (425 Hz, 0.4 s on, 0.2 s off, 0.4 s on, 2 s off), as a SIP client plays after 180 Ringing.
ffmpeg -hide_banner -loglevel error -y -f lavfi -i "sine=f=425:d=10,volume='if(lt(mod(t,3),0.4)+between(mod(t,3),0.6,1.0),1,0)':eval=frame" -ar 8000 -ac 1 "$work/ringback.wav"
expect fail "$work/ringback.wav" "a ringback"
ffmpeg -hide_banner -loglevel error -y -f lavfi -i anullsrc=r=8000:cl=mono -t 10 "$work/silence.wav"
expect fail "$work/silence.wav" "silence"
# Edinburgh's words heard, only Alba's recording to compare with: not the words looked for.
ffmpeg -hide_banner -loglevel error -y -i "$edinburgh" -ar 8000 -ac 1 "$work/edinburgh.wav"
if m=$(mojo run "$here/words_match.mojo" "$work/edinburgh.wav" --within 10 "$alba" 2>&1 | tail -1); then
  echo "FAIL  another voice's words matched Alba's: $m"; fail=1
else
  echo "pass  another voice's words do not match Alba's: $m"
fi
[ "$fail" = 0 ] && echo "the words check tells the words from a ringback" || exit 1
