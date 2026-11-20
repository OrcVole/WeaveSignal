# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The phone line's acceptance run (17f), timed as the operator asked (2026-11-20): each question
# appears as text, with its time, at the second it is asked; the caller talks for 10 s, pauses
# for 5 s, and the next question comes (timed.json sets both). Nobody needs to hear the phone.
#
#   mojo run tests/phone/acceptance/timed.mojo tests/phone/acceptance/timed.json [options]
#
#   --route trunk|browser|sip  which lead-in cues to show (default trunk; sip is the unattended
#                              run's, which presses the key itself)
#   --t0 EPOCH                 the shared start (default: 10 s from now, printed)
#   --speed N                  run the clock N times faster: a dry run of the schedule only
#   --speak                    also speak each question with a TTS voice, if one is found:
#                              WEAVESIGNAL_TTS_CMD (a command that reads the text on stdin and
#                              writes a WAV to the path in place of {out}), else piper with
#                              PIPER_VOICE, else espeak-ng's Scottish voice, else espeak; played
#                              with pw-play, paplay or aplay. Without one it says so and goes on
#                              with text only.
#   --schedule                 print every timed event as `at<TAB>kind<TAB>text` and stop (the
#                              unattended driver reads the key's and the end's times from it)
#   --caller-wav PATH          write the unattended caller's microphone and stop: silence through
#                              the lead-in, then each round's answer for talk_s (spoken by the TTS
#                              voice where there is one, looped to fill; else a tone of its own,
#                              300 Hz + 100 Hz a round, so a check can find each round in the
#                              programme), silence for silent rounds and every pause. 8 kHz mono,
#                              16-bit, as a SIP client sends. Needs ffmpeg.
#
# tests/phone/acceptance/unattended.sh runs the same schedule against a station's SIP port with a
# local SIP client (baresip), with no person at all. script.json and cues.mojo are the AI-voice
# harness's longer script (Richard screening, then presenting), kept for that private run.
from std.python import Python, PythonObject
from std.sys import argv


def event(at: Float64, kind: String, text: String) raises -> PythonObject:
    var builtins = Python.import_module("builtins")
    var e = builtins.dict()
    e["at"] = at
    e["kind"] = kind
    e["text"] = text
    return e


def schedule(script: PythonObject, route: String) raises -> PythonObject:
    """Every event of the run, in time order: the lead-in, then each round's question, talk (or
    silence), pause, and the studio's acts."""
    var builtins = Python.import_module("builtins")
    var operator = Python.import_module("operator")
    var talk = Float64(py=script["talk_s"])
    var pause = Float64(py=script["pause_s"])
    var events = builtins.list()
    for c in script["lead_in"]:
        if c.__contains__("route") and String(c["route"]) != route:
            if not (route == "sip" and String(c["route"]) == "trunk"):
                continue
        var kind = String("act")
        if c.__contains__("do"):
            kind = String(c["do"])
        var e = event(Float64(py=c["at"]), kind, String(c["text"]))
        if c.__contains__("key"):
            e["key"] = c["key"]
        events.append(e)
    var start = Float64(py=script["start"])
    var n = 0
    for r in script["rounds"]:
        var at = start + Float64(n) * (talk + pause)
        n += 1
        var q = event(at, "ask", String(r["ask"]))
        q["round"] = n
        q["role"] = r["role"]
        events.append(q)
        var silent = r.__contains__("silent") and String(r["silent"]) == "True"
        if silent:
            events.append(event(at, "silent", "Say nothing until the pause."))
        else:
            var t = event(at, "talk", "Talk now (" + String(Int(talk)) + " s).")
            t["round"] = n
            if r.__contains__("answer"):
                t["answer"] = r["answer"]
            events.append(t)
        events.append(event(at + talk, "pause", "Pause (" + String(Int(pause)) + " s)."))
        if r.__contains__("acts"):
            for a in r["acts"]:
                events.append(
                    event(at + Float64(py=a["after"]), String(a["act"]), String(a["text"]))
                )
    var end = start + Float64(n) * (talk + pause)
    events.append(event(end, "done", "Done. Note anything that was wrong."))
    return builtins.sorted(events, key=operator.itemgetter("at"))


