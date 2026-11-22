# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Did the caller hear the consent words? (1.2.1.) The unattended run's old check took any sound
# before the key as the words, and baresip's own ringback after 180 Ringing passed it on a call
# the station never answered (the real-kit tries, 2026-11-21). This compares what the caller
# heard with the words' recordings instead: each is cut into 20 ms frames, each frame's level in
# dB (ffmpeg's astats), and the recording's level contour is slid along what was heard; the best
# normalised correlation over at least --overlap seconds (3) is the match, as the caller may miss
# the words' first second while the call's sound starts. On the real-kit run that passed, Alba's
# words matched 0.92 and Edinburgh's (not played) 0.46; a ringback, silence or another sound
# does not reach 0.7.
#
#   mojo run tests/phone/acceptance/words_match.mojo HEARD.wav [--within SECONDS] [--min R]
#       [--overlap SECONDS] REF.opus [REF...]
#
# Prints `match <best> <reference>` and exits 0 when the best is at least --min (0.7), else 1.
# Only the first --within seconds of what was heard are searched (the lead-in, before the key).
from std.python import Python, PythonObject
from std.sys import argv, exit


def contour(path: String, within: Float64) raises -> List[Float64]:
    """The level of each 20 ms frame of `path` (8 kHz mono), in dB, clamped at -60."""
    var subprocess = Python.import_module("subprocess")
    var cmd = Python.list()
    for a in [
        "ffmpeg", "-hide_banner", "-nostats", "-loglevel", "error", "-i", path,
    ]:
        cmd.append(a)
    if within > 0.0:
        cmd.append("-t")
        cmd.append(String(within))
    for a in [
        "-af",
        "aresample=8000,pan=mono|c0=c0,asetnsamples=n=160:p=0,astats=metadata=1:reset=1,"
        + "ametadata=print:key=lavfi.astats.Overall.RMS_level:file=-",
        "-f", "null", "-",
    ]:
        cmd.append(a)
    var r = subprocess.run(cmd, capture_output=True, text=True, check=True)
    var out = List[Float64]()
    for line in String(r.stdout).splitlines():
        var l = String(line)
        if not l.startswith("lavfi.astats.Overall.RMS_level="):
            continue
        var v = String(l[byte=31:])  # past `lavfi.astats.Overall.RMS_level=`
        var db: Float64 = -60.0
        if v != "-inf" and v != "inf" and v != "nan":
            db = Float64(v)
        if db < -60.0:
            db = -60.0
        out.append(db)
    return out^


def best_match(heard: List[Float64], words: List[Float64], least_frames: Int) -> Float64:
    """The best Pearson correlation of `words` against what was heard, at any offset where at
    least `least_frames` frames overlap: the caller may hear the words with their start cut (the
    station plays them as the call is answered, before the caller's side has its sound), or the
    key may cut their end."""
    var best: Float64 = -1.0
    var n = len(words)
    for lag in range(-(n - least_frames), len(heard) - least_frames + 1):
        var first = 0 if lag >= 0 else -lag
        var last = n if lag + n <= len(heard) else len(heard) - lag
        var count = last - first
        if count < least_frames:
            continue
        var hm: Float64 = 0.0
        var wm: Float64 = 0.0
        for i in range(first, last):
            hm += heard[lag + i]
            wm += words[i]
        hm /= Float64(count)
        wm /= Float64(count)
        var cov: Float64 = 0.0
        var hv: Float64 = 0.0
        var wv: Float64 = 0.0
        for i in range(first, last):
            var dh = heard[lag + i] - hm
            var dw = words[i] - wm
            cov += dh * dw
            hv += dh * dh
            wv += dw * dw
        if hv <= 0.0 or wv <= 0.0:
            continue
        # sqrt(hv * wv) by Newton's method: no import for one square root.
        var x = hv * wv
        var root = x
        for _ in range(60):
            root = 0.5 * (root + x / root)
        var r = cov / root
        if r > best:
            best = r
    return best


def main() raises:
    var args = argv()
    if len(args) < 3:
        raise Error(
            "usage: words_match.mojo HEARD.wav [--within SECONDS] [--min R] [--overlap SECONDS]"
            " REF [REF...]"
        )
    var heard_path = String(args[1])
    var within: Float64 = 0.0
    var least: Float64 = 0.7
    var overlap: Float64 = 3.0
    var refs = List[String]()
    var i = 2
    while i < len(args):
        var a = String(args[i])
        if a == "--within":
            within = Float64(String(args[i + 1]))
            i += 1
        elif a == "--min":
            least = Float64(String(args[i + 1]))
            i += 1
        elif a == "--overlap":
            overlap = Float64(String(args[i + 1]))
            i += 1
        else:
            refs.append(a)
        i += 1
    if len(refs) == 0:
        raise Error("words_match.mojo: no recording of the words to compare with")
    var heard = contour(heard_path, within)
    var best: Float64 = -1.0
    var which = String("")
    for r in refs:
        # 20 ms frames: --overlap seconds of them must line up.
        var m = best_match(heard, contour(r, 0.0), Int(overlap * 50.0))
        if m > best:
            best = m
            which = r
    print("match", String(best), which)
    if best < least:
        exit(1)
