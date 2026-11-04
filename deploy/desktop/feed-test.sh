#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The update feed end to end, without building a package (`just feed-test`, ADR-0140):
#   1. a throwaway signing key under target/feed-test/ (never committed, deleted with target/),
#      and a second one to prove another key is refused;
#   2. stand-in packages named as the Tauri bundler names them, signed with the Tauri CLI
#      (`signer sign --app-version`, as `tauri build` signs);
#   3. latest-json.sh builds the feed; its refusals are exercised (no .sig, a signature for
#      another version, a missing platform, the feed going backwards);
#   4. the tree is served on a loopback port, latest.json and every file it names are fetched
#      over HTTP, and the fetched files are verified with the crate the updater uses
#      (apps/studio/src-tauri/tests/feed.rs).
# The port is WEAVESIGNAL_FEED_TEST_PORT (default 23010, below Linux's ephemeral range). The CLI
# is the studio app's own pinned one: pnpm 10 first on PATH.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
port=${WEAVESIGNAL_FEED_TEST_PORT:-23010}
work=$root/target/feed-test
version=0.2.0
rm -rf "$work"
mkdir -p "$work"/{keys,dist,got}
cd "$root/apps/studio"
# Offline: the test reaches nothing but its own loopback port (the store is filled by studio-check).
pnpm install --frozen-lockfile --offline >/dev/null
tauri() { pnpm -s tauri "$@"; }

fail() { echo "feed-test: FAIL: $*" >&2; exit 1; }
refuses() { # refuses 'part of the message' command...
  local want=$1; shift
  if out=$("$@" 2>&1); then fail "accepted: $*"; fi
  grep -qF -- "$want" <<<"$out" || fail "refused for another reason than '$want': $out"
}

echo "== keys"
tauri signer generate --ci -p '' -w "$work/keys/test.key" >/dev/null
tauri signer generate --ci -p '' -w "$work/keys/other.key" >/dev/null
[ -s "$work/keys/test.key.pub" ] && [ -s "$work/keys/other.key.pub" ] || fail "no public keys written"

sign() { # sign file version [keyname]
  tauri signer sign -f "$work/keys/${3:-test}.key" -p '' --app-version "$2" "$1" >/dev/null
}
mkpkg() { # mkpkg dir version
  local d=$1 v=$2
  mkdir -p "$d"
  # Distinct bytes per file; the content is not parsed here, only signed and downloaded.
  printf 'stand-in AppImage %s\n' "$v" > "$d/WeaveSignal_${v}_amd64.AppImage"
  printf 'stand-in deb %s\n' "$v" > "$d/WeaveSignal_${v}_amd64.deb"
  printf 'stand-in rpm %s\n' "$v" > "$d/WeaveSignal-${v}-1.x86_64.rpm"
  printf 'stand-in nsis %s\n' "$v" > "$d/WeaveSignal_${v}_x64-setup.exe"
  for f in "$d"/WeaveSignal*; do sign "$f" "$v"; done
  (cd "$d" && sha256sum -- $(ls WeaveSignal* | grep -v '\.sig$') > "SHA256SUMS-$v")
}
# (argparse keeps the last --version, so a call may override this default.)
feed() { "$root/deploy/desktop/latest-json.sh" --version "$version" --base-url "http://127.0.0.1:$port/studio" "$@"; }

echo "== the feed for $version"
mkpkg "$work/dist" "$version"
printf 'The update screen.\nRollback from a kept AppImage.\n' > "$work/notes.txt"
feed --from "$work/dist" --out "$work/site" --notes "$work/notes.txt" --date 2026-11-02T10:00:00Z

python3 -I - "$work" "$version" "$port" <<'PY'
import json, pathlib, sys
work, version, port = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
m = json.load(open(work / "site/latest.json"))
assert m["version"] == version and m["pub_date"] == "2026-11-02T10:00:00Z", m
assert m["notes"].startswith("The update screen."), m["notes"]
want = {"linux-x86_64-appimage": "WeaveSignal_0.2.0_amd64.AppImage", "linux-x86_64-deb": "WeaveSignal_0.2.0_amd64.deb",
        "linux-x86_64-rpm": "WeaveSignal-0.2.0-1.x86_64.rpm", "windows-x86_64-nsis": "WeaveSignal_0.2.0_x64-setup.exe"}
assert set(m["platforms"]) == set(want), sorted(m["platforms"])   # no bare linux-x86_64
for t, name in want.items():
    e = m["platforms"][t]
    assert e["url"] == f"http://127.0.0.1:{port}/studio/{version}/{name}", e["url"]
    assert e["signature"] == (work / "dist" / (name + ".sig")).read_text().strip(), t
    assert (work / "site" / version / name).is_file() and (work / "site" / version / (name + ".sig")).is_file()
