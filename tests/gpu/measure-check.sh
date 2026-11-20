#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# tests/gpu/measure.sh's control flow with podman, nvidia-smi and journalctl stubbed: no card is
# touched and no container starts. Four runs: a clean one (exit 0, an Xid from an earlier boot
# counted but not taken as the run's), one whose second parallel encode fails (said, and no
# division by zero), one where an Xid appears during an encode (stopped, exit 3, said once), and
# one where the follower misses an Xid that the final read of the kernel log finds (exit 3).
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
fail() { echo "FAIL  $*" >&2; exit 1; }
pass() { echo "pass  $*"; }
mkdir -p "$work/bin" "$work/cdi"
printf 'kind: nvidia.com/gpu\n' > "$work/cdi/nvidia.yaml"

# podman: image exists; run prints a gst-launch ending (or fails, or writes an Xid and hangs);
# compose run prints the probe's word; kill is a no-op.
cat > "$work/bin/podman" <<'SH'
#!/usr/bin/env bash
case "$1" in
  image) exit 0 ;;
  kill) exit 0 ;;
  compose) [[ " $* " == *" run "* ]] && echo nvenc-found; exit 0 ;;
  run)
    name=''; for ((i = 1; i <= $#; i++)); do [ "${!i}" = --name ] && { j=$((i + 1)); name=${!j}; }; done
    case "$*" in *gst-inspect*) exit 0 ;; esac
    [[ -n ${STUB_FAIL:-} && $name == *"-$STUB_FAIL-"* ]] && { echo "ERROR: no NVENC session" >&2; exit 1; }
    if [[ -n ${STUB_XID:-} && $name == *"-$STUB_XID-"* ]]; then
      echo "NVRM: Xid (PCI:0000:01:00): 79, GPU has fallen off the bus." >> "$STUB_FOLLOW"
      sleep 8; exit 1
    fi
    echo "Execution ended after 0:00:02.500000000"; exit 0 ;;
esac
exit 0
SH
# nvidia-smi: a free card; dmon prints a header and a few samples.
cat > "$work/bin/nvidia-smi" <<'SH'
#!/usr/bin/env bash
case "$*" in
  *query-compute-apps*) exit 0 ;;
  *query-gpu*) echo "Stub GPU, 999.99, 16 MiB" ;;
  dmon*) printf '# gpu    sm   mem   enc   dec\n# Idx     %%     %%     %%     %%\n    0    10     5    42     0\n'; sleep 30 ;;
esac
SH
# journalctl: -f follows STUB_FOLLOW (unless the follower is told to miss it), --since reads it
# whole, anything else reads STUB_JOURNAL (earlier boots).
cat > "$work/bin/journalctl" <<'SH'
#!/usr/bin/env bash
case " $* " in
  *" -f "*) if [ -n "${STUB_BLIND:-}" ]; then exec sleep 60; else exec tail -n +1 -f "$STUB_FOLLOW"; fi ;;
  *" --since "*) cat "$STUB_FOLLOW" ;;
  *" -n 1 "*) exit 0 ;;
  *) cat "$STUB_JOURNAL" ;;
esac
SH
chmod +x "$work/bin/"*
echo "NVRM: Xid (PCI:0000:01:00): 79, from an earlier boot" > "$work/journal"
export WEAVESIGNAL_CDI_DIRS="$work/cdi" STUB_JOURNAL="$work/journal"

run() { # name, then measure.sh's options; sets $status and $out
  local name=$1; shift
  export STUB_FOLLOW="$work/follow-$name"; : > "$STUB_FOLLOW"
  out="$work/out-$name"
  status=0
  PATH="$work/bin:$PATH" "$root/tests/gpu/measure.sh" --image stub --rungs "480p20 720p30" --seconds 5 --out "$out" "$@" > "$work/$name.txt" 2>&1 || status=$?
}

run clean
[ "$status" = 0 ] || fail "a clean run exited $status: $(cat "$work/clean.txt")"
grep -q 'No Xid in the kernel log since' "$out/RESULT.md" || fail "a clean run did not say no Xid"
grep -q 'before the start: 1 this boot' "$out/RESULT.md" || fail "the earlier Xid was not counted"
grep -q 'NVENC AV1 720p30: 150 frames in 2.5 s, 60.0 frames a second (2.0 times' "$out/RESULT.md" || fail "a rung's figures are wrong: $(cat "$out/RESULT.md")"
[ "$(grep -c 'at once' "$out/RESULT.md")" = 3 ] && grep -q 'SVT-AV1 720p30 (CPU)' "$out/RESULT.md" && grep -q 'podman compose. hands the card' "$out/RESULT.md" \
  || fail "a clean run lacks the parallel, CPU or compose lines"
grep -q 'at most 42 % busy' "$out/RESULT.md" || fail "the encoder's use was not read"
pass "a clean run: every step said, an earlier boot's Xid counted and not taken, exit 0"

STUB_FAIL=720p30-x2 run sessions
[ "$status" = 0 ] || fail "a failed parallel encode stopped the run (exit $status): $(cat "$work/sessions.txt")"
grep -q '2 of 3 at once: failed' "$out/RESULT.md" && [ "$(grep -c 'of 3 at once: [0-9]* frames' "$out/RESULT.md")" = 2 ] \
  || fail "a failed parallel encode was not said as one failure: $(cat "$out/RESULT.md")"
grep -qi 'division' "$work/sessions.txt" && fail "a failed encode divided by zero"
pass "a parallel encode past the session limit is said as failed; the others are reported"

STUB_XID=720p30 run xid
[ "$status" = 3 ] || fail "an Xid during an encode exited $status, not 3: $(cat "$work/xid.txt")"
[ "$(grep -c 'STOPPED at the first Xid' "$out/RESULT.md")" = 1 ] || fail "the stop was not said exactly once: $(cat "$out/RESULT.md")"
grep -q 'GPU has fallen off the bus' "$out/RESULT.md" || fail "the Xid line is not in the results"
grep -q 'SVT-AV1 720p30 (CPU)' "$out/RESULT.md" && fail "steps ran after the Xid"
pass "an Xid during an encode stops the run at once (exit 3), said once, nothing after it"

STUB_BLIND=1 STUB_XID=svt-720p30 run missed
[ "$status" = 3 ] || fail "an Xid the follower missed exited $status, not 3: $(cat "$work/missed.txt")"
grep -q 'STOPPED at the first Xid' "$out/RESULT.md" && ! grep -q 'No Xid' "$out/RESULT.md" || fail "the final read did not catch the Xid"
pass "an Xid the follower missed is found by the final read of the kernel log (exit 3)"
echo "measure.sh: its control flow holds"
