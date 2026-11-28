# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# WeaveSignal playout: the one fixed Liquidsoap 2.5 script. It is never generated: the control
# plane writes data (JSON) and this script reads it (ADR-0048 rule 1). Ports are settings
# (ADR-0075), so they arrive in the data too.
#
#   WEAVESIGNAL_PLAYOUT_DATA                 the data file (see playout.example.json)
#   WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD    the source password for icecast.server, and for the
#                                          desk's feed port (ADR-0172)
#
# Carts sound in the desk, inside the programme that arrives here (ADR-0117); this script
# plays none.
#
# The programme arrives from the desk as Opus in Matroska over SRT on loopback (ADR-0049) and
# goes, lossless, to the listener process (listeners.liq), which holds the listener mount and
# encodes what listeners hear: playout never holds the mount, so listeners outlive a playout or
# desk restart (M4). The listener process runs icecast.server; in one process with the SRT
# input, output.icecast to icecast.server stalled (spike 01), so the SRT input stays here.

settings.log.level := 3
settings.log.stdout := true
settings.log.file := false

data_path = environment.get("WEAVESIGNAL_PLAYOUT_DATA")
if data_path == "" then
  print("weavesignal: WEAVESIGNAL_PLAYOUT_DATA is not set")
  shutdown(code=2)
end
password = environment.get("WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD")
if password == "" then
  print("weavesignal: WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD is not set")
  shutdown(code=2)
end

let json.parse (data :
  {
    station_id : string,
    srt : {host : string, port : int, format : string?},
    listeners : {host : string, feed_port : int},
    desk_feed : {host : string, port : int, enabled : bool?}?,
    control_socket : string,
    carts : {dir : string}?,
    library : {dir : string}?,
    fallback : {dir : string, default_file : string?}?,
    processing : {enabled : bool, target_lufs : float?, max_gain_db : float?}?,
    programme : {mode : string?, dead_air_s : float?, dead_air_db : float?}?,
    keyed :
      {
        enabled : bool,
        window : bool?,
        window_file : string?,
        slate : string?,
        lead_s : float?,
        hold_s : float?
      }?
  }
) = file.contents(data_path)

# The control plane drives playout over a Unix socket, never a TCP port.
settings.server.socket := true
settings.server.socket.path := data.control_socket
settings.server.socket.permissions := 0o600

# ---- Events ----
# What happened, for the control plane to collect (`events.since <n>`): each track and fallback
# file as it starts (the per-play log), each change of what is on air, and dead air. The ring
# keeps the last 500; `boot` tells the control plane this process started again.
boot = string(time())
event_seq = ref(0)
event_ring = ref([])

def emit(kind, ~uid="", ~item="", ~name="", ~detail="") =
  event_seq := event_seq() + 1
  e =
    json.stringify(
      compact=true,
      {
        seq=event_seq(),
        at=time(),
        kind=kind,
        id=uid,
        item=item,
        name=name,
        detail=detail
      }
    )
  event_ring := list.prefix(500, list.cons((event_seq(), e), event_ring()))
end

# `events.since <n>`: {"boot": ..., "seq": <latest>, "events": [...]} with the events after n,
# oldest first.
def events_since(arg) =
  n = int_of_string(default=0, string.trim(arg))
  fresh = list.rev(list.filter(fun (e) -> fst(e) > n, event_ring()))
  "{\"boot\":\"#{boot}\",\"seq\":#{event_seq()},\"events\":[#{
    string.concat(separator=",", list.map(snd, fresh))
  }]}"
end
server.register(namespace="events", usage="since <n>", description="Events after n, as JSON.", "since", events_since)

# ---- The programme ----
# Live (the default): the desk, and automation when the desk is away or silent too long (the
# dead-air fallback); automation: the scheduled log only. Then the station's fallback files,
# then silence. The control plane sets the mode (`programme.mode`) from the schedule.
programme_settings = data.programme ?? {mode = null, dead_air_s = null, dead_air_db = null}
mode = ref(programme_settings.mode ?? "live")
dead_air_s = programme_settings.dead_air_s ?? 10.
dead_air_db = programme_settings.dead_air_db ?? -50.
on_air = ref("silence")
dead_air = ref(false)

