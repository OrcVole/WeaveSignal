#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Phone line spike (17f, module 0): LiveKit's SIP service v1.25.0 against our pinned
# livekit-server 1.13.10, with Valkey, and baresip as the provider and the caller in one. No
# provider account, nothing outside this machine: everything listens on loopback, on ports
# below Linux's ephemeral range. It answers the design note's three inferences:
#   1. a caller's key press (RFC 4733, from baresip) reaches the desk's own LiveKit elements
#      (probe.py in the desk image) as a SipDTMF data packet;
#   2. the call is answered (200 OK) only once someone in the booth publishes audio for the
#      caller (the source, inbound.go and room.go at v1.25.0, says so; this checks it runs so),
#      and a booth nobody publishes into keeps ringing until `ringing_timeout`;
#   3. v1.25.0 works with server 1.13.10 and Redis mode, and makes the identity and room forms
#      read from livekit/protocol (`sip_` + 16 hex of SHA-256 of the From user; the room
#      `<prefix>_<last four>_<random>`); and what each process logs of the number.
#
# One stack at a time on this machine: it takes the stack lock, and runs everything niced.
#   tests/phone/spike/spike.sh OUT_DIR
# OUT_DIR gets every log and the result lines; the throwaway keys live there (mode 600) only
# for the run and are removed at the end.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out=${1:?usage: spike.sh OUT_DIR}
mkdir -p "$out"; out=$(cd "$out" && pwd)
lock=${STACK_LOCK:-$HOME/Code/weavesignal-release-tools/1.1.1/stack.lock}
exec 9>"$lock"
flock -w "${FLOCK_WAIT:-600}" 9 || { echo "the stack lock is busy" >&2; exit 1; }

P=${WEAVESIGNAL_TEST_PREFIX:-ws17f-spike}
VALKEY_IMG=${VALKEY_IMG:-docker.io/valkey/valkey:8.1-alpine}
LK_IMG=docker.io/livekit/livekit-server:v1.13.10
SIP_IMG=${SIP_IMG:-docker.io/livekit/sip:v1.25.0}
CLI_IMG=${CLI_IMG:-docker.io/livekit/livekit-cli:latest}
DESK_IMG=${DESK_IMG:-localhost/weavesignal-desk-spike:dev}
CALLER_IMG=${CALLER_IMG:-docker.io/library/debian:13}
REDIS_PORT=16379 LK_PORT=17880 LK_TCP=17881 LK_UDP_A=17900 LK_UDP_B=17949
SIP_PORT=15060 RTP_A=17340 RTP_B=17355 CALLER_SIP=15070
FROM_USER=+447700900123          # Ofcom's drama range: never a real line
TO_USER=+441314960000            # Ofcom's drama range for Edinburgh
ROOM_PREFIX=spike-phone
N=${N:-20}                       # keys to press

