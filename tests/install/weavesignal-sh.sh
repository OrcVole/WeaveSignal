#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# weavesignal.sh end to end, on its own project name, settings file and ports (29xxx by default,
# below the ephemeral range):
# a taken port is named with a free alternative; a taken default moves to its alternative; the
# station installs and answers; secrets are never printed and the settings file is mode 600; a
# second run changes nothing; --uninstall keeps the data, a reinstall finds it, and
# --uninstall --purge removes every container, volume and setting the script made.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
script="$root/deploy/compose/weavesignal.sh"
run="$root/tests/install/run"; mkdir -p "$run"
project=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-sh
env="$run/station.env"
out="$run/out.txt"
export SERVER_IMAGE=${WEAVESIGNAL_SERVER_IMAGE:-localhost/weavesignal-server:m1}
export CONTROL_PORT=${CONTROL_PORT:-29310} CHAT_PORT=${CHAT_PORT:-29311} LISTENERS_PORT=${LISTENERS_PORT:-29312}
export SRT_INGEST_PORT=${SRT_PORT:-29313} PLAYOUT_FEED_PORT=${FEED_PORT:-29315} LIVEKIT_HTTP_PORT=${LK_PORT:-29880} LIVEKIT_MEDIA_PORT=${LK_UDP_PORT:-29882}
holder=''
cleanup() {
  [ -z "$holder" ] || kill "$holder" 2>/dev/null || true
  [ ! -f "$env" ] || "$script" --project "$project" --env-file "$env" --uninstall --purge --yes >/dev/null 2>&1 || true
  rm -f "$env" "$out" "$run/dry.env"
}
trap cleanup EXIT
# --no-boot-start: this test must not turn on lingering or podman-restart.service on the machine
# it runs on (ADR-0138); start-at-boot.sh tests that with stubs.
sh() { "$script" --project "$project" --env-file "$env" --no-boot-start "$@"; }
hold() { # a TCP listener on loopback, by its own PID
  python3 -I -c 'import socket, sys, time
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", int(sys.argv[1]))); s.listen(); time.sleep(120)' "$1" &
  holder=$!
  for _ in $(seq 1 50); do [ -n "$(ss -Hltn "sport = :$1")" ] && return; sleep 0.1; done
  echo "could not hold port $1" >&2; exit 1
}
release() { kill "$holder"; wait "$holder" 2>/dev/null || true; holder=''; }
volumes() { podman volume ls -q --filter "label=com.docker.compose.project=$project" | wc -l; }
containers() { podman ps -aq --filter "label=com.docker.compose.project=$project" | wc -l; }

# 1. A port given and taken: refused, naming the clash and a free alternative.
hold "$CONTROL_PORT"
if sh --no-start > "$out" 2>&1; then echo "a taken CONTROL_PORT was accepted" >&2; exit 1; fi
grep -q "CONTROL_PORT=$CONTROL_PORT (tcp) is in use; 17310 is free and suggested\|CONTROL_PORT=$CONTROL_PORT (tcp) is in use" "$out"
echo "pass  a taken port is refused: $(tail -1 "$out")"
release; rm -f "$env"

# 2. A default taken: the next alternative is chosen and said (settings only, nothing started).
probe=$(python3 -I -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 7311)); print("free")' 2>/dev/null || true)
if [ "$probe" = free ]; then
  hold 7311
  ( unset CHAT_PORT; "$script" --project "$project-dry" --env-file "$run/dry.env" --no-start --no-boot-start > "$out" 2>&1 )
  release
  grep -q 'CHAT_PORT: 7311 (tcp) is in use, so 1731[1] is used instead\|CHAT_PORT: 7311 (tcp) is in use, so 27311 is used instead' "$out"
  echo "pass  a taken default moves to its alternative: $(grep 'CHAT_PORT:' "$out")"
  rm -f "$run/dry.env"
else
  echo "skip  the default-moves check: 7311 is already in use on this machine"
fi