# The desk's programme. Playout connects to the desk's SRT listener and tries again every
# second while the desk is away: the desk sends Matroska's header to each connection, which
# it did not do when it was the one connecting (2026-10-25). Not mksafe: it starts on
# silence and only switches at a track boundary, which a live input never reaches (spike 01).
# The container is named (`format=matroska`), so FFmpeg does not probe for it. Probing the
# desk's streamable Matroska took about 3 s at every connection; the SRT input paces playout's
# clock, so nothing went to the listener process meanwhile, then the backlog went at once, and
# the listener process kept it in its feed buffer as delay for as long as playout stayed
# connected (feed.buffer_length 3.2-3.5 s with 0.5 s set, 2026-11-03; 0.34-0.62 s with the
# name, across three playout restarts and a desk restart). Liquidsoap 2.5 refuses FFmpeg's
# own options here (`analyzeduration`, `probesize`: "Invalid mime type arguments").
# `srt.format` is for test-onair's stand-in desk, which sends MPEG-TS; the desk sends Matroska.
srt_format = data.srt.format ?? "matroska"
desk =
  input.srt(
    host=data.srt.host,
    port=data.srt.port,
    mode="caller",
    polling_delay=1.0,
    content_type="application/ffmpeg;format=#{srt_format}"
  )
# Not a number never leaves the desk's input: `clip` turns NaN into 0. The first frames decoded
# from a fresh SRT connection can carry NaN, and a moment of it left the limiter below at NaN
# for good, so playout sent digital silence until a restart (staging, 2026-11-01; reproduced
# with tests/playout/nan.liq).
desk = clip(desk)

# The desk always sends (a silent bed from its first moment), so a connected desk is not on
# air by itself: after `dead_air_s` under `dead_air_db` it counts as away until it sounds again.
# blank.detect watches it, kept running by a dummy output so it hears the desk come back while
# something else is on air. (Not blank.strip: in 2.5.0-rc1 it passed the desk on as silence.)
desk_sounding =
  blank.detect(
    id="desk_sounding",
    max_blank=dead_air_s,
    threshold=dead_air_db,
    track_sensitive=false,
    desk
  )
output.dummy(id="desk_watch", fallible=true, desk_sounding)
desk_live =
  switch(
    id="desk_live",
    [
      (
        {mode() == "live" and not desk_sounding.is_blank()},
        desk_sounding.{track_sensitive = {false}}
      )
    ]
  )

# Automation: the control plane queues the scheduled log here (`autodj.push`), a few items
# ahead. Each request names its log item, so the per-play log knows what started when.
# Scheduled carts (station idents in a clock) stay in playout (ADR-0117); the wall's carts sound
# in the desk.
carts_dir = null.case(data.carts, fun () -> "/var/lib/weavesignal/carts", fun (c) -> c.dir)
library_dir = null.case(data.library, fun () -> "/var/lib/weavesignal/library", fun (l) -> l.dir)
media_file_pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}[.]flac$"
autodj_queue = request.queue(id="autodj", interactive=false)
autodj_items = ref([])
autodj_current = ref("")
autodj_queue.on_track(
  synchronous=true,
  fun (m) ->
    begin
      item = m["weavesignal_item"]
      autodj_current := item
      autodj_items := list.filter(fun (i) -> i != item, autodj_items())
      emit(m["weavesignal_kind"], uid=m["weavesignal_id"], item=item)
    end
)
autodj = amplify(id="autodj_gain", override="liq_amplify", 1., autodj_queue)

