# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Playout's guard against NaN (staging, 2026-11-01): a tenth of a second of NaN, then a tone,
# through playout's whole chain at its default settings (playout/chain.liq, the chain that plays:
# `clip`, the automatic gain control, `clip`, the limiter, `clip`). Without the first
# clip, the limiter's state stays NaN and everything after is digital silence, and the gain
# control's stays NaN and leaves the tone untouched. So the tone must come out, and the gain
# control must have turned it down (a 0.5 sine is about -6 LUFS, 10 dB over the -16 LUFS target, so
# its 6 dB limit applies, to an rms near 0.18): an rms between 0.1 and 0.3, where an untouched tone is 0.354. Exits 0 when it does, 1
# when it does not. `just liq-check` runs it.
%include "probe.liq"
settings.frame.audio.samplerate := 48000
settings.log.stdout := true
settings.log.level := 2
nan = 0. / 0.
probe(
  "the tone after NaN",
  sequence([amplify(nan, sine(440., duration=0.1)), amplify(0.5, sine(440.))]),
  60.,
  fun (_, _, rms) -> rms > 0.1 and rms < 0.3
)
