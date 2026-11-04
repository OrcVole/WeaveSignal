#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The studio app's desktop packages (ADR-0015), copied to dist/desktop/ with their SHA-256
# checksums:
#   build.sh            AppImage, .deb and .rpm for Linux amd64 (`just package-desktop`)
#   build.sh windows    the unsigned NSIS installer for Windows x64 (ADR-0076), cross-built
#                       for the GNU target (`just package-windows`)
#
# The interface is built here (pnpm 10); the Rust build and the bundlers run in a build image:
# Ubuntu 24.04 for Linux (`just desktop-image`, ADR-0126), so the packages run on distributions as
# old as it, and Debian 13 with MinGW-w64 and NSIS for Windows (`just desktop-windows-image`).
# WEAVESIGNAL_DESKTOP_IMAGE= (empty) builds Linux packages on this machine instead, for
# development only: such packages need this machine's C library or newer.
#
# Updates (ADR-0015). The updater's public key and its endpoint are build settings, never in
# the repository:
#   WEAVESIGNAL_UPDATER_PUBKEY    the public key, as `tauri signer generate` writes it (the .pub
#                               file's contents)
#   WEAVESIGNAL_UPDATER_ENDPOINT  where the app reads latest.json (https)
#   WEAVESIGNAL_UPDATER_KEY_FILE  the private key file that signs the update files; kept offline
#                               with the release key, never on a shared machine
#   TAURI_SIGNING_PRIVATE_KEY_PASSWORD
#                               its password
# With no public key the packages are built without updater files, and the app they install
# never updates itself.
# The update feed (latest.json and a folder per release) is made from the signed packages that
# land in dist/desktop/ by `latest-json.sh` (`just package-feed`); this script does not publish.
#
# Pinned tools: the bundler's downloads are checked against tools.lock (fetch-tools.sh), and the
# build images pin rustup, the Tauri CLI's source and the Rust version (their Containerfiles).
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
out=${WEAVESIGNAL_DESKTOP_OUT:-$root/dist/desktop}
platform=${1:-linux}
case $platform in
  linux)
    bundles=${WEAVESIGNAL_DESKTOP_BUNDLES:-appimage,deb,rpm}
    image=${WEAVESIGNAL_DESKTOP_IMAGE-localhost/weavesignal-desktop-build:m1}
    triple=''
    ;;
  windows)
    bundles=nsis
    image=${WEAVESIGNAL_DESKTOP_WINDOWS_IMAGE:-localhost/weavesignal-desktop-windows:m1}
    triple=x86_64-pc-windows-gnu
    ;;
  *) echo "build.sh: linux or windows" >&2; exit 2 ;;
esac
mkdir -p "$root/target"
overlay=$(mktemp "$root/target/weavesignal-tauri.XXXXXX.json")
trap 'rm -f "$overlay"' EXIT

signing=()
if [ -n "${WEAVESIGNAL_UPDATER_PUBKEY:-}" ]; then
  [ -n "${WEAVESIGNAL_UPDATER_ENDPOINT:-}" ] || { echo "WEAVESIGNAL_UPDATER_ENDPOINT is not set" >&2; exit 2; }
  # The updater refuses an endpoint that is not https when the app starts, so the app would not
  # open at all: refuse it here instead.
  case $WEAVESIGNAL_UPDATER_ENDPOINT in
    https://*) ;;
    *) echo "WEAVESIGNAL_UPDATER_ENDPOINT must be https: $WEAVESIGNAL_UPDATER_ENDPOINT" >&2; exit 2 ;;
  esac
  key=${WEAVESIGNAL_UPDATER_KEY_FILE:-}
  [ -n "$key" ] && [ -f "$key" ] \
    || { echo "an updater key is set but WEAVESIGNAL_UPDATER_KEY_FILE names no file" >&2; exit 2; }
  updater=true
  echo "updater: on, endpoint $WEAVESIGNAL_UPDATER_ENDPOINT"
else
  updater=false
  echo "updater: off (WEAVESIGNAL_UPDATER_PUBKEY is not set); these packages will never update themselves"