assert (work / "site" / version / f"SHA256SUMS-{version}").is_file()
print("manifest shape ok")
PY

echo "== refusals"
mkpkg "$work/nosig" "$version"; rm "$work/nosig/WeaveSignal_${version}_amd64.deb.sig"
refuses "has no WeaveSignal_${version}_amd64.deb.sig" feed --from "$work/nosig" --out "$work/site-x"
mkpkg "$work/wrongver" "$version"; sign "$work/wrongver/WeaveSignal_${version}_amd64.AppImage" 0.1.0
refuses "was signed for version 0.1.0, not $version" feed --from "$work/wrongver" --out "$work/site-x"
mkpkg "$work/nowin" "$version"; rm "$work/nowin"/WeaveSignal_*setup.exe*
refuses "no package for windows-x86_64-nsis" feed --from "$work/nowin" --out "$work/site-x"
feed --from "$work/nowin" --out "$work/site-y" --allow-missing | grep -q 'without windows-x86_64-nsis' || fail "--allow-missing"
echo 'dGV4dA==' > "$work/wrongver/WeaveSignal_${version}_amd64.AppImage.sig"
refuses "is not a minisign signature" feed --from "$work/wrongver" --out "$work/site-x"
test ! -e "$work/site-x/latest.json" || fail "a refused release left a latest.json"
refuses "no user name or password" "$root/deploy/desktop/latest-json.sh" --version "$version" \
  --base-url "https://feed:secret@updates.example.org/studio" --from "$work/dist" --out "$work/site-x"
refuses "must be https" "$root/deploy/desktop/latest-json.sh" --version "$version" \
  --base-url "http://updates.example.org/studio" --from "$work/dist" --out "$work/site-x"
# A later release goes beside the first; the feed refuses to go backwards unless told.
mkpkg "$work/dist3" 0.3.0
feed --from "$work/dist3" --out "$work/site" --version 0.3.0 >/dev/null
refuses "newer than $version" feed --from "$work/dist" --out "$work/site" --version "$version"
feed --from "$work/dist" --out "$work/site" --version "$version" --allow-older >/dev/null
test -d "$work/site/0.3.0" && test -d "$work/site/$version" || fail "an earlier release was not kept"
grep -q "\"version\": \"$version\"" "$work/site/latest.json" || fail "--allow-older did not pull the feed back"
# A pre-release is another release, not this one's package.
mkpkg "$work/rc" 0.4.0-rc1
refuses "no signed packages for 0.4.0" feed --from "$work/rc" --out "$work/site-x" --version 0.4.0
test ! -e "$work/site-x/latest.json" || fail "a refused release left a latest.json"
feed --from "$work/rc" --out "$work/site-r" --version 0.4.0-rc1 >/dev/null || fail "a pre-release was refused"
feed --from "$work/dist" --out "$work/site-n" --version "$version" >/dev/null
echo "refusals ok"

echo "== served over HTTP on 127.0.0.1:$port"
# The feed's URLs carry the /studio prefix of the base URL, so the served root holds the tree
# under that name, as a real host's path would.
mkdir -p "$work/serve"
ln -sfn "$work/site-n" "$work/serve/studio"
python3 -I -m http.server "$port" --bind 127.0.0.1 --directory "$work/serve" >"$work/http.log" 2>&1 &
server=$!
trap 'kill $server 2>/dev/null || true' EXIT
for _ in $(seq 1 50); do curl -fsS "http://127.0.0.1:$port/studio/latest.json" -o "$work/got/latest.json" 2>/dev/null && break; sleep 0.1; done
[ -s "$work/got/latest.json" ] || fail "latest.json was not served"
python3 -I - "$work/got/latest.json" > "$work/got/urls" <<'PY'
import json, sys
for e in json.load(open(sys.argv[1]))["platforms"].values():
    print(e["url"])
PY
[ "$(wc -l < "$work/got/urls")" = 4 ] || fail "the manifest names $(wc -l < "$work/got/urls") files"
while read -r url; do curl -fsS "$url" -o "$work/got/${url##*/}" || fail "$url was not served"; done < "$work/got/urls"
cp "$work/keys/test.key.pub" "$work/got/pubkey"
cp "$work/keys/other.key.pub" "$work/got/pubkey-other"

echo "== signatures, with the updater's own crate"
cd "$root"
WEAVESIGNAL_FEED_DIR=$work/got cargo test -p weavesignal-studio --test feed -- --ignored
echo "feed-test: ok"