# `autodj.push <item> <track|cart> <id> <file> <gain dB>`: the file is a name the media worker
# gave it, in the library (a track) or the cart directory (a cart).
def autodj_push(arg) =
  words = string.split(separator=" ", string.trim(arg))
  keyed_word = if list.length(words) == 6 then list.nth(words, 5) else "" end
  if
    list.length(words) != 5 and not (list.length(words) == 6 and keyed_word == "keyed")
  then
    "error usage: push <item> <track|cart> <id> <file> <gain dB> [keyed]"
  else
    item = list.nth(words, 0)
    kind = list.nth(words, 1)
    uid = list.nth(words, 2)
    name = list.nth(words, 3)
    gain = float_of_string(default=999., list.nth(words, 4))
    dir = if kind == "cart" then carts_dir else library_dir end
    if
      not string.match(pattern="^[0-9]{1,18}$", item)
    then
      "error not a log item"
    elsif
      kind != "track" and kind != "cart"
    then
      "error kind must be track or cart"
    elsif
      not string.match(pattern="^[0-9a-f-]{36}$", uid)
    then
      "error not an identifier"
    elsif
      not string.match(pattern=media_file_pattern, name)
    then
      "error not a media file name"
    elsif
      gain < -40. or gain > 20.
    then
      "error gain must be -40 to 20 dB"
    elsif
      not file.exists("#{dir}/#{name}")
    then
      "error missing #{name}"
    else
      autodj_items := list.append(autodj_items(), [item])
      autodj_queue.push(
        request.create(
          "annotate:weavesignal_item=\"#{item}\",weavesignal_kind=\"#{kind}\",weavesignal_id=\"#{
            uid
          }\",weavesignal_keyed=\"#{if keyed_word == "keyed" then "1" else "0" end}\",liq_amplify=\"#{
            gain
          } dB\":#{dir}/#{name}"
        )
      )
      "ok #{list.length(autodj_items())}"
    end
  end
end

def autodj_status(_) =
  json.stringify(
    compact=true,
    {
      current=autodj_current(),
      queued=autodj_items(),
      remaining=autodj_queue.remaining()
    }
  )
end

def autodj_skip(_) =
  autodj_queue.skip()
  "ok"
end

def autodj_clear(_) =
  autodj_queue.set_queue([])
  autodj_items := []
  "ok"
end

server.register(namespace="autodj", usage="push <item> <track|cart> <id> <file> <gain dB> [keyed]", description="Queue a log item.", "push", autodj_push)
server.register(namespace="autodj", usage="status", description="The current and queued log items, as JSON.", "status", autodj_status)
server.register(namespace="autodj", usage="skip", description="End the current item.", "skip", autodj_skip)
server.register(namespace="autodj", usage="clear", description="Empty the queue.", "clear", autodj_clear)

# The station's fallback files (shared with the listener process), when automation has
# nothing queued either; then the default loop WeaveSignal ships (ADR-0130), which plays only
# while the folder holds nothing it can play (an empty folder is what a new station has); then
# silence. Each is a child of the programme below, not a fallback of its own: one that ended in
# silence would never let the next through.
# The folder is read by station_files.liq's playlist, which leaves out files that cannot be sounds
# (a README, a picture), so they do not fill the log with decoder errors (1.3.3).
%include "station_files.liq"
fallback_dir = null.case(data.fallback, fun () -> "", fun (f) -> f.dir)
default_file = null.case(data.fallback, fun () -> "", fun (f) -> f.default_file ?? "")
station_files =
  if
    fallback_dir != "" and file.is_directory(fallback_dir)
  then
    s = station_playlist("fallback_files", fallback_dir)
    s.on_track(
      synchronous=true,
      fun (m) -> emit("fallback", name=path.basename(m["filename"]))
    )
    [(s : source)]
  else
    []
  end
# The loop is not reported play by play: it repeats every few seconds, and the per-play log
# would fill with it. Going on air as `default` is the one event (and the studio app says so).
default_loop =
  if
    default_file != "" and file.exists(default_file)
  then
    [(single(id="default_loop", fallible=true, default_file) : source)]
  else
    []
  end

