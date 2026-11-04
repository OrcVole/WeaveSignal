#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The Quadlets generated from the Compose bundle: weavesignal.sh writes throwaway settings
# (--no-start), generate.sh writes the units, and Podman's own generator converts them
# (`quadlet -dryrun -user`) into one service per container and volume. Nothing is installed
# into systemd and nothing is started; the units are not run here.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
run="$root/tests/install/run/quadlets"
project=${WEAVESIGNAL_TEST_PREFIX:-weavesignal-m1}-q
quadlet=${QUADLET:-/usr/libexec/podman/quadlet}
[ -x "$quadlet" ] || { echo "Podman's quadlet generator is not at $quadlet (set QUADLET)" >&2; exit 2; }
rm -rf -- "$run"; mkdir -p "$run"
trap 'rm -rf -- "$run"' EXIT
# Twice: the loopback bundle, and a public station (the proxy joins with its health check, and LiveKit's unit waits for it).
for variant in loopback public; do
  dir="$run/$variant"; mkdir -p "$dir"
  extra=(); [ "$variant" = loopback ] || extra=(--public station.example)
  # The settings only: ports are written, not bound, so the defaults' alternatives may be chosen.
  # The proxy's 443 and 80 are not bindable by a rootless Podman, so a public station's ports are
  # given above 1024 here (nothing is bound).
  PROXY_HTTPS_PORT=18443 PROXY_HTTP_PORT=18080 \
    "$root/deploy/compose/weavesignal.sh" --project "$project" --env-file "$dir/station.env" --no-start "${extra[@]}" >/dev/null
  "$root/deploy/quadlets/generate.sh" --project "$project" --env-file "$dir/station.env" --out "$dir/out" 2>/dev/null | tail -1
  units="$dir/out/containers/systemd"
  for key in LIVEKIT_API_SECRET LISTENERS_SOURCE_PASSWORD POSTGRES_PASSWORD CONTROL_ADMIN_TOKEN CHAT_EMAIL_KEY CONTROL_EMAIL_KEY CONTROL_TOKEN_KEY; do
    value=$(grep "^$key=" "$dir/station.env" | cut -d= -f2-)
    ! grep -rqF -- "$value" "$dir/out" || { echo "$key is written into a unit or a settings file" >&2; exit 1; }
  done
  QUADLET_UNIT_DIRS="$units" "$quadlet" -dryrun -user > "$dir/dryrun.txt" 2> "$dir/dryrun.err" \
    || { cat "$dir/dryrun.err" >&2; exit 1; }
  services=$(grep -c '^---.*\.service---$' "$dir/dryrun.txt")
  containers=$(find "$units" -name '*.container' | wc -l); volumes=$(find "$units" -name '*.volume' | wc -l)
  [ "$services" = $(( containers + volumes )) ] || { echo "quadlet made $services services from $containers containers and $volumes volumes" >&2; exit 1; }
  grep -q -- "--secret $project-postgres-password,type=env,target=POSTGRES_PASSWORD" "$dir/dryrun.txt"
  grep -q -- "--secret $project-chat-email-key,type=env,target=WEAVESIGNAL_CHAT_EMAIL_KEY" "$dir/dryrun.txt"
  grep -q -- "--secret $project-control-email-key,type=env,target=WEAVESIGNAL_CONTROL_EMAIL_KEY" "$dir/dryrun.txt"
  grep -q -- "--secret $project-control-token-key,type=env,target=WEAVESIGNAL_CONTROL_TOKEN_KEY" "$dir/dryrun.txt"
  grep -q -- "--health-cmd" "$dir/dryrun.txt"
  if [ "$variant" = public ]; then
    [ -f "$units/$project-proxy.container" ] || { echo "the public station has no proxy unit" >&2; exit 1; }
    grep -q '^HealthCmd=test -s .*\.crt && test -s .*\.key' "$units/$project-proxy.container" || { echo "the proxy's health check is not the certificate and key" >&2; exit 1; }
    grep -q "$project-proxy.service" "$units/$project-livekit.container" || { echo "LiveKit's unit does not wait for the proxy" >&2; exit 1; }
  else
    [ ! -f "$units/$project-proxy.container" ] || { echo "the loopback station has a proxy unit" >&2; exit 1; }
  fi
  echo "pass  $variant: $containers containers and $volumes volumes become $services services; no secret in any file"
done
