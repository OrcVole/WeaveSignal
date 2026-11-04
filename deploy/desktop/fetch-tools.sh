#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Puts the Tauri bundler's downloaded tools, pinned in tools.lock, into its tool cache, each
# checked against its SHA-256.
#
#   fetch-tools.sh PLATFORM WORK_DIR TOOL_CACHE
#
#   PLATFORM    linux or windows: which lines of tools.lock to use
#   WORK_DIR    where the verified originals are kept between builds (created)
#   TOOL_CACHE  the bundler's tool cache (~/.cache/tauri, or the directory mounted there)
#
# A file already in WORK_DIR with the right checksum is not downloaded again; one with the wrong
# checksum is thrown away and downloaded again; a download with the wrong checksum stops the
# build. Whatever the bundler left in TOOL_CACHE is overwritten with the verified original.
# WEAVESIGNAL_TOOLS_LOCK names another lock file (the test uses it).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
lock=${WEAVESIGNAL_TOOLS_LOCK:-$here/tools.lock}
platform=${1:?platform: linux or windows}
work=${2:?work directory}
cache=${3:?tool cache}
case $platform in linux|windows) ;; *) echo "fetch-tools.sh: linux or windows" >&2; exit 2 ;; esac
[ -f "$lock" ] || { echo "fetch-tools.sh: no lock file at $lock" >&2; exit 2; }

mkdir -p "$work" "$cache"
n=0
while read -r sum plat path url; do
  case $sum in ''|'#'*) continue ;; esac
  [ "$plat" = "$platform" ] || continue
  [[ $sum =~ ^[0-9a-f]{64}$ ]] || { echo "fetch-tools.sh: $path: not a SHA-256 in $lock" >&2; exit 2; }
  case $path in /*|*..*) echo "fetch-tools.sh: $path: a path inside the cache only" >&2; exit 2 ;; esac
  file=$work/$path
  if [ -f "$file" ] && [ "$(sha256sum < "$file" | cut -d' ' -f1)" != "$sum" ]; then
    echo "fetch-tools.sh: $path: the kept copy does not match its pin; downloading it again"
    rm -f -- "$file"
  fi
  if [ ! -f "$file" ]; then
    mkdir -p "$(dirname "$file")"
    part=$file.part
    echo "fetch-tools.sh: downloading $path"
    curl --proto '=https' --tlsv1.2 -fsSL --retry 3 -o "$part" "$url"
    got=$(sha256sum < "$part" | cut -d' ' -f1)
    if [ "$got" != "$sum" ]; then
      rm -f -- "$part"
      echo "fetch-tools.sh: $path from $url has SHA-256 $got, pinned $sum: refusing it" >&2
      exit 1
    fi
    mv -f -- "$part" "$file"
  fi
  mkdir -p "$(dirname "$cache/$path")"
  install -m 0755 -- "$file" "$cache/$path"
  n=$((n + 1))
done < "$lock"
[ "$n" -gt 0 ] || { echo "fetch-tools.sh: $lock pins nothing for $platform" >&2; exit 1; }
echo "fetch-tools.sh: $n pinned tool(s) for $platform checked and in $cache"
