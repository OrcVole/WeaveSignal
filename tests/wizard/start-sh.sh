#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The Cloudron start script's part in the setup page (ADR-0237), without an image:
#
#   1. the block that reads /app/data/setup.env, run by itself against hostile files: it takes
#      the two keys it knows when they pass their patterns, ignores everything else, and never
#      runs a line of the file;
#   2. the nginx configuration it writes: the public domain passes the three branding routes
#      for reading only and answers 404 for the setup page and its routes.
#
# The front page (1.1) is checked in the same configuration: the app's own domain's `/` asks the
# control plane, the public domain's does not.
#
# `just wizard-check` runs it. The package's own test (deploy/cloudron/test-local.sh) runs the
# real image.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
start="$root/deploy/cloudron/start.sh"
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
fail() { echo "FAIL  $*" >&2; exit 1; }
pass() { echo "pass  $*"; }

# --- 1. the setup.env block ---------------------------------------------------------------
block=$(sed -n '/^# --- The station.s setup page (ADR-0237)/,/^stream_auth=/p' "$start")
[ -n "$block" ] || fail "the setup.env block was not found in start.sh"
grep -Eq '(^|[^a-z_])(source|eval)[[:space:]]|^[[:space:]]*\.[[:space:]]' <<<"$block" && fail "the block sources or evaluates something"
read_setup() { # the file's text; prints "AUTH PASSWORD" as the block leaves them
  printf '%s' "$1" > "$work/setup.env"
  ( set -euo pipefail
    data=$work; log() { echo "log: $*" >&2; }
    LISTENERS_AUTH=off; LISTENERS_PASSWORD=from-secrets-env
    eval "$block"
    echo "$LISTENERS_AUTH $LISTENERS_PASSWORD $stream_auth" ) 2>"$work/log"
}
expect() { # name, file text, expected
  local got; got=$(read_setup "$2")
  [ "$got" = "$3" ] || fail "$1: got '$got', expected '$3'"
  pass "$1"
}
expect "what the page writes is taken" $'# a comment\nLISTENERS_AUTH=on\nLISTENERS_PASSWORD=kettle-fm-2026\n' "on kettle-fm-2026 true"
expect "off, with the password kept for later" $'LISTENERS_AUTH=off\nLISTENERS_PASSWORD=kettle-fm-2026\n' "off kettle-fm-2026 false"
expect "auth alone keeps the password of secrets.env" $'LISTENERS_AUTH=on\n' "on from-secrets-env true"
expect "an empty file changes nothing" '' "off from-secrets-env false"
expect "the last of two lines wins" $'LISTENERS_AUTH=on\nLISTENERS_AUTH=off\n' "off from-secrets-env false"
rm -f "$work/pwned"
hostile="LISTENERS_AUTH=on; touch $work/pwned
LISTENERS_PASSWORD=\$(touch $work/pwned)
touch $work/pwned
STATION_NAME=\"taken over\"
CONTROL_ADMIN_TOKEN=known
\`touch $work/pwned\`
"
expect "a hostile file is read as text: bad values ignored, other keys ignored" "$hostile" "off from-secrets-env false"
[ ! -e "$work/pwned" ] || fail "a line of setup.env was run"
pass "no line of setup.env is run"
grep -q 'LISTENERS_AUTH is not on or off' "$work/log" && grep -q 'LISTENERS_PASSWORD is not' "$work/log" || fail "an ignored value is not logged"
pass "an ignored value is said in the log, by its key only"
grep -q 'touch' "$work/log" && fail "the log repeats a refused value"
for bad in 'short' 'has space1' 'semi;colon12' 'quote"12345' "tick\`1234567" 'colon:12345' 'dollar$12345' "$(printf 'x%.0s' {1..65})"; do
  got=$(read_setup "LISTENERS_AUTH=on"$'\n'"LISTENERS_PASSWORD=$bad"$'\n')
  [ "$got" = "on from-secrets-env true" ] || fail "the password '$bad' was taken: $got"
done
pass "a password outside the pattern is never taken"
if [ -f "$work/setup.env" ]; then rm -f "$work/setup.env"; fi
got=$( set -euo pipefail; data=$work; log() { :; }; LISTENERS_AUTH=on; LISTENERS_PASSWORD=p; eval "$block"; echo "$LISTENERS_AUTH $stream_auth" )
[ "$got" = "on true" ] || fail "without a setup.env, weavesignal.env decides: $got"
pass "without a setup.env, weavesignal.env decides"

# --- 1b. STUN servers from Settings (ADR-0290) -------------------------------------------
stun_block=$(sed -n '/^# --- STUN servers from Settings (ADR-0290)/,/^fi$/p' "$start")
[ -n "$stun_block" ] || fail "the STUN block was not found in start.sh"
grep -Eq '(^|[^a-z_])(source|eval)[[:space:]]|^[[:space:]]*\.[[:space:]]' <<<"$stun_block" && fail "the STUN block sources or evaluates something"
read_stun() { # the file's text (or - for no file); prints STUN_SERVERS as the block leaves it
  rm -f "$work/setup.env"
  [ "$1" = - ] || printf '%s' "$1" > "$work/setup.env"
  ( set -euo pipefail
    data=$work; log() { echo "log: $*" >&2; }
    STUN_SERVERS=from.weavesignal.env:3478
    eval "$stun_block"
    echo "[$STUN_SERVERS]" ) 2>"$work/log"
}
expect_stun() { # name, file text, expected
  local got; got=$(read_stun "$2")
  [ "$got" = "$3" ] || fail "$1: got '$got', expected '$3'"
  pass "$1"
}
expect_stun "without a setup.env, weavesignal.env decides" - "[from.weavesignal.env:3478]"
expect_stun "a setup.env without the key changes nothing" $'LISTENERS_AUTH=on\n' "[from.weavesignal.env:3478]"
expect_stun "what Settings writes is taken" $'LISTENERS_AUTH=on\nSTUN_SERVERS=stun.nextcloud.com:443,192.0.2.10:3478\n' "[stun.nextcloud.com:443,192.0.2.10:3478]"
expect_stun "none is taken as none" $'STUN_SERVERS=\n' "[]"
expect_stun "the last of two lines wins" $'STUN_SERVERS=a.example:3478\nSTUN_SERVERS=b.example:3478\n' "[b.example:3478]"
rm -f "$work/pwned"
for bad in 'stun:a.example:3478' 'A.example:3478' 'a.example' 'a.example:0' 'a.example:65536' 'a:1,b:2,c:3,d:4,e:5' \
  "\$(touch $work/pwned):3478" "a.example:3478;touch $work/pwned" 'a.example:3478 b.example:3478' '-a.example:3478'; do
  got=$(read_stun "STUN_SERVERS=$bad"$'\n')
  [ "$got" = "[from.weavesignal.env:3478]" ] || fail "the STUN line '$bad' was taken: $got"
  grep -q 'STUN_SERVERS is not' "$work/log" || fail "the ignored STUN line '$bad' was not logged"
done
[ ! -e "$work/pwned" ] || fail "a STUN line of setup.env was run"
pass "a STUN line outside the pattern is ignored, said in the log, and never run"
rm -f "$work/setup.env"

# --- 2. the nginx configuration ---------------------------------------------------------------
# The heredoc as start.sh writes it, with the variables it uses given plain values.
conf=$( set -euo pipefail
  run=/run/weavesignal CONTROL_PORT=7310 CHAT_PORT=7311 LISTENERS_PORT=7312 LIVEKIT_HTTP_PORT=7880 SOURCE_BRIDGE_PORT=7320
  stream_locations='' listener_root='' pub_noindex='' pub_never='' pub_robots='x'
  eval "$(sed -n "/^chat_public=/p;/^control_public=/p;/^branding_public=/p;/^monitor_public=/p;/^calendar_public=/p;/^whip_public=/p" "$start")"
  eval "$(sed -n '/^cat > "\$run\/nginx.conf" <<NGINX$/,/^NGINX$/p' "$start" | sed '1s/.*/cat <<NGINX/')" )
public=$(sed -n '/listen 8003;/,$p' <<<"$conf")
studio=$(sed -n '/listen 8000 default_server;/,/listen 8001;/p' <<<"$conf")
grep -q 'location ~ \^/api/v1/branding(/icon|/banner)?\$ {' <<<"$public" || fail "the public domain does not pass the branding routes"
sed -n '/location ~ \^\/api\/v1\/branding/,/}/p' <<<"$public" | grep -q 'limit_except GET { deny all; }' || fail "the public domain lets the branding routes be written"
pass "the public domain passes the three branding routes, for reading only"
# Calendar files (ADR-0281) and the read-only JSON for a station's own website (ADR-0282), for
# reading only, and nothing else under /api/v1/public/.
grep -q 'location ~ \^/api/v1/public/(station|schedule|episodes|schedule\\.ics|shows/\[a-z0-9-\]+\\.ics)\$ {' <<<"$public" || fail "the public domain does not pass the calendar files and the public JSON routes"
sed -n '/location ~ \^\/api\/v1\/public\//,/}/p' <<<"$public" | grep -q 'limit_except GET { deny all; }' || fail "the public domain lets the public routes be written"
re='^/api/v1/public/(station|schedule|episodes|schedule\.ics|shows/[a-z0-9-]+\.ics)$'
for p in /api/v1/public/station /api/v1/public/schedule /api/v1/public/episodes /api/v1/public/schedule.ics /api/v1/public/shows/the-late-kettle.ics; do [[ $p =~ $re ]] || fail "$p is not passed"; done
for p in /api/v1/public/shows/x /api/v1/public/shows/a/b.ics /api/v1/public/scheduleXics /api/v1/public/stations /api/v1/schedule/import; do [[ $p =~ $re ]] && fail "$p is passed on the public domain"; done
pass "the public domain passes the calendar files and the three public JSON routes, for reading only, and nothing else of /api/v1/public/"
grep -q 'location ~ \^/setup(/|\$) { return 404; }' <<<"$public" || fail "the public domain serves the setup page"
grep -q 'location /api/ {  return 404; }' <<<"$public" || fail "the public domain's API fallback is gone"
pass "the public domain answers 404 for /setup/ (and, as for every other route, for /api/v1/setup)"
re='^/api/v1/branding(/icon|/banner)?$'
for p in /api/v1/branding /api/v1/branding/icon /api/v1/branding/banner; do [[ $p =~ $re ]] || fail "$p is not passed"; done
for p in /api/v1/branding/accent /api/v1/setup /api/v1/setup/finish /api/v1/branding/icon/x /api/v1/brandingx; do [[ $p =~ $re ]] && fail "$p is passed on the public domain"; done
pass "nothing but the three branding routes matches"
# The monitor link's routes (ADR-0280): passed without Cloudron's login, the bearer being the
# link's own secret, with the client's address and the host restated (a location that sets one
# proxy_set_header sends none of the server's).
grep -q 'location ~ \^/api/v1/monitor(/(token|events|go|not-yet|cue|handover|end-show|dump))?\$ {' <<<"$public" || fail "the public domain does not pass the monitor link's routes"
blk=$(sed -n '/location ~ \^\/api\/v1\/monitor/,/^        }/p' <<<"$public")
grep -q 'proxy_set_header X-Forwarded-For \$remote_addr;' <<<"$blk" && grep -q 'proxy_set_header Host \$host;' <<<"$blk" || fail "the monitor link's location does not restate the client's address and the host"
grep -q 'limit_except GET POST { deny all; }' <<<"$blk" || fail "the monitor link's routes take more than GET and POST"
re='^/api/v1/monitor(/(token|events|go|not-yet|cue|handover|end-show|dump))?$'
for p in /api/v1/monitor /api/v1/monitor/token /api/v1/monitor/events /api/v1/monitor/go /api/v1/monitor/not-yet /api/v1/monitor/cue /api/v1/monitor/handover /api/v1/monitor/end-show /api/v1/monitor/dump; do [[ $p =~ $re ]] || fail "$p is not passed"; done
for p in /api/v1/monitor/hold /api/v1/monitor/go/x /api/v1/monitorx /api/v1/monitor/token/x /api/v1/internal/sources/admit; do [[ $p =~ $re ]] && fail "$p is passed on the public domain"; done
grep -q 'location ~ \^/(join|echo-test|monitor)(/|\$) {' <<<"$public" || fail "the public domain does not serve the monitor page"
grep -q 'location /api/v1/internal/ { return 404; }' <<<"$studio" || fail "the internal routes are not closed on the studio domain"
pass "the public domain passes the monitor link's routes (view, token, events, the six controls) and its page, restating the client's address; nothing else under /api/v1/monitor"
# The front page (1.1): the app's own domain asks the control plane whether the station is set up,
# and keeps the player when the answer is 404 or there is none; the public domain never asks and
# never serves the notice page.
blk=$(sed -n '/location = \/ {/,/^        }/p' <<<"$studio")
grep -q 'proxy_pass http://127.0.0.1:7310/api/v1/front;' <<<"$blk" || fail "the studio domain's / does not ask /api/v1/front"
grep -q 'proxy_intercept_errors on;' <<<"$blk" && grep -q 'error_page 404 502 503 504 = @player;' <<<"$blk" || fail "the studio domain's / keeps no player for a 404 or a silent control plane"
grep -q 'location @player { expires epoch; try_files /index.html =404; }' <<<"$studio" || fail "the studio domain has no player to fall back to"
grep -q '/api/v1/front' <<<"$public" && fail "the public domain asks whether the station is set up"
grep -q 'location = / {' <<<"$public" && fail "the public domain's / is not the plain player"
grep -q 'location ~ \^/not-set-up(/|\$) { return 404; }' <<<"$public" || fail "the public domain serves the not-set-up page"
grep -q 'not-set-up' <<<"$studio" && grep -q 'location ~ \^/not-set-up' <<<"$studio" && fail "the studio domain blocks the not-set-up page"
pass "the app's own domain's / asks the control plane (the player stays on a 404 or no answer); the public domain's / is the player and its /not-set-up/ is 404"
grep -q 'location /api/ {' <<<"$studio" && grep -q 'try_files' <<<"$studio" || fail "the studio domain lost its routes"
grep -q '/setup' <<<"$studio" && fail "the studio domain blocks the setup page"
pass "the app's own domain serves the setup page and its routes"
grep -q '^overrides_file = "\$setup_env"$' "$start" && grep -q '^stream_auth = \$stream_auth$' "$start" || fail "control.toml is not given [setup]"
pass "control.toml is given the overrides file and what is in force"

# --- 3. the consent words (issue #26) -----------------------------------------------------
# weavesignal.env is sourced by bash, so the words are written as its comment shows (double
# quotes, \" and \$); the block turns them into a [join] line that a TOML parser reads whole.
block=$(sed -n '/^# --- The consent words (issue #26) ---$/,/^fi$/p' "$start")
[ -n "$block" ] || fail "the consent words' block was not found in start.sh"
grep -q '^\$consent_text_line$' "$start" || fail "control.toml is not given the consent words' line"
consent_toml() { # weavesignal.env's text; prints what [join] holds as JSON, or "refused"
  printf '%s\n' "$1" > "$work/weavesignal.env"
  ( set -euo pipefail
    log() { echo "log: $*" >&2; }
    CONSENT_TEXT=
    # shellcheck source=/dev/null
    source "$work/weavesignal.env"
    eval "$block"
    printf '[join]\n%s\n' "$consent_text_line" ) 2>"$work/log" > "$work/control.toml" || { echo refused; return; }
  python3 -I -c 'import json,sys,tomllib; print(json.dumps(tomllib.load(open(sys.argv[1], "rb"))["join"]))' "$work/control.toml"
}
want='You'"'"'ll be on {presenter}'"'"'s show, "live", on {station}; kept {kept} # all, $5, 100% and a \ back. Ask {contact}.'
got=$(consent_toml 'CONSENT_TEXT="You'"'"'ll be on {presenter}'"'"'s show, \"live\", on {station}; kept {kept} # all, \$5, 100% and a \\ back. Ask {contact}."')
[ "$got" = "$(python3 -I -c 'import json,sys; print(json.dumps({"consent_text": sys.argv[1]}))' "$want")" ] || fail "the consent words reach control.toml as $got"
[ "$(consent_toml 'CONSENT_TEXT=')" = '{}' ] || fail "an empty CONSENT_TEXT does not leave the default words"
[ "$(consent_toml 'CONSENT_TEXT="   "')" = '{}' ] || fail "a blank CONSENT_TEXT does not leave the default words"
[ "$(consent_toml 'CONSENT_TEXT="two
lines"')" = '{"consent_text": "two\nlines"}' ] || fail "a line break in CONSENT_TEXT is not kept"
[ "$(consent_toml "CONSENT_TEXT=\$'a\\x01b'")" = refused ] || fail "a control character in CONSENT_TEXT does not stop the start"
pass "CONSENT_TEXT reaches control.toml whole (apostrophes, quotes, commas, braces, \$, %, a backslash, a line break); empty or blank leaves the default words"

# --- 4. the install wizard's Cloudron file (17j): the consent words and the STUN servers --------
# The generator's own lines, sourced by bash as start.sh sources weavesignal.env: hostile words
# arrive whole and nothing in them runs; the STUN list passes start.sh's own check.
rm -f "$work/pwned"
hostile_words='You'"'"'ll be on {presenter}'"'"'s show, "live", on {station} # all; $5, 100%, a \ back, `touch '"$work"'/pwned` $(touch '"$work"'/pwned) ${HOME}. Ask {contact}.'
wizard_file=$(WORDS="$hostile_words" node --no-warnings --input-type=module -e "
  import { defaults, generate } from '$root/packages/wizard/generate.ts';
  const g = generate({ ...defaults('cloudron'), name: 'Radio Ceilidh', contact: 'calls@station.example', studioDomain: 'studio.station.example', publicDomain: 'station.example', listenDomain: 'listen.station.example', roomDomain: 'room.station.example', consentText: process.env.WORDS, stun: 'other', stunOther: 'STUN.example.org:3478, 192.0.2.7:3478' });
  if (!g.ok) { console.error(JSON.stringify(g.problems)); process.exit(1); }
  process.stdout.write(g.output.file.text);") || fail "the generator refused the Cloudron answers"
got=$(consent_toml "$wizard_file")
[ "$got" = "$(python3 -I -c 'import json,sys; print(json.dumps({"consent_text": sys.argv[1]}))' "$hostile_words")" ] || fail "the wizard's consent words reach control.toml as $got"
[ ! -e "$work/pwned" ] || fail "the wizard's consent words were run by the shell"
stun_block=$(sed -n '/^stun_servers=$/,/^(( \${#stun_commas} < 4 ))/p' "$start")
[ -n "$stun_block" ] || fail "the STUN block was not found in start.sh"
stun_got=$( set -euo pipefail; log() { echo "log: $*" >&2; }; STUN_SERVERS=; source "$work/weavesignal.env"; eval "$stun_block"; echo "$stun_servers" ) || fail "start.sh refused the wizard's STUN servers"
[ "$stun_got" = "stun.example.org:3478,192.0.2.7:3478" ] || fail "the wizard's STUN servers reach start.sh as $stun_got"
pass "the wizard's Cloudron file: hostile consent words reach control.toml whole and run nothing; its STUN servers pass start.sh's check"

# --- 5. video's encoder (17e): a Cloudron app has no GPU, so SVT-AV1 is the default -------------
grep -q '^setting VIDEO_ENCODER svt "Video.s AV1 encoder (with VIDEO=true): svt, SVT-AV1 on the CPU (the default: a Cloudron app has no GPU)' "$start" \
  || fail "start.sh does not add VIDEO_ENCODER=svt, saying a Cloudron app has no GPU"
check=$(grep '^\[\[ \$VIDEO_ENCODER == ' "$start")
[ -n "$check" ] || fail "start.sh does not check VIDEO_ENCODER"
for v in svt auto nvenc; do ( VIDEO_ENCODER=$v; log() { :; }; eval "$check" ) || fail "start.sh refuses VIDEO_ENCODER=$v"; done
for v in vaapi NVENC x264 ''; do ( VIDEO_ENCODER=$v; log() { :; }; eval "$check" ) && fail "start.sh takes VIDEO_ENCODER='$v'"; done
grep -q '^encoder = "\$VIDEO_ENCODER"$' "$start" || fail "the desk's [video] is not given VIDEO_ENCODER"
pass "VIDEO_ENCODER: svt by default (no GPU on Cloudron), auto and nvenc taken (the desk falls back), anything else stops the start; the desk is given it"
echo "start.sh: the setup page's part is sound"
