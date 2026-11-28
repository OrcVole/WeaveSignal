# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The station's fallback folder as a playlist (1.3.3), in one place: weavesignal.liq includes it
# for the fallback files and the keyed slate, listeners.liq for its own fallback, and
# tests/playout/station-files.liq to test it.
#
# Liquidsoap's own directory playlist offers every file in the folder to the decoder. One that is
# not a sound (a README, a cover picture, a playlist) fails on every round, and when it is all the
# folder holds, about once a second for each playlist, three decoder lines each time
# ("Available decoders cannot decode", "Error while checking file's content", "Error while
# decoding file tags"): noise in playout's log that hid real faults (the 1.3.2 keyed slate tests).
# This playlist leaves out what cannot be a sound by its name: folders, hidden files (a leading
# dot) and the extensions below. Everything else is offered as before, so a file with a sound's
# name that does not decode is still tried and still logged: that is a fault for the station to
# see. What plays does not change: the same files, shuffled each round (Liquidsoap's
# `playlist.list` in `randomize` mode, which the directory playlist uses itself), and the folder
# read again half a second after a change, as its `reload_mode="watch"` does. A folder with
# nothing to play leaves the playlist not ready, so the next child of the fallback plays, and it
# does not retry.

# Extensions (lower case) of files that are never sounds.
station_not_sounds =
  [
    "txt", "md", "nfo", "pdf", "doc", "docx", "odt", "rtf", "html", "htm", "json", "xml", "jpg",
    "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "svg", "ico", "heic", "m3u", "m3u8",
    "pls", "xspf", "cue", "lrc", "log", "ini", "db", "url", "lnk", "sfv", "md5", "sha1",
    "sha256", "zip", "rar", "7z", "tar", "gz", "part", "tmp"
  ]

# The files under `dir` that may be sounds, sorted, with their full paths.
def station_sound_files(dir) =
  def maybe_sound(f) =
    ext = string.case(lower=true, file.extension(leading_dot=false, f))
    not file.is_directory(f)
    and
      string.sub(path.basename(f), start=0, length=1) != "."
    and
      not list.mem(ext, station_not_sounds)
  end
  # A folder that cannot be read (taken away while playing) has nothing to offer, as Liquidsoap's
  # own playlist then has.
  files =
    try
      file.ls(absolute=true, recursive=true, sorted=true, dir)
    catch err do
      log.important(label="station_files", "Cannot read #{dir}: #{err}")
      []
    end
  list.filter(maybe_sound, files)
end

# A shuffled playlist of the sounds in `dir`, named `name` (its source id), read again when the
# folder changes.
def station_playlist(name, dir) =
  s = playlist.list(id=name, mode="randomize", station_sound_files(dir))
  pending = ref(false)
  def reread() =
    if
      not pending()
    then
      pending := true
      thread.run(
        delay=0.5,
        fun () ->
          begin
            pending := false
            s.reload(station_sound_files(dir))
          end
      )
    end
  end
  watch = file.watch(dir, reread)
  s.on_shutdown(synchronous=true, fun () -> watch.unwatch())
  s
end