def clock(at: Float64) raises -> String:
    """`m:ss` from T0."""
    var s = Int(at)
    var sec = s % 60
    var pad = String("")
    if sec < 10:
        pad = "0"
    return String(s // 60) + ":" + pad + String(sec)


def find_tts() raises -> PythonObject:
    """The TTS command as an argument list with `{out}` in it, or None."""
    var os = Python.import_module("os")
    var shutil = Python.import_module("shutil")
    var shlex = Python.import_module("shlex")
    var custom = os.environ.get("WEAVESIGNAL_TTS_CMD", "")
    if String(custom) != "":
        return shlex.split(custom)
    var voice = os.environ.get("PIPER_VOICE", "")
    if String(voice) != "" and shutil.which("piper") is not None:
        return shlex.split("piper --model " + String(shlex.quote(voice)) + " --output_file {out}")
    if shutil.which("espeak-ng") is not None:
        return shlex.split("espeak-ng -v en-gb-scotland -s 150 --stdin -w {out}")
    if shutil.which("espeak") is not None:
        return shlex.split("espeak -v en-gb -s 150 --stdin -w {out}")
    return None


def synth(tts: PythonObject, text: String, dest: String) raises -> Bool:
    var subprocess = Python.import_module("subprocess")
    var builtins = Python.import_module("builtins")
    var args = builtins.list()
    for a in tts:
        args.append(String(a).replace("{out}", dest))
    var r = subprocess.run(args, input=text, text=True, capture_output=True)
    return Int(py=r.returncode) == 0


def find_player() raises -> PythonObject:
    var shutil = Python.import_module("shutil")
    for p in ["pw-play", "paplay", "aplay"]:
        if shutil.which(p) is not None:
            return PythonObject(p)
    return None


def caller_wav(script: PythonObject, events: PythonObject, path: String) raises:
    """The unattended caller's microphone, made with ffmpeg from segments in a scratch folder."""
    var subprocess = Python.import_module("subprocess")
    var tempfile = Python.import_module("tempfile")
    var os = Python.import_module("os")
    var builtins = Python.import_module("builtins")
    var shutil = Python.import_module("shutil")
    if shutil.which("ffmpeg") is None:
        raise Error("--caller-wav needs ffmpeg")
    var talk = Float64(py=script["talk_s"])
    var tts = find_tts()
    if tts is None:
        print("no TTS voice found: each round's answer is a tone of its own")
    else:
        print("answers spoken with:", String(tts[0]))
    var tmp = String(tempfile.mkdtemp(prefix="ws-phone-caller-"))
    var ff = String("ffmpeg -loglevel error -y ")
    var out_fmt = String(" -ar 8000 -ac 1 -c:a pcm_s16le ")
    var parts = builtins.list()
    var t: Float64 = 0.0
    var i = 0
    for e in events:
        var kind = String(e["kind"])
        if kind != "talk":
            continue
        var at = Float64(py=e["at"])
        if at > t:
            var gap = tmp + "/gap-" + String(i) + ".wav"
            _ = subprocess.run(
                ff + "-f lavfi -i anullsrc=r=8000:cl=mono -t " + String(at - t) + out_fmt + gap,
                shell=True,
                check=True,
            )
            parts.append(gap)
        var seg = tmp + "/round-" + String(i) + ".wav"
        var spoken = False
        if tts is not None and e.__contains__("answer"):
            var raw = tmp + "/say-" + String(i) + ".wav"
            if synth(tts, String(e["answer"]), raw):
                # The answer looped (with half a second between) to fill the talk.
                var padded = tmp + "/pad-" + String(i) + ".wav"
                _ = subprocess.run(
                    ff + "-i " + raw + " -af apad=pad_dur=0.5" + out_fmt + padded,
                    shell=True,
                    check=True,
                )
                _ = subprocess.run(
                    ff + "-stream_loop -1 -i " + padded + " -t " + String(talk) + out_fmt + seg,
                    shell=True,
                    check=True,
                )
                spoken = True
        if not spoken:
            var freq = 300 + 100 * Int(py=e["round"])
            _ = subprocess.run(
                ff + "-f lavfi -i sine=frequency=" + String(freq) + ":duration=" + String(talk)
                # ffmpeg's sine is an eighth of full scale; twice that is a quarter.
                + " -af volume=2" + out_fmt + seg,
                shell=True,
                check=True,
            )
        parts.append(seg)
        t = at + talk
        i += 1
    # Silence to the end of the run.
    var end = Float64(py=events[len(events) - 1]["at"]) + 5.0
    var tail = tmp + "/tail.wav"
    _ = subprocess.run(
        ff + "-f lavfi -i anullsrc=r=8000:cl=mono -t " + String(end - t) + out_fmt + tail,
        shell=True,
        check=True,
    )
    parts.append(tail)
    var listing = tmp + "/parts.txt"
    var f = builtins.open(listing, "w")
    for p in parts:
        _ = f.write("file '" + String(p) + "'\n")
    _ = f.close()
    _ = subprocess.run(
        ff + "-f concat -safe 0 -i " + listing + out_fmt + path, shell=True, check=True
    )
    shutil.rmtree(tmp)
    print("caller's microphone written:", path, "(" + String(Int(end)) + " s)")


def main() raises:
    var args = argv()
    if len(args) < 2:
        raise Error(
            "usage: timed.mojo <timed.json> [--route trunk|browser|sip] [--t0 EPOCH] [--speed N]"
            " [--speak] [--schedule] [--caller-wav PATH]"
        )
    var json = Python.import_module("json")
    var time = Python.import_module("time")
    var builtins = Python.import_module("builtins")
    var subprocess = Python.import_module("subprocess")
    var tempfile = Python.import_module("tempfile")
    var route = String("trunk")
    var t0: Float64 = Float64(py=time.time()) + 10.0
    var speed: Float64 = 1.0
    var speak = False
    var only_schedule = False
    var wav_path = String("")
    var i = 2
    while i < len(args):
        var a = String(args[i])
        if a == "--route":
            route = String(args[i + 1])
            i += 1
        elif a == "--t0":
            t0 = Float64(String(args[i + 1]))
            i += 1
        elif a == "--speed":
            speed = Float64(String(args[i + 1]))
            i += 1
        elif a == "--speak":
            speak = True
        elif a == "--schedule":
            only_schedule = True
        elif a == "--caller-wav":
            wav_path = String(args[i + 1])
            i += 1
        else:
            raise Error("unknown option " + a)
        i += 1
    if route != "trunk" and route != "browser" and route != "sip":
        raise Error("--route is trunk, browser or sip")
    if speed <= 0.0:
        raise Error("--speed is above 0")

    var script = json.load(builtins.open(String(args[1])))
    var events = schedule(script, route)
    if only_schedule:
        for e in events:
            var extra = String("")
            if e.__contains__("key"):
                extra = "\t" + String(e["key"])
            print(String(e["at"]) + "\t" + String(e["kind"]) + "\t" + String(e["text"]) + extra)
        return
    if wav_path != "":
        caller_wav(script, events, wav_path)
        return

    var tts = PythonObject(None)
    var player = PythonObject(None)
    var scratch = String("")
    if speak:
        tts = find_tts()
        player = find_player()
        if tts is None or player is None:
            print("--speak: no TTS voice or no audio player found; questions as text only")
            speak = False
        else:
            print("--speak: questions spoken with", String(tts[0]), "through", String(player))
            scratch = String(tempfile.mkdtemp(prefix="ws-phone-cues-"))
    print(String(script["title"]))
    print("Route:", route)
    print(
        "Start (T0):",
        String(time.strftime("%H:%M:%S", time.localtime(t0))),
        "local time; times below are minutes:seconds from T0, then the clock",
    )
    print("")
    for e in events:
        var at = Float64(py=e["at"])
        var kind = String(e["kind"])
        var due = t0 + at / speed
        var spoken = String("")
        if speak and kind == "ask":
            # Made ahead, so it starts at its second.
            spoken = scratch + "/q-" + String(e["round"]) + ".wav"
            if not synth(tts, String(e["text"]), spoken):
                spoken = ""
        var now = Float64(py=time.time())
        if due > now:
            time.sleep(due - now)
        var stamp = (
            "[" + clock(at) + " " + String(time.strftime("%H:%M:%S", time.localtime(t0 + at)))
            + "]"
        )
        if kind == "ask":
            print("")
            print(
                stamp,
                "Q" + String(e["round"]),
                "(" + String(e["role"]) + "):",
                String(e["text"]),
            )
            if spoken != "":
                _ = subprocess.Popen(
                    [player, spoken], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
                )
        elif kind == "talk":
            print(stamp, "TALK", String(e["text"]))
        elif kind == "silent":
            print(stamp, "SILENT", String(e["text"]))
        elif kind == "pause":
            print(stamp, "PAUSE", String(e["text"]))
        else:
            print(stamp, kind.upper() + ":", String(e["text"]))
    print("")
    print("End of the run.")
