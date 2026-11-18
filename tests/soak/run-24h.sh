#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The 24-hour unattended run (public issue 12, PLAN Phase 14d): release one's M4 done-when, "a
# station runs 24 hours unattended from a seeded week, with the dead-air fallback, the legal
# logger and the per-play log checked". The Compose bundle up on its own project and ports with
# throwaway secrets (as `tests/ports/bundle.sh`), a station named and seeded with a library, a
# clock and a week, and then left alone for DURATION while `soak.py` samples it, holds an Opus
# listener, and at the end checks the run and writes RESULT.md (see soak.py's header for what is
# seeded and which checks are made). Not part of `ci`: it takes as long as it is told to.
#
#   just soak-24h                      the whole day
#   DURATION=20m SAMPLE_S=15 tests/soak/run-24h.sh
#
# To leave it running after the terminal closes (the output goes to $OUT/run.out):
#   OUT=$HOME/Code/weavesignal-soak-runs/$(date -u +%Y%m%dT%H%M%SZ); mkdir -p "$OUT"
#   OUT=$OUT setsid nohup just soak-24h > "$OUT/run.out" 2>&1 < /dev/null &
#
# Settings (environment): DURATION (24h; 90s, 20m, 2h, 1d), SAMPLE_S (60), OUT (a folder under
# ~/Code/weavesignal-soak-runs, never under a target/), WEAVESIGNAL_SERVER_IMAGE (the image to
# run; build it first with `just server-image`), WEAVESIGNAL_TEST_PREFIX (the project is
# <prefix>-soak), and the ports CONTROL_PORT, CHAT_PORT, LISTENERS_PORT, SRT_PORT, FEED_PORT,
# DESK_FEED_PORT, DESK_SOURCES_PORT, LK_PORT and LK_UDP_PORT (31410 to 31482 by default). Only
# one stack at a time: it stops if its project is already up. Exit status 0 only if every check
# passed.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
here="$root/tests/soak"

duration=${DURATION:-24h}
case "$duration" in
  *[!0-9smhd]* | "" ) echo "DURATION is a number with s, m, h or d (24h, 20m)" >&2; exit 2 ;;
esac
n=${duration%[smhd]}; unit=${duration#"$n"}
[[ "$n" =~ ^[0-9]+$ ]] || { echo "DURATION is a number with s, m, h or d (24h, 20m)" >&2; exit 2; }
case "${unit:-s}" in s) mult=1 ;; m) mult=60 ;; h) mult=3600 ;; d) mult=86400 ;; esac
seconds=$((n * mult))
{ [ "$seconds" -ge 300 ] && [ "$seconds" -le 604800 ]; } || { echo "DURATION must be from 5 minutes to 7 days" >&2; exit 2; }

