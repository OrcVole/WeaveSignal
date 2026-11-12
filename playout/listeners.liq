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
#   WEAVESIGNAL_ICECAST_OUT_PASSWORD         optional: the password of the remote Icecast server
#                                          the programme can be sent to (ADR-0226); never in
#                                          the data file, never logged
#
# Two more optional streams (ADR-0002, ADR-0232): MP3 beside the Opus one, for directories,
# smart speakers and listeners who choose it, at two rates (128 and 320 kbit/s by default), each
# off unless the data says `listeners.compat.enabled` or `listeners.compat_high.enabled`. LAME
# encodes them (not fdk-aac, which Ubuntu's Liquidsoap links and which blocks the app store).
# Each takes the same programme, on the same port, under its own mount.
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
        bitrate : int?,
        compat : {enabled : bool, mount : string?, bitrate : int?}?,
        compat_high : {enabled : bool, mount : string?, bitrate : int?}?
      },
    fallback : {dir : string, default_file : string?}?,
    logger : {dir : string, bitrate : int?}?,
    icecast_out :
      {
        enabled : bool,
        host : string,
        port : int,
        mount : string,
        user : string?,
        tls : bool?,
        bitrate : int?,
        name : string?
      }?
  }
) = file.contents(data_path)

# The Opus stream's constant bit rate (ADR-0233): 96 kbit/s unless the data says otherwise,
# 64 to 256. A rate outside that stops this process here, with the setting named, rather than
# leaving the encoder to refuse it or listeners to hear a starved stream.
opus_bitrate = data.listeners.bitrate ?? 96
if
  opus_bitrate < 64 or opus_bitrate > 256
then
  print(
    "weavesignal: listeners.bitrate (OPUS_BITRATE) is #{opus_bitrate} kbit/s; the Opus stream takes 64 to 256"
  )
  shutdown(code=2)
end

# The MP3 mounts (ADR-0002, ADR-0232): each one's mount and rate, and no two streams on one mount.
compat = data.listeners.compat ?? {enabled = false, mount = null, bitrate = null}
compat_mount = compat.mount ?? "programme.mp3"
compat_high = data.listeners.compat_high ?? {enabled = false, mount = null, bitrate = null}
compat_high_mount = compat_high.mount ?? "programme-high.mp3"
if
  compat.enabled and compat_mount == data.listeners.mount
then
  print(
    "weavesignal: listeners.compat.mount (COMPAT_MOUNT) is the Opus stream's mount, #{compat_mount}"
  )
  shutdown(code=2)
end
if
  compat_high.enabled
  and
    (
      compat_high_mount == data.listeners.mount
    or
      (compat.enabled and compat_high_mount == compat_mount)
    )
then
  print(
    "weavesignal: listeners.compat_high.mount (COMPAT_HIGH_MOUNT) is another stream's mount, #{compat_high_mount}"
  )
  shutdown(code=2)
