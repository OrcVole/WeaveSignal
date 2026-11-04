#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The edge stack test (M7): bench 02's shape at a smaller scale, on loopback.
#
#   HLS    ffmpeg writes a live HLS test stream (2 s segments, six in the playlist) that a
#          stand-in origin (nginx, logging every request) serves; two edges (the weavesignal-edge
#          image) pull it through; the control plane steers viewers between them
#          (/api/v1/steering, edges as clones of the playlist's pathway, health checked every
#          second). A clone replaces the host name only, as hls.js does, so the edges listen
#          on the origin's port at their own loopback addresses (127.0.0.2 and 127.0.0.3). VIEWERS simulated players join over RAMP seconds; edge 1 is killed at
#          KILL_AT and started again at RESTART_AT. Half the players penalise a failed edge at
#          once, as hls.js does; the other half never penalise, so only the steering manifest
#          can move them.
#   Audio  the listener process (playout/listeners.liq, icecast.server) fed a tone as playout
#          feeds it; a relay (the weavesignal-relay image) in front of it, heard directly and
#          through edge 2's /listen/; and a plain Icecast origin sending a new title every 3 s
#          through a second relay, to show the relay passes in-band metadata (icecast.server's
#          listener process strips it, ADR-0044 and the third fault).
#
# Measured: origin egress against edge egress, how fast the control plane drops a dead edge
# and takes it back, how long each moved viewer went without a segment, and what each
# listener heard. Ports sit in 23400-23499 by default, below Linux's ephemeral range and clear
# of the station defaults and the other stack tests. Needs the images (`just edge-image`),
# ffmpeg with libx264 and libopus, and python3.
#
#   ORIGIN_PORT=23410 EDGE1_ADDR=127.0.0.2 EDGE2_ADDR=127.0.0.3 CONTROL_PORT=23413 LISTENERS_PORT=23414 \
#   FEED_PORT=23419 RELAY_PORT=23416 ICE_PORT=23417 RELAY2_PORT=23418 tests/edges/run-edges.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
here="$root/tests/edges"
run="$here/run"
ORIGIN_PORT=${ORIGIN_PORT:-23410}
EDGE1_ADDR=${EDGE1_ADDR:-127.0.0.2}
EDGE2_ADDR=${EDGE2_ADDR:-127.0.0.3}
CONTROL_PORT=${CONTROL_PORT:-23413}
LISTENERS_PORT=${LISTENERS_PORT:-23414}
FEED_PORT=${FEED_PORT:-23419}
RELAY_PORT=${RELAY_PORT:-23416}
ICE_PORT=${ICE_PORT:-23417}
RELAY2_PORT=${RELAY2_PORT:-23418}
VIEWERS=${VIEWERS:-400}
PROCS=${PROCS:-4}
RAMP=${RAMP:-50}
VIEW_SECONDS=${VIEW_SECONDS:-30}
KILL_AT=${KILL_AT:-20}
RESTART_AT=${RESTART_AT:-40}
TTL=${STEERING_TTL:-10}
EDGE_IMAGE=${EDGE_IMAGE:-localhost/weavesignal-edge:m1}
RELAY_IMAGE=${RELAY_IMAGE:-localhost/weavesignal-relay:m1}
LIQ_IMAGE=${LIQ_IMAGE:-docker.io/savonet/liquidsoap:v2.5.0-rc1}
SOURCE_PASSWORD=edges-test-only
ADMIN=edges-test-admin
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-edges

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$ORIGIN_PORT" "$CONTROL_PORT" "$LISTENERS_PORT" "$FEED_PORT" "$RELAY_PORT" "$ICE_PORT" "$RELAY2_PORT"; do
  taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }
done
# icecast.server also holds the listener port + 1 (its legacy source port).
[ "$FEED_PORT" != $((LISTENERS_PORT + 1)) ] || { echo "FEED_PORT cannot be LISTENERS_PORT + 1: icecast.server holds it" >&2; exit 1; }
for p in $((LISTENERS_PORT + 1)); do taken -t "$p" && { echo "port $p/tcp (the listener port + 1, which icecast.server takes) is in use" >&2; exit 1; }; done
for img in "$EDGE_IMAGE" "$RELAY_IMAGE"; do
  podman image exists "$img" || { echo "$img is missing: run \`just edge-image\`" >&2; exit 1; }
