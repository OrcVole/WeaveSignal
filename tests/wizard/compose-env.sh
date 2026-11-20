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
# A GPU (17e): weavesignal.sh looks for an NVIDIA CDI spec where it is told. This machine may have a
# real one, so every run here looks in a folder of the test's own: empty, unless a case fills it.
mkdir -p "$work/cdi-none" "$work/cdi"
export WEAVESIGNAL_CDI_DIRS="$work/cdi-none"
# A stand-in spec, as `nvidia-ctk cdi generate` writes one (its devices cut down to two).
printf '%s\n' '---' 'cdiVersion: 0.5.0' 'kind: nvidia.com/gpu' 'devices:' '  - name: "0"' '    containerEdits: {}' '  - name: all' '    containerEdits: {}' > "$work/cdi/nvidia.yaml"

generate() { # route, extra answers as a JS object literal
  node --no-warnings --input-type=module -e "
    import { defaults, generate } from '$root/packages/wizard/generate.ts';
    const g = generate({ ...defaults('$1'), name: \"Paddy's Wireless\", contact: 'calls@station.example', domain: 'station.example', ...$2 });
    if (!g.ok) { console.error(JSON.stringify(g.problems)); process.exit(1); }
    process.stdout.write(g.output.file.text);"
}
value() { sed -n "s/^$2=//p" "$1" | head -1; }

# Consent words with apostrophes, quotes, commas, braces and a " #", as the file holds them.
consent='You'"'"'ll be on {presenter}'"'"'s show, "live", on {station}; kept {kept} # all. Ask {contact}.'
consent_line="'${consent//\'/\\\'}'"

for route in compose quadlets; do
  env="$work/$route.env"
  # The station's own consent words (issue #26) and a typed STUN list (ADR-0290) are the wizard's
  # answers too: the generator writes both lines as the script keeps them.
  CONSENT="$consent" generate "$route" "{ video: true, searchListing: true, acmeEmail: 'ops@station.example', nodeIp: '203.0.113.7', mediaPort: 52000, turnUdpPort: 3478, mail: 'outbox', mailFrom: 'chat@station.example', consentText: process.env.CONSENT, stun: 'other', stunOther: 'STUN.example.org:3478, 192.0.2.7:3478' }" > "$env"
  [ "$(value "$env" CONSENT_TEXT)" = "$consent_line" ] || fail "$route: the generator wrote the consent words as: $(value "$env" CONSENT_TEXT)"
  out=$(PATH="$work/bin:$PATH" WEAVESIGNAL_UNPRIV_PORT_FILE="$work/unpriv" "$script" --engine podman --no-start --env-file "$env" --public station.example 2>&1) || fail "$route: weavesignal.sh refused the wizard's file: $out"
  [ "$(value "$env" CONSENT_TEXT)" = "$consent_line" ] || fail "$route: the consent words were changed: $(value "$env" CONSENT_TEXT)"
  [ "$(value "$env" STUN_SERVERS)" = "stun.example.org:3478,192.0.2.7:3478" ] || fail "$route: the STUN servers were changed: $(value "$env" STUN_SERVERS)"
  [ "$(grep -c '^STUN_SERVERS=' "$env")" = 1 ] && [ "$(grep -c '^CONSENT_TEXT=' "$env")" = 1 ] || fail "$route: the consent words or the STUN servers are there twice"
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

# What the script refuses, the generator refuses first: the wizard gives no file for them.
refused() { # extra answers; succeeds when the generator names a problem with the field $2
  node --no-warnings --input-type=module -e "
    import { defaults, generate } from '$root/packages/wizard/generate.ts';
    const g = generate({ ...defaults('compose'), name: 'Kettle Two', contact: 'calls@station.example', domain: 'station.example', ...$1 });
    process.exit(!g.ok && g.problems.some((p) => p.field === '$2') ? 0 : 1);"
}
for words in 'costs \$5' '100% live' 'a \\\\ b'; do
  refused "{ consentText: '$words' }" consentText || fail "the generator takes consent words the script refuses: $words"
done
for servers in 'stun:stun.example.org:3478' 'stun.example.org' 'stun.example.org:70000' 'a.example:1,b.example:1,c.example:1,d.example:1,e.example:1'; do
  refused "{ stun: 'other', stunOther: '$servers' }" stunOther || fail "the generator takes STUN servers the script refuses: $servers"
  printf 'STATION_NAME=Kettle Two\nSTUN_SERVERS=%s\n' "$servers" > "$work/bad.env"
  said=$(PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$work/bad.env" 2>&1) && fail "weavesignal.sh takes STUN_SERVERS=$servers, which the generator refuses"
  grep -q 'STUN_SERVERS' <<<"$said" || fail "weavesignal.sh refused STUN_SERVERS=$servers for another reason: $said"
done
for words in 'costs $5' '100% live' 'a \ b'; do
  printf 'STATION_NAME=Kettle Two\nCONSENT_TEXT=%s\n' "'$words'" > "$work/bad.env"
  said=$(PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$work/bad.env" 2>&1) && fail "weavesignal.sh takes CONSENT_TEXT=$words, which the generator refuses"
  grep -q 'CONSENT_TEXT holds a backslash, a dollar sign or a per cent sign' <<<"$said" || fail "weavesignal.sh refused CONSENT_TEXT=$words for another reason: $said"
done
pass "the generator refuses the consent words and the STUN servers that weavesignal.sh refuses"

# A station for this machine only: no --public, and nothing public in the file.
env="$work/local.env"
generate compose "{ isPublic: false, domain: '' }" > "$env"
grep -q '^STUN_SERVERS=\|^CONSENT_TEXT=' "$env" && fail "loopback: the wizard wrote STUN servers or consent words nobody gave"
PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$env" >/dev/null 2>&1 || fail "loopback: weavesignal.sh refused the file"
[ -z "$(value "$env" STUN_SERVERS)" ] || fail "loopback: a STUN server appeared: $(value "$env" STUN_SERVERS)"
grep -q '^PUBLIC_DOMAIN=' "$env" && fail "loopback: a public name appeared"
[ "$(value "$env" STATION_NAME)" = "Paddy's Wireless" ] || fail "loopback: the name was changed"
pass "loopback: the file installs a station for this machine only"

# The bridge's loopback port (Phase 17g): checked and written like the others, and 0 (off) kept.
[ "$(value "$env" SOURCE_BRIDGE_PORT)" = 7320 ] || fail "the bridge's port was not chosen: $(value "$env" SOURCE_BRIDGE_PORT)"
sed -i 's/^SOURCE_BRIDGE_PORT=.*/SOURCE_BRIDGE_PORT=0/' "$env"
PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$env" >/dev/null 2>&1 || fail "SOURCE_BRIDGE_PORT=0 was refused"
[ "$(value "$env" SOURCE_BRIDGE_PORT)" = 0 ] || fail "SOURCE_BRIDGE_PORT=0 was changed to $(value "$env" SOURCE_BRIDGE_PORT)"
pass "the bridge's port: 7320 chosen, and 0 (off) kept"

# The telephone line (17f): off, nothing of it is written but PHONE=false; on, its ports are
# chosen clear of the others (SIP 5060, RTP 7340-7355 for 8 lines, Valkey 7321), Valkey's
# password is made, a second run keeps them, an odd RTP start and a port another setting holds
# are refused, and a station that turns it off again keeps what it had set.
[ "$(value "$env" PHONE)" = false ] || fail "PHONE was not written as false: $(value "$env" PHONE)"
grep -q '^PHONE_SIP_PORT=' "$env" && fail "the phone line's ports were written with the line off"
sed -i 's/^PHONE=.*/PHONE=true/' "$env"
# On without a trunk: refused, as the control plane would refuse it at start.
out=$(PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$env" 2>&1) && fail "PHONE=true with no trunk was taken"
grep -q 'needs a trunk first' <<<"$out" || fail "the missing trunk was not named: $out"
# A documentation trunk (RFC 5737 addresses, Ofcom's drama numbers).
printf '%s\n' 'PHONE_NUMBERS="+441314960000"' 'PHONE_ALLOWED_ADDRESSES="198.51.100.0/24"' 'PHONE_PUBLIC_IP=203.0.113.7' 'PHONE_DEFAULT_VOICE=edinburgh' >> "$env"
out=$(PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$env" 2>&1) || fail "PHONE=true was refused: $out"
[ "$(value "$env" PHONE_DEFAULT_VOICE)" = edinburgh ] || fail "the phone line's voice was not kept"
for kv in PHONE_SIP_PORT=5060 PHONE_RTP_PORT=7340 PHONE_RTP_LAST=7355 PHONE_REDIS_PORT=7321 PHONE_LINES=8; do
  [ "$(value "$env" "${kv%%=*}")" = "${kv#*=}" ] || fail "the phone line: ${kv%%=*} is $(value "$env" "${kv%%=*}"), not ${kv#*=}"
done
[ "$(value "$env" PHONE_REDIS_PASSWORD | wc -c)" -ge 33 ] || fail "the phone line: Valkey's password was not made"
again=$(PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$env" 2>&1)
grep -q 'Settings unchanged' <<<"$again" || fail "the phone line: a second run changed the file: $again"
sed -i 's/^PHONE_LINES=.*/PHONE_LINES=4/; /^PHONE_RTP_LAST=/d' "$env"
PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$env" >/dev/null 2>&1 || fail "PHONE_LINES=4 was refused"
[ "$(value "$env" PHONE_RTP_LAST)" = 7347 ] || fail "4 lines: the RTP range ends at $(value "$env" PHONE_RTP_LAST), not 7347"
bad="$work/bad.env"
sed 's/^PHONE_RTP_PORT=.*/PHONE_RTP_PORT=7341/' "$env" > "$bad"
PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$bad" >/dev/null 2>&1 && fail "an odd PHONE_RTP_PORT was taken"
sed 's/^PHONE_REDIS_PORT=.*/PHONE_REDIS_PORT=7319/' "$env" > "$bad"
out=$(PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$bad" 2>&1) && fail "PHONE_REDIS_PORT on WHIP's 7319 was taken"
grep -q 'PHONE_REDIS_PORT=7319' <<<"$out" || fail "the clash was not named: $out"
sed -i 's/^PHONE=.*/PHONE=false/' "$env"
PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$env" >/dev/null 2>&1 || fail "PHONE=false was refused"
[ "$(value "$env" PHONE_SIP_PORT)" = 5060 ] || fail "turning the line off lost its settings"
sed -i 's/^PHONE=.*/PHONE=true/' "$env"
pass "the phone line's ports: chosen clear of the others, kept, refused when odd or taken, kept when off"

# A GPU for the desk (17e). GPU unset: an NVIDIA CDI spec found is written as GPU=nvidia and said,
# none found as GPU=none, and a second run changes nothing; GPU=nvidia with no spec is refused with
# the command that writes one; the encoder, the device and the session count are checked.
gpu_env="$work/gpu.env"
generate compose "{ isPublic: false, domain: '', video: true }" > "$gpu_env"
grep -q '^GPU=' "$gpu_env" && fail "the wizard wrote GPU for the answer auto"
out=$(WEAVESIGNAL_CDI_DIRS="$work/cdi-none $work/cdi" PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$gpu_env" 2>&1) || fail "GPU detection: weavesignal.sh refused the file: $out"
[ "$(value "$gpu_env" GPU)" = nvidia ] || fail "GPU detection: a spec was there, and GPU is $(value "$gpu_env" GPU)"
grep -q "CDI spec is at $work/cdi/nvidia.yaml, so GPU=nvidia" <<<"$out" || fail "GPU detection: the card found was not said: $out"
[ "$(value "$gpu_env" GPU_DEVICE)" = nvidia.com/gpu=all ] && [ "$(value "$gpu_env" NVENC_SESSIONS)" = 12 ] && [ "$(value "$gpu_env" VIDEO_ENCODER)" = auto ] \
  || fail "GPU detection: the device, sessions or encoder are not the defaults"
again=$(WEAVESIGNAL_CDI_DIRS="$work/cdi" PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$gpu_env" 2>&1)
grep -q 'Settings unchanged' <<<"$again" || fail "GPU detection: a second run changed the file: $again"
# The spec gone and GPU=nvidia kept: refused, naming the command.
out=$(PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$gpu_env" 2>&1) && fail "GPU=nvidia was taken with no CDI spec"
grep -q 'nvidia-ctk cdi generate' <<<"$out" || fail "GPU=nvidia with no spec: the way to make one was not said: $out"
nogpu_env="$work/nogpu.env"
generate compose "{ isPublic: false, domain: '', video: true }" > "$nogpu_env"
PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$nogpu_env" >/dev/null 2>&1 || fail "no GPU: weavesignal.sh refused the file"
[ "$(value "$nogpu_env" GPU)" = none ] || fail "no spec, and GPU is $(value "$nogpu_env" GPU)"
grep -q '^GPU_DEVICE=\|^NVENC_SESSIONS=' "$nogpu_env" && fail "no GPU, yet its device or sessions were written"
generate compose "{ isPublic: false, domain: '', video: true, gpu: 'none' }" > "$work/none.env"
WEAVESIGNAL_CDI_DIRS="$work/cdi" PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$work/none.env" >/dev/null 2>&1 || fail "GPU=none was refused"
[ "$(value "$work/none.env" GPU)" = none ] || fail "the wizard's GPU=none became $(value "$work/none.env" GPU) where a card is"
for kv in GPU=amd VIDEO_ENCODER=vaapi 'GPU_DEVICE=/dev/nvidia0' NVENC_SESSIONS=65 NVENC_SESSIONS=many; do
  grep -v "^${kv%%=*}=" "$gpu_env" > "$work/bad.env"; printf '%s\n' "$kv" >> "$work/bad.env"
  out=$(WEAVESIGNAL_CDI_DIRS="$work/cdi" PATH="$work/bin:$PATH" "$script" --engine podman --no-start --env-file "$work/bad.env" 2>&1) && fail "$kv was taken"
  grep -q "${kv%%=*}" <<<"$out" || fail "$kv was refused for another reason: $out"
done
pass "GPU: a CDI spec found is written as GPU=nvidia and said, none as GPU=none; a second run keeps it; GPU=nvidia without a spec, and a wrong GPU, encoder, device or session count are refused"

# The real compose, where there is one: the name reaches control.toml whole.
compose=''
if podman compose version >/dev/null 2>&1 && ! podman compose version 2>&1 | grep -q 'podman-compose'; then compose="podman compose"
elif docker compose version >/dev/null 2>&1; then compose="docker compose"; fi
if [ -n "$compose" ]; then
  rendered=$(cd "$root/deploy/compose" && $compose --env-file "$work/compose.env" -f compose.yaml -f public.yaml config 2>/dev/null) || fail "compose config refused the wizard's file"
  grep -q "name = \"Paddy's Wireless\"" <<<"$rendered" || fail "the name did not reach the services' settings whole"
  grep -q 'consent_contact = "calls@station.example"' <<<"$rendered" || fail "the contact did not reach the control plane's settings"
  grep -Eq 'enabled = true' <<<"$rendered" || fail "VIDEO did not reach the desk's settings"
  got=$(cd "$root/deploy/compose" && $compose --env-file "$work/compose.env" -f compose.yaml -f public.yaml config --format json 2>/dev/null \
    | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["services"]["control"]["environment"]["WEAVESIGNAL_CONSENT_TEXT"], end="")')
  [ "$got" = "$consent" ] || fail "the consent words reach the control plane as: $got"
  pass "compose config ($compose): the name, the contact, the consent words and video reach the services"
  # The phone overlay over the same station (17f): the desk reads the words, the SIP service's
  # log is one small file, and Valkey has the port and password chosen.
  phone=$(cd "$root/deploy/compose" && $compose --env-file "$work/local.env" -f compose.yaml -f phone.yaml config --format json 2>/dev/null) || fail "compose config refused the phone overlay"
  python3 -I -c '
import json, sys
c = json.loads(sys.argv[1])
s = c["services"]
desk = [v["target"] for v in s["desk"]["volumes"]]
assert "/var/lib/weavesignal/phone-words" in desk, desk
log = s["livekit-sip"]["logging"]
assert log["driver"] == "json-file" and log["options"]["max-size"], log
assert "port 7321" in c["configs"]["valkey"]["content"], c["configs"]["valkey"]
assert "rtp_port: 7340-7347" in s["livekit-sip"]["environment"]["SIP_CONFIG_BODY"]
control = c["configs"]["control"]["content"]
assert "default_voice = \"edinburgh\"" in control and "enabled = true" in control, control
' "$phone" || fail "the phone overlay does not render as chosen"
  pass "compose config ($compose): the phone overlay renders with the station's chosen ports"
  # The GPU overlay (17e): the desk is handed the card by its CDI name, with labelling off for it,
  # and told the encoder; without the overlay it has no device.
  for case in "$gpu_env gpu" "$nogpu_env plain"; do
    read -r f kind <<<"$case"
    files=(-f compose.yaml); [ "$kind" = plain ] || files+=(-f gpu.yaml)
    sed -i 's/^GPU_DEVICE=.*/GPU_DEVICE=nvidia.com\/gpu=0/' "$f"
    rendered=$(cd "$root/deploy/compose" && $compose --env-file "$f" "${files[@]}" config --format json 2>/dev/null) || fail "compose config refused the $kind desk"
    KIND=$kind python3 -I -c '
import json, os, sys, tomllib
c = json.loads(sys.argv[1])
desk = c["services"]["desk"]
video = tomllib.loads(c["configs"]["desk"]["content"])["video"]
assert video["encoder"] == "auto" and video["nvenc_sessions"] == 12, video
if os.environ["KIND"] == "gpu":
    assert [d["source"] for d in desk["devices"]] == ["nvidia.com/gpu=0"], desk.get("devices")
    assert "label=disable" in desk["security_opt"], desk.get("security_opt")
else:
    assert not desk.get("devices"), desk["devices"]
' "$rendered" || fail "the $kind desk does not render as chosen"
  done
  pass "compose config ($compose): gpu.yaml hands the desk the card named by GPU_DEVICE, labelling off; without it, no device; the desk's settings take the encoder"
else
  echo "skip  no 'podman compose' (docker-compose) or 'docker compose' here: compose config not run"
fi
echo "the wizard's Compose file installs"
