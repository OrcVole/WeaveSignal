# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Playout's processing chain, checked on tones (playout/chain.liq, the chain that plays, at its
# defaults): a quiet one is brought up and a loud one down towards
# -16 LUFS, within the 6 dB limit; one already at the target is left alone; one under the
# threshold is not pumped up; and what comes out never passes the limiter's -1.5 dBFS. Exits 0
# when all six hold, 1 when one does not. `just liq-check` runs it, two minutes of
# tone each, in a few seconds.
%include "probe.liq"
settings.frame.audio.samplerate := 48000
settings.log.stdout := true
settings.log.level := 2

# A 1 kHz sine, the same in both channels, has the loudness of its level in dBFS (20 log10 of
# its amplitude) in LUFS; the output must sit within half a dB of where the chain should take it.
def tone(name, amplitude, expected_lufs) =
  probe(
    name,
    amplify(amplitude, sine(1000.)),
    120.,
    fun (lufs, peak_db, _) -> abs(lufs - expected_lufs) < 0.5 and peak_db <= -1.5
  )
end

tone("-40 LUFS in: raised by the 6 dB limit", 0.01, -34.)
tone("-16 LUFS in: left alone", 0.1585, -16.)
tone("-12 LUFS in: brought down to the target", 0.2512, -16.)
tone("-8 LUFS in: lowered by the 6 dB limit", 0.3981, -14.)
tone("-1 LUFS in: lowered by the 6 dB limit", 0.9, -7.)
tone("-66 LUFS in, under the threshold: not pumped up", 0.0005, -66.)
