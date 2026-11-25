#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The Keys tab and the player's key against the real control plane: starts the Schedule's own
# run (tests/screens/run-screens.sh: a throwaway PostgreSQL, the control plane, Kettle FM's week,
# the studio's dev server), adds the public pages' dev server beside it, drives both with
# tests/keys/drive-real.mjs, and takes everything down again. A stack: run it under the stack
# lock. Needs Playwright (set PLAYWRIGHT_CORE to a playwright-core directory).
#
#   flock ~/Code/WeaveSignal/tools/1.1.1/stack.lock tests/keys/run-real.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
CONTROL_PORT=${CONTROL_PORT:-26310}
STUDIO_PORT=${STUDIO_PORT:-26420}
WEB_PORT=${WEB_PORT:-26430}
log="$root/target/keys-real"
mkdir -p "$log"

pids=()
# Each part runs in a session of its own, so that taking it down takes its dev server with it
# (a dev server left behind holds its port against the next run).
cleanup() {
  for pid in "${pids[@]}"; do kill -TERM -- "-$pid" 2>/dev/null || kill "$pid" 2>/dev/null || true; done
  sleep 2
  for pid in "${pids[@]}"; do kill -KILL -- "-$pid" 2>/dev/null || true; done
  podman rm -f "${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-screens-pg" >/dev/null 2>&1 || true
}
trap cleanup EXIT

CONTROL_PORT=$CONTROL_PORT STUDIO_PORT=$STUDIO_PORT setsid "$root/tests/screens/run-screens.sh" > "$log/screens.log" 2>&1 &
pids+=($!)
for i in $(seq 1 600); do
  grep -q '^up: the studio app' "$log/screens.log" 2>/dev/null && break
  kill -0 "${pids[0]}" 2>/dev/null || { echo "the Schedule's run ended early:" >&2; tail -20 "$log/screens.log" >&2; exit 1; }
  sleep 1; [ "$i" = 600 ] && { echo "the Schedule's run did not come up" >&2; tail -20 "$log/screens.log" >&2; exit 1; }
done

# Playout's keyed gate, stood in for: the station keys a show only when playout answers that it
# can keep it from the public (1.3.2), and the Schedule's stack has no playout.
setsid python3 -I "$root/tests/keys/playout-standin.py" "$root/tests/screens/run/sock/playout.sock" > "$log/playout.log" 2>&1 &
pids+=($!)

(cd "$root/apps/web" && WEAVESIGNAL_CONTROL_URL="http://127.0.0.1:$CONTROL_PORT" WEAVESIGNAL_WEB_PORT=$WEB_PORT exec setsid pnpm exec vite --logLevel warn) > "$log/web.log" 2>&1 &
pids+=($!)
for i in $(seq 1 60); do
  curl -fs "http://127.0.0.1:$WEB_PORT/listen/" >/dev/null 2>&1 && break
  sleep 0.5; [ "$i" = 60 ] && { echo "the public pages did not come up" >&2; cat "$log/web.log" >&2; exit 1; }
done

STUDIO="http://127.0.0.1:$STUDIO_PORT" WEB="http://127.0.0.1:$WEB_PORT" CONTROL="http://127.0.0.1:$CONTROL_PORT" \
  ADMIN_TOKEN=screens-admin-test-only node "$root/tests/keys/drive-real.mjs"
