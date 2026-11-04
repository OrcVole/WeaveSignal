#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# weavesignal.sh's start at boot on Podman (ADR-0138), with every system command stubbed: podman,
# docker, ss, curl, loginctl and systemctl are scripts in a directory put first on PATH, which
# record how they were called. No container starts, no port is bound and lingering and the
# user's services are never touched, so this runs anywhere (`just boot-check`).
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
script="$root/deploy/compose/weavesignal.sh"
run="$root/tests/install/run/boot"
rm -rf -- "$run"; mkdir -p "$run/bin"
trap 'rm -rf -- "$run"' EXIT
calls="$run/calls.log"

stub() { # name body: a stub that logs its call, then runs the body
  printf '#!/usr/bin/env bash\nprintf "%%s %%s\\n" "$(basename "$0")" "$*" >> %q\n%s\n' "$calls" "$2" > "$run/bin/$1"
  chmod +x "$run/bin/$1"
}
# The engines: compose is there, the image is there, nothing runs yet.
for engine in podman docker; do
  stub "$engine" 'if [ "${1:-}" = compose ] && [[ " $* " == *" ps "* ]]; then :; fi; exit 0'
done
stub ss 'exit 0'
stub curl 'echo "{\"database\":\"ok\"}"'
stub loginctl 'case "$*" in
  *show-user*) echo "${STUB_LINGER:-no}" ;;
  *enable-linger*) [ "${STUB_LINGER_FAIL:-}" != 1 ] ;;
esac'
stub systemctl 'case "$*" in
  *is-enabled*) [ "${STUB_RESTART_ENABLED:-}" = 1 ] ;;
  *enable*) [ "${STUB_RESTART_FAIL:-}" != 1 ] ;;
esac'

n=0
install() { # arguments for weavesignal.sh; the output is in $out, the calls in $calls
  n=$((n + 1)); : > "$calls"; out="$run/out-$n.txt"
  PATH="$run/bin:$PATH" "$script" --project boot-check --env-file "$run/station-$n.env" \
    --image localhost/stub:none "$@" > "$out" 2>&1 || { cat "$out" >&2; return 1; }
}
called() { grep -qF -- "$1" "$calls"; }
said() { grep -qF -- "$1" "$out" || { echo "missing in the output: $1" >&2; cat "$out" >&2; exit 1; }; }
fail() { echo "$*" >&2; echo "--- calls" >&2; cat "$calls" >&2; echo "--- output" >&2; cat "$out" >&2; exit 1; }
user=$(id -un)

# 1. Podman, nothing set yet: both turned on, and said so.
install
called "loginctl --no-ask-password enable-linger $user" || fail "lingering was not turned on (without a password prompt)"
called "systemctl --user enable podman-restart.service" || fail "podman-restart.service was not enabled"
! called "enable --now" || fail "podman-restart.service was started now, which starts other stopped containers"
said "turned on lingering"; said "enabled podman-restart.service"; said "start again after a reboot"
said "--no-boot-start skips both"
echo "pass  Podman: lingering and podman-restart.service turned on (enabled, not started), and said"

# 2. Both already on: nothing changed, and said so.
STUB_LINGER=yes STUB_RESTART_ENABLED=1 install
! called "enable-linger" && ! called "systemctl --user enable" || fail "something already on was changed"
said "lingering was already on"; said "podman-restart.service was already enabled"
echo "pass  Podman, both already on: nothing changed"

# 3. The flag: neither command is run, and the output says it was left alone.
install --no-boot-start
! called loginctl && ! called systemctl || fail "--no-boot-start still ran loginctl or systemctl"
said "Start at boot left alone"
echo "pass  --no-boot-start: loginctl and systemctl never run"

# 4. Docker is unaffected: no mention, no call.
install --engine docker
! called loginctl && ! called systemctl || fail "Docker ran loginctl or systemctl"
! grep -qi "linger\|podman-restart" "$out" || fail "Docker's output mentions lingering"
echo "pass  Docker: unaffected"

# 5. A step that fails does not fail the install, and names the command to run by hand.
STUB_LINGER_FAIL=1 STUB_RESTART_FAIL=1 install
said "could not turn on lingering; run 'loginctl enable-linger' yourself"
said "could not enable podman-restart.service; run 'systemctl --user enable podman-restart.service' yourself"
echo "pass  a refused step is reported with its command and the install still succeeds"

# 5b. Lingering refused but the service enabled: it does not claim a start at boot.
STUB_LINGER_FAIL=1 install
said "could not turn on lingering"
said "the containers start again at your next login after a reboot (at boot once lingering is on)"
echo "pass  without lingering, the service's line promises a start at login, not at boot"

# 6. --no-start writes the settings and never turns on start at boot.
install --no-start
! called loginctl && ! called systemctl || fail "--no-start ran loginctl or systemctl"
echo "pass  --no-start: loginctl and systemctl never run"

# 7. Every system command above was a stub: the real ones are never ahead of them on PATH.
[ "$(PATH="$run/bin:$PATH" command -v loginctl)" = "$run/bin/loginctl" ] \
  && [ "$(PATH="$run/bin:$PATH" command -v systemctl)" = "$run/bin/systemctl" ] \
  || fail "loginctl or systemctl is not a stub"
echo "pass  loginctl and systemctl were stubs: no lingering or service changed"
