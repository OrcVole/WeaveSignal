#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Install each desktop package in a throwaway container (rootless Podman) and run the installed
# app's `--version`, which answers before any window opens: the package installs with its
# dependencies, and the binary loads every library it links. Then three of them open the window
# on a virtual display (Xvfb, software rendering): the app and WebKit's web process must still
# run after 15 s, and a screenshot of the display is kept in target/desktop-shots/ to look at.
# That is X11 only: neither Wayland nor a GPU is tried. `just package-desktop-test` runs it
# after `just package-desktop`.
#
#   .deb       Debian 13, Ubuntu 24.04 and 26.04          apt-get install ./WeaveSignal_<v>_amd64.deb
#              (the floor is Ubuntu 24.04, ADR-0126: Debian 12 and Ubuntu 22.04 are below it)
#   .rpm       Fedora 43 and 44                           dnf install ./WeaveSignal-<v>-1.x86_64.rpm
#   AppImage   the same five, with only the libraries every desktop has (the AppImage leaves
#              those out by design: fontconfig, FreeType, HarfBuzz, FriBidi, X11, EGL, GBM...)
set -uo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
dist=${WEAVESIGNAL_DESKTOP_OUT:-$root/dist/desktop}
version=$(python3 -I -c 'import json, sys; print(json.load(open(sys.argv[1]))["version"])' "$root/apps/studio/src-tauri/tauri.conf.json")
deb="WeaveSignal_${version}_amd64.deb"
rpm="WeaveSignal-${version}-1.x86_64.rpm"
appimage="WeaveSignal_${version}_amd64.AppImage"
for f in "$deb" "$rpm" "$appimage"; do
  [ -f "$dist/$f" ] || { echo "missing $dist/$f: run just package-desktop first" >&2; exit 2; }
done
want="weavesignal-studio $version"
failed=0
run() { # name image command...
  local name=$1 image=$2; shift 2
  local got
  got=$(podman run --rm --network host --security-opt label=disable -v "$dist:/dist:ro" "$image" bash -c "$*" 2>&1)
  if [ "$(printf '%s\n' "$got" | tail -1)" = "$want" ]; then
    echo "pass  $name: $(printf '%s\n' "$got" | tail -1)"
  else
    echo "FAIL  $name"; printf '%s\n' "$got" | tail -8 | sed 's/^/      /'; failed=1
  fi
}
quiet_apt='export DEBIAN_FRONTEND=noninteractive; apt-get update -qq >/dev/null && apt-get install -y -qq'
debs=(docker.io/library/debian:13 docker.io/library/ubuntu:24.04 docker.io/library/ubuntu:26.04)
rpms=(registry.fedoraproject.org/fedora:43 registry.fedoraproject.org/fedora:44)
for image in "${debs[@]}"; do
  run "deb on ${image##*/}" "$image" \
    "$quiet_apt /dist/$deb >/dev/null 2>&1 || { apt-get install -y /dist/$deb 2>&1 | tail -5; exit 1; }; weavesignal-studio --version"
done
for image in "${rpms[@]}"; do
  run "rpm on ${image##*/}" "$image" \
    "dnf install -y -q /dist/$rpm >/dev/null 2>&1 || { dnf install -y /dist/$rpm 2>&1 | tail -5; exit 1; }; weavesignal-studio --version"
done
# The AppImage carries its libraries except those every desktop has; a container has no FUSE,
# so it extracts and runs.
desktop_deb='libgl1 libfontconfig1 libfreetype6 libharfbuzz0b libfribidi0 libx11-6 libx11-xcb1 libxcb1 libegl1 libglx0 libgbm1 libdrm2 libwayland-client0 libexpat1 libgpg-error0 libcom-err2'
desktop_rpm='fontconfig freetype harfbuzz fribidi libX11 libX11-xcb libxcb libglvnd-egl libglvnd-glx mesa-libgbm libdrm libwayland-client expat libgpg-error libcom_err'
for image in "${debs[@]}"; do
  run "AppImage on ${image##*/}" "$image" "$quiet_apt $desktop_deb >/dev/null 2>&1; /dist/$appimage --appimage-extract-and-run --version"
done
for image in "${rpms[@]}"; do
  run "AppImage on ${image##*/}" "$image" "dnf install -y -q $desktop_rpm >/dev/null 2>&1; /dist/$appimage --appimage-extract-and-run --version"
done
# The window, on a virtual X display. The process is looked up by its name as the kernel keeps
# it, cut to 15 characters (`weavesignal-stu`); procps refuses a longer `-x` pattern outright, so
# since the rename (ADR-0206) the check failed with the window up.
shots="$root/target/desktop-shots"; mkdir -p "$shots"
window() { # name image install-command launch-command
  local name=$1 image=$2 install=$3 launch=$4 shot
  shot="$(tr ' :./' '----' <<< "$name").png"
  podman run --rm --security-opt label=disable -v "$dist:/dist:ro" -v "$shots:/shots" "$image" bash -c "
    $install >/dev/null 2>&1 || { echo 'the install failed'; exit 1; }
    Xvfb :9 -screen 0 1440x900x24 >/dev/null 2>&1 & export DISPLAY=:9
    for i in \$(seq 1 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.1; done
    $launch > /tmp/app.log 2>&1 &
    for i in \$(seq 1 15); do sleep 1; done
    xwd -root -silent | convert xwd:- /shots/$shot
    if pgrep -x weavesignal-stu >/dev/null && pgrep -f WebKitWebProcess >/dev/null; then echo '$want'; else tail -5 /tmp/app.log; fi" > "$shots/$shot.log" 2>&1
  if [ "$(tail -1 "$shots/$shot.log")" = "$want" ]; then
    echo "pass  window of $name: up after 15 s, target/desktop-shots/$shot"
  else
    echo "FAIL  window of $name"; tail -6 "$shots/$shot.log" | sed 's/^/      /'; failed=1
  fi
}
x_deb='xvfb x11-utils x11-apps imagemagick procps'
x_rpm='xorg-x11-server-Xvfb xdpyinfo xwd ImageMagick procps-ng'
window "deb on debian:13" docker.io/library/debian:13 \
  "$quiet_apt /dist/$deb $x_deb" "weavesignal-studio"
window "rpm on fedora:44" registry.fedoraproject.org/fedora:44 \
  "dnf install -y -q /dist/$rpm $x_rpm" "weavesignal-studio"
window "AppImage on ubuntu:24.04" docker.io/library/ubuntu:24.04 \
  "$quiet_apt $desktop_deb $x_deb" "/dist/$appimage --appimage-extract-and-run"
# Removing the packages leaves nothing of the app's own behind.
run "deb removed on Debian 13" docker.io/library/debian:13 \
  "$quiet_apt /dist/$deb >/dev/null 2>&1 && apt-get remove -y -qq weave-signal >/dev/null 2>&1 && ! command -v weavesignal-studio && ! ls /usr/share/applications/WeaveSignal.desktop 2>/dev/null && echo '$want'"
run "rpm removed on Fedora 44" registry.fedoraproject.org/fedora:44 \
  "dnf install -y -q /dist/$rpm >/dev/null 2>&1 && dnf remove -y -q weave-signal >/dev/null 2>&1 && ! command -v weavesignal-studio && ! ls /usr/share/applications/WeaveSignal.desktop 2>/dev/null && echo '$want'"
exit $failed
