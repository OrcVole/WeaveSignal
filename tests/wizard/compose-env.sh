#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The install wizard's Compose settings file through the real installer (ADR-0237): the file
# the generator writes (packages/wizard/generate.ts) is given to weavesignal.sh --no-start, with
# the engines and `ss` stubbed as tests/install/public-check.sh stubs them, and what comes out
# must keep every answer, gain the secrets, and stay the same on a second run. Then, where
# `podman compose` or `docker compose` is here, the real `compose config` renders the bundle
# with it, and the name must reach the control plane's settings whole (an apostrophe included).
#
# Needs Node 22.18 or later (it runs the generator's TypeScript as it is). `just wizard-check`.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
script="$root/deploy/compose/weavesignal.sh"
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
fail() { echo "FAIL  $*" >&2; exit 1; }
pass() { echo "pass  $*"; }
mkdir -p "$work/bin"
for engine in podman docker; do printf '#!/usr/bin/env bash\nexit 0\n' > "$work/bin/$engine"; chmod +x "$work/bin/$engine"; done
printf '#!/usr/bin/env bash\nexit 0\n' > "$work/bin/ss"; chmod +x "$work/bin/ss"
echo 80 > "$work/unpriv"

generate() { # route, extra answers as a JS object literal
  node --no-warnings --input-type=module -e "
    import { defaults, generate } from '$root/packages/wizard/generate.ts';
    const g = generate({ ...defaults('$1'), name: \"Paddy's Wireless\", contact: 'calls@station.example', domain: 'station.example', ...$2 });
    if (!g.ok) { console.error(JSON.stringify(g.problems)); process.exit(1); }
    process.stdout.write(g.output.file.text);"
}
value() { sed -n "s/^$2=//p" "$1" | head -1; }

for route in compose quadlets; do
  env="$work/$route.env"
  generate "$route" "{ video: true, searchListing: true, acmeEmail: 'ops@station.example', nodeIp: '203.0.113.7', mediaPort: 52000, turnUdpPort: 3478, mail: 'outbox', mailFrom: 'chat@station.example' }" > "$env"
  out=$(PATH="$work/bin:$PATH" WEAVESIGNAL_UNPRIV_PORT_FILE="$work/unpriv" "$script" --engine podman --no-start --env-file "$env" --public station.example 2>&1) || fail "$route: weavesignal.sh refused the wizard's file: $out"
  grep -q 'Settings written' <<<"$out" || fail "$route: no settings written: $out"
  [ "$(value "$env" STATION_NAME)" = "Paddy's Wireless" ] || fail "$route: the name was changed: $(value "$env" STATION_NAME)"
  [ "$(value "$env" CONSENT_CONTACT)" = calls@station.example ] || fail "$route: the contact was lost"
  [ "$(value "$env" MAIL_MODE)" = outbox ] && [ "$(value "$env" MAIL_FROM)" = chat@station.example ] || fail "$route: mail was lost"
  [ "$(value "$env" SEARCH_LISTING)" = true ] && [ "$(value "$env" DIRECTORY_LISTING)" = false ] || fail "$route: listing was lost"
  [ "$(value "$env" VIDEO)" = true ] || fail "$route: VIDEO was dropped"
  [ "$(value "$env" ACME_EMAIL)" = ops@station.example ] && [ "$(value "$env" NODE_IP)" = 203.0.113.7 ] || fail "$route: the public settings were lost"
  [ "$(value "$env" LIVEKIT_MEDIA_PORT)" = 52000 ] && [ "$(value "$env" TURN_UDP_PORT)" = 3478 ] && [ "$(value "$env" TURN_TLS_PORT)" = 5349 ] || fail "$route: the ports are not the answers'"
  [ "$(value "$env" PUBLIC_DOMAIN)" = station.example ] && [ "$(value "$env" PUBLIC_ORIGIN)" = https://station.example ] || fail "$route: the command's name did not arrive"
  for secret in CONTROL_ADMIN_TOKEN POSTGRES_PASSWORD LIVEKIT_API_SECRET CHAT_EMAIL_KEY; do
    [ "$(value "$env" "$secret" | wc -c)" -ge 33 ] || fail "$route: $secret was not made"
  done
  [ "$(stat -c %a "$env")" = 600 ] || fail "$route: the file is not mode 600"
  [ "$(grep -c '^STATION_NAME=' "$env")" = 1 ] && [ "$(grep -c '^VIDEO=' "$env")" = 1 ] || fail "$route: a key is there twice"
  again=$(PATH="$work/bin:$PATH" WEAVESIGNAL_UNPRIV_PORT_FILE="$work/unpriv" "$script" --engine podman --no-start --env-file "$env" 2>&1)
  grep -q 'Settings unchanged' <<<"$again" || fail "$route: a second run changed the file: $again"
  pass "$route: weavesignal.sh takes the wizard's file, keeps every answer, adds the secrets, and a second run changes nothing"
done

# A station for this machine only: no --public, and nothing public in the file.
env="$work/local.env"
generate compose "{ isPublic: false, domain: '' }" > "$env"
PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$env" >/dev/null 2>&1 || fail "loopback: weavesignal.sh refused the file"
grep -q '^PUBLIC_DOMAIN=' "$env" && fail "loopback: a public name appeared"
[ "$(value "$env" STATION_NAME)" = "Paddy's Wireless" ] || fail "loopback: the name was changed"
pass "loopback: the file installs a station for this machine only"

# The real compose, where there is one: the name reaches control.toml whole.
compose=''
if podman compose version >/dev/null 2>&1 && ! podman compose version 2>&1 | grep -q 'podman-compose'; then compose="podman compose"
elif docker compose version >/dev/null 2>&1; then compose="docker compose"; fi
if [ -n "$compose" ]; then
  rendered=$(cd "$root/deploy/compose" && $compose --env-file "$work/compose.env" -f compose.yaml -f public.yaml config 2>/dev/null) || fail "compose config refused the wizard's file"
  grep -q "name = \"Paddy's Wireless\"" <<<"$rendered" || fail "the name did not reach the services' settings whole"
  grep -q 'consent_contact = "calls@station.example"' <<<"$rendered" || fail "the contact did not reach the control plane's settings"
  grep -Eq 'enabled = true' <<<"$rendered" || fail "VIDEO did not reach the desk's settings"
  pass "compose config ($compose): the name, the contact and video reach the services' settings"
else
  echo "skip  no 'podman compose' (docker-compose) or 'docker compose' here: compose config not run"
fi
echo "the wizard's Compose file installs"
