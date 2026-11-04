#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Runs design.mojo against VoiceLab's OmniVoice environment (VOICELAB_VENV, by default
# ~/.local/opt/voicelab/venvs/tts-omnivoice): Mojo's Python is pointed at that environment's own
# interpreter library, site-packages and extension modules (Mojo's embedded interpreter
# otherwise looks for them under a build prefix). The designed references land in
# target/demo-video/voices/. voicelab-tts must be stopped (one OmniVoice on the GPU at a time).
#
#   tests/demo-video/design.sh [seed-offset]
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
venv=${VOICELAB_VENV:-$HOME/.local/opt/voicelab/venvs/tts-omnivoice}
py="$venv/bin/python"
if systemctl --user is-active -q voicelab-tts 2>/dev/null; then
  echo "voicelab-tts is running: stop it first (one OmniVoice on the GPU at a time)" >&2; exit 1
fi
libdir=$("$py" -c 'import sysconfig; print(sysconfig.get_config_var("LIBDIR"))')
lib=$("$py" -c 'import sysconfig; print(sysconfig.get_config_var("LDLIBRARY"))')
site=$("$py" -c 'import site; print(site.getsitepackages()[0])')
dynload=$("$py" -c 'import sysconfig; print(sysconfig.get_path("platstdlib"))')/lib-dynload

MOJO_PYTHON_LIBRARY="$libdir/$lib" PYTHONPATH="$site:$dynload" \
  mojo run "$here/design.mojo" "$here/script.json" "$root/target/demo-video/voices" "${1:-0}"
