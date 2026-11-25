# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Keyed spike, traps 2 and 3 (2026-11-23): the gate as add(weights=...) scales the shared frame in place
# (MODE=weights: the keyed output goes silent when computed after the public one, ORDER=pubfirst); as two
# amplify (MODE=amp) it does not. LEAD=0.1 shuts the gate 0.1 s before a keyed item (the queue knows its
# next request); the queue moves on 15 ms before the last samples of a keyed item have played (hence a hold).
# Same /x files as switch.liq.
settings.log.level := 2
lead = float_of_string(default=0.1, environment.get("LEAD"))
q = request.queue(id="q")
list.iter(fun (_) -> begin
    q.push.uri("annotate:keyed=\"0\":/x/pub.flac")
    q.push.uri("annotate:keyed=\"1\":/x/key.flac")
  end, [1,2,3,4,5,6])
def is_keyed(r) =
  m = request.metadata(r)
  m["keyed"] == "1"
end
def item_keyed() =
  cur = null.case(q.current(), fun () -> false, is_keyed)
  rem = q.remaining()
  pending = q.queue()
  nxt = lead > 0. and rem >= 0. and rem < lead and list.length(pending) > 0 and is_keyed(list.hd(pending))
  cur or nxt
end
cut = fun (sw) -> sw.starting
content = fallback([q.{track_sensitive = {false}, on_select = cut}, blank().{track_sensitive = {false}, on_select = cut}])
slate = single("/x/slate.flac")
shut = {item_keyed()}
public = if environment.get("MODE") == "amp" then add(normalize=false, [amplify({if shut() then 0. else 1. end}, content), amplify({if shut() then 1. else 0. end}, slate)]) else add(normalize=false, weights=[{if shut() then 0. else 1. end}, {if shut() then 1. else 0. end}], [content, slate]) end
public = source.drop.metadata_track_marks(public)
if environment.get("ORDER") == "keyfirst" then
  output.file(fallible=true, %wav(stereo=true), "/x/keyed-b.wav", content)
  output.file(fallible=true, %wav(stereo=true), "/x/public-b.wav", public)
else
  output.file(fallible=true, %wav(stereo=true), "/x/public-b.wav", public)
  output.file(fallible=true, %wav(stereo=true), "/x/keyed-b.wav", content)
end
thread.run(delay=12.5, fun () -> shutdown())
