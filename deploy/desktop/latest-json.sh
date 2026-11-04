#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Builds the studio app's update feed (ADR-0015, ADR-0140) from the signed packages that
# `deploy/desktop/build.sh` left in dist/desktop/: a static tree that any web server can serve,
# in Tauri's static `latest.json` format.
#
#   <out>/latest.json                  what the app reads (its WEAVESIGNAL_UPDATER_ENDPOINT)
#   <out>/<version>/<package>          each package and its .sig, kept for every release
#   <out>/<version>/SHA256SUMS*        the checksums build.sh wrote, for the download page
#
# Usage:
#   latest-json.sh --base-url https://updates.example.org/studio [options]
#     --base-url URL    where <out> is served (https; http only for a loopback address).
#                       Also WEAVESIGNAL_UPDATE_BASE_URL. No default: the domain is not chosen.
#     --version V       the release (default: the version in tauri.conf.json)
#     --from DIR        the signed packages (default: dist/desktop)
#     --out DIR         the tree to write (default: dist/updates); earlier releases stay in it
#     --notes FILE      the release notes, plain text (default: none)
#     --date RFC3339    the release date (default: now, UTC)
#     --allow-missing   publish although a package is missing (below)
#     --allow-older     let latest.json go back to an older release (pulling a bad one)
#
# What it refuses, because the app would refuse the release anyway (each is a user's error
# screen, so it is better found here):
#   * a package without its .sig, or a .sig that is not a minisign signature;
#   * a signature made for another version: the app is built with requireSignedVersion, which
#     compares the version announced here with the one inside the signed trusted comment;
#   * a missing platform. The static format needs an entry for the target the app asks for, and
#     an app whose target is absent shows an error instead of "up to date". All four
#     (linux-x86_64-appimage, -deb, -rpm and windows-x86_64-nsis) are required unless
#     --allow-missing says this release is without one;
#   * latest.json going backwards without --allow-older.
#
# This script does not sign anything and never sees the private key: build.sh signs, with the key
# file mounted for that run (ADR-0015). Bash with inline Python, as deploy/quadlets/generate.sh
# is (ADR-0136, item 15).
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
exec python3 -I - "$root" "$@" <<'PY'
import argparse, base64, binascii, datetime, json, os, pathlib, re, shutil, sys, urllib.parse

root = pathlib.Path(sys.argv[1])
ap = argparse.ArgumentParser(prog="latest-json.sh")
ap.add_argument("--base-url")
ap.add_argument("--version")
ap.add_argument("--from", dest="src", default=str(root / "dist/desktop"))
ap.add_argument("--out", default=str(root / "dist/updates"))
ap.add_argument("--notes")
ap.add_argument("--date")
ap.add_argument("--allow-missing", action="store_true")
ap.add_argument("--allow-older", action="store_true")
args = ap.parse_args(sys.argv[2:])


def fail(msg):
    print(f"latest-json.sh: {msg}", file=sys.stderr)
    sys.exit(1)


base = (args.base_url or os.environ.get("WEAVESIGNAL_UPDATE_BASE_URL") or "").rstrip("/")
if not base:
    fail("--base-url (or WEAVESIGNAL_UPDATE_BASE_URL) is not set; the feed's address is a build setting")
u = urllib.parse.urlsplit(base)
if u.scheme == "http" and u.hostname in ("127.0.0.1", "localhost", "::1"):
    pass  # a local test run
elif u.scheme != "https" or not u.hostname:
    fail(f"the base URL must be https: {base}")
if u.query or u.fragment:
    fail("the base URL has no query or fragment")
if u.username is not None or u.password is not None:
    fail("the base URL carries no user name or password: the feed is public")

version = args.version
if not version:
    conf = json.load(open(root / "apps/studio/src-tauri/tauri.conf.json"))
    version = conf["version"]
if not re.fullmatch(r"\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?", version):
    fail(f"not a semantic version: {version}")


def key(v):
    """Semver precedence for the numbers; a pre-release sorts before its release."""
    m = re.match(r"(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?", v)
    if m is None:
        fail(f"not a semantic version: {v}")
    pre = m.group(4)
    ids = [(0, int(p)) if p.isdigit() else (1, p) for p in pre.split(".")] if pre else None
    return (int(m.group(1)), int(m.group(2)), int(m.group(3)), 1 if ids is None else 0, ids or [])


