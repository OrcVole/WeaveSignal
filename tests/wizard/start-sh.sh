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
  eval "$(sed -n "/^chat_public=/p;/^control_public=/p;/^branding_public=/p" "$start")"
  eval "$(sed -n '/^cat > "\$run\/nginx.conf" <<NGINX$/,/^NGINX$/p' "$start" | sed '1s/.*/cat <<NGINX/')" )
public=$(sed -n '/listen 8003;/,$p' <<<"$conf")
studio=$(sed -n '/listen 8000 default_server;/,/listen 8001;/p' <<<"$conf")
grep -q 'location ~ \^/api/v1/branding(/icon|/banner)?\$ {' <<<"$public" || fail "the public domain does not pass the branding routes"
sed -n '/location ~ \^\/api\/v1\/branding/,/}/p' <<<"$public" | grep -q 'limit_except GET { deny all; }' || fail "the public domain lets the branding routes be written"
pass "the public domain passes the three branding routes, for reading only"
grep -q 'location ~ \^/setup(/|\$) { return 404; }' <<<"$public" || fail "the public domain serves the setup page"
grep -q 'location /api/ {  return 404; }' <<<"$public" || fail "the public domain's API fallback is gone"
pass "the public domain answers 404 for /setup/ (and, as for every other route, for /api/v1/setup)"
re='^/api/v1/branding(/icon|/banner)?$'
for p in /api/v1/branding /api/v1/branding/icon /api/v1/branding/banner; do [[ $p =~ $re ]] || fail "$p is not passed"; done
for p in /api/v1/branding/accent /api/v1/setup /api/v1/setup/finish /api/v1/branding/icon/x /api/v1/brandingx; do [[ $p =~ $re ]] && fail "$p is passed on the public domain"; done
pass "nothing but the three branding routes matches"
grep -q 'location /api/ {' <<<"$studio" && grep -q 'try_files' <<<"$studio" || fail "the studio domain lost its routes"
grep -q '/setup' <<<"$studio" && fail "the studio domain blocks the setup page"
pass "the app's own domain serves the setup page and its routes"
grep -q '^overrides_file = "\$setup_env"$' "$start" && grep -q '^stream_auth = \$stream_auth$' "$start" || fail "control.toml is not given [setup]"
pass "control.toml is given the overrides file and what is in force"
echo "start.sh: the setup page's part is sound"
