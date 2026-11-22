#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The phone line end to end on a real Compose station, with nothing made by hand (1.2.1): the
# bundle from weavesignal.sh with PHONE=true on its own project and ports, then
#
#   1. the station makes its own trunk and dispatch rule in LiveKit (`trunk.state` made in
#      GET /api/v1/phone; exactly one of each named weavesignal-phone-line in LiveKit's SIP API);
#   2. unattended.sh rings it with baresip (answered, the consent words heard, the key, the
#      queue, screen, send, on air, the studio's drop);
#   3. the control plane restarted: still exactly one trunk and one rule (made afresh, not added);
#   4. Valkey restarted (it keeps nothing on disk, so LiveKit forgets both): made again within
#      a minute;
#
# then the station is purged. In 1.2.0 step 1 never happened and every call was refused with
# `404 Does not match Trunks or Dispatch Rules` (research 2026-11-21, real-kit tries, finding 1).
#
#   flock ~/Code/WeaveSignal/tools/1.1.1/stack.lock tests/phone/acceptance/stack.sh [--out DIR] [--keep]
#
#   WEAVESIGNAL_SERVER_IMAGE  the image to run (default localhost/weavesignal-server:m1); build it
#                             first (`just server-image`): a station runs what the image holds
#   WEAVESIGNAL_TEST_PREFIX   the Compose project's suffix (default phone)
#   WEAVESIGNAL_PHONE_PORTS   the ports' first two digits (default 23: ports 23xxx, below Linux's
#                             ephemeral range)
#   --keep                    leave the station up afterwards (purge it yourself with
#                             weavesignal.sh --uninstall --purge --yes and the same project and file)
#
# Needs Podman with docker-compose as its provider, mojo, ffmpeg and python3. Runs containers:
# only under the stack lock, never beside a soak.
set -euo pipefail
root=$(cd "$(dirname "$0")/../../.." && pwd)
here="$root/tests/phone/acceptance"
out=''
keep=false
while [ $# -gt 0 ]; do
  case $1 in
    --out) out=$2; shift ;;
    --keep) keep=true ;;
    *) echo "stack.sh: unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
out=${out:-$(mktemp -d "${TMPDIR:-/tmp}/ws-phone-stack-XXXXXX")}
mkdir -p "$out"
image=${WEAVESIGNAL_SERVER_IMAGE:-localhost/weavesignal-server:m1}
project=ws-phone-${WEAVESIGNAL_TEST_PREFIX:-phone}
p=${WEAVESIGNAL_PHONE_PORTS:-23}
env_file="$out/station.env"
note() { printf '%s  %s\n' "$(date -u +%H:%M:%S)" "$*" | tee -a "$out/STACK.txt"; }
fail=0
check() { if [ "$1" = ok ]; then note "PASS  $2"; else note "FAIL  $2"; fail=1; fi; }

# The station: loopback, its own ports, the phone line on with a trunk for calls from this
# machine (unattended.sh's caller is 127.0.0.1, with no digest password).
mkdir -p "$out/no-cdi"
export WEAVESIGNAL_CDI_DIRS="$out/no-cdi"
export CONTROL_PORT=${p}310 CHAT_PORT=${p}311 LISTENERS_PORT=${p}312 SRT_INGEST_PORT=${p}313 PLAYOUT_FEED_PORT=${p}315
export DESK_FEED_PORT=${p}326 DESK_SOURCES_PORT=${p}318 INGEST_WHIP_PORT=${p}319 SOURCE_BRIDGE_PORT=${p}320
export LIVEKIT_HTTP_PORT=${p}880 LIVEKIT_MEDIA_PORT=${p}882
export PHONE=true PHONE_SIP_PORT=${p}060 PHONE_RTP_PORT=${p}340 PHONE_LINES=8 PHONE_REDIS_PORT=${p}321
export PHONE_NUMBERS='"+441314960000"' PHONE_ALLOWED_ADDRESSES='"127.0.0.1/32"' PHONE_PUBLIC_IP=127.0.0.1 PHONE_COUNTRY_CODE=44
export CONSENT_CONTACT=takedown@station.example STATION_NAME="Kettle FM"
ws() { nice -n 10 "$root/deploy/compose/weavesignal.sh" --project "$project" --env-file "$env_file" --image "$image" --engine podman --no-boot-start "$@"; }
purge() {
  if [ "$keep" = true ]; then note "kept: $project with $env_file"; return; fi
  ws --uninstall --purge --yes > "$out/down.log" 2>&1 || true
}
trap purge EXIT

