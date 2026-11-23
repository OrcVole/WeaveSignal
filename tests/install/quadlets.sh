#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The Quadlets generated from the Compose bundle: weavesignal.sh writes throwaway settings
# (--no-start), generate.sh writes the units, and Podman's own generator converts them
# (`quadlet -dryrun -user`) into one service per container and volume. Nothing is installed
# into systemd and nothing is started; the units are not run here.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/install/run/quadlets"
project=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-q
quadlet=${QUADLET:-/usr/libexec/podman/quadlet}
[ -x "$quadlet" ] || { echo "Podman's quadlet generator is not at $quadlet (set QUADLET)" >&2; exit 2; }
rm -rf -- "$run"; mkdir -p "$run"
trap 'rm -rf -- "$run"' EXIT
# A GPU (17e): weavesignal.sh looks for an NVIDIA CDI spec only where it is told, never in this
# machine's own /etc/cdi; the gpu station is given a stand-in spec, the others none.
mkdir -p "$run/cdi-none" "$run/cdi"
printf '%s\n' '---' 'cdiVersion: 0.5.0' 'kind: nvidia.com/gpu' 'devices:' '  - name: all' '    containerEdits: {}' > "$run/cdi/nvidia.yaml"
# A station's own consent words (issue #26) with apostrophes, quotes, commas, braces and a " #":
# they must reach the control plane's unit whole.
consent='You'"'"'ll be on {presenter}'"'"'s show, "live", on {station}; kept {kept} # all. Ask {contact}.'
# Six times: the loopback bundle, a public station (the proxy joins with its health check, and
# LiveKit's unit waits for it), a loopback station with the telephone line (17f: phone.yaml), one
# with video on an NVIDIA card (17e: gpu.yaml, the card by its CDI name), one with the
# rehearsal studio (1.3: rehearsal.yaml, the rehearsal desk's unit), and one installed before
# 1.2 that is updated on a server with a card (1.2.1, ADR-0292: it keeps the CPU, and is told).
for variant in loopback public phone gpu rehearsal upgrade; do
  dir="$run/$variant"; mkdir -p "$dir"
  # A public station's LiveKit address is the name's A record unless NODE_IP is given (ADR-0290),
  # and station.example has none: a documentation address stands in, as nothing is started.
  extra=(); [ "$variant" != public ] || extra=(--public station.example)
  node_ip=; [ "$variant" != public ] || node_ip=203.0.113.7
  phone=false; [ "$variant" != phone ] || phone=true
  rehearsal=false; [ "$variant" != rehearsal ] || rehearsal=true
  cdi="$run/cdi-none"; gpu=auto; [ "$variant" != gpu ] || { cdi="$run/cdi"; gpu=nvidia; }
  # VBAN on and a printed address by hand, on the loopback station (1.2.1: 1.2.0's weavesignal.sh
  # dropped both from the settings file, so neither reached the units).
  vban=false; origin=''; [ "$variant" != loopback ] || { vban=true; origin=http://192.0.2.10:7310; }
  # The settings only: ports are written, not bound, so the defaults' alternatives may be chosen.
  # The proxy's 443 and 80 are not bindable by a rootless Podman, so a public station's ports are
  # given above 1024 here (nothing is bound).
  # The phone station's trunk is a documentation one (RFC 5737 addresses, an Ofcom drama number).
  PHONE_NUMBERS='"+441314960000"' PHONE_ALLOWED_ADDRESSES='"198.51.100.0/24"' PHONE_PUBLIC_IP=203.0.113.7 \
  PROXY_HTTPS_PORT=18443 PROXY_HTTP_PORT=18080 NODE_IP=$node_ip CONSENT_TEXT="$consent" PHONE=$phone \
  WEAVESIGNAL_CDI_DIRS=$cdi GPU=$gpu REHEARSAL=$rehearsal KIT_VBAN=$vban PUBLIC_ORIGIN=$origin \
    "$root/deploy/compose/weavesignal.sh" --project "$project" --env-file "$dir/station.env" --no-start "${extra[@]}" >/dev/null
  [ "$variant" != gpu ] || echo VIDEO=true >> "$dir/station.env"
  if [ "$variant" = upgrade ]; then
    # The settings file a 1.1 station has: its secrets, video on, and no GPU line. Then the update's
    # own run of weavesignal.sh, on a server whose card has a CDI spec, with nothing asked.
    grep -v '^GPU=\|^GPU_FOUND=\|^VIDEO_ENCODER=\|^VIDEO=' "$dir/station.env" > "$dir/station.tmp"; cat "$dir/station.tmp" > "$dir/station.env"; rm -f "$dir/station.tmp"
    echo VIDEO=true >> "$dir/station.env"
    WEAVESIGNAL_CDI_DIRS="$run/cdi" "$root/deploy/compose/weavesignal.sh" --project "$project" --env-file "$dir/station.env" --no-start > "$dir/update.txt"
    grep -q 'an update never switches it' "$dir/update.txt" || { echo "the update did not say the card is there and left alone" >&2; cat "$dir/update.txt" >&2; exit 1; }
  fi
  # icecast-out (ADR-0226): the remote server's password is a Podman secret like the rest.
  printf 'ICECAST_OUT=true\nICECAST_OUT_HOST=azuracast.example\nICECAST_OUT_USER=dj\nICECAST_OUT_PASSWORD=quadlet-test-streamer-password\n' >> "$dir/station.env"
  # The phone line's digest password (1.2.1 reads it, to make the trunk): a Podman secret too.
  [ "$variant" != phone ] || printf 'PHONE_AUTH_USERNAME=kettle\nPHONE_TRUNK_PASSWORD=quadlet-test-trunk-password\n' >> "$dir/station.env"
  "$root/deploy/quadlets/generate.sh" --project "$project" --env-file "$dir/station.env" --out "$dir/out" 2>/dev/null | tail -1
  units="$dir/out/containers/systemd"
  for key in LIVEKIT_API_SECRET LISTENERS_SOURCE_PASSWORD POSTGRES_PASSWORD CONTROL_ADMIN_TOKEN CHAT_EMAIL_KEY CONTROL_EMAIL_KEY CONTROL_TOKEN_KEY ICECAST_OUT_PASSWORD INGEST_TOKEN DESK_SOURCES_PASSWORD PHONE_REDIS_PASSWORD PHONE_TRUNK_PASSWORD; do
    value=$(grep "^$key=" "$dir/station.env" | cut -d= -f2- || true)
    [ -n "$value" ] || continue  # PHONE_REDIS_PASSWORD is made only with the line on
    ! grep -rqF -- "$value" "$dir/out" || { echo "$key is written into a unit or a settings file" >&2; exit 1; }
  done
  QUADLET_UNIT_DIRS="$units" "$quadlet" -dryrun -user > "$dir/dryrun.txt" 2> "$dir/dryrun.err" \
    || { cat "$dir/dryrun.err" >&2; exit 1; }
  services=$(grep -c '^---.*\.service---$' "$dir/dryrun.txt")
  containers=$(find "$units" -name '*.container' | wc -l); volumes=$(find "$units" -name '*.volume' | wc -l)
  [ "$services" = $(( containers + volumes )) ] || { echo "quadlet made $services services from $containers containers and $volumes volumes" >&2; exit 1; }
  grep -q -- "--secret $project-postgres-password,type=env,target=POSTGRES_PASSWORD" "$dir/dryrun.txt"
  grep -q -- "--secret $project-chat-email-key,type=env,target=WEAVESIGNAL_CHAT_EMAIL_KEY" "$dir/dryrun.txt"
  grep -q -- "--secret $project-control-email-key,type=env,target=WEAVESIGNAL_CONTROL_EMAIL_KEY" "$dir/dryrun.txt"
  grep -q -- "--secret $project-control-token-key,type=env,target=WEAVESIGNAL_CONTROL_TOKEN_KEY" "$dir/dryrun.txt"
  grep -q -- "--secret $project-icecast-out-password,type=env,target=WEAVESIGNAL_ICECAST_OUT_PASSWORD" "$dir/dryrun.txt"
  grep -q '"host": "azuracast.example"' "$dir/out/config/playout" || { echo "the outgoing Icecast host is not in playout's settings" >&2; exit 1; }
  # The show and steering's settings (issue 5) are in the control plane's settings file: the
  # Quadlets take them from the same Compose configuration, empty lists and all.
  grep -qx '\[studio\]' "$dir/out/config/control" && grep -qx 'show = ""' "$dir/out/config/control" \
    && grep -qx '\[steering\]' "$dir/out/config/control" && grep -qx 'edges = \[\]' "$dir/out/config/control" \
    || { echo "the control plane's settings lack [studio] or [steering]" >&2; exit 1; }
  # No STUN server unless the station names one (ADR-0290): LiveKit, the desk and the control plane
  # read the same empty setting, and LiveKit never asks for its address (use_external_ip off).
  grep -q 'stun_servers: \[\]' "$dir/out/config/livekit" && grep -q 'use_external_ip: false' "$dir/out/config/livekit" \
    && grep -qx 'stun_servers = ""' "$dir/out/config/desk" && grep -qx 'stun_servers = ""' "$dir/out/config/control" \
    || { echo "a Quadlets station would ask a STUN server by default" >&2; exit 1; }
  # VBAN and the printed address reach the control plane's settings as weavesignal.sh wrote them.
  grep -qx "vban = $vban" "$dir/out/config/control" || { echo "the $variant station's control plane lacks vban = $vban" >&2; exit 1; }
  [ "$variant" != loopback ] || grep -qx "public_url = \"$origin\"" "$dir/out/config/control" \
    || { echo "the loopback station's hand-set PUBLIC_ORIGIN did not reach the control plane" >&2; exit 1; }
  # The desk's congestion control reaches the desk's settings, gcc unless the station chose (16c).
  grep -qx 'congestion_control = "gcc"' "$dir/out/config/desk" \
    || { echo "the Quadlets desk's settings lack congestion_control = \"gcc\"" >&2; exit 1; }
  [ "$variant" != public ] || grep -q 'node_ip: 203.0.113.7' "$dir/out/config/livekit" \
    || { echo "a public Quadlets station's LiveKit lacks the address weavesignal.sh chose" >&2; exit 1; }
  # The words as Podman will be given them: systemd's quoting undone (\xNN, \", \\).
  got=$(grep -o -- '--env "WEAVESIGNAL_CONSENT_TEXT=\([^"\\]\|\\.\)*"' "$dir/dryrun.txt" \
    | python3 -I -c 'import re,sys; a=sys.stdin.read().strip()[len("--env \"WEAVESIGNAL_CONSENT_TEXT="):-1]; print(re.sub(r"\\(x[0-9a-fA-F]{2}|.)", lambda m: chr(int(m[1][1:], 16)) if m[1][0] == "x" else m[1], a), end="")')
  [ "$got" = "$consent" ] || { echo "the consent words reach the control plane's unit as: $got" >&2; exit 1; }
  grep -q -- "--health-cmd" "$dir/dryrun.txt"
  # Hosts' own studios (ADR-0280): the ingest's unit, its bearer shared with the control plane only,
  # and the desk's password with the desk only.
  [ -f "$units/$project-ingest.container" ] || { echo "no ingest unit" >&2; exit 1; }
  for unit in ingest control; do
    grep -q "^Secret=$project-ingest-token,type=env,target=WEAVESIGNAL_INGEST_TOKEN$" "$units/$project-$unit.container" || { echo "the $unit unit lacks the ingest's bearer" >&2; exit 1; }
  done
  for unit in ingest desk; do
    grep -q "^Secret=$project-desk-sources-password,type=env,target=WEAVESIGNAL_DESK_SOURCES_PASSWORD$" "$units/$project-$unit.container" || { echo "the $unit unit lacks the desk's sources password" >&2; exit 1; }
  done
  [ "$(grep -l 'WEAVESIGNAL_INGEST_TOKEN' "$units"/*.container | wc -l)" = 2 ] || { echo "the ingest's bearer reaches a unit beyond the ingest and the control plane" >&2; exit 1; }
  [ "$(grep -l 'WEAVESIGNAL_DESK_SOURCES_PASSWORD' "$units"/*.container | wc -l)" = 2 ] || { echo "the desk's sources password reaches a unit beyond the ingest and the desk" >&2; exit 1; }
  # rtmp-out: the desk's settings carry RTMP out's block (ADR-0230).
  grep -rq '^\[video.rtmp\]' "$dir/out" || { echo "the desk's settings have no [video.rtmp]" >&2; exit 1; }
  # A GPU (17e): only the gpu station's desk is handed the card, by its CDI name, labelling off,
  # and Podman's generator passes it on as --device; every desk is told the encoder.
  grep -qx 'encoder = "auto"' "$dir/out/config/desk" || { echo "the desk's settings lack encoder = \"auto\"" >&2; exit 1; }
  if [ "$variant" = gpu ]; then
    grep -qx 'GPU=nvidia' "$dir/station.env" || { echo "weavesignal.sh did not take GPU=nvidia with a CDI spec there" >&2; exit 1; }
    grep -qx 'AddDevice=nvidia.com/gpu=all' "$units/$project-desk.container" && grep -qx 'SecurityLabelDisable=true' "$units/$project-desk.container" \
      || { echo "the gpu station's desk unit is not given the card with labelling off" >&2; exit 1; }
    grep -q -- '--device nvidia.com/gpu=all' "$dir/dryrun.txt" || { echo "Podman's generator dropped the desk's card" >&2; exit 1; }
    [ "$(grep -l '^AddDevice=' "$units"/*.container | wc -l)" = 1 ] || { echo "the card reaches a unit beyond the desk" >&2; exit 1; }
  else
    ! grep -q '^AddDevice=' "$units"/*.container || { echo "a $variant station without a GPU hands a device to a container" >&2; exit 1; }
    ! grep -q -- '--device ' "$dir/dryrun.txt" || { echo "a $variant station's units pass a device" >&2; exit 1; }
    grep -qx 'GPU=none' "$dir/station.env" || { echo "a $variant station is not GPU=none" >&2; exit 1; }
  fi
  # What the control plane is told for the studio app's Settings (1.2.1): the card the script
  # found on the server, and whether the desk has it. An updated station's card is found, not used.
  case $variant in gpu) told='nvidia nvidia' ;; upgrade) told='nvidia none' ;; *) told='none none' ;; esac
  read -r found used <<<"$told"
  grep -qx "GPU_FOUND=$found" "$dir/station.env" || { echo "the $variant station's GPU_FOUND is not $found" >&2; exit 1; }
  grep -qx "gpu_found = \"$found\"" "$dir/out/config/control" && grep -qx "gpu = \"$used\"" "$dir/out/config/control" \
    || { echo "the $variant station's control plane is not told gpu_found = $found, gpu = $used" >&2; exit 1; }
  if [ "$variant" = phone ]; then
    # The telephone line (17f): Valkey and LiveKit's SIP service, the SIP log one small file
    # (never the journal), and the desk reading the words.
    for unit in valkey livekit-sip; do
      [ -f "$units/$project-$unit.container" ] || { echo "the phone station has no $unit unit" >&2; exit 1; }
    done
    grep -qx 'LogDriver=json-file' "$units/$project-livekit-sip.container" && grep -qx 'LogOpt=max-size=512k' "$units/$project-livekit-sip.container" \
      || { echo "the SIP service's log is not kept brief" >&2; exit 1; }
    grep -q -- '--log-driver json-file' "$dir/dryrun.txt" && grep -q -- '--log-opt max-size=512k' "$dir/dryrun.txt" \
      || { echo "Podman's generator dropped the SIP service's log options" >&2; exit 1; }
    grep -q '^Volume=.*phone-words.volume:/var/lib/weavesignal/phone-words:ro$' "$units/$project-desk.container" \
      || { echo "the desk does not read the phone words" >&2; exit 1; }
    grep -q -- "--secret $project-phone-trunk-password,type=env,target=WEAVESIGNAL_PHONE_TRUNK_PASSWORD" "$dir/dryrun.txt" \
      || { echo "the control plane is not given the trunk's digest password as a secret" >&2; exit 1; }
  else
    [ ! -f "$units/$project-livekit-sip.container" ] || { echo "a station without the line has a SIP unit" >&2; exit 1; }
  fi
  if [ "$variant" = rehearsal ]; then
    # The rehearsal studio (1.3): the rehearsal desk's unit, raised for the kernel to stop first,
    # with its own settings and none of the sources password; the control plane's turned on.
    grep -qx 'REHEARSAL=true' "$dir/station.env" || { echo "weavesignal.sh did not keep REHEARSAL=true" >&2; exit 1; }
    u="$units/$project-desk-rehearsal.container"
    [ -f "$u" ] || { echo "the rehearsal station has no rehearsal desk unit" >&2; exit 1; }
    grep -qx 'PodmanArgs=--oom-score-adj=500' "$u" || { echo "the rehearsal desk's unit is not at oom_score_adj 500" >&2; exit 1; }
    grep -q -- '--oom-score-adj=500' "$dir/dryrun.txt" || { echo "Podman's generator dropped the rehearsal desk's oom score" >&2; exit 1; }
    ! grep -q 'DESK_SOURCES_PASSWORD\|PLAYOUT_FEED_PASSWORD' "$u" || { echo "the rehearsal desk's unit carries a password it must not have" >&2; exit 1; }
    ! grep -q 'hls-media\|phone-words' "$u" || { echo "the rehearsal desk mounts the HLS or the phone words" >&2; exit 1; }
    grep -qx 'mode = "rehearsal"' "$dir/out/config/desk-rehearsal" && grep -qx 'room = "studio-rehearsal"' "$dir/out/config/desk-rehearsal" \
      && ! grep -q '^\[video\|^\[sources\|^\[programme' "$dir/out/config/desk-rehearsal" \
      || { echo "the rehearsal desk's settings are not a rehearsal desk's with no way to the air" >&2; exit 1; }
    grep -A1 -x '\[rehearsal\]' "$dir/out/config/control" | grep -qx 'enabled = true' || { echo "the control plane's rehearsal studio is not on" >&2; exit 1; }
  else
    [ ! -f "$units/$project-desk-rehearsal.container" ] || { echo "a $variant station without the rehearsal studio has a rehearsal desk unit" >&2; exit 1; }
    grep -A1 -x '\[rehearsal\]' "$dir/out/config/control" | grep -qx 'enabled = false' || { echo "the rehearsal studio is not off by default" >&2; exit 1; }
  fi
  if [ "$variant" = public ]; then
    grep -q "^Environment=WEAVESIGNAL_INGEST_BIND=0.0.0.0$" "$units/$project-ingest.container" || { echo "the public station's ingest does not take every address" >&2; exit 1; }
    [ -f "$units/$project-proxy.container" ] || { echo "the public station has no proxy unit" >&2; exit 1; }
    grep -q '^HealthCmd=test -s .*\.crt && test -s .*\.key' "$units/$project-proxy.container" || { echo "the proxy's health check is not the certificate and key" >&2; exit 1; }
    grep -q "$project-proxy.service" "$units/$project-livekit.container" || { echo "LiveKit's unit does not wait for the proxy" >&2; exit 1; }
  else
    grep -q "^Environment=WEAVESIGNAL_INGEST_BIND=127.0.0.1$" "$units/$project-ingest.container" || { echo "the loopback station's ingest is not on loopback" >&2; exit 1; }
    [ ! -f "$units/$project-proxy.container" ] || { echo "the loopback station has a proxy unit" >&2; exit 1; }
  fi
  echo "pass  $variant: $containers containers and $volumes volumes become $services services; no secret in any file"
done
