#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# deploy/desktop/fetch-tools.sh, offline: curl is a stub that serves files from a directory, so
# the checks are the script's own. A download with the pinned checksum is installed in the cache
# and kept; a kept copy is not downloaded again; a kept copy that no longer matches is replaced;
# a download with another checksum stops the run and reaches no cache; the bundler's patched
# copy in the cache is overwritten with the original; and the real tools.lock is well formed.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
script="$root/deploy/desktop/fetch-tools.sh"
run="$root/tests/install/run/tools"
rm -rf -- "$run"; mkdir -p "$run/bin" "$run/serve"
trap 'rm -rf -- "$run"' EXIT
calls="$run/calls.log"; : > "$calls"

printf 'apprun original' > "$run/serve/AppRun-x86_64"
printf 'linuxdeploy original' > "$run/serve/linuxdeploy.AppImage"
printf 'nsis dll' > "$run/serve/utils.dll"
sum() { sha256sum < "$run/serve/$1" | cut -d' ' -f1; }
cat > "$run/tools.lock" <<LOCK
# a comment
$(sum AppRun-x86_64) linux AppRun-x86_64 https://example.invalid/AppRun-x86_64
$(sum linuxdeploy.AppImage) linux ld/linuxdeploy.AppImage https://example.invalid/linuxdeploy.AppImage

$(sum utils.dll) windows NSIS/Plugins/utils.dll https://example.invalid/utils.dll
LOCK
cat > "$run/bin/curl" <<'STUB'
#!/usr/bin/env bash
out=''; url=''
while [ $# -gt 0 ]; do case $1 in -o) out=$2; shift ;; https://*) url=$1 ;; esac; shift; done
echo "$url" >> "$STUB_CALLS"
[ "${STUB_FAIL:-}" != 1 ] || exit 22
cp -- "$STUB_SERVE/$(basename "$url")" "$out"
STUB
chmod +x "$run/bin/curl"
export STUB_CALLS=$calls STUB_SERVE=$run/serve WEAVESIGNAL_TOOLS_LOCK=$run/tools.lock
fetch() { PATH="$run/bin:$PATH" "$script" linux "$run/work" "$run/cache" > "$run/out.txt" 2>&1; }
downloads() { wc -l < "$calls"; }
fail() { echo "FAIL  $*" >&2; cat "$run/out.txt" >&2; exit 1; }

# 1. The first run downloads the platform's tools, checks them and installs them; the other platform's is left.
fetch || fail "the first run failed"
[ "$(downloads)" = 2 ] || fail "expected 2 downloads, made $(downloads)"
[ "$(cat "$run/cache/AppRun-x86_64")" = "apprun original" ] && [ -x "$run/cache/ld/linuxdeploy.AppImage" ] || fail "tools not in the cache"
[ ! -e "$run/cache/NSIS" ] || fail "a Windows tool was fetched for Linux"
echo "pass  the pinned tools are downloaded, checked and put in the cache; the other platform's is not fetched"

# 2. The bundler patches a cached file; the next run puts the original back, with no download.
printf 'patched by the bundler' > "$run/cache/ld/linuxdeploy.AppImage"
fetch || fail "the second run failed"
[ "$(downloads)" = 2 ] || fail "a kept tool was downloaded again"
[ "$(cat "$run/cache/ld/linuxdeploy.AppImage")" = "linuxdeploy original" ] || fail "the patched copy was not replaced"
echo "pass  a kept tool is not downloaded again, and the bundler's patched copy is replaced by the original"

# 3. A kept copy that no longer matches its pin is downloaded again.
printf 'corrupted' > "$run/work/AppRun-x86_64"
fetch || fail "the run after corruption failed"
[ "$(downloads)" = 3 ] || fail "the corrupted copy was not downloaded again"
[ "$(cat "$run/cache/AppRun-x86_64")" = "apprun original" ] || fail "the corrupted copy reached the cache"
echo "pass  a kept copy that fails its pin is replaced by a fresh download"

# 4. A download whose checksum differs from the pin is refused and reaches no cache.
rm -rf -- "$run/work" "$run/cache"
printf 'something else' > "$run/serve/AppRun-x86_64"
if fetch; then fail "a download with the wrong checksum was accepted"; fi
grep -q 'refusing it' "$run/out.txt" || fail "the refusal is not said"
[ ! -e "$run/cache/AppRun-x86_64" ] && [ ! -e "$run/work/AppRun-x86_64" ] || fail "the refused file was kept"
echo "pass  a download that fails its pin stops the run and is kept nowhere"

# 5. The real lock file: every line is a SHA-256, a platform, a cache path and an https URL.
python3 -I - "$root/deploy/desktop/tools.lock" <<'PY'
import re, sys
n = 0
for line in open(sys.argv[1]):
    line = line.strip()
    if not line or line.startswith("#"):
        continue
    sha, plat, path, url = line.split()
    assert re.fullmatch(r"[0-9a-f]{64}", sha), line
    assert plat in ("linux", "windows"), line
    assert not path.startswith("/") and ".." not in path, line
    assert url.startswith("https://"), line
    n += 1
assert n >= 4, n
print("tools.lock: %d pins" % n)
PY
echo "pass  tools.lock is well formed"
