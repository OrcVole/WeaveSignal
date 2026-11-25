# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Keyed spike, trap 1 (2026-11-23): a switch that leaves a shared source ends its track.
# The public output is a switch between the content and a slate; the keyed output reads the content directly.
# Run in the Liquidsoap 2.5.0-rc1 image with /x holding pub.flac (440 Hz, 2.013 s), key.flac (1 kHz, 2.007 s)
# and slate.flac (2500 Hz), writable: each keyed item then plays about 30 ms on the keyed output and is skipped,
# and the public output gets the rest of the frame the keyed item began in (7.5 ms) before the switch.
settings.log.level := 2
settings.frame.duration := 0.02
keyed = ref(false)
q = request.queue(id="q")
list.iter(fun (_) -> begin
    q.push.uri("annotate:keyed=\"0\":/x/pub.flac")
    q.push.uri("annotate:keyed=\"1\":/x/key.flac")
  end, [1,2,3,4,5,6])
q.on_track(synchronous=true, fun (m) -> keyed := (m["keyed"] == "1"))
content = fallback([q.{track_sensitive = {false}, on_select = fun (sw) -> sw.starting}, blank().{track_sensitive = {false}, on_select = fun (sw) -> sw.starting}])
slate = single("/x/slate.flac")
public = switch([({not keyed()}, content.{track_sensitive = {false}, on_select = fun (sw) -> sw.starting}), ({true}, slate.{track_sensitive = {false}, on_select = fun (sw) -> sw.starting})])
public = source.drop.metadata_track_marks(public)
output.file(fallible=true, %wav(stereo=true), "/x/public-a.wav", public)
output.file(fallible=true, %wav(stereo=true), "/x/keyed-a.wav", content)
thread.run(delay=24., fun () -> shutdown())
