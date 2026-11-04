# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The test programme for `just test-edges`: a 440 Hz tone, sent either as playout sends it
# (PCM in WAV to the listener process's `feed` harbor, EDGES_FORMAT=wav) or as Ogg Opus at
# 96 kbit/s constant to a plain Icecast (EDGES_FORMAT=ogg). With EDGES_TITLE_EVERY > 0 a new
# title is inserted that often, which Ogg carries in-band as a new chained stream.
#
#   EDGES_FORMAT, EDGES_HOST, EDGES_PORT, EDGES_PASSWORD, EDGES_MOUNT,
#   EDGES_TITLE_EVERY (seconds; 0 = never)

settings.log.level := 3
settings.log.stdout := true
settings.log.file := false

every = float_of_string(default=0., environment.get("EDGES_TITLE_EVERY"))
s = sine(440.)
s = insert_metadata(s)
n = ref(0)
if every > 0. then
  thread.run(every=every, fun () -> begin
    n := n() + 1
    s.insert_metadata([("title", "Edge test #{n()}"), ("artist", "WeaveSignal")])
  end)
end
to_host = environment.get("EDGES_HOST")
port = int_of_string(environment.get("EDGES_PORT"))
password = environment.get("EDGES_PASSWORD")
mount = environment.get("EDGES_MOUNT")
if environment.get("EDGES_FORMAT") == "wav" then
  output.icecast(
    %wav(stereo = true, samplerate = 48000, header = true),
    format="audio/wav", host=to_host, port=port, password=password, mount=mount, s
  )
else
  output.icecast(
    %ogg(%opus(bitrate = 96, channels = 2, vbr = "none")),
    host=to_host, port=port, password=password, mount=mount, s
  )
end
