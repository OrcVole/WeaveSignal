#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The chat's browser stack (M6) on loopback, ports 37xxx: a throwaway PostgreSQL 18, the
# control plane (no desk: the chat follows its studio view for the modes), the chat with its
# email codes written to tests/chat/run/outbox, the player page and the studio app on Vite.
# Then it runs tests/chat/chat.mjs (PLAYWRIGHT_CORE must name a playwright-core with Chromium),
# or, with WAIT=1, waits for Ctrl+C. Screenshots go to OUT (default tests/chat/run/shots).
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/chat/run"
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-chat}
PG_PORT=${WEAVESIGNAL_TEST_PG_PORT:-37432}
CONTROL_PORT=${CONTROL_PORT:-37310}
CHAT_PORT=${CHAT_PORT:-37311}
STUDIO_PORT=${STUDIO_PORT:-37420}
WEB_PORT=${WEB_PORT:-37430}
ADMIN_TOKEN=chat-browser-admin-only
STATION=6a1f3c2e-8f43-4c9b-9a51-1d2f0b7e4c11

taken() { ss -H -ln "$1" "sport = :$2" | grep -q .; }
for p in "$PG_PORT" "$CONTROL_PORT" "$CHAT_PORT" "$STUDIO_PORT" "$WEB_PORT"; do
  taken -t "$p" && { echo "port $p/tcp is in use; set another" >&2; exit 1; }
done

rm -rf "$run/outbox"
mkdir -p "$run/outbox"
cargo build -p weavesignal-control -p weavesignal-chat --manifest-path "$root/Cargo.toml" >/dev/null

pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
  for pid in "${pids[@]}"; do wait "$pid" 2>/dev/null || true; done
  podman rm -f "$prefix-browser-pg" >/dev/null 2>&1 || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM

podman rm -f "$prefix-browser-pg" >/dev/null 2>&1 || true
podman run -d --rm --name "$prefix-browser-pg" -p "127.0.0.1:$PG_PORT:5432" -e POSTGRES_PASSWORD=test-only -e POSTGRES_DB=weavesignal docker.io/library/postgres:18 >/dev/null
for _ in $(seq 1 60); do podman exec "$prefix-browser-pg" pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 && break; sleep 0.5; done
DB="postgres://postgres:test-only@127.0.0.1:$PG_PORT/weavesignal"

cat > "$run/control.toml" <<TOML
[station]
id = "$STATION"
name = "Kettle FM"
[ports]
control = $CONTROL_PORT
chat = $CHAT_PORT
[studio]
show = "The Kettle Phone-In"
[log]
format = "pretty"
filter = "warn"
TOML
cat > "$run/chat.toml" <<TOML
[station]
id = "$STATION"
name = "Kettle FM"
[listen]
port = $CHAT_PORT
[control]
url = "http://127.0.0.1:$CONTROL_PORT"
[mail]
mode = "outbox"
from = "chat@kettle.example"
outbox_dir = "$run/outbox"
[defaults]
terms = "Be kind to the callers and to each other. No abuse, no spam, nothing unlawful. The moderators' word is final for tonight; ask for a review afterwards."
accountable_person = "Elspeth Gordon, station manager"
contact = "chat@kettle.example"
[log]
format = "pretty"
filter = "info"
TOML

WEAVESIGNAL_CONTROL_ADMIN_TOKEN=$ADMIN_TOKEN "$root/target/debug/weavesignal-control" --config "$run/control.toml" > "$run/control.log" 2>&1 &
pids+=($!)
WEAVESIGNAL_DATABASE_URL=$DB WEAVESIGNAL_CONTROL_ADMIN_TOKEN=$ADMIN_TOKEN WEAVESIGNAL_CHAT_EMAIL_KEY=chat-browser-email-key-0123456789abcdef "$root/target/debug/weavesignal-chat" --config "$run/chat.toml" > "$run/chat.log" 2>&1 &
pids+=($!)
for _ in $(seq 1 100); do curl -sf "http://127.0.0.1:$CHAT_PORT/health" >/dev/null && break; sleep 0.1; done

(cd "$root/apps/studio" && WEAVESIGNAL_CONTROL_URL="http://127.0.0.1:$CONTROL_PORT" WEAVESIGNAL_CHAT_URL="http://127.0.0.1:$CHAT_PORT" \
  WEAVESIGNAL_CONTROL_TOKEN=$ADMIN_TOKEN WEAVESIGNAL_STUDIO_PORT=$STUDIO_PORT exec pnpm exec vite --logLevel warn) > "$run/studio-vite.log" 2>&1 &
pids+=($!)
(cd "$root/apps/web" && WEAVESIGNAL_CONTROL_URL="http://127.0.0.1:$CONTROL_PORT" WEAVESIGNAL_CHAT_URL="http://127.0.0.1:$CHAT_PORT" \
  WEAVESIGNAL_WEB_PORT=$WEB_PORT exec pnpm exec vite --logLevel warn) > "$run/web-vite.log" 2>&1 &
pids+=($!)
for _ in $(seq 1 100); do curl -sf "http://127.0.0.1:$WEB_PORT/" >/dev/null && curl -sf "http://127.0.0.1:$STUDIO_PORT/" >/dev/null && break; sleep 0.2; done
echo "player http://127.0.0.1:$WEB_PORT/  studio http://127.0.0.1:$STUDIO_PORT/  chat :$CHAT_PORT  control :$CONTROL_PORT"

if [[ -n "${WAIT:-}" ]]; then
  wait
else
  WEB="http://127.0.0.1:$WEB_PORT" STUDIO="http://127.0.0.1:$STUDIO_PORT/" CONTROL="http://127.0.0.1:$CONTROL_PORT" \
  CHAT="http://127.0.0.1:$CHAT_PORT" ADMIN_TOKEN=$ADMIN_TOKEN OUTBOX="$run/outbox" PG="$prefix-browser-pg" \
  OUT="${OUT:-$run/shots}" node "$root/tests/chat/chat.mjs"
fi
