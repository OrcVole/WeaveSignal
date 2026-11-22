#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# No outside STUN by default (ADR-0290), shown by what leaves the station rather than by its
# settings. The Compose bundle runs with the example settings (.env.example; only the secrets
# and the image are filled in, as `just test-bundle` does) in one network namespace of its own
# (tests/no-stun/netns.yaml), so every packet that leaves the station crosses one interface, and
# tcpdump captures it there under `podman unshare`: rootless, no sudo. A person joins the studio
# room (GStreamer's livekitwebrtcsink, publish-only, inside the namespace, its own STUN pointed
# at 127.0.0.1:9 so it never asks one) and the desk must hear them: the desk's WebRTC
# connections are made, so its STUN setting is used, not merely read.
#
#   Phase 1, the defaults: the player's settings (relay.stun_servers) name no STUN server, and the capture holds no
#     STUN request (the magic cookie, any UDP port; TCP to 3478, 5349 or 19302-19309) and no
#     lookup of a name with "stun" in it (stun.l.google.com, global.stun.twilio.com ...).
#   Phase 2, a STUN server named (STUN_SERVERS=stun-local:3478, a small responder in a container
#     of its own on the project's network): the player names it, and the capture must hold the
#     station's binding requests to it, and none to any other server. So phase 1 cannot pass
#     because the capture or the detector sees nothing.
#
# Each phase ends with a canary: one binding request the test itself sends from the namespace
# to TEST-NET-1 (192.0.2.1), which the judge (tests/no-stun/analyse.py) must find, or the
# capture was not live. The packets, the judge's report, the sockets with their processes and
# the containers' logs are kept in tests/no-stun/run/.
#
# What it does not cover: the public route (public.yaml: Caddy, TURN), the phone line
# (phone.yaml), Cloudron's package and the Quadlets (generated from this bundle), and browsers:
# the join pages are covered only by what the station tells them (`/api/v1/player`). LiveKit
# here has `use_external_ip` false as the bundle sets it; the ICE servers it hands its own
# clients are read from its signalling on loopback and reported, not judged.
#
#   WEAVESIGNAL_SERVER_IMAGE=localhost/weavesignal-server:mine tests/no-stun/run.sh
#   WATCH_S=60 tests/no-stun/run.sh        # watch each phase for longer after the person is heard
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=tests/oom-score-adj.sh
. "$root/tests/oom-score-adj.sh"
here="$root/tests/no-stun"
run="$here/run"
project=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-nostun
image=${WEAVESIGNAL_SERVER_IMAGE:-localhost/weavesignal-server:m1}
WATCH_S=${WATCH_S:-30}
CANARY=192.0.2.1
ADMIN=nostun-admin-test
GUEST=elspeth

podman image exists "$image" || { echo "no image $image: run 'just server-image' (or set WEAVESIGNAL_SERVER_IMAGE)" >&2; exit 1; }
command -v tcpdump >/dev/null || { echo "tcpdump is needed on this machine (it runs in the station's namespace under podman unshare)" >&2; exit 1; }
rm -rf "$run"; mkdir -p "$run"; chmod 777 "$run"

fail() { echo "FAIL: $*" >&2; echo "logs and captures in $run" >&2; exit 1; }
files=(-f compose.yaml -f "$here/netns.yaml")
compose() { (cd "$root/deploy/compose" && podman compose --env-file "$env" -p "$project" "${files[@]}" "$@") 2> >(grep -v -E -e 'Executing external compose provider|^\s*(Container|Network|Volume) |^\s*$|^.\[0m$' >&2); }
# A command in the station's namespace, from this machine (its curl, python3, ss and tcpdump).
ns() { podman unshare nsenter -t "$nspid" -n "$@"; }
api() { ns curl -fs -H "Authorization: Bearer $ADMIN" -H 'Content-Type: application/json' "$@"; }
captures=()
stop_captures() {
  for f in "${captures[@]}"; do
    [ -s "$f" ] && kill -INT "$(cat "$f")" 2>/dev/null || true
  done
  sleep 1; captures=()
}
down() {
  stop_captures
  if [ -n "${env:-}" ] && [ -f "$env" ]; then
    podman logs "$project-guest" > "$run/${phase:-x}-guest.log" 2>&1 || true
    compose logs --no-color > "$run/${phase:-x}-compose.log" 2>&1 || true
  fi
  podman rm -f "$project-guest" >/dev/null 2>&1 || true
  [ -n "${env:-}" ] && [ -f "$env" ] && { compose down -v -t 5 >/dev/null 2>&1 || true; }
  env=
  return 0
}
trap down EXIT
trap 'exit 130' INT TERM