# The static format's target keys, `{os}-{arch}-{bundle type}` (tauri-plugin-updater looks the
# bundle type up first). A plain `linux-x86_64` is never written: it would hand an AppImage to a
# .deb install, which refuses it.
KINDS = [
    # (suffix, target os-bundle, name's arch patterns -> updater arch)
    (".AppImage", "linux", "appimage", {"amd64": "x86_64", "aarch64": "aarch64", "arm64": "aarch64"}),
    (".deb", "linux", "deb", {"amd64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}),
    (".rpm", "linux", "rpm", {"x86_64": "x86_64", "aarch64": "aarch64"}),
    ("-setup.exe", "windows", "nsis", {"x64": "x86_64", "arm64": "aarch64"}),
]
REQUIRED = ["linux-x86_64-appimage", "linux-x86_64-deb", "linux-x86_64-rpm", "windows-x86_64-nsis"]

src = pathlib.Path(args.src)
if not src.is_dir():
    fail(f"{src} is not a folder; run build.sh first")

platforms, files = {}, []
for path in sorted(src.iterdir()):
    for suffix, osname, bundle, arches in KINDS:
        if not path.name.endswith(suffix):
            continue
        # The bundler names a package for its version: Name_0.2.0_amd64.AppImage, Name-0.2.0-1.x86_64.rpm
        # (not Name_0.2.0-rc1_..., which is another release).
        if not re.search(r"[_-]" + re.escape(version) + r"(?:_|-\d+\.)", path.name):
            continue
        arch = next((a for pat, a in arches.items() if re.search(r"(?<![0-9a-z])" + pat + r"(?![0-9a-z])", path.name)), None)
        if arch is None:
            fail(f"cannot tell the architecture of {path.name}")
        target = f"{osname}-{arch}-{bundle}"
        if target in platforms:
            fail(f"two packages for {target}: {platforms[target][0].name} and {path.name}")
        sig = path.with_name(path.name + ".sig")
        if not sig.is_file():
            fail(f"{path.name} has no {sig.name}: it was built without the updater key, so the app could not install it")
        platforms[target] = (path, sig)

if not platforms:
    fail(f"no signed packages for {version} in {src}")
missing = [t for t in REQUIRED if t not in platforms]
if missing and not args.allow_missing:
    fail(f"no package for {', '.join(missing)} (--allow-missing if this release is without it; "
         "the app on that platform then shows an error when it checks)")


def check_signature(path, sig_path):
    """The format and the version, not the cryptography (the test verifies that with the
    updater's own crate)."""
    text = sig_path.read_text().strip()
    try:
        raw = base64.b64decode(text, validate=True).decode("utf-8")
    except (binascii.Error, UnicodeDecodeError):
        fail(f"{sig_path.name} is not base64 text; it must be the .sig file as the Tauri CLI wrote it")
    lines = raw.rstrip("\n").split("\n")
    if (len(lines) != 4 or not lines[0].startswith("untrusted comment:")
            or not lines[2].startswith("trusted comment:")):
        fail(f"{sig_path.name} is not a minisign signature (four lines, trusted comment third)")
    signed = None
    for field in lines[2][len("trusted comment:"):].strip().split("\t"):
        if field.startswith("version:"):
            signed = field[len("version:"):]
    if signed is None:
        fail(f"{sig_path.name} records no version: sign with a Tauri CLI that does (2.12.4 does), "
             "or the app (requireSignedVersion) refuses the update")
    if signed.lstrip("v") != version:
        fail(f"{sig_path.name} was signed for version {signed}, not {version}")
    return text


entries = {}
for target in sorted(platforms):
    path, sig = platforms[target]
    url = f"{base}/{urllib.parse.quote(version)}/{urllib.parse.quote(path.name)}"
    entries[target] = {"signature": check_signature(path, sig), "url": url}

out = pathlib.Path(args.out)
feed = out / "latest.json"
if feed.is_file() and not args.allow_older:
    try:
        have = str(json.load(open(feed))["version"]).lstrip("v")
    except (ValueError, KeyError, TypeError):
        have = None
    if have and key(version) < key(have):
        fail(f"{feed} already announces {have}, newer than {version} (--allow-older to pull the feed back)")

notes = pathlib.Path(args.notes).read_text().strip() if args.notes else ""
date = args.date or datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
try:
    datetime.datetime.fromisoformat(date.replace("Z", "+00:00"))
except ValueError:
    fail(f"not an RFC 3339 date: {date}")

# Everything is checked: now write. The packages go first and latest.json last, so a server
# never announces a release whose files are not there yet.
dest = out / version
dest.mkdir(parents=True, exist_ok=True)
for path, sig in platforms.values():
    for f in (path, sig):
        shutil.copyfile(f, dest / f.name)
for sums in sorted(src.glob(f"SHA256SUMS-{version}*")):
    shutil.copyfile(sums, dest / sums.name)
manifest = {"version": version, "notes": notes, "pub_date": date, "platforms": entries}
tmp = out / "latest.json.tmp"
tmp.write_text(json.dumps(manifest, indent=2) + "\n")
tmp.replace(feed)
for t, e in entries.items():
    print(f"  {t:24} {e['url']}")
print(f"{feed}: version {version}" + (f" (without {', '.join(missing)})" if missing else ""))
PY
