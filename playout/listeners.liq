# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# WeaveSignal's listener server: Liquidsoap 2.5's icecast.server, and the one encoder listeners
# hear, in their own process, fixed like weavesignal.liq and reading the same data (ADR-0048
# rule 1).
#
#   WEAVESIGNAL_PLAYOUT_DATA                 the data file (see playout.example.json)
#   WEAVESIGNAL_LISTENERS_SOURCE_PASSWORD    the source password: playout's feed, and this
#                                          process's own source on the mount
#
# One more optional stream (ADR-0002): an MP3 compatibility stream beside the Opus one, for
# directories and smart speakers, off unless the data says `listeners.compat.enabled`. LAME
# encodes it (not fdk-aac, which Ubuntu's Liquidsoap links and which blocks the app store). It
# takes the same programme, once, on the same port, under its own mount.
#
# Playout does not hold the listener mount (M4; rehearsal 02). Playout sends its programme here
# as PCM in WAV over loopback (the feed); this process falls back to the station's
# fallback (files in `fallback.dir`, else the shipped default loop, else silence) while the feed
# is away, and encodes the listener stream itself. So the mount, its one Ogg Opus stream and every listener on it
# outlive a playout or desk restart: a listener hears the fallback, then the programme again,
# without reconnecting.
#
# Metadata and track marks are dropped before the encoder: a new track starts a chained Ogg
# stream, and icecast.server drops the source and every listener on it ("non-monotonic DTS",
# the third fault). Now-playing reaches listeners from the control plane instead
# (`/api/v1/now-playing`).
#
# dedicated_encoder=true is required: in its default mode icecast.server sends a listener who
# joins late no Opus headers (bench 01; ADR-0011, ADR-0029; Liquidsoap issue 5542). Keep it
# until a fixed release passes the late-listener check.
# The harbors bind loopback only; the reverse proxy publishes the streams (ADR-0048 rule 3).
# A listener or relay must not ask for ICY metadata: icecast.server splices ICY blocks into
# Ogg when asked (bench 02).

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
    listeners :
      {
        bind : string,
        host : string,
        port : int,
        mount : string,
        feed_port : int,
        feed_buffer_s : float?,
        control_socket : string?,
        directory : bool?,
        compat : {enabled : bool, mount : string?, bitrate : int?}?
      },
    fallback : {dir : string, default_file : string?}?,
    logger : {dir : string, bitrate : int?}?
  }
) = file.contents(data_path)

# The control plane turns the legal logger on and off here, over a Unix socket.
null.case(
  data.listeners.control_socket,
  fun () -> (),
  fun (socket_path) ->
    begin
      settings.server.socket := true
      settings.server.socket.path := socket_path
      settings.server.socket.permissions := 0o600
    end
)

settings.harbor.bind_addrs := [data.listeners.bind]
# `/status.json` (loopback only; the proxy must not publish it) gives the control plane each
# listener's connection number and how long it has listened, and nothing that identifies a
# person: no address, no user agent (ADR-0115). The control plane turns it into listener hours
# and sessions (ADR-0046).
def anonymous_status(mounts) =
  def listener(l) =
    {id=l.id, duration=l.duration}
  end
  def mount(m) =
    let (name, st) = m
    {mount=name, listeners=list.map(listener, st.listeners)}
  end
  json.stringify(compact=true, {mounts=list.map(mount, mounts)})
end
icecast.server(
  port=data.listeners.port,
  password=password,
  dedicated_encoder=true,
  serve_json=anonymous_status
)

# Playout's programme. Half a second of pre-buffer on loopback by default (ADR-0182 step one: 2 h
# behind the real desk on 2026-11-03, the buffer read 0.36-0.66 s, no underrun; it was 1 s
# before): harbor's default (12 s) would hold listeners on the fallback for 12 s after every
# playout start. `listeners.feed_buffer_s` sets it: on a loaded host (staging, load 15 on 12
# cores, 2026-11-03) this process's clock fell behind, 0.5 s ran dry, listeners heard the
# fallback files for moments, and each refill left the backlog bigger (6 s after ten flips).
# Decoded by Liquidsoap's own WAV decoder, not FFmpeg's (which ranks first by default): FFmpeg
# probes the stream for 4 to 6 s before it gives a frame, harbor keeps buffering meanwhile, and
# what it buffered stays as latency for as long as playout stays connected (the feed and the
# listener stream run at the same rate, so it never drains). Measured 2026-11-02: 4.1-4.3 s in
# the buffer for the whole run with FFmpeg, about 1 s with this; test-onair heard the desk
# return 19 s late before (its deadline is 20 s), 15 s after. Behind the real desk the buffer
# still held about 3 s more until playout stopped FFmpeg probing the desk's Matroska too
# (weavesignal.liq, 2026-11-03); test-bundle checks it.
settings.decoder.priorities.wav := 20
feed =
  input.harbor(
    id="feed",
    "feed",
    port=data.listeners.feed_port,
    password=password,
    buffer=data.listeners.feed_buffer_s ?? 0.5,
    max=10.
  )

# What listeners hear while the feed is away, in order: the station's fallback files in rotation;
# else the default loop WeaveSignal ships (ADR-0130, `fallback.default_file`), which plays only
# while the folder holds nothing it can play and the studio app says so; else silence. All of it
# is one flat list with the feed first: a fallback inside the list that ended in silence would
# never let the next child through. A child that can play wins at once (`track_sensitive`
# false), so files added to the folder replace the loop within seconds.
fallback_dir = null.case(data.fallback, fun () -> "", fun (f) -> f.dir)
default_file = null.case(data.fallback, fun () -> "", fun (f) -> f.default_file ?? "")