# phase NAME [STUN_SERVERS]
phase() {
  phase=$1; local stun=${2:-}
  env="$run/$phase.env"
  sed -e 's/^STATION_ID=.*/STATION_ID=6b2f0c9e-41d3-4f6e-8a7c-2d5e9f1a3b40/' \
      -e 's/^LIVEKIT_API_SECRET=.*/LIVEKIT_API_SECRET=nostun-test-only-secret-not-for-use-0/' \
      -e 's/^LISTENERS_SOURCE_PASSWORD=.*/LISTENERS_SOURCE_PASSWORD=nostun-test-only/' \
      -e 's/^POSTGRES_PASSWORD=.*/POSTGRES_PASSWORD=nostun-test-only/' \
      -e "s/^CONTROL_ADMIN_TOKEN=.*/CONTROL_ADMIN_TOKEN=$ADMIN/" \
      -e 's/^CHAT_EMAIL_KEY=.*/CHAT_EMAIL_KEY=nostun-test-only-chat-email-key-0123456789/' \
      -e 's/^INGEST_TOKEN=.*/INGEST_TOKEN=nostun-test-only-ingest-token-0123456789/' \
      -e 's/^DESK_SOURCES_PASSWORD=.*/DESK_SOURCES_PASSWORD=nostun-test-only-desk-sources-0123456789/' \
      -e "s|^SERVER_IMAGE=.*|SERVER_IMAGE=$image|" \
      "$root/deploy/compose/.env.example" > "$env"
  # The example names no STUN server; a named one is the only setting phase 2 adds.
  grep -q '^STUN_SERVERS=' "$env" && fail ".env.example sets STUN_SERVERS: the defaults are not 'none'"
  files=(-f compose.yaml -f "$here/netns.yaml")
  if [ -n "$stun" ]; then
    echo "STUN_SERVERS=$stun" >> "$env"
    files+=(-f "$here/stun-local.yaml")
  fi
  # shellcheck disable=SC1090
  . <(grep -E '^(LIVEKIT_HTTP_PORT|CONTROL_PORT|ROOM)=' "$env")

  # The namespace first, and its captures, so the station's start is captured too.
  compose up -d netns >/dev/null
  holder=$project-netns-1
  nspid=$(podman inspect -f '{{.State.Pid}}' "$holder")
  own_ip=$(podman inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$holder")
  [ -n "$own_ip" ] && [ "$nspid" -gt 0 ] || fail "the station's namespace did not start"
  # Everything on eth0; on loopback, LiveKit's signalling only (what it tells its clients).
  podman unshare nsenter -t "$nspid" -n sh -c 'tcpdump -U -Z root -n -i eth0 -w "$1" 2>"$1.log" & echo $! > "$2"; wait' \
    _ "$run/$phase.pcap" "$run/$phase.cap.pid" &
  podman unshare nsenter -t "$nspid" -n sh -c 'tcpdump -U -Z root -n -i lo -w "$1" "tcp port $3" 2>"$1.log" & echo $! > "$2"; wait' \
    _ "$run/$phase-lo.pcap" "$run/$phase-lo.cap.pid" "$LIVEKIT_HTTP_PORT" &
  captures=("$run/$phase.cap.pid" "$run/$phase-lo.cap.pid")
  for _ in $(seq 1 20); do [ -s "$run/$phase.cap.pid" ] && [ -s "$run/$phase-lo.cap.pid" ] && grep -q listening "$run/$phase.pcap.log" 2>/dev/null && break; sleep 0.5; done
  grep -q listening "$run/$phase.pcap.log" || fail "tcpdump did not start in the station's namespace: $(cat "$run/$phase.pcap.log")"

  compose up -d >/dev/null
  [ "$(podman inspect -f '{{.State.Pid}}' "$holder")" = "$nspid" ] || fail "Compose made the namespace again: the capture lost it"
  for i in $(seq 1 90); do
    api "http://127.0.0.1:$CONTROL_PORT/health" 2>/dev/null | grep -q '"database":"ok"' && break
    sleep 1; [ "$i" = 90 ] && { compose logs control | tail -20; fail "the control plane did not come up with its database"; }
  done

  # What the station tells the join pages (ADR-0290: none unless named).
  local want='[]'; [ -n "$stun" ] && want=$(jq -cn --arg s "$stun" '[$s]')
  got=$(api "http://127.0.0.1:$CONTROL_PORT/api/v1/player" | jq -c '.relay.stun_servers')
  [ "$got" = "$want" ] || fail "the player's relay.stun_servers are $got, not $want"
  echo "  the join pages are told STUN servers $got"

  # A person in the studio room, publish-only, as a native publisher: the desk subscribes to them
  # and publishes their return, so its WebRTC connections are made with its STUN setting.
  tok=$(api -X POST -d "{\"identity\": \"$GUEST\", \"role\": \"presenter\", \"name\": \"Elspeth Gordon\", \"subscribe\": false}" \
    "http://127.0.0.1:$CONTROL_PORT/api/v1/room/tokens") || fail "no room token"
  podman run -d --name "$project-guest" --network "container:$holder" --security-opt label=disable \
    --entrypoint gst-launch-1.0 "$image" -q \
    audiotestsrc is-live=true wave=sine freq=440 volume=0.3 ! audioconvert ! audioresample \
    ! livekitwebrtcsink stun-server=stun://127.0.0.1:9 \
    "signaller::ws-url=ws://127.0.0.1:$LIVEKIT_HTTP_PORT" "signaller::auth-token=$(jq -r .token <<<"$tok")" \
    "signaller::room-name=$(jq -r .room <<<"$tok")" >/dev/null
  for i in $(seq 1 60); do
    api "http://127.0.0.1:$CONTROL_PORT/api/v1/studio" \
      | jq -e --arg i "$GUEST" '[.. | objects | select(.identity? == $i and .lufs? != null)] | length > 0' >/dev/null 2>&1 && break
    sleep 1; [ "$i" = 60 ] && { podman logs "$project-guest" 2>&1 | tail -10; compose logs desk | tail -20; fail "the desk never heard the person in the room"; }
  done
  echo "  the desk hears $GUEST (its WebRTC connections are made); watching for ${WATCH_S} s"
  sleep "$WATCH_S"
  if [ -n "$stun" ]; then
    for i in $(seq 1 30); do
      compose logs stun-local 2>/dev/null | grep -q 'binding request from' && break
      sleep 1
    done
    echo "  stun-local answered $(compose logs stun-local 2>/dev/null | grep -c 'binding request from') binding request(s)"
  fi
  # Which process holds each UDP port, while the connections are still up.
  ns ss -H -uanp > "$run/$phase-ss.txt" 2>&1 || true
  # The canary: one binding request from the namespace to TEST-NET-1, which nobody routes.
  ns python3 -I -c 'import os, socket, sys; socket.socket(socket.AF_INET, socket.SOCK_DGRAM).sendto(bytes.fromhex("000100002112a442") + os.urandom(12), (sys.argv[1], 3478))' "$CANARY"
  sleep 1
  stop_captures

  echo "  LiveKit's signalling named these ICE servers to its clients: $(python3 -I -c '
import re, sys
b = open(sys.argv[1], "rb").read()
found = sorted({m.decode() for m in re.findall(rb"(?:stuns?|turns?):[A-Za-z0-9._-]+(?::[0-9]{1,5})?", b)})
print(", ".join(found) or "none")' "$run/$phase-lo.pcap")"
  local judge=(python3 -I "$here/analyse.py" "$run/$phase.pcap" --own-ip "$own_ip" --canary "$CANARY"
               --senders "$run/$phase-ss.txt" --report "$run/$phase-report.json")
  if [ -n "$stun" ]; then
    stun_ip=$(podman inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$project-stun-local-1")
    judge+=(--expect-stun "$stun_ip" --expect-name "${stun%%:*}")
  fi
  # tcpdump wrote the files as this user's namespace root, which is this user.
  "${judge[@]}" || fail "phase $phase: see $run/$phase-report.json"
  down
}

echo "1. the defaults: nothing asks an outside STUN server"
phase defaults
echo "2. STUN_SERVERS=stun-local:3478: the station asks that server, and the capture sees it"
phase named stun-local:3478
echo "PASS: no STUN request leaves a station on the defaults; a named server is used and seen"