fi
# The interface is built below, before the bundler runs, so the bundler does not build it again.
UPDATER=$updater python3 -I -c 'import json, os, sys
c = {"build": {"beforeBuildCommand": ""}, "bundle": {"active": True}}
if os.environ["UPDATER"] == "true":
    c["bundle"]["createUpdaterArtifacts"] = True
    # requireSignedVersion is in tauri.conf.json too; said again so the overlay alone decides it.
    c["plugins"] = {"updater": {"pubkey": os.environ["WEAVESIGNAL_UPDATER_PUBKEY"].strip(),
                                "endpoints": [os.environ["WEAVESIGNAL_UPDATER_ENDPOINT"]],
                                "requireSignedVersion": True}}
json.dump(c, open(sys.argv[1], "w"))' "$overlay"

cd "$root/apps/studio"
pnpm install --frozen-lockfile
pnpm build

# The bundler downloads its tools (linuxdeploy, AppRun, an AppImage plugin, an NSIS plugin) when
# its cache lacks them. They are pinned with their SHA-256 in tools.lock: fetch-tools.sh checks
# them and puts them in the cache first, so the bundler downloads nothing unchecked.
if [ -n "$image" ]; then tool_cache=$root/target/desktop-cache/tauri; else tool_cache=${XDG_CACHE_HOME:-$HOME/.cache}/tauri; fi
"$root/deploy/desktop/fetch-tools.sh" "$platform" "$root/target/desktop-cache/pinned" "$tool_cache"

tauri_args=(build --ci --bundles "$bundles" --config "/work/${overlay#"$root"/}")
[ -z "$triple" ] || tauri_args+=(--target "$triple")
if [ -n "$image" ]; then
  target=$root/target/desktop${triple:+-$platform}
  mkdir -p "$target" "$root/target/desktop-cache/tauri" "$HOME/.cargo/registry"
  if [ "$updater" = true ]; then
    # The key file is mounted read-only for this run; its password passes by name, never on a
    # command line.
    # (The Tauri CLI takes a path in TAURI_SIGNING_PRIVATE_KEY as well as the key itself.)
    signing=(-v "$key:/run/secrets/updater.key:ro" -e TAURI_SIGNING_PRIVATE_KEY=/run/secrets/updater.key
             -e TAURI_SIGNING_PRIVATE_KEY_PASSWORD)
  fi
  podman run --rm --network host --security-opt label=disable \
    -v "$root:/work" -v "$HOME/.cargo/registry:/opt/cargo/registry" \
    -v "$root/target/desktop-cache/tauri:/root/.cache/tauri" \
    -e CARGO_TARGET_DIR="/work/${target#"$root"/}" -e NO_STRIP -e CARGO_BUILD_JOBS \
    "${signing[@]}" -w /work/apps/studio/src-tauri "$image" \
    cargo tauri "${tauri_args[@]}"
else
  target=${CARGO_TARGET_DIR:-$root/target}
  tauri_args[5]=$overlay
  # linuxdeploy is itself an AppImage; extract-and-run spares it FUSE.
  if [ "$updater" = true ]; then export TAURI_SIGNING_PRIVATE_KEY=$key; fi
  APPIMAGE_EXTRACT_AND_RUN=1 pnpm tauri "${tauri_args[@]}"
fi

version=$(python3 -I -c 'import json, sys; print(json.load(open(sys.argv[1]))["version"])' src-tauri/tauri.conf.json)
bundle="$target/${triple:+$triple/}release/bundle"
mkdir -p "$out"
shopt -s nullglob
if [ "$platform" = linux ]; then
  files=("$bundle"/appimage/*"_${version}_"*.AppImage* "$bundle"/deb/*"_${version}_"*.deb* "$bundle"/rpm/*"-${version}-"*.rpm*)
  sums=SHA256SUMS-$version
else
  files=("$bundle"/nsis/*"_${version}_"*-setup.exe*)
  sums=SHA256SUMS-$version-windows
fi
[ ${#files[@]} -gt 0 ] || { echo "build.sh: the bundler left no packages in $bundle" >&2; exit 1; }
names=()
for f in "${files[@]}"; do
  # A signature is copied only from a build that made one: an earlier build's would be stale.
  if [[ $f == *.sig ]] && { [ "$updater" != true ] || [ "$f" -ot "$overlay" ]; }; then continue; fi
  cp -f "$f" "$out/"; names+=("$(basename "$f")")
done
# The download page gives these checksums beside each file (ADR-0076).
(cd "$out" && sha256sum -- "${names[@]}" | grep -v '\.sig$' > "$sums" || true)
echo "packages in $out:"
(cd "$out" && ls -l -- "${names[@]}" "$sums")
