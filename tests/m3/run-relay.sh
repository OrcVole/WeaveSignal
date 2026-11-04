#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# M3's browser runs against the join stack with LiveKit's TURN over TLS on and the public pages
# served by the control plane: the relay-only caller heard (tests/m3/relay.mjs), then the whole
# join flow at 390 px with touch (tests/m3/join.mjs). Ports in 39000-39999 unless set; the
# pages are built first. Screenshots and results go to OUT, never the repository.
#
#   PLAYWRIGHT_CORE=/path/to/node_modules/playwright-core tests/m3/run-relay.sh
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
export M3_PREFIX=${M3_PREFIX:-${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-m3}
export LK_PORT=${M3_LK_PORT:-39880} LK_UDP_PORT=${M3_LK_UDP_PORT:-39882} SRT_PORT=${M3_SRT_PORT:-39913}
export LISTENERS_PORT=${M3_LISTENERS_PORT:-39812} FEED_PORT=${M3_FEED_PORT:-39815} CONTROL_PORT=${M3_CONTROL_PORT:-39310}
export STUDIO_PORT=${M3_STUDIO_PORT:-39420} WEB_PORT=${M3_WEB_PORT:-39430}
export TURN_TLS_PORT=${M3_TURN_TLS_PORT:-39349} RELAY_START=${M3_RELAY_START:-39600} RELAY_END=${M3_RELAY_END:-39699}
export SERVE_WEB=1
OUT=${OUT:-${TMPDIR:-/tmp}/$M3_PREFIX-shots}
: "${PLAYWRIGHT_CORE:?set PLAYWRIGHT_CORE to an installed playwright-core with its Chromium}"
export PLAYWRIGHT_CORE OUT
mkdir -p "$OUT"

(cd "$root/apps/web" && pnpm install --frozen-lockfile >/dev/null && pnpm build >/dev/null)
log="$OUT/stack.log"
setsid "$root/tests/m3/run-join.sh" > "$log" 2>&1 &
stack=$!
stop() { kill -INT -- "-$stack" 2>/dev/null || true; wait "$stack" 2>/dev/null || true; }
trap stop EXIT
for i in $(seq 1 240); do
  grep -q '^up:' "$log" && break
  kill -0 "$stack" 2>/dev/null || { cat "$log" >&2; exit 1; }
  sleep 1; [ "$i" = 240 ] && { echo "the join stack did not come up" >&2; tail "$log" >&2; exit 1; }
done
# The listener stream can take a few seconds more than the rest to serve its mount.
for i in $(seq 1 120); do
  [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 1 "http://127.0.0.1:$LISTENERS_PORT/programme.opus")" = 200 ] && break
  sleep 1; [ "$i" = 120 ] && { echo "the listener stream did not come up" >&2; exit 1; }
done
web="http://127.0.0.1:$CONTROL_PORT"
status=0
CONTROL=$web WEB=$web STREAM="http://127.0.0.1:$LISTENERS_PORT/programme.opus" node "$root/tests/m3/relay.mjs" || status=1
CONTROL=$web WEB=$web STUDIO="http://127.0.0.1:$STUDIO_PORT/" LIVEKIT="http://127.0.0.1:$LK_PORT" node "$root/tests/m3/join.mjs" || status=1
exit $status
