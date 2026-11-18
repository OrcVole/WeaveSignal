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

# --- 2. the nginx configuration ---------------------------------------------------------------
# The heredoc as start.sh writes it, with the variables it uses given plain values.
conf=$( set -euo pipefail
  run=/run/weavesignal CONTROL_PORT=7310 CHAT_PORT=7311 LISTENERS_PORT=7312 LIVEKIT_HTTP_PORT=7880
  stream_locations='' listener_root='' pub_noindex='' pub_never='' pub_robots='x'
  eval "$(sed -n "/^chat_public=/p;/^control_public=/p;/^branding_public=/p;/^monitor_public=/p;/^calendar_public=/p" "$start")"
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
echo "start.sh: the setup page's part is sound"
