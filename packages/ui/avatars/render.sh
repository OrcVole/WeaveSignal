#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
# Draw the shipped chat avatars again (ADR-0235): fetch each flat SVG named in manifest.json from
# Fluent Emoji at the pinned commit, and write it as a 128-pixel WebP on a soft ground beside
# this script. Needs curl, jq, ImageMagick with librsvg, and cwebp. The SVGs are data: they are
# only ever handed to the renderer, and are not kept in the repository.
#   packages/ui/avatars/render.sh [work-dir]
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
work=${1:-$(mktemp -d)}
mkdir -p "$work/svg" "$work/png"
commit=$(jq -r .commit "$here/manifest.json")
grounds=("#F6D9C9" "#F3E3B4" "#D5E8C5" "#C4E6DF" "#C9DDF3" "#DAD3F2" "#F0D0E4" "#BFD4E6")
i=0
jq -r '.avatars[] | [.id, .source] | @tsv' "$here/manifest.json" | while IFS=$'\t' read -r id source; do
    file=$(printf '%s' "$source" | tr 'A-Z ' 'a-z_')_flat.svg
    path=$(jq -rn --arg s "assets/$source/Flat/$file" '$s | @uri | gsub("%2F"; "/")')
    [ -s "$work/svg/$id.svg" ] || curl -fsS -m 30 -o "$work/svg/$id.svg" \
        "https://raw.githubusercontent.com/microsoft/fluentui-emoji/$commit/$path"
    magick -background none -density 600 "$work/svg/$id.svg" -resize 92x92 \
        -background "${grounds[$((i % 8))]}" -gravity center -extent 128x128 -alpha remove -strip "$work/png/$id.png"
    cwebp -quiet -q 82 -m 6 -sharp_yuv "$work/png/$id.png" -o "$here/$id.webp"
    i=$((i + 1))
done
echo "drawn into $here from $work"