# What is on air now: `feed` (playout), `files`, `default` or `silence`. The control plane asks
# (`fallback.status`) so the studio app can say a default loop is playing, also while playout
# is away.
playing = ref("silence")
def child(name) =
  fun (sw) ->
    begin
      playing := name
      sw.starting
    end
end

feed_child = feed.{track_sensitive = {false}, on_select = child("feed")}
files_child =
  if
    fallback_dir != "" and file.is_directory(fallback_dir)
  then
    [
      playlist(id="fallback_files", mode="randomize", reload_mode="watch", fallback_dir).{
        track_sensitive = {false},
        on_select = child("files")
      }
    ]
  else
    []
  end
default_child =
  if
    default_file != "" and file.exists(default_file)
  then
    [
      single(id="default_loop", fallible=true, default_file).{
        track_sensitive = {false},
        on_select = child("default")
      }
    ]
  else
    []
  end

programme =
  fallback(
    id="listener_programme",
    list.flatten(
      [
        [feed_child],
        files_child,
        default_child,
        [blank(id="station_silence").{track_sensitive = {false}, on_select = child("silence")}]
      ]
    )
  )
programme = source.drop.metadata_track_marks(programme)

# Constant bitrate: Opus's default (unconstrained VBR) codes digital silence in a few bytes a
# frame, which a player that buffers by bytes waits on for minutes (the staging run,
# 2026-10-25). 96 kbit/s is what the audience sizing budgets (0005).
output.icecast(
  id="to-listeners",
  %ogg(%opus(bitrate = 96, channels = 2, vbr = "none")),
  host=data.listeners.host,
  port=data.listeners.port,
  password=password,
  mount=data.listeners.mount,
  # Whether the stream asks internet radio directories to list it (ADR-0129). Off unless the
  # station's data says `"directory": true`; the engine's own default would ask to be listed.
  public=(data.listeners.directory ?? false),
  programme
)

# `fallback.status`: what is on air in this process, as JSON. `default` is true while the
# shipped default loop plays, so the studio app can warn a station that has not added its own
# fallback (ADR-0130).
def fallback_status(_) =
  json.stringify(compact=true, {on_air=playing(), default=(playing() == "default")})
end
server.register(namespace="fallback", usage="status", description="What is on air here, as JSON.", "status", fallback_status)

# ---- The compatibility stream (ADR-0002) ----
# One MP3 stream for directories and smart speakers that do not take Opus: off unless the
# station's data turns it on. LAME encodes it, constant bitrate, at 44.1 kHz; the metadata and
# track marks are already dropped. It shares the listener port (the mount is its own), so it
# needs no port of its own (ADR-0075); the reverse proxy must publish its mount to reach
# anyone. Directory listing (ADR-0129) applies to it as to the Opus mount.
compat = data.listeners.compat ?? {enabled = false, mount = null, bitrate = null}
if
  compat.enabled
then
  output.icecast(
    id="to-compat",
    %mp3(bitrate = compat.bitrate ?? 128, samplerate = 44100, stereo = true),
    host=data.listeners.host,
    port=data.listeners.port,
    password=password,
    mount=compat.mount ?? "programme.mp3",
    public=(data.listeners.directory ?? false),
    programme
  )
end

# ---- The legal logger (ADR-0044) ----
# What listeners heard, fallback included, in hourly Ogg Opus files named by the hour they
# start (UTC: the container's time), e.g. 2026-10-26/14-00-00.opus. Off until the control plane
# turns it on: how long recordings are kept is the station's retention setting, and the default
# (ADR-0113, No retention) keeps none. The control plane deletes files past their period.
logger_dir = null.case(data.logger, fun () -> "", fun (l) -> l.dir)
logger_bitrate = null.case(data.logger, fun () -> 64, fun (l) -> l.bitrate ?? 64)
logger_on = ref(false)

def logger_command(o) =
  def on(_) =
    if
      logger_dir == ""
    then
      "error no logger directory in the data"
    else
      if not logger_on() then o.start() end
      logger_on := true
      "ok on"
    end
  end
  def off(_) =
    if logger_on() then o.stop() end
    logger_on := false
    "ok off"
  end
  def status(_) =
    json.stringify(compact=true, {on=logger_on(), dir=logger_dir, bitrate=logger_bitrate})
  end
  server.register(namespace="logger", usage="on", description="Record what listeners hear.", "on", on)
  server.register(namespace="logger", usage="off", description="Stop recording.", "off", off)
  server.register(namespace="logger", usage="status", description="Whether the logger records, as JSON.", "status", status)
end

# A new file as each hour turns: true on the first frame of the hour, not for its whole first
# second (which would reopen, and truncate, the same file several times).
logger_hour = ref(int_of_float(time() / 3600.))
def hour_turned() =
  h = int_of_float(time() / 3600.)
  if
    h != logger_hour()
  then
    logger_hour := h
    true
  else
    false
  end
end

if
  logger_dir != ""
then
  logger =
    output.file(
      id="legal-logger",
      start=false,
      fallible=true,
      reopen_when=hour_turned,
      %ogg(%opus(bitrate = logger_bitrate, channels = 2, vbr = "constrained")),
      {time.string("#{logger_dir}/%Y-%m-%d/%H-%M-%S.opus")},
      programme
    )
  logger_command(logger)
end
