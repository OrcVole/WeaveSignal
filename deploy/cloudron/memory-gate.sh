#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The local proxy for the idle-vs-production memory gate, run after test-local.sh, which leaves
# the app running. It reads an idle snapshot, then loads every service for LOAD_S seconds:
# 3 tone publishers (a presenter and two guests; a caller's token needs consent, M3) through
# LiveKit and the desk, LISTENERS listeners through nginx and the listener process, an SSE
# client on the studio events, and CHAT_SOCKETS chat readers with posters (the chat's own load generator, through nginx on the
# station domain), repeated until the window ends. The peak is read through one file
# descriptor after a reset (cgroup v2). Test tones only. The real gate runs on a Cloudron.
#
#   cargo build --release -p weavesignal-chat --bin weavesignal-chat-load   # once, for CHAT_LOAD
#   deploy/cloudron/memory-gate.sh
#
# Settings: LOAD_S=300 LISTENERS=50 CHAT_SOCKETS=1000 HTTP_PORT=27400 LISTEN_PORT=27401
#   CHAT_LOAD=<repository>/target/release/weavesignal-chat-load  (missing: no chat load, said so)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
P=weavesignal-cloudron-local-app
L=weavesignal-cloudron-local-load
LOAD_S=${LOAD_S:-300}
LISTENERS=${LISTENERS:-50}
CHAT_SOCKETS=${CHAT_SOCKETS:-1000}
HTTP_PORT=${HTTP_PORT:-27400}
LISTEN_PORT=${LISTEN_PORT:-27401}
DESK_IMAGE=${DESK_IMAGE:-localhost/weavesignal-desk-dev:m1}
CHAT_LOAD=${CHAT_LOAD:-$here/../../target/release/weavesignal-chat-load}
out=${OUT:-$here/.test-local/memory-gate}
rm -rf "$out"; mkdir -p "$out"
cg=/sys/fs/cgroup$(podman inspect -f '{{.State.CgroupPath}}' $P)
limit=$(cat "$cg/memory.max")
sec() { podman exec $P sh -c ". /app/data/secrets.env; printf %s \"\$$1\""; }
admin=$(sec CONTROL_ADMIN_TOKEN); lp=$(sec LISTENERS_PASSWORD)
rss() { podman exec $P sh -c 'ps -eo rss,args --sort=-rss | head -10 | cut -c1-80'; }

# Idle: the app as test-local.sh left it, at rest.
{
    echo "idle: current $(($(cat "$cg/memory.current") / 1048576)) MiB, peak since start $(($(cat "$cg/memory.peak") / 1048576)) MiB, limit $((limit / 1048576)) MiB"
    grep '^oom_kill ' "$cg/memory.events"
    rss
} > "$out/idle.txt"

python3 - "$cg" "$out/samples.tsv" "$LOAD_S" <<'PY' &
import os, sys, time
cg, path, load = sys.argv[1], sys.argv[2], int(sys.argv[3])
fd = os.open(f"{cg}/memory.peak", os.O_RDWR)
os.write(fd, b"reset")
end = time.time() + load + 40
with open(path, "w") as f:
    f.write("t\tcurrent_mib\tpeak_mib\n")
    t0 = time.time()
    while time.time() < end:
        cur = int(open(f"{cg}/memory.current").read())
        os.lseek(fd, 0, 0); peak = int(os.read(fd, 64))
        f.write(f"{time.time()-t0:.0f}\t{cur/2**20:.0f}\t{peak/2**20:.0f}\n"); f.flush()
        time.sleep(5)
PY
sampler=$!

tok() { curl -fsS -X POST "http://127.0.0.1:$HTTP_PORT/api/v1/room/tokens" -H "Authorization: Bearer $admin" \
  -H 'content-type: application/json' -d "{\"identity\":\"$1\",\"role\":\"$2\",\"subscribe\":false}" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])'; }
for spec in "eilidh:presenter:440" "morag:guest:1000" "fraser:guest:2500"; do
  IFS=: read -r id role f <<<"$spec"
  t=$(tok "$id" "$role")
  podman run -d --rm --name "$L-$id" --network container:$P "$DESK_IMAGE" \
    gst-launch-1.0 -q audiotestsrc is-live=true wave=sine freq="$f" volume=0.3 ! audioconvert ! audioresample \
    ! livekitwebrtcsink signaller::ws-url=ws://127.0.0.1:8002 signaller::auth-token="$t" \
      signaller::room-name=studio >/dev/null
done
curl -s -N --max-time "$LOAD_S" -H "Authorization: Bearer $admin" "http://127.0.0.1:$HTTP_PORT/api/v1/studio/events" -o "$out/sse.txt" &
for n in $(seq 1 "$LISTENERS"); do
  curl -s -u "listener:$lp" --max-time "$LOAD_S" "http://127.0.0.1:$LISTEN_PORT/programme.opus" -o "$out/l$n.opus" &
done
chat_runs=0
if [[ -x $CHAT_LOAD ]]; then
  ( end=$((SECONDS + LOAD_S - 90))
    while (( SECONDS < end )); do
      "$CHAT_LOAD" --base "http://127.0.0.1:$HTTP_PORT" --admin "$admin" --sockets "$CHAT_SOCKETS" \
        --slow 10 --reconnect 50 >> "$out/chat-load.txt" 2>&1 || echo "chat load run failed: $?" >> "$out/chat-load.txt"
    done ) &
  chat_runs=1
else
  echo "no chat load: $CHAT_LOAD is missing" > "$out/chat-load.txt"
fi
sleep $((LOAD_S / 2))
rss > "$out/rss-mid.txt"
# shellcheck disable=SC2046  # one pid a word
wait $(jobs -p | grep -v "^$sampler$") 2>/dev/null || true
podman rm -f "$L-eilidh" "$L-morag" "$L-fraser" >/dev/null 2>&1 || true
wait $sampler || true
{ grep '^oom_kill ' "$cg/memory.events"; rss; } > "$out/after.txt"
peak=$(tail -1 "$out/samples.tsv" | cut -f3)
echo "loaded peak ${peak} MiB of $((limit / 1048576)) MiB ($((100 * peak * 1048576 / limit)) %); chat load runs: $(grep -c '^# weavesignal-chat load test' "$out/chat-load.txt" 2>/dev/null || echo 0) (enabled: $chat_runs)"
cat "$out/idle.txt"; tail -3 "$out/samples.tsv"; cat "$out/rss-mid.txt" "$out/after.txt"
ls -la "$out"/l1.opus "$out/l$LISTENERS.opus" 2>/dev/null; wc -c < "$out/sse.txt"
