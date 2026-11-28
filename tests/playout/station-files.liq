# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The station's fallback folder as playout and the listener process read it
# (playout/station_files.liq, 1.3.3): files that cannot be sounds by their name (a README, a
# cover picture, a hidden file, a folder) are left out, so they never reach the decoder and its
# three error lines a try; every other file is kept, upper-case extensions and files in a
# sub-folder included. Then, faster than real time, a folder with the default loop (assets/
# default-loop, at /a) among junk plays the loop lap after lap, and a folder of junk alone is never
# ready (its fallback's next child plays) and never offers a file. Exits 0 when all of it holds,
# 1 when not. `just liq-check` runs it.
%include "../../playout/station_files.liq"
settings.frame.audio.samplerate := 48000
settings.log.stdout := true
settings.log.level := 2

failed = ref(false)
def check(name, ok) =
  word = if ok then "ok  " else "FAIL" end
  print("#{word}  #{name}")
  if not ok then failed := true end
end

base = file.temp_dir("station-files")
def put(dir, name, data) =
  file.mkdir(parents=true, dir)
  file.write(data=data, "#{dir}/#{name}")
end
loop = file.contents("/a/default-loop.flac")

# What the folder offers.
names = "#{base}/names"
list.iter(
  fun (n) -> put(names, n, "x"),
  [
    "a.flac", "B.MP3", "c.Opus", "d", "notes.txt", "README.md", "cover.JPG", "folder.png",
    "list.m3u", ".hidden.mp3", ".DS_Store", "Thumbs.db"
  ]
)
put("#{names}/more", "e.ogg", "x")
put("#{names}/more", "back.jpeg", "x")
file.mkdir(parents=true, "#{names}/empty")
got = list.map(fun (f) -> string.sub(f, start=string.length(names) + 1, length=string.length(f) - string.length(names) - 1), station_sound_files(names))
check(
  "only what may be a sound is offered: #{got}",
  got == ["B.MP3", "a.flac", "c.Opus", "d", "more/e.ogg"]
)

# Playing: the loop among junk, and junk alone.
mixed = "#{base}/mixed"
put(mixed, "loop.flac", loop)
put(mixed, "notes.txt", "not a sound")
put(mixed, "cover.jpg", "not a picture either")
junk = "#{base}/junk"
put(junk, "notes.txt", "not a sound")
put(junk, ".keep", "")

played = ref([])
junk_ready = ref(false)
m = station_playlist("mixed_files", mixed)
m.on_track(synchronous=true, fun (meta) -> played := [...played(), path.basename(meta["filename"])])
j = station_playlist("junk_files", junk)
p =
  fallback(
    [
      j.{track_sensitive = {false}},
      m.{track_sensitive = {false}},
      blank().{track_sensitive = {false}}
    ]
  )
elapsed = ref(0.)
p.on_frame(
  synchronous=true,
  fun () ->
    begin
      if j.is_ready() then junk_ready := true end
      elapsed := elapsed() + settings.frame.duration()
      if
        elapsed() >= 70.
      then
        check(
          "the loop among junk plays lap after lap: #{played()}",
          list.length(played()) >= 2
          and
            list.for_all(fun (f) -> f == "loop.flac", played())
        )
        check("a folder of junk alone is never ready", not junk_ready())
        check("and offers nothing", station_sound_files(junk) == [])
        exit(if failed() then 1 else 0 end)
      end
    end
)
o = output.dummy(fallible=true, p)
clock.assign_new(sync="none", [o])