# Each child says when it goes on air.
def on_air_as(name) =
  fun (sw) ->
    begin
      if
        on_air() != name
      then
        was = on_air()
        on_air := name
        emit("on_air", name=name, detail=was)
        # Dead air: the desk went quiet (or away) while live, and something else took over.
        if
          mode() == "live" and was == "desk" and not dead_air()
        then
          dead_air := true
          emit("dead_air", name=name)
        elsif
          name == "desk" and dead_air()
        then
          dead_air := false
          emit("dead_air_end", name=name)
        end
        # A live show ends: automation starts the next item, not the rest of the one it left.
        if name == "desk" then autodj_queue.skip() end
      end
      sw.starting
    end
end

# Each child below carries the same two methods, so they sit in one list.
def child(name, src) =
  src.{track_sensitive = {false}, on_select = on_air_as(name)}
end

programme =
  fallback(
    id="programme",
    list.flatten(
      [
        [child("desk", desk_live), child("automation", autodj)],
        list.map(fun (s) -> child("fallback", s), station_files),
        list.map(fun (s) -> child("default", s), default_loop),
        [child("silence", blank())]
      ]
    )
  )

# `programme.mode <live|auto>` and `programme.status`.
def programme_mode(arg) =
  m = string.trim(arg)
  if
    m != "live" and m != "auto"
  then
    "error mode must be live or auto"
  else
    if m != mode() then emit("mode", name=m, detail=mode()) end
    mode := m
    "ok #{m}"
  end
end

def programme_status(_) =
  json.stringify(
    compact=true,
    {
      boot=boot,
      mode=mode(),
      on_air=on_air(),
      dead_air=dead_air(),
      desk_connected=desk.is_ready(),
      desk_sounding=desk_sounding.is_ready() and not desk_sounding.is_blank()
    }
  )
end

server.register(namespace="programme", usage="mode <live|auto>", description="Live (the desk first) or automation.", "mode", programme_mode)
server.register(namespace="programme", usage="status", description="The mode and what is on air, as JSON.", "status", programme_status)


# The end of playout's chain, shared with its tests (tests/playout/chain.liq and nan.liq), so a
# test runs the chain that plays, not a copy: clip (no NaN reaches what follows), the gain
# control towards the station's loudness target, clip, the peak limiter, clip. See chain.liq.
%include "chain.liq"
processing = data.processing ?? {enabled = true, target_lufs = null, max_gain_db = null}
agc_target = processing.target_lufs ?? default_target_lufs
agc_max_gain = processing.max_gain_db ?? 6.
if
  agc_target < -40. or agc_target > -5. or agc_max_gain < 0. or agc_max_gain > 12.
then
  print("weavesignal: processing.target_lufs must be -40 to -5 and processing.max_gain_db 0 to 12")
  shutdown(code=2)
end
agc_gain = ref(fun () -> 1.)
programme =
  weavesignal_chain(
    enabled=processing.enabled, target=agc_target, max_gain=agc_max_gain, agc_gain, programme
  )

# `processing.status`: whether the chain runs, its target and limit, and the gain it applies now.
def processing_status(_) =
  get_gain = agc_gain()
  json.stringify(
    compact=true,
    {
      enabled=processing.enabled,
      target_lufs=agc_target,
      max_gain_db=agc_max_gain,
      gain_db=20. * log10(get_gain())
    }
  )
end
server.register(namespace="processing", usage="status", description="The processing chain, as JSON.", "status", processing_status)

# `playout.restart`: shut playout down so the supervisor starts it again (compose's restart policy,
# the Quadlet's Restart=always, the Cloudron app's supervisord): the studio's restart key. It
# answers first and exits a second later, so the answer reaches the control plane; exit code 0
# is restarted by every one of them. Listeners keep their mount (the listener process holds it)
# and hear its fallback until playout is back.
def playout_restart(_) =
  thread.run(delay=1., fun () -> shutdown())
  "Playout is restarting."
end
server.register(namespace="playout", usage="restart", description="Shut playout down; its supervisor starts it again.", "restart", playout_restart)