end

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
# 2026-10-25). 96 kbit/s, the default, is what the audience sizing budgets (0005); the station
# sets its own with `listeners.bitrate` (OPUS_BITRATE, ADR-0233), still constant.
output.icecast(
  id="to-listeners",
  %ogg(%opus(bitrate = opus_bitrate, channels = 2, vbr = "none")),
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

# ---- The MP3 streams (ADR-0002, ADR-0232) ----
# MP3 for directories and smart speakers that do not take Opus, and for listeners who choose
# it: at two rates, each off unless the station's data turns it on. `compat` is 1.0.0's one MP3
# mount (COMPAT_STREAM, COMPAT_MOUNT, COMPAT_BITRATE: 128 kbit/s on programme.mp3), unchanged;
# `compat_high` is the second (COMPAT_HIGH_*: 320 kbit/s on programme-high.mp3). LAME encodes
# each, constant bitrate, at 44.1 kHz; the metadata and track marks are already dropped, and MP3
# has no chained stream to start. They share the listener port (each mount is its own), so they
# need no port of their own (ADR-0075); the reverse proxy must publish each mount to reach
# anyone. Directory listing (ADR-0129) applies to them as to the Opus mount. Each has its own
# encoder (dedicated_encoder above) on this process's one clock, with the Opus one.
def mp3_mount(id, mount, bitrate) =
  ignore(
    output.icecast(
      id=id,
      %mp3(bitrate = bitrate, samplerate = 44100, stereo = true),
      host=data.listeners.host,
      port=data.listeners.port,
      password=password,
      mount=mount,
      public=(data.listeners.directory ?? false),
      programme
    )
  )
end
if compat.enabled then mp3_mount("to-compat", compat_mount, compat.bitrate ?? 128) end
if
  compat_high.enabled
then
  mp3_mount("to-compat-high", compat_high_mount, compat_high.bitrate ?? 320)
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

# ---- The outgoing Icecast source (ADR-0226; branch icecast-out) ----
# The programme sent to another Icecast server as a source, for a DJ's AzuraCast (a streamer
# account: user, password, mount) or any Icecast: its AutoDJ gives way to our programme, with the
# profanity delay and Dump already in it. Off unless the data says `icecast_out.enabled`
# and WEAVESIGNAL_ICECAST_OUT_PASSWORD is set.
#
# MP3, not Ogg: a chained Ogg stream makes Icecast-type servers drop the source (the third fault,
# above), and MP3 has no chain. LAME encodes it (ADR-0002's exception).
#
# A remote failure must never reach our own listeners or playout's clock. Three things see to it:
#   - `buffer` puts the output on a clock of its own, so a remote that stalls a write (an
#     accepted connection nobody reads, a login that never answers) blocks only that clock;
#   - the output starts and stops on demand (`icecast_out.send`, `icecast_out.stop`) and is
#     stopped again when the control plane stops asking: each `send` lasts a few seconds only,
#     so a control plane that dies mid-show does not leave the remote station held on us;
#   - a failed connection is tried again after a pause that doubles, from 5 s to 30 s (so a wrong
#     password is one refused login every 30 s, not a storm), and the pause starts again once a
#     connection holds. The last error is kept for the studio to show.
# The output reads the same `programme` as the Opus mount, after the metadata is dropped, behind
# `clip` (NaN to 0). There is no `compress` here, so no lookahead.
io = data.icecast_out ?? {
  enabled = false,
  host = "",
  port = 0,
  mount = "",
  user = null,
  tls = null,
  bitrate = null,
  name = null
}
io_password = environment.get("WEAVESIGNAL_ICECAST_OUT_PASSWORD")
io_bitrate = io.bitrate ?? 128
io_ready = io.enabled and io_password != "" and io.host != ""

io_connected = ref(false)
io_since = ref(0.)
io_audio_s = ref(0.)
io_error = ref("")
io_error_at = ref(0.)
io_failures = ref(0)
io_until = ref(0.)

if
  io.enabled and not io_ready
then
  io_error :=
    if
      io.host == ""
    then
      "the remote server's address is not set (ICECAST_OUT_HOST)"
    else
      "the remote server's password is not set (ICECAST_OUT_PASSWORD)"
    end
  io_error_at := time()
  log.important(label="icecast_out", "enabled, but #{io_error()}: nothing will be sent")
end

def icecast_out_status(_) =
  json.stringify(
    compact=true,
    {
      configured=io_ready,
      host=io.host,
      port=io.port,
      mount=io.mount,
      tls=(io.tls ?? false),
      bitrate=io_bitrate,
      sending=(io_until() > time()),
      connected=io_connected(),
      connected_since=(if io_connected() then io_since() else 0. end),
      bytes_sent=int_of_float(io_audio_s() * float_of_int(io_bitrate) * 125.),
      failures=io_failures(),
      last_error=io_error(),
      last_error_at=io_error_at()
    }
  )
end
server.register(
  namespace="icecast_out",
  usage="status",
  description="The outgoing Icecast source, as JSON.",
  "status",
  icecast_out_status
)

if
  io_ready
then
  io_source =
    buffer(id="icecast_out_buffer", buffer=1., max=8., fallible=true, clip(programme))
  io_transport = if (io.tls ?? false) then http.transport.ssl() else http.transport.unix end
  io_out =
    output.icecast(
      id="to-icecast-out",
      %mp3(bitrate = io_bitrate, samplerate = 44100, stereo = true),
      host=io.host,
      port=io.port,
      user=(io.user ?? "source"),
      password=io_password,
      mount=io.mount,
      name=io.name,
      public=false,
      transport=io_transport,
      start=false,
      fallible=true,
      register_telnet=false,
      io_source
    )

  # Audio time sent while connected, for the byte count: the stream is constant bit rate.
  ignore(
    io_out.on_frame(
      synchronous=false,
      fun () ->
        if
          io_connected()
        then
          io_audio_s := io_audio_s() + settings.frame.duration()
        end
    )
  )
  ignore(
    io_out.on_connect(
      synchronous=false,
      fun () ->
        begin
          io_connected := true
          io_since := time()
          io_failures := 0
          log.important(label="icecast_out", "connected to #{io.host}:#{io.port}")
        end
    )
  )
  ignore(
    io_out.on_disconnect(
      synchronous=false,
      fun () ->
        begin
          io_connected := false
          log.important(label="icecast_out", "disconnected from #{io.host}:#{io.port}")
        end
    )
  )
  # The server's own words, with what they mean in front for the ones people meet.
  def io_explain(m) =
    if
      string.contains(substring="401", m) or string.contains(substring="403", m)
    then
      "the server refused the user name or password (#{m})"
    elsif
      string.contains(substring="Timeout", m)
    then
      "the server did not answer in time (#{m})"
    elsif
      string.contains(substring="refused in connect", m)
    then
      "nothing is listening at that address and port (#{m})"
    elsif
      string.contains(substring="could not write", m) or string.contains(substring="could not read", m)
    then
      "the connection to the server was lost (#{m})"
    else
      m
    end
  end
  # 5, 10, 20 and then 30 s between tries.
  def io_backoff(failures) =
    if
      failures <= 1
    then
      5.
    elsif
      failures == 2
    then
      10.
    elsif
      failures == 3
    then
      20.
    else
      30.
    end
  end
  ignore(
    io_out.on_error(
      synchronous=true,
      fun (~restart_in, err) ->
        begin
          io_connected := false
          io_failures := io_failures() + 1
          io_error := io_explain(error.message(err))
          io_error_at := time()
          wait = io_backoff(io_failures())
          log.important(
            label="icecast_out",
            "#{io.host}:#{io.port}: #{io_error()}; trying again in #{wait} s"
          )
          restart_in(wait)
        end
    )
  )

  # `send` is a lease: the output runs for this many seconds, and the control plane repeats the
  # command while it wants the programme sent.
  def io_send(arg) =
    seconds = float_of_string(default=0., string.trim(arg))
    if
      seconds <= 0. or seconds > 120.
    then
      "error give a number of seconds between 1 and 120"
    else
      io_until := time() + seconds
      if not io_out.is_started() then io_out.start() end
      "ok sending for #{seconds} s"
    end
  end
  def io_stop(_) =
    io_until := 0.
    if io_out.is_started() then io_out.stop() end
    io_connected := false
    "ok stopped"
  end
  server.register(
    namespace="icecast_out",
    usage="send <seconds>",
    description="Send the programme to the remote server for this long (a lease to repeat).",
    "send",
    io_send
  )
  server.register(
    namespace="icecast_out",
    usage="stop",
    description="Stop sending to the remote server.",
    "stop",
    io_stop
  )
  thread.run(
    every=1.,
    fun () ->
      if
        io_until() <= time() and io_out.is_started()
      then
        ignore(io_stop(""))
      end
  )
end