note "station $project from $image, ports ${p}xxx"
ws > "$out/up.log" 2>&1 || { note "FAIL  weavesignal.sh did not bring the station up (see up.log)"; exit 1; }
setting() { sed -n "s/^$1=//p" "$env_file" | head -1; }
token=$(setting CONTROL_ADMIN_TOKEN)
lk_key=$(setting LIVEKIT_API_KEY)
lk_secret=$(setting LIVEKIT_API_SECRET)
control=http://127.0.0.1:$CONTROL_PORT
container() { podman ps --format '{{.Names}}' | grep -E "^${project}[-_]$1[-_]" | head -1; }

trunk_state() { # the trunk's state in GET /api/v1/phone, or nothing
  curl -fsS -H "Authorization: Bearer $token" "$control/api/v1/phone" 2>/dev/null \
    | python3 -I -c 'import json,sys; t=(json.load(sys.stdin).get("trunk") or {}); print(t.get("state",""), (t.get("message") or "")[:200])' || true
}
wait_made() { # seconds: true when the trunk is made within them
  local end=$(( $(date +%s) + $1 )) s=''
  while [ "$(date +%s)" -lt "$end" ]; do
    s=$(trunk_state)
    [ "${s%% *}" = made ] && return 0
    sleep 2
  done
  note "trunk: ${s:-no answer}"
  return 1
}
own_count() { # how many trunks and rules named weavesignal-phone-line LiveKit holds, as `trunks rules`
  LK_KEY=$lk_key LK_SECRET=$lk_secret LK_URL=http://127.0.0.1:$LIVEKIT_HTTP_PORT python3 -I -c '
import base64, hashlib, hmac, json, os, time, urllib.request
b64 = lambda b: base64.urlsafe_b64encode(b).rstrip(b"=")
now = int(time.time())
h = b64(json.dumps({"alg": "HS256", "typ": "JWT"}).encode())
c = b64(json.dumps({"iss": os.environ["LK_KEY"], "nbf": now - 10, "exp": now + 60, "sip": {"admin": True}}).encode())
jwt = (h + b"." + c + b"." + b64(hmac.new(os.environ["LK_SECRET"].encode(), h + b"." + c, hashlib.sha256).digest())).decode()
def items(method):
    r = urllib.request.Request(os.environ["LK_URL"] + "/twirp/livekit.SIP/" + method, data=b"{}", method="POST",
        headers={"Authorization": "Bearer " + jwt, "Content-Type": "application/json"})
    return json.load(urllib.request.urlopen(r, timeout=5)).get("items") or []
own = lambda xs: sum(1 for x in xs if x.get("name") == "weavesignal-phone-line")
print(own(items("ListSIPInboundTrunk")), own(items("ListSIPDispatchRule")))'
}

# 1. The station makes its trunk and rule itself.
wait_made 120 && check ok "the station made its trunk and dispatch rule in LiveKit" \
  || check no "the trunk was not made within 120 s"
[ "$(own_count)" = "1 1" ] && check ok "LiveKit holds exactly one trunk and one rule of the station's" \
  || check no "LiveKit holds trunks and rules: $(own_count)"

# 2. A call, from the key to the air and the drop.
if WEAVESIGNAL_CONTROL_URL=$control WEAVESIGNAL_CONTROL_TOKEN=$token \
   "$here/unattended.sh" --sip "127.0.0.1:$PHONE_SIP_PORT" --out "$out/call" > "$out/call.log" 2>&1; then
  check ok "the unattended call passed every check (call/RESULT.txt)"
else
  check no "the unattended call failed a check (call/RESULT.txt)"
fi
sed 's/^/    /' "$out/call/RESULT.txt" 2>/dev/null | tee -a "$out/STACK.txt" >/dev/null || true

# 3. A restart of the control plane makes them afresh, not twice.
c=$(container control || true)
if [ -n "$c" ]; then
  podman restart "$c" >/dev/null
  sleep 3
  wait_made 90 && [ "$(own_count)" = "1 1" ] && check ok "after the control plane's restart: still one trunk and one rule" \
    || check no "after the control plane's restart: $(own_count)"
else
  check no "no control plane container found to restart"
fi

# 4. Valkey keeps nothing on disk: after its restart LiveKit has forgotten both, and the
#    station makes them again at its next look (every 30 s).
v=$(container valkey || true)
if [ -n "$v" ]; then
  podman restart "$v" >/dev/null
  end=$(( $(date +%s) + 90 )); n=''
  while [ "$(date +%s)" -lt "$end" ]; do n=$(own_count 2>/dev/null || true); [ "$n" = "1 1" ] && break; sleep 3; done
  [ "$n" = "1 1" ] && check ok "after Valkey's restart: the trunk and rule made again" \
    || check no "after Valkey's restart: ${n:-LiveKit not answering}"
else
  check no "no Valkey container found to restart"
fi

[ "$fail" = 0 ] && note "every check passed" || note "some checks failed"
note "files: $out"
exit "$fail"