# ---- Keyed shows (1.3.2; spike on branch p132-keyed-spike, PLAN 19a) ----
# A keyed show is heard only with a key, so the public must not hear one sample of it. The
# decision is made here, on playout's own clock, where the programme is chosen, not in the
# listener process: there the feed arrives up to its buffer late (0.5 s, at most 10 s), so a
# switch there on the schedule would go back to the public feed while the end of a keyed show
# was still in that buffer.
#
# Two feeds go to the listener process: `feed` is the public programme, which is the slate while
# the gate is shut, and `feed_keyed` is the programme as it plays. The gate is shut while
#   - the control plane holds the window open (`keyed.window on`; the data's `keyed.window` at
#     start, so a playout that restarts mid-show starts shut), or
#   - automation plays an item pushed with `keyed`, or will start one within `lead_s`
#     (the queue knows its next request before it starts), so an item that starts early,
#     because the one before it was short, is still covered;
# and it stays shut for `hold_s` after both end, counted in frames, not on the wall clock (the
# queue moves on to its next request a frame or so before the last samples of the one before
# have played: 15 ms measured; and a wall-clock hold would run out while a loaded host stalls
# the clock between two frames).
# The control plane's rules, so that the gate never opens on keyed content: open the window at
# least `lead_s` before any change into a keyed show (`programme.mode live` for a keyed live
# show), close it only after the content has left it, and never push a keyed item into an empty
# queue while the window is closed.
#
# The gate is a mix of the programme and the slate with gains of 0 and 1 (`amplify`), never a
# `switch`: a switch that leaves a source ends that source's track (automation skipped the
# rest of a keyed item for everyone), and `add`'s own weights scale the shared frame in place
# (the keyed feed went silent). `override=null`: automation's items carry `liq_amplify`, which
# would otherwise replace the gate's 0.
keyed_cfg =
  data.keyed ?? {
    enabled = false,
    window = null,
    window_file = null,
    slate = null,
    lead_s = null,
    hold_s = null
  }
# Where the window stands at start: the data's `window`, or the control plane's own record of it
# (`window_file`, a file it writes beside this socket before each `keyed.window`, holding `on` or
# `off`), so a playout that restarts in the middle of a keyed show starts shut (1.3.2).
keyed_window_file = keyed_cfg.window_file ?? ""
keyed_window_at_start =
  (keyed_cfg.window ?? false)
or
  (
    keyed_window_file != ""
  and
    file.exists(keyed_window_file)
  and
    string.trim(file.contents(keyed_window_file)) == "on"
  )
keyed_window = ref(keyed_window_at_start)
keyed_lead = keyed_cfg.lead_s ?? 0.2
keyed_hold = keyed_cfg.hold_s ?? 0.2
# At least one frame, so a hold of 0 still covers the frame in which a keyed item ends.
keyed_hold_frames =
  max(1, int_of_float(ceil(keyed_hold / settings.frame.duration())))
keyed_held = ref(if keyed_window() then keyed_hold_frames else 0 end)
keyed_shut = ref(keyed_window())
keyed_item = ref(false)

def keyed_request(r) =
  m = request.metadata(r)
  m["weavesignal_keyed"] == "1"
end
def keyed_item_now() =
  if
    on_air() != "automation"
  then
    false
  else
    current = null.case(autodj_queue.current(), fun () -> false, keyed_request)
    rem = autodj_queue.remaining()
    pending = autodj_queue.queue()
    current
    or
      (
        rem >= 0.
      and
        rem < keyed_lead
      and
        list.length(pending) > 0
      and
        keyed_request(list.hd(pending))
      )
  end
end
# Once per frame, before the programme makes it: both gains below read the same answer.
def keyed_update() =
  item = keyed_item_now()
  keyed_item := item
  window = keyed_window()
  # Shut for this frame if keyed now or held over from a keyed frame; then the hold runs down
  # one frame, so a hold of n frames covers the n frames after the last keyed one.
  shut = window or item or keyed_held() > 0
  if
    window or item
  then
    keyed_held := keyed_hold_frames
  elsif
    keyed_held() > 0
  then
    keyed_held := keyed_held() - 1
  end
  if
    shut != keyed_shut()
  then
    keyed_shut := shut
    emit("keyed", name=(if shut then "shut" else "open" end))
  end