umask 077
key=spikekey
secret=$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')
rpass=$(head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n')
run() { nice -n 10 podman run "$@"; }
stamp() { date -u +%H:%M:%S.%3N; }
note() { echo "$(stamp) $*" | tee -a "$out/result.txt"; }
cleanup() {
  for c in caller probe sip lk valkey; do podman rm -f "$P-$c" >/dev/null 2>&1 || true; done
  rm -f "$out/livekit.yaml" "$out/sip.yaml" "$out/keys.env"
}
trap cleanup EXIT

for port in $REDIS_PORT $LK_PORT $LK_TCP $SIP_PORT $CALLER_SIP; do
  if ss -Hltun "( sport = :$port )" | grep -q .; then echo "port $port is taken" >&2; exit 1; fi
done

cat > "$out/livekit.yaml" <<YAML
port: $LK_PORT
bind_addresses: [127.0.0.1]
rtc:
  tcp_port: $LK_TCP
  port_range_start: $LK_UDP_A
  port_range_end: $LK_UDP_B
  node_ip: 127.0.0.1
  use_external_ip: false
redis:
  address: 127.0.0.1:$REDIS_PORT
  password: $rpass
keys:
  $key: $secret
logging:
  level: info
YAML
cat > "$out/sip.yaml" <<YAML
api_key: $key
api_secret: $secret
ws_url: ws://127.0.0.1:$LK_PORT
redis:
  address: 127.0.0.1:$REDIS_PORT
  password: $rpass
sip_port: $SIP_PORT
sip_port_listen: $SIP_PORT
listen_ip: 127.0.0.1
rtp_port: $RTP_A-$RTP_B
nat_1_to_1_ip: 127.0.0.1
use_external_ip: false
health_port: 0
prometheus_port: 0
logging:
  level: debug
YAML

note "valkey: $VALKEY_IMG, no snapshots, no append-only file, loopback, password"
run -d --name "$P-valkey" --network host "$VALKEY_IMG" valkey-server --bind 127.0.0.1 \
  --port $REDIS_PORT --requirepass "$rpass" --save "" --appendonly no >/dev/null
run -d --name "$P-lk" --network host -v "$out/livekit.yaml:/lk.yaml:ro,Z" "$LK_IMG" --config /lk.yaml >/dev/null
for _ in $(seq 1 60); do curl -fs "http://127.0.0.1:$LK_PORT/" >/dev/null 2>&1 && break; sleep 0.5; done
note "livekit-server: $(podman logs "$P-lk" 2>&1 | grep -m1 -o '"version": *"[^"]*"' || echo 'version not logged')"
run -d --name "$P-sip" --network host -v "$out/sip.yaml:/sip/config.yaml:ro,Z" "$SIP_IMG" >/dev/null
sleep 4
podman logs "$P-sip" > "$out/sip-start.log" 2>&1 || true
if ! podman ps --format '{{.Names}}' | grep -qx "$P-sip"; then note "FAIL 3: livekit-sip exited at start"; exit 1; fi
note "livekit-sip $SIP_IMG is up"

lk() { run --rm --network host -v "$out:/w:z" "$CLI_IMG" --url "http://127.0.0.1:$LK_PORT" --api-key "$key" --api-secret "$secret" "$@"; }
cat > "$out/trunk.json" <<JSON
{"trunk": {"name": "spike", "numbers": ["$TO_USER"], "allowed_addresses": ["127.0.0.1/32"],
  "ringing_timeout": "45s", "max_call_duration": "300s", "ringing_timeout_status": "SIP_STATUS_BUSY_HERE"}}
JSON
cat > "$out/rule.json" <<JSON
{"dispatch_rule": {"name": "spike", "hide_phone_number": true,
  "rule": {"dispatch_rule_individual": {"room_prefix": "$ROOM_PREFIX"}},
  "attributes": {"weavesignal.role": "caller", "weavesignal.line": "phone"}}}
JSON
lk sip inbound create /w/trunk.json > "$out/trunk.out" 2>&1 && note "inbound trunk made" || { note "FAIL 3: trunk refused: $(tail -2 "$out/trunk.out")"; exit 1; }
lk sip dispatch create /w/rule.json > "$out/rule.out" 2>&1 && note "dispatch rule made" || { note "FAIL 3: rule refused: $(tail -2 "$out/rule.out")"; exit 1; }

# The caller: baresip, a 1 kHz tone as its microphone, what it hears to a file, keys from a FIFO.
ffmpeg -loglevel error -y -f lavfi -i "sine=frequency=1000:duration=120" -ar 8000 -ac 1 "$out/caller.wav"
mkdir -p "$out/baresip"; mkfifo "$out/caller.in"
cat > "$out/baresip/config" <<CFG
sip_listen 127.0.0.1:$CALLER_SIP
audio_source aufile,/w/caller.wav
audio_player aufile,/w/heard.wav
module_path /usr/lib/baresip/modules
module stdio.so
module g711.so
module aufile.so
module_app account.so
module_app menu.so
CFG
echo "<sip:$FROM_USER@127.0.0.1>;regint=0;audio_codecs=PCMU,PCMA" > "$out/baresip/accounts"
run -d --name "$P-caller" --network host -i -v "$out:/w:z" "$CALLER_IMG" sh -c \
  "apt-get -qq update >/dev/null && DEBIAN_FRONTEND=noninteractive apt-get -qq install -y baresip-core >/dev/null && \
   exec baresip -f /w/baresip < /w/caller.in" >/dev/null
exec 8<>"$out/caller.in"   # read-write: the open never waits for baresip
for _ in $(seq 1 120); do podman logs "$P-caller" 2>&1 | grep -qi "baresip is ready\|ua: " && break; sleep 1; done
sleep 2
note "dialling sip:$TO_USER@127.0.0.1:$SIP_PORT from $FROM_USER"
echo "/dial sip:$TO_USER@127.0.0.1:$SIP_PORT" >&8
room=""
for _ in $(seq 1 40); do
  room=$(lk room list 2>/dev/null | grep -o "$ROOM_PREFIX[^ │|]*" | head -1 || true)
  [ -n "$room" ] && break; sleep 0.5
done
[ -n "$room" ] || { note "FAIL 3: no phone room appeared"; exit 1; }
note "room: $room"
lk room participants list "$room" > "$out/participants.txt" 2>&1 || true
want="sip_$(printf %s "$FROM_USER" | sha256sum | cut -c1-16)"
grep -q "$want" "$out/participants.txt" && note "PASS 3: identity is $want (sip_ + 16 hex of SHA-256 of the From user)" \
  || note "CHECK 3: identity not $want: see participants.txt"
case "$room" in "${ROOM_PREFIX}_${FROM_USER: -4}_"*) note "PASS 3: room form ${ROOM_PREFIX}_<last four>_<random>";;
  *) note "CHECK 3: room form differs: $room";; esac