done

containers="origin edge1 edge2 listeners source relay ice source2 relay2"
cleanup() {
  for pid in ${encoder:-} ${control:-}; do kill "$pid" 2>/dev/null || true; done
  for c in $containers; do
    podman logs "$prefix-$c" > "$run/$c.log" 2>&1 || true
    podman rm -f -t 0 "$prefix-$c" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT
rm -rf "$run"; mkdir -p "$run/hls" "$run/logs"
chmod 0777 "$run/logs"
cleanup

until_ok() { # <tries of 0.25 s> <what> <command...>
  local n=$1 what=$2; shift 2
  for _ in $(seq 1 "$n"); do "$@" >/dev/null 2>&1 && return 0; sleep 0.25; done
  echo "$what did not come up" >&2; return 1
}
code() { curl -s -o /dev/null -m 2 -w '%{http_code}' "$1" 2>/dev/null || true; }
serves() { [ "$(code "$1")" = 200 ]; }
streams() { curl -s -o /dev/null -m 1 "$1" 2>/dev/null; [ $? = 28 ]; } # still sending at 1 s

# ---- The HLS origin: a live test stream, served by a stand-in nginx ----
ffmpeg -v error -re -f lavfi -i "testsrc2=size=854x480:rate=30" -f lavfi -i "sine=frequency=440" \
  -c:v libx264 -preset veryfast -tune zerolatency -b:v 800k -maxrate 800k -bufsize 1600k -g 60 -keyint_min 60 -sc_threshold 0 \
  -c:a libopus -b:a 96k -ac 2 \
  -f hls -hls_time 2 -hls_list_size 6 -hls_segment_type fmp4 -hls_flags delete_segments+independent_segments \
  -hls_fmp4_init_filename init.mp4 -hls_segment_filename "$run/hls/seg-%05d.m4s" "$run/hls/live.m3u8" \
  > "$run/encoder.log" 2>&1 &
encoder=$!
# The multivariant playlist the video path will write (M5): one variant, the steering tag.
cat > "$run/hls/master.m3u8" <<M3U8
#EXTM3U
#EXT-X-CONTENT-STEERING:SERVER-URI="http://127.0.0.1:$CONTROL_PORT/api/v1/steering",PATHWAY-ID="."
#EXT-X-STREAM-INF:BANDWIDTH=1000000,CODECS="avc1.64001f,opus"
live.m3u8
M3U8
sed "s/ORIGINPORT/$ORIGIN_PORT/" "$here/origin-nginx.conf" > "$run/origin.conf"
podman run -d --name "$prefix-origin" --network host --security-opt label=disable \
  -v "$run/origin.conf:/etc/nginx/origin.conf:ro" -v "$run/hls:/srv/hls:ro" -v "$run/logs:/logs" \
  --entrypoint nginx "$EDGE_IMAGE" -c /etc/nginx/origin.conf -g 'daemon off;' >/dev/null
for n in 1 2; do
  addr_var=EDGE${n}_ADDR
  extra=()
  [ "$n" = 2 ] && extra=(-e "EDGE_RELAY=127.0.0.1:$RELAY_PORT")
  podman run -d --name "$prefix-edge$n" --network host \
    -e "EDGE_ORIGIN=http://127.0.0.1:$ORIGIN_PORT" -e "EDGE_HTTP_PORT=$ORIGIN_PORT" -e "EDGE_BIND=${!addr_var}" \
    -e EDGE_WORKERS=2 "${extra[@]}" "$EDGE_IMAGE" >/dev/null
done

# ---- The control plane, steering between the two edges ----
cat > "$run/control.toml" <<TOML
[station]
id = "5b1e0c9a-7f1d-4c55-8a42-3e9b6d2f0e17"
name = "Kettle FM"

[ports]
control = $CONTROL_PORT

[log]
format = "pretty"
filter = "info,weavesignal_control::steering=debug"

[steering]
ttl_s = $TTL
check_every_s = 1
check_timeout_ms = 500
down_after = 2
clone_from = "."

[[steering.edges]]
pathway = "edge-1"
host = "$EDGE1_ADDR"
check_url = "http://$EDGE1_ADDR:$ORIGIN_PORT/health"

[[steering.edges]]
pathway = "edge-2"
host = "$EDGE2_ADDR"
check_url = "http://$EDGE2_ADDR:$ORIGIN_PORT/health"

# The origin, after the pool: only when both edges are down.
[[steering.edges]]
pathway = "."
weight = 0
TOML
cargo build -q -p weavesignal-control
WEAVESIGNAL_CONTROL_ADMIN_TOKEN=$ADMIN WEAVESIGNAL_DATABASE_URL= WEAVESIGNAL_LIVEKIT_API_SECRET= \
  "$root/target/debug/weavesignal-control" --config "$run/control.toml" serve > "$run/control.log" 2>&1 &
control=$!

# ---- Audio: the listener process fed as playout feeds it, and a relay in front ----
cat > "$run/listeners.json" <<JSON
{ "listeners": { "bind": "127.0.0.1", "host": "127.0.0.1", "port": $LISTENERS_PORT, "mount": "programme.opus", "feed_port": $FEED_PORT } }
JSON
podman run -d --name "$prefix-listeners" --network host --security-opt label=disable \
  -v "$root/playout:/opt/weavesignal/playout:ro" -v "$run/listeners.json:/etc/weavesignal/listeners.json:ro" \
  -e WEAVESIGNAL_PLAYOUT_DATA=/etc/weavesignal/listeners.json -e WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD=$SOURCE_PASSWORD \
  --entrypoint /usr/bin/liquidsoap "$LIQ_IMAGE" /opt/weavesignal/playout/listeners.liq >/dev/null
source_run() { # <name> <format> <port> <mount> <title every>
  podman run -d --name "$prefix-$1" --network host --security-opt label=disable -v "$here/source.liq:/t/source.liq:ro" \
    -e "EDGES_FORMAT=$2" -e EDGES_HOST=127.0.0.1 -e "EDGES_PORT=$3" -e "EDGES_PASSWORD=$SOURCE_PASSWORD" \
    -e "EDGES_MOUNT=$4" -e "EDGES_TITLE_EVERY=$5" --entrypoint /usr/bin/liquidsoap "$LIQ_IMAGE" /t/source.liq >/dev/null
}
until_ok 160 "the listener process's feed harbor" bash -c "ss -H -ltn 'sport = :$FEED_PORT' | grep -q ."
source_run source wav "$FEED_PORT" feed 0
# A plain Icecast as the origin of the metadata check (the relay image's Icecast, configured
# as a server that takes a source).
cat > "$run/ice.xml" <<XML
<icecast>
  <limits><sources>1</sources></limits>
  <authentication><source-password>$SOURCE_PASSWORD</source-password><relay-password>$SOURCE_PASSWORD-relay</relay-password><admin-password>$SOURCE_PASSWORD-admin</admin-password></authentication>
  <listen-socket><port>$ICE_PORT</port><bind-address>127.0.0.1</bind-address></listen-socket>
  <fileserve>0</fileserve>
  <paths><basedir>/usr/share/icecast</basedir><logdir>/tmp</logdir><webroot>/usr/share/icecast/web</webroot><adminroot>/usr/share/icecast/admin</adminroot></paths>
  <logging><accesslog>-</accesslog><errorlog>-</errorlog><loglevel>warning</loglevel></logging>
  <security><chroot>0</chroot></security>
</icecast>
XML
chmod 0644 "$run/ice.xml"
podman run -d --name "$prefix-ice" --network host --security-opt label=disable -v "$run/ice.xml:/etc/ice.xml:ro" \
  --entrypoint icecast "$RELAY_IMAGE" -c /etc/ice.xml >/dev/null
until_ok 40 "the plain Icecast origin" bash -c "ss -H -ltn 'sport = :$ICE_PORT' | grep -q ."
source_run source2 ogg "$ICE_PORT" meta.opus 3
# The relays start once their origin's mount answers: a relay that finds none waits a whole
# RELAY_RETRY_S (15 s) before it tries again.
until_ok 160 "the listener mount" streams "http://127.0.0.1:$LISTENERS_PORT/programme.opus"
until_ok 80 "the Icecast origin's mount" streams "http://127.0.0.1:$ICE_PORT/meta.opus"
relay_run() { # <name> <origin port> <mount> <port>
  podman run -d --name "$prefix-$1" --network host -e RELAY_ORIGIN_HOST=127.0.0.1 -e RELAY_ORIGIN_SCHEME=http -e "RELAY_ORIGIN_PORT=$2" \
    -e "RELAY_MOUNTS=$3" -e "RELAY_PORT=$4" -e RELAY_BIND=127.0.0.1 "$RELAY_IMAGE" >/dev/null
}
relay_run relay "$LISTENERS_PORT" /programme.opus "$RELAY_PORT"
relay_run relay2 "$ICE_PORT" /meta.opus "$RELAY2_PORT"

until_ok 80 "the control plane" serves "http://127.0.0.1:$CONTROL_PORT/health"
until_ok 80 "the encoder's playlist" test -s "$run/hls/live.m3u8"
until_ok 80 "edge 1" serves "http://$EDGE1_ADDR:$ORIGIN_PORT/hls/live.m3u8"
until_ok 80 "edge 2" serves "http://$EDGE2_ADDR:$ORIGIN_PORT/hls/live.m3u8"
until_ok 80 "the relay" streams "http://127.0.0.1:$RELAY_PORT/programme.opus"
until_ok 80 "the second relay" streams "http://127.0.0.1:$RELAY2_PORT/meta.opus"
[ "$(code "http://$EDGE1_ADDR:$ORIGIN_PORT/health")" = 200 ] || { echo "edge 1's /health does not pass through the origin's 200" >&2; exit 1; }
# A viewer gets the edge's Cache-Control only, never the origin's beside it.
for pair in "live.m3u8|max-age=1" "$(curl -s "http://$EDGE1_ADDR:$ORIGIN_PORT/hls/live.m3u8" | grep -v '^#' | grep . | head -1)|max-age=60"; do
  path=${pair%|*}; want=${pair#*|}
  got=$(curl -sI -m 2 "http://$EDGE1_ADDR:$ORIGIN_PORT/hls/$path" | tr -d '\r' | grep -i '^cache-control:' | sed 's/^[^:]*: *//' | paste -sd'|')
  [ "$got" = "$want" ] || { echo "edge 1 sent Cache-Control '$got' for /hls/$path, not only '$want'" >&2; exit 1; }
done
echo "edge 1 sends its own Cache-Control only"

[ "$(code "http://$EDGE1_ADDR:$ORIGIN_PORT/api/v1/station")" = 404 ] || { echo "edge 1 serves a path outside /hls/" >&2; exit 1; }
echo "stack up: origin 127.0.0.1:$ORIGIN_PORT, edges $EDGE1_ADDR and $EDGE2_ADDR on the same port, control :$CONTROL_PORT, relays :$RELAY_PORT :$RELAY2_PORT"

# ---- The run ----
total=$((RAMP + VIEW_SECONDS))
python3 -I "$here/ogg-listen.py" 127.0.0.1 "$RELAY_PORT" /programme.opus "$total" > "$run/listen-relay.json" &
l1=$!
python3 -I "$here/ogg-listen.py" "$EDGE2_ADDR" "$ORIGIN_PORT" /listen/programme.opus "$total" > "$run/listen-edge.json" &
l2=$!
python3 -I "$here/ogg-listen.py" 127.0.0.1 "$RELAY2_PORT" /meta.opus 20 > "$run/listen-meta.json" &
l3=$!
ffmpeg -v info -nostats -t 5 -i "http://127.0.0.1:$RELAY_PORT/programme.opus" -af volumedetect -f null - > "$run/volume.log" 2>&1 &
l4=$!
t0=$(date +%s.%N)
pids=()
half=$((PROCS / 2))
for i in $(seq 1 "$PROCS"); do
  penalty=120; [ "$i" -gt "$half" ] && penalty=0
  PENALTY_S=$penalty python3 -I "$here/hls-viewers.py" $((VIEWERS / PROCS)) "$VIEW_SECONDS" \
    "http://127.0.0.1:$ORIGIN_PORT/hls/master.m3u8" "$RAMP" > "$run/viewers-$i-p$penalty.json" &
  pids+=($!)
done
at() { while [ "$(echo "$(date +%s.%N) - $t0 < $1" | bc)" = 1 ]; do sleep 0.1; done; }
edge1_up() { curl -fs -H "Authorization: Bearer $ADMIN" "http://127.0.0.1:$CONTROL_PORT/api/v1/steering/edges" \
  | python3 -I -c 'import json,sys; print(str(next(e["up"] for e in json.load(sys.stdin)["edges"] if e["pathway"] == "edge-1")).lower())'; }
at "$KILL_AT"
podman kill "$prefix-edge1" >/dev/null
killed=$(date +%s.%N)
for _ in $(seq 1 200); do [ "$(edge1_up)" = false ] && break; sleep 0.05; done
dropped=$(date +%s.%N)
manifest=$(curl -fs "http://127.0.0.1:$CONTROL_PORT/api/v1/steering")
at "$RESTART_AT"
podman start "$prefix-edge1" >/dev/null
restarted=$(date +%s.%N)
for _ in $(seq 1 200); do [ "$(edge1_up)" = true ] && break; sleep 0.05; done
back=$(date +%s.%N)
wait "${pids[@]}"
t1=$(date +%s.%N)
wait "$l1" "$l2" "$l3" "$l4" || true
curl -fs -H "Authorization: Bearer $ADMIN" "http://127.0.0.1:$CONTROL_PORT/api/v1/steering/edges" > "$run/edges.json"

python3 -I - "$run" "$t0" "$t1" "$killed" "$dropped" "$restarted" "$back" "$manifest" <<'PY'
import glob, json, re, sys
run, t0, t1, killed, dropped, restarted, back = sys.argv[1], *map(float, sys.argv[2:8])
manifest = json.loads(sys.argv[8])
fail = []
def check(ok, what):
    print(("ok    " if ok else "FAIL  ") + what)
    if not ok: fail.append(what)

# HLS: origin against edge egress over the run.
groups = {}
for f in sorted(glob.glob(f"{run}/viewers-*.json")):
    d = json.load(open(f)); g = "penalise" if f.endswith("-p120.json") else "steering only"
    a = groups.setdefault(g, {"viewers": 0, "req": 0, "bytes": 0, "seg": 0, "late": 0, "err": 0, "failover": 0, "steered": 0, "events": []})
    for k in ("viewers", "req", "bytes", "seg", "late", "err", "failover", "steered"): a[k] += d[k]
    a["events"] += [[e[0], f"{f}:{e[1]}", e[2], e[3]] for e in d["events"]]
secs = t1 - t0
client_bytes = sum(a["bytes"] for a in groups.values())
req = byt = 0
for line in open(f"{run}/logs/origin.log"):
    ts, uri, st, b = line.split()
    if t0 <= float(ts) <= t1: req += 1; byt += int(b)
client_mbit = client_bytes * 8 / 1e6 / secs
origin_mbit = byt * 8 / 1e6 / secs
seg = sum(a["seg"] for a in groups.values()); late = sum(a["late"] for a in groups.values())
print(f"HLS: {sum(a['viewers'] for a in groups.values())} viewers over {secs:.0f} s; edges served {client_mbit:.1f} Mbit/s, "
      f"the origin {origin_mbit:.2f} Mbit/s in {req} requests ({origin_mbit / client_mbit * 100:.2f} %); {seg} segments, {late} late")
check(origin_mbit < client_mbit / 20, "the origin sends under 5 % of what the edges send")
check(late <= seg / 100, "at most 1 % of segments arrive late")

# The dead edge: when the control plane dropped it, and what its viewers went through.
print(f"edge 1 killed at {killed - t0:.1f} s; out of the steering order {dropped - killed:.2f} s later; "
      f"started again at {restarted - t0:.1f} s and back in the order {back - restarted:.2f} s later")
check(dropped - killed <= 4.0, "the control plane drops a dead edge within 4 s (1 s checks, two failures)")
check("edge-1" not in manifest["PATHWAY-PRIORITY"], "a manifest after the drop leaves edge 1 out")
check(back - restarted <= 4.0, "a restarted edge is back in the order within 4 s")
for g, a in groups.items():
    ev = sorted(a["events"])
    viewers = {}
    for t, v, kind, p in ev: viewers.setdefault(v, []).append((t, kind, p))
    on_dead, gaps, joined_dead, unmoved = 0, [], 0, 0
    for v, es in viewers.items():
        before = [e for e in es if e[0] < killed and e[1] in ("segment", "joined", "steered", "start", "failover")]
        if before and before[-1][2] == "edge-1" and any(e[0] >= killed for e in es):
            on_dead += 1
            moved = next((e[0] for e in es if e[0] > killed and e[1] == "segment" and e[2] != "edge-1"), None)
            if moved is not None:
                gaps.append(moved - killed)
            else:  # the viewer's session ended first: a gap of at least that long
                ended = next(e[0] for e in reversed(es) if e[1] == "end")
                gaps.append(ended - killed); unmoved += 1
        joined = next((e for e in es if e[1] == "joined"), None)
        if joined and dropped < joined[0] < restarted and joined[2] == "edge-1": joined_dead += 1
    gaps.sort()
    p50 = gaps[len(gaps) // 2] if gaps else 0; worst = gaps[-1] if gaps else 0
    print(f"{g}: {on_dead} viewers on edge 1 when it died; a new segment from another edge after "
          f"{p50:.1f} s (median), {worst:.1f} s (worst; {unmoved} left first); {a['failover']} failovers, {a['steered']} moves by the manifest, "
          f"{a['err']} errors, {a['late']} late of {a['seg']}")
    check(on_dead > 0, f"{g}: some viewers were on edge 1 when it died")
    check(joined_dead == 0, f"{g}: nobody who joined while edge 1 was down was sent to it")
    if g == "penalise":
        check(worst < 6.0, f"{g}: every moved viewer had a new segment within 6 s (three segments held)")
    else:
        bound = (dropped - killed) + float(manifest["TTL"]) + 4.0
        check(worst < bound, f"{g}: every viewer moved by the manifest alone within detection + TTL ({bound:.0f} s)")

# Audio through the relay, directly and through edge 2, and metadata through the second relay.
for name, label in (("listen-relay", "the relay"), ("listen-edge", "edge 2's /listen/")):
    d = json.load(open(f"{run}/{name}.json"))
    print(f"{label}: HTTP {d['status']}, {d['kbit_s']} kbit/s, {d['heads']} Opus header(s), closed at {d['closed_at']}")
    check(d["status"] == 200 and d["heads"] >= 1 and d["kbit_s"] >= 85 and d["closed_at"] is None,
          f"{label}: a listener hears the stream for the whole run without being dropped")
vol = open(f"{run}/volume.log").read()
m = re.search(r"mean_volume: (-?[\d.]+) dB", vol)
mean = float(m.group(1)) if m else -999.0
print(f"the relay's audio decodes at a mean of {mean} dB (the tone; the listener process's fallback is silence)")
check(mean > -30, "the relay carries the programme, not silence")
d = json.load(open(f"{run}/listen-meta.json"))
titles = sorted({t for _, t in d["titles"]})
print(f"metadata through the second relay: {len(titles)} titles in 20 s, {d['heads']} chained streams")
check(len(titles) >= 4, "the relay passes in-band title updates from an Icecast origin")
sys.exit(1 if fail else 0)
PY