end

# What the public hears while the gate is shut, for `keyed.status` (the desk's Playout panel):
# the slate's child on air now, set as each is selected.
keyed_slate_kind = ref("silence")
def keyed_gate(programme) =
  slate_file = keyed_cfg.slate ?? ""
  # The station's slate file (`keyed.slate`); else its fallback files (a playlist of its own, so
  # the slate does not move the fallback's place); else the default loop; else silence. Every
  # one is a child of one fallback, tried at every frame, as the programme's are above: one that
  # has nothing to play (a fallback folder with no file that plays, which is what a new station
  # has) lets the next through. It used to pick one by what existed, and an empty fallback
  # folder, which every Cloudron install makes, gave the public digital silence for a whole
  # keyed show (the test station, 2026-11-25).
  def slate_child(name, s) =
    s.{
      track_sensitive = {false},
      on_select =
        fun (sw) ->
          begin
            keyed_slate_kind := name
            sw.starting
          end
    }
  end
  slate_children =
    list.flatten(
      [
        if
          slate_file != "" and file.exists(slate_file)
        then
          [
            slate_child(
              "slate_file",
              (single(id="keyed_slate_file", fallible=true, slate_file) : source)
            )
          ]
        else
          []
        end,
        if
          fallback_dir != "" and file.is_directory(fallback_dir)
        then
          [
            slate_child(
              "fallback",
              (station_playlist("keyed_slate_files", fallback_dir) : source)
            )
          ]
        else
          []
        end,
        if
          default_file != "" and file.exists(default_file)
        then
          [
            slate_child(
              "default_loop",
              (single(id="keyed_slate_loop", fallible=true, default_file) : source)
            )
          ]
        else
          []
        end,
        [slate_child("silence", (blank(id="keyed_slate_silence") : source))]
      ]
    )
  keyed_slate = fallback(id="keyed_slate", slate_children)
  ignore(programme.on_frame(before=true, synchronous=true, keyed_update))
  add(
    id="keyed_gate",
    normalize=false,
    [
      amplify(
        id="keyed_gate_programme", override=null, {if keyed_shut() then 0. else 1. end}, programme
      ),
      amplify(
        id="keyed_gate_slate", override=null, {if keyed_shut() then 1. else 0. end}, keyed_slate
      )
    ]
  )
end
keyed_programme = programme
programme = if keyed_cfg.enabled then keyed_gate(programme) else programme end

def keyed_window_command(arg) =
  a = string.trim(arg)
  if
    not keyed_cfg.enabled
  then
    "error keyed shows are off in the data"
  elsif
    a == "on"
  then
    keyed_window := true
    "ok on"
  elsif
    a == "off"
  then
    keyed_window := false
    "ok off"
  else
    "error window takes on or off"
  end
end
# `keyed.mute <seconds>`: the keyed feed silent for that long, counted in frames (at most 60 s).
# The control plane sends it at a change of keyed show, so the first show's listeners, who are
# dropped a moment later, never hear the next one's start through the listener process's buffer
# (the spike's question 6; 1.3.2).
keyed_mute_frames = ref(0)
def keyed_mute_command(arg) =
  s = float_of_string(default=-1., string.trim(arg))
  if
    not keyed_cfg.enabled
  then
    "error keyed shows are off in the data"
  elsif
    s < 0. or s > 60.
  then
    "error mute takes 0 to 60 seconds"
  else
    keyed_mute_frames := int_of_float(ceil(s / settings.frame.duration()))
    "ok muted #{s}"
  end
end
def keyed_status(_) =
  json.stringify(
    compact=true,
    {
      enabled=keyed_cfg.enabled,
      window=keyed_window(),
      item=keyed_item(),
      shut=keyed_shut(),
      muted=keyed_mute_frames() > 0,
      slate=keyed_slate_kind()
    }
  )