# 2: nobody publishes into the booth for 10 s: the caller should still be ringing. (The trunk
# and rule bodies are crates/control/src/phone/trunk.rs's, with a shorter ringing timeout.)
sleep 10
if podman logs "$P-caller" 2>&1 | grep -qi "established"; then note "FAIL 2: answered before anyone published into the booth"
else note "PASS 2a: still ringing after 10 s with no publisher in the booth"; fi

# The desk's stand-in joins: subscriber on the caller, publisher of a tone for the caller only.
sub=$(lk token create --join --room "$room" --identity desk-probe-in --valid-for 10m 2>/dev/null | grep -o 'eyJ[^ ]*' | head -1)
pub=$(lk token create --join --room "$room" --identity desk-probe-out --valid-for 10m --grant '{"canSubscribe": false}' 2>/dev/null | grep -o 'eyJ[^ ]*' | head -1)
t_pub=$(stamp)
run -d --name "$P-probe" --network host -v "$here:/h:ro,Z" "$DESK_IMG" \
  python3 -u /h/probe.py "ws://127.0.0.1:$LK_PORT" "$sub" "$pub" "$want" $((N + 25)) >/dev/null
note "probe started at $t_pub (it publishes the tone at once)"
for _ in $(seq 1 30); do podman logs "$P-caller" 2>&1 | grep -qi "established" && break; sleep 0.5; done
if podman logs "$P-caller" 2>&1 | grep -qi "established"; then note "PASS 2b: answered once the booth had a publisher for the caller ($(stamp))"
else note "FAIL 2b: not answered after the probe published"; fi

# 1: N keys, one a second.
sleep 3
for i in $(seq 1 "$N"); do d=$(( i % 10 )); echo "$d" >&8; sleep 1; done
sleep 5
podman logs "$P-probe" > "$out/probe.log" 2>&1 || true
got=$(grep -c "SIP DTMF" "$out/probe.log" || true)
note "keys pressed: $N; SipDTMF packets at the desk's subscriber: $got"
[ "$got" -ge "$N" ] && note "PASS 1" || note "FAIL 1 (see probe.log for the data channels it saw)"

echo "/hangup" >&8; sleep 2; exec 8>&-
podman logs "$P-caller" > "$out/caller.log" 2>&1 || true
podman logs "$P-sip" > "$out/sip.log" 2>&1 || true
podman logs "$P-lk" > "$out/livekit.log" 2>&1 || true
for f in sip.log livekit.log probe.log; do
  n=$(grep -c -- "${FROM_USER#+}" "$out/$f" || true)
  note "the number appears $n times in $f"
done
note "done"
