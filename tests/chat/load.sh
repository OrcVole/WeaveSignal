#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The chat's load test (M6, ADR-0031): a release build of weavesignal-chat on a throwaway
# PostgreSQL 18, then weavesignal-chat-load with 4,000 sockets (40 that never read, 200 that drop
# and reconnect with their position), a steady 7 messages a second, a burst at 20 and a spike at
# 60. Prints the report; the server's log stays in tests/chat/run/. A measurement, not in `ci`.
#
#   tests/chat/load.sh                       # defaults
#   SOCKETS=2000 tests/chat/load.sh -- --spike-s 0   # anything after -- goes to the load tool
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/chat/run"
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-chat}
pg_port=${WEAVESIGNAL_TEST_PG_PORT:-37432}
chat_port=${CHAT_PORT:-37311}
sockets=${SOCKETS:-4000}
admin=load-test-admin-only
mkdir -p "$run"

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
taken -t "$chat_port" && { echo "port $chat_port/tcp is in use; set CHAT_PORT" >&2; exit 1; }

cargo build --release -p weavesignal-chat --manifest-path "$root/Cargo.toml" >/dev/null

pid=
cleanup() {
  if [[ -n "$pid" ]]; then kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; fi
  podman rm -f "$prefix-load-pg" >/dev/null 2>&1 || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM

podman rm -f "$prefix-load-pg" >/dev/null 2>&1 || true
podman run -d --rm --name "$prefix-load-pg" -p "127.0.0.1:$pg_port:5432" -e POSTGRES_PASSWORD=test-only -e POSTGRES_DB=weavesignal docker.io/library/postgres:18 >/dev/null
for _ in $(seq 1 60); do podman exec "$prefix-load-pg" pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 && break; sleep 0.5; done

cat > "$run/chat.toml" <<TOML
[station]
id = "6a1f3c2e-8f43-4c9b-9a51-1d2f0b7e4c11"
name = "Kettle FM"
[listen]
port = $chat_port
[control]
url = ""
[log]
format = "pretty"
filter = "warn"
TOML

WEAVESIGNAL_DATABASE_URL="postgres://postgres:test-only@127.0.0.1:$pg_port/weavesignal" \
WEAVESIGNAL_CONTROL_ADMIN_TOKEN="$admin" \
WEAVESIGNAL_CHAT_EMAIL_KEY=chat-load-email-key-0123456789abcdef \
  "$root/target/release/weavesignal-chat" --config "$run/chat.toml" > "$run/chat.log" 2>&1 &
pid=$!
for _ in $(seq 1 100); do curl -sf "http://127.0.0.1:$chat_port/health" >/dev/null && break; sleep 0.1; done

shift_args=()
if [[ "${1:-}" == "--" ]]; then shift; shift_args=("$@"); fi
"$root/target/release/weavesignal-chat-load" --base "http://127.0.0.1:$chat_port" --admin "$admin" --pid "$pid" --sockets "$sockets" "${shift_args[@]}"
