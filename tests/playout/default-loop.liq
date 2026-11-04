# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The default loop (ADR-0130) as the stations play it: `single` over assets/default-loop/
# default-loop.flac, for 100 seconds of its own time (three 32-second laps and a bit) faster than
# real time. It must never go quiet, not even for 20 ms, which is what a gap or a click at the
# join of two laps would look like to the blank detector; and it must be heard (a file that did
# not load plays nothing). The loop's own quietest stretch is far above the detector's -60 dB.
# Exits 0 when it holds, 1 when it does not. `just liq-check` runs it, with the file at /a.
settings.frame.audio.samplerate := 48000
settings.log.stdout := true
settings.log.level := 2

path = "/a/default-loop.flac"
loop = single(id="default_loop", fallible=true, path)
loop = blank.detect(id="gap", max_blank=0.02, threshold=-60., track_sensitive=false, loop)
gaps = ref(0)
loop.on_blank(synchronous=true, fun () -> gaps := gaps() + 1)

elapsed = ref(0.)
heard = ref(0.)
r = rms(loop)
r.on_frame(synchronous=true, fun () -> begin
  if
    r.is_ready() and elapsed() < 100.
  then
    elapsed := elapsed() + settings.frame.duration()
    if r.rms() > heard() then heard := r.rms() end
    if
      elapsed() >= 100.
    then
      print("default loop: 100 s, #{gaps()} gaps, loudest rms #{heard()}")
      exit(if gaps() == 0 and heard() > 0.01 then 0 else 1 end)
    end
  end
end)
o = output.dummy(fallible=true, r)
clock.assign_new(sync="none", [o])