end
server.register(namespace="keyed", usage="mute <seconds>", description="Silence the keyed feed for a while (a change of keyed show).", "mute", keyed_mute_command)
server.register(namespace="keyed", usage="window <on|off>", description="Hold the public gate shut (a keyed show's window).", "window", keyed_window_command)
server.register(namespace="keyed", usage="status", description="The keyed gate, as JSON.", "status", keyed_status)

# To the listener process: PCM in WAV over loopback (1.5 Mbit/s), so the listener stream is
# encoded once. Not Ogg FLAC: FLAC codes digital silence in a few bytes, so with nothing on air
# Ogg pages arrived about every 10 s and the listener process's buffer ran dry and overflowed
# by turns (the carts stack test, 2026-10-26).
# Metadata and track marks are dropped first: every switch (the desk arriving, a fallback) would
# otherwise start a chained Ogg stream, which icecast.server rejected on "non-monotonic DTS",
# dropping the mount and every listener with it (the third fault; it also caused the EPIPE on
# each SRT reconnect). Titles reach the control plane over the socket instead.
programme = source.drop.metadata_track_marks(programme)
# The video's sound gets the programme as it plays, keyed shows included: HLS is one ladder, and
# the control plane serves a keyed show's segments only on its keyed path (1.3.2 spike).
keyed_out = source.drop.metadata_track_marks(keyed_programme)
# The keyed feed alone carries the mute (the video's sound does not).
def keyed_mute_tick() =
  if keyed_mute_frames() > 0 then keyed_mute_frames := keyed_mute_frames() - 1 end
end
keyed_feed =
  amplify(
    id="keyed_feed_mute", override=null, {if keyed_mute_frames() > 0 then 0. else 1. end}, keyed_out
  )
ignore(keyed_feed.on_frame(before=false, synchronous=true, keyed_mute_tick))
to_desk_programme = if keyed_cfg.enabled then keyed_out else programme end
output.icecast(
  id="to-listener-process",
  %wav(stereo = true, samplerate = 48000, header = true),
  format="audio/wav",
  host=data.listeners.host,
  port=data.listeners.feed_port,
  password=password,
  mount="feed",
  programme
)

# The keyed feed: the programme as it plays, keyed shows included, for the listener process's
# keyed mounts only. Off unless the data says `keyed.enabled`.
if
  keyed_cfg.enabled
then
  output.icecast(
    id="to-listener-process-keyed",
    %wav(stereo = true, samplerate = 48000, header = true),
    format="audio/wav",
    host=data.listeners.host,
    port=data.listeners.feed_port,
    password=password,
    mount="feed_keyed",
    keyed_feed
  )
end

# Playout's output back to the desk (ADR-0172), when the station has video: the video stream's
# sound is the desk's own programme while someone is live, and this while nobody is, so viewers
# hear what listeners hear. The same PCM in WAV as the listener process gets, to the desk's feed
# port on loopback; the desk listens, and this tries again every 3 s while the desk is away, so
# either restarts without the other.
# On playout's own clock, as the listener process's feed is (2026-11-12). It used to go through
# a buffer onto a clock of its own, so that a desk that stopped reading could hold up nothing
# else; on the test station that clock fell behind real time on a busy server (its tick cost
# climbing as playout ran, while playout's clock kept time), emptied its buffer and filled the
# video's sound with silence about once a second. The desk reads as data arrives and never
# pushes back; if it stops reading, the kernel's socket buffers hold some seconds, and then the
# 1 s write timeout drops the connection and playout goes on (a write waits only once those
# buffers are full; measured locally: listeners kept time through a 25 s stall).
null.case(
  data.desk_feed,
  fun () -> (),
  fun (f) ->
    if
      f.enabled ?? true
    then
      output.icecast(
        id="to-desk",
        %wav(stereo = true, samplerate = 48000, header = true),
        format="audio/wav",
        host=f.host,
        port=f.port,
        password=password,
        mount="feed",
        send_icy_metadata=false,
        timeout=1.,
        connection_timeout=1.,
        to_desk_programme
      )
    end
)
