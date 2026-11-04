# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The test helper for playout's chain: `probe` runs a source through the chain that plays
# (playout/chain.liq, included here, at its default settings) and checks what comes out.
# `nan.liq` and `chain.liq` include it.
%include "../../playout/chain.liq"

# `probe` runs a source through the chain faster than real time (a clock without sync) for
# `seconds` of its own time, counted in frames, and then calls `check(loudness_lufs, peak_db,
# rms)` (what the last third of the run showed), which gives the verdict; the verdicts are collected and the script exits 0 when every
# probe passed and 1 when one did not. It counts the frames the source makes and does not wait
# for it to end: a finite source on an unsynced clock sometimes ended before it made a frame
# (one run in six, with the gain control in the chain), which looks like silence and is not.
probes_pending = ref(0)
probes_failed = ref(false)

def probe(name, input, seconds, check) =
  probes_pending := probes_pending() + 1
  s = weavesignal_chain(ref(fun () -> 1.), input)
  p = peak(s)
  r = rms(p)
  m = lufs(window=3., r)
  elapsed = ref(0.)
  done = ref(false)
  loudest = ref(0.)
  loudest_rms = ref(0.)
  last_lufs = ref(-200.)
  m.on_frame(synchronous=true, fun () -> begin
    if
      not done() and m.is_ready()
    then
      elapsed := elapsed() + settings.frame.duration()
      # What the last third of the run did, after the gain control has settled. The meters are
      # read from another thread and sometimes read nothing (0, or NaN) for a frame: the peak
      # and the rms are the largest seen, and the loudness the last number that was one.
      if
        elapsed() > seconds * 0.67
      then
        if p.peak() > loudest() then loudest := p.peak() end
        if r.rms() > loudest_rms() then loudest_rms := r.rms() end
        if m.lufs() == m.lufs() and m.lufs() > -150. then last_lufs := m.lufs() end
      end
      if
        elapsed() >= seconds
      then
        done := true
        peak_db = 20. * log10(max(loudest(), 0.000001))
        ok = check(last_lufs(), peak_db, loudest_rms())
        print("#{name}: #{last_lufs()} LUFS, peak #{peak_db} dBFS, rms #{loudest_rms()}: #{if ok then 'ok' else 'FAILED' end}")
        if not ok then probes_failed := true end
        probes_pending := probes_pending() - 1
        if probes_pending() == 0 then exit(if probes_failed() then 1 else 0 end) end
      end
    end
  end)
  o = output.dummy(fallible=true, m)
  clock.assign_new(sync="none", [o])
end
