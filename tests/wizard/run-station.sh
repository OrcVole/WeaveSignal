#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# A station for the wizards' browser checks (ADR-0237): PostgreSQL in a container and the
# control plane on the host, serving the public pages' build (the player and /setup/) on
# loopback. No room, desk or playout: the setup page, branding and the player's look need none.
#
#   tests/wizard/run-station.sh            # up, and wait (Ctrl+C stops it and removes the database)
#   WEB_DIR=target/wizard/dist-base PORT=18951 tests/wizard/run-station.sh   # another build of the pages
#
# The station key is `wizard-admin-test-only`. NAME and CONTACT set the settings file's values
# (default: the demo name and no contact, so the station is not set up). OVERRIDES=1 gives the
# control plane a `[setup] overrides_file`, as the Cloudron package does.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
prefix=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-wizard}-wiz
PORT=${PORT:-18950}
PG_PORT=${PG_PORT:-18999}
WEB_DIR=${WEB_DIR:-$root/apps/web/dist}
case $WEB_DIR in /*) ;; *) WEB_DIR=$root/$WEB_DIR ;; esac
NAME=${NAME:-Kettle FM}
CONTACT=${CONTACT:-}
run="$root/target/wizard/run-$PORT"
mkdir -p "$run"
bin=${CONTROL_BIN:-$root/target/debug/weavesignal-control}
[ -x "$bin" ] || { echo "build the control plane first: cargo build -p weavesignal-control" >&2; exit 1; }
[ -f "$WEB_DIR/index.html" ] || { echo "no pages at $WEB_DIR: build them (cd apps/web && pnpm build)" >&2; exit 1; }

pid=''
cleanup() {
  [ -z "$pid" ] || kill "$pid" 2>/dev/null || true
  podman rm -f "$prefix-pg-$PORT" >/dev/null 2>&1 || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM
cleanup
podman run -d --rm --name "$prefix-pg-$PORT" -p "127.0.0.1:$PG_PORT:5432" -e POSTGRES_PASSWORD=test-only -e POSTGRES_DB=weavesignal docker.io/library/postgres:18 >/dev/null
for _ in $(seq 1 120); do podman exec "$prefix-pg-$PORT" pg_isready -h 127.0.0.1 -U postgres -d weavesignal >/dev/null 2>&1 && break; sleep 0.5; done

# A fresh station each time: an answer the setup page wrote for the last one is not this one's.
rm -f "$run/setup.env"
overrides=''
[ -z "${OVERRIDES:-}" ] || overrides="
[setup]
overrides_file = \"$run/setup.env\"
stream_auth = false"
cat > "$run/control.toml" <<TOML
[station]
id = "7d1b7c0e-5a55-4a51-9d0c-0a37c0de0237"
name = "$NAME"

[ports]
control = $PORT

[web]
dir = "$WEB_DIR"

[join]
consent_contact = "$CONTACT"
stream_url = "http://127.0.0.1:$PORT/no-stream.opus"

[carts]
media_dir = "$run/carts"

[retention]
recordings_dir = "$run/recordings"
$overrides
TOML
WEAVESIGNAL_DATABASE_URL=postgres://postgres:test-only@127.0.0.1:$PG_PORT/weavesignal \
WEAVESIGNAL_CONTROL_ADMIN_TOKEN=wizard-admin-test-only \
  "$bin" --config "$run/control.toml" > "$run/control.log" 2>&1 &
pid=$!
for _ in $(seq 1 120); do curl -fs "http://127.0.0.1:$PORT/health" 2>/dev/null | grep -q '"database":"ok"' && break; sleep 0.5; done
curl -fs "http://127.0.0.1:$PORT/health" | grep -q '"database":"ok"' || { cat "$run/control.log" >&2; echo "the control plane did not come up" >&2; exit 1; }
echo "station up: http://127.0.0.1:$PORT/  (setup: /setup/, key: wizard-admin-test-only)"
wait "$pid"
