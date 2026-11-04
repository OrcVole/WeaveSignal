# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The end of playout's chain (ADR-0044's "one processing chain"), in one place: weavesignal.liq
# includes it for the programme, and tests/playout/chain.liq and nan.liq include it to test the
# chain that plays (review 0028, C1: they used to test a hand-made copy).
#
# A conservative chain: a slow automatic gain control towards the station's loudness target, then
# a peak limiter. It evens out what the desk does not process (the library, the fallback files,
# the default loop) and leaves programme at its target alone: a signal already at the target
# gets no gain. Single band on purpose, so there is no tone to tune and nothing that pumps on
# speech.
#   - the target is in LUFS (a 3 s window), -16 by default: the operator's choice of 2026-11-02
#     after a listening comparison against -23 and off (research/2026-11-02-loudness-comparison.md
#     in the project folder). Every source WeaveSignal plays measured -16 to -19 LUFS, so -23 mostly
#     turned the station down; -16 is near where internet streams sit;
#   - the gain stays within +-max_gain dB (6 by default), rises over about 15 s and falls over
#     about 2 s, so a loud track is brought down quickly and a quiet one up slowly;
#   - under -50 dB it holds its gain, so a quiet room or the desk's silent bed is not pumped up.
# A clip comes before and after the gain control: once NaN reaches `normalize` or `compress`,
# their state stays NaN and everything after is silence until playout restarts (staging,
# 2026-11-01); `clip` maps NaN to 0 and leaves real audio alone.

# The loudness target when the station's data gives none.
default_target_lufs = -16.

# `gain` is a reference that is given the gain control's own gain getter (for
# `processing.status`); it is left alone when the gain control is off.
def weavesignal_chain(~enabled=true, ~target=default_target_lufs, ~max_gain=6., gain, s) =
  s = clip(s)
  s =
    if
      enabled
    then
      agc =
        normalize(
          id="agc",
          lufs=true,
          target=target,
          up=15.,
          down=2.,
          gain_min=-max_gain,
          gain_max=max_gain,
          threshold=-50.,
          window=3.,
          track_sensitive=false,
          s
        )
      gain := agc.gain
      (agc : source)
    else
      (s : source)
    end
  s = clip(s)
  # A peak limiter as the last guard (the desk already limits its programme at -1 dBFS, carts
  # included): peak mode, 1.5 dB under full scale, then a clip that catches what a 1 ms attack lets
  # past. Below its knee it does nothing.
  # `lookahead` must stay 0 in Liquidsoap 2.5, in peak and RMS mode alike. With any lookahead, `compress`
  # keeps its detector's samples in a ring buffer made with OCaml's `Array.create_float` (mm's
  # `Audio.create`), which is uninitialised memory; the first `lookahead` samples it reads are
  # whatever the heap held. A NaN or an infinity there makes its gain NaN for good on the first
  # frame, and the clip after turns that into zeros until playout restarts; the clip before cannot
  # help, the input is clean. Measured 2026-11-02 (real time, this script with lookahead 5 ms and
  # silence in): 7 of 80 starts silent for good, 0 of 80 with lookahead 0; q-all's script, without
  # the gain control, 0 of 240; a bare sine on an unsynced clock, 133 of 300 against 0 of 300. The
  # rate is whatever the heap holds, so any lookahead is unsafe. It bought nothing either: the
  # buffer delays the detector, not the audio, so the first 5 ms of a peak passed untouched.
  s =
    compress(
      id="limiter",
      attack=1.,
      release=150.,
      ratio=20.,
      threshold=-1.5,
      knee=1.,
      lookahead=0.,
      window=0.,
      s
    )
  clip(s)
end