out=${OUT:-$HOME/Code/weavesignal-soak-runs/$(date -u +%Y%m%dT%H%M%SZ)}
case "$out" in */target/*|*/target) echo "OUT must not be under a target/ (the operator wipes those)" >&2; exit 2 ;; esac
case "$out" in "$root"/*) echo "OUT must be outside the repository" >&2; exit 2 ;; esac
mkdir -p "$out"; out=$(cd "$out" && pwd)
env="$out/soak.env"
project=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-soak
image=${WEAVESIGNAL_SERVER_IMAGE:-localhost/weavesignal-server:m1}
podman image exists "$image" || { echo "no image $image (just server-image, or set WEAVESIGNAL_SERVER_IMAGE)" >&2; exit 2; }
[ -z "$(podman ps -aq --filter "label=com.docker.compose.project=$project")" ] \
  || { echo "the project $project is already up: stop it first" >&2; exit 2; }

token=soak-admin-test
sed -e 's/^STATION_ID=.*/STATION_ID=5a1e0b7c-93d2-4c6f-8b1a-7e4d2f0c6a35/' \
    -e 's/^LIVEKIT_API_SECRET=.*/LIVEKIT_API_SECRET=soak-test-only-secret-not-for-use-0000/' \
    -e 's/^LISTENERS_SOURCE_PASSWORD=.*/LISTENERS_SOURCE_PASSWORD=soak-test-only/' \
    -e 's/^POSTGRES_PASSWORD=.*/POSTGRES_PASSWORD=soak-test-only/' \
    -e "s/^CONTROL_ADMIN_TOKEN=.*/CONTROL_ADMIN_TOKEN=$token/" \
    -e 's/^CHAT_EMAIL_KEY=.*/CHAT_EMAIL_KEY=soak-test-only-chat-email-key-0123456789/' \
    -e 's/^INGEST_TOKEN=.*/INGEST_TOKEN=soak-test-only-ingest-token-0123456789/' \
    -e 's/^DESK_SOURCES_PASSWORD=.*/DESK_SOURCES_PASSWORD=soak-test-only-desk-sources-0123456789/' \
    -e "s|^SERVER_IMAGE=.*|SERVER_IMAGE=$image|" \
    -e 's/^COMPAT_STREAM=.*/COMPAT_STREAM=true/' \
    -e 's/^COMPAT_HIGH_STREAM=.*/COMPAT_HIGH_STREAM=true/' \
    -e "s/^CONTROL_PORT=.*/CONTROL_PORT=${CONTROL_PORT:-31410}/" \
    -e "s/^CHAT_PORT=.*/CHAT_PORT=${CHAT_PORT:-31411}/" \
    -e "s/^LISTENERS_PORT=.*/LISTENERS_PORT=${LISTENERS_PORT:-31412}/" \
    -e "s/^PLAYOUT_FEED_PORT=.*/PLAYOUT_FEED_PORT=${FEED_PORT:-31415}/" \
    -e "s/^SRT_INGEST_PORT=.*/SRT_INGEST_PORT=${SRT_PORT:-31413}/" \
    -e "s/^DESK_FEED_PORT=.*/DESK_FEED_PORT=${DESK_FEED_PORT:-31417}/" \
    -e "s/^DESK_SOURCES_PORT=.*/DESK_SOURCES_PORT=${DESK_SOURCES_PORT:-31418}/" \
    -e "s/^LIVEKIT_HTTP_PORT=.*/LIVEKIT_HTTP_PORT=${LK_PORT:-31480}/" \
    -e "s/^LIVEKIT_MEDIA_PORT=.*/LIVEKIT_MEDIA_PORT=${LK_UDP_PORT:-31482}/" \
    "$root/deploy/compose/.env.example" > "$env"
# shellcheck disable=SC1090
. <(grep -E '^[A-Z_]+_PORT=' "$env")
# A public address (the station's own loopback one) so that shows can be made: without it the
# control plane refuses them ("cards are off").
echo "PUBLIC_ORIGIN=http://127.0.0.1:$CONTROL_PORT" >> "$env"

compose() { (cd "$root/deploy/compose" && podman compose --env-file "$env" -p "$project" "$@"); }
# shellcheck disable=SC2329
down() {
  trap - EXIT INT TERM
  [ -d "$out/logs" ] || { mkdir -p "$out/logs"; compose logs --no-color > "$out/logs/compose-all.log" 2>&1 || true; }
  compose down -v >/dev/null 2>&1 || true
  rm -f "$env"
}
trap down EXIT
trap 'exit 130' INT TERM

echo "soak: $duration (every ${SAMPLE_S:-60} s), project $project, image $image, output $out"
compose up -d >/dev/null 2>&1
for i in $(seq 1 90); do
  curl -fs "http://127.0.0.1:$CONTROL_PORT/health" 2>/dev/null | grep -q '"database":"ok"' && break
  sleep 1; [ "$i" = 90 ] && { echo "the control plane did not come up with its database" >&2; compose logs control | tail -20; exit 1; }
done

set +e
SOAK_PROJECT=$project CONTROL_PORT=$CONTROL_PORT LISTENERS_PORT=$LISTENERS_PORT SOAK_TOKEN=$token \
  SOAK_DURATION_S=$seconds SAMPLE_S=${SAMPLE_S:-60} OUT=$out SOAK_IMAGE=$image \
  python3 -I "$here/soak.py" &
pid=$!
trap 'kill -TERM "$pid" 2>/dev/null' INT TERM
status=0
while kill -0 "$pid" 2>/dev/null; do wait "$pid"; status=$?; done
set -e
echo "soak: $out/RESULT.md (exit $status)"
exit "$status"