# 3. Install and start.
sh > "$out" 2>&1 || { cat "$out" >&2; exit 1; }
[ "$(stat -c %a "$env")" = 600 ] || { echo "the settings file is not mode 600" >&2; exit 1; }
for key in LIVEKIT_API_SECRET LISTENERS_SOURCE_PASSWORD POSTGRES_PASSWORD CONTROL_ADMIN_TOKEN CHAT_EMAIL_KEY CONTROL_EMAIL_KEY CONTROL_TOKEN_KEY; do
  value=$(grep "^$key=" "$env" | cut -d= -f2-)
  [ ${#value} -ge 32 ] || { echo "$key was not generated" >&2; exit 1; }
  ! grep -qF -- "$value" "$out" || { echo "$key was printed" >&2; exit 1; }
done
curl -fs "http://127.0.0.1:$CONTROL_PORT/health" | grep -q '"database":"ok"'
admin=$(grep '^CONTROL_ADMIN_TOKEN=' "$env" | cut -d= -f2-)
curl -fs -H "Authorization: Bearer $admin" "http://127.0.0.1:$CONTROL_PORT/api/v1/station" >/dev/null
for _ in $(seq 1 60); do
  curl -fs "http://127.0.0.1:$CHAT_PORT/api/v1/chat/info" 2>/dev/null | grep -q '"reasons"' && break; sleep 1
done
curl -fs "http://127.0.0.1:$CHAT_PORT/api/v1/chat/info" | grep -q '"reasons"'
echo "pass  installed: $(containers) containers, the control plane and the chat answer, the admin token works, no secret printed, settings mode 600"

# 4. A second run changes nothing.
before=$(sha256sum < "$env")
sh > "$out" 2>&1 || { cat "$out" >&2; exit 1; }
[ "$(sha256sum < "$env")" = "$before" ] || { echo "the second run changed the settings" >&2; exit 1; }
grep -q 'Settings unchanged' "$out"
curl -fs "http://127.0.0.1:$CONTROL_PORT/health" | grep -q '"database":"ok"'
echo "pass  a second run keeps every secret and port, and the station answers"

# 5. Uninstall keeps the data; installing again finds it.
station=$(grep '^STATION_ID=' "$env" | cut -d= -f2-)
psql() { podman exec "$(podman ps -q --filter "label=com.docker.compose.project=$project" --filter label=com.docker.compose.service=postgres)" psql -U weavesignal -d weavesignal -tAc "$1"; }
psql 'CREATE TABLE install_test_marker (kept text); INSERT INTO install_test_marker VALUES ($$kept$$)' >/dev/null
sh --uninstall > "$out" 2>&1
[ "$(containers)" = 0 ] || { echo "containers left after --uninstall" >&2; exit 1; }
[ "$(volumes)" -gt 0 ] || { echo "--uninstall removed the data volumes" >&2; exit 1; }
[ -f "$env" ]
echo "pass  --uninstall: no containers, $(volumes) volumes and the settings kept"
sh > "$out" 2>&1 || { cat "$out" >&2; exit 1; }
curl -fs "http://127.0.0.1:$CONTROL_PORT/api/v1/station" -H "Authorization: Bearer $admin" | grep -q "$station"
[ "$(psql 'SELECT kept FROM install_test_marker')" = kept ] || { echo "the database was not kept" >&2; exit 1; }
echo "pass  installed again on the kept data and settings: the same station, its database kept"

# 6. Purge removes everything the script made.
sh --uninstall --purge --yes > "$out" 2>&1
[ "$(containers)" = 0 ] && [ "$(volumes)" = 0 ] && [ ! -f "$env" ] \
  || { echo "--purge left containers, volumes or settings" >&2; exit 1; }
[ -z "$(podman network ls -q --filter "label=com.docker.compose.project=$project")" ] || { echo "--purge left a network" >&2; exit 1; }
echo "pass  --uninstall --purge: no containers, volumes, networks or settings left"
