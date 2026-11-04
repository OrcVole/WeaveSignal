#!/usr/bin/env python3
# Lay the synthesised cues and the music on one timeline (Python by exception, like the spike harness: a test
# fixture builder, not product code). Needs out/cues/ from synth.sh and music/ from the extraction step.
# Writes: out/timeline.json (every cue with its start, end and participant), out/stems/<participant>.wav
# (one track per room participant, silence where they do not speak), out/room.wav (what the room hears)
# and out/listeners.wav (what listeners should hear: caller-only lines left out, the dump cut, the fallback
# playing while playout is down). Each is 48 kHz stereo.
import json, os, subprocess, sys

os.chdir(os.path.dirname(os.path.abspath(__file__)))
show = json.load(open("show.json"))
DELAY = 8.0  # the profanity delay, ADR and desk design


def dur(path):
    return float(subprocess.check_output(["ffprobe", "-v", "error", "-show_entries", "format=duration",
                                          "-of", "csv=p=0", path]).decode())


t, cues = 0.0, []
for i, c in enumerate(show["cues"]):
    # "gap" counts from the previous cue's end; "after": <index> counts from that cue's end instead.
    start = max(0.0, (cues[c["after"]]["end"] if "after" in c else t) + c["gap"])
    if c["kind"] == "speech":
        f = f"out/cues/{i:03d}-{c['who']}.wav"
        if not os.path.exists(f):
            sys.exit(f"missing {f}: run synth.sh")
        d, part = dur(f), show["cast"][c["who"]]["participant"]
    elif c["kind"] == "music":
        f, d, part = f"music/{c['music']}.wav", None, "playout"
        d = dur(f)
    else:
        f, d, part = None, 0.0, None
    cues.append({**c, "i": i, "file": f, "start": round(start, 3), "end": round(start + d, 3), "participant": part})
    t = start + d
total = max(c["end"] for c in cues) + 1.0
for c in cues:  # on-air speech never starts inside a record (beds excepted)
    for m in cues:
        if c["kind"] == "speech" and c["route"] == "air" and m["kind"] == "music" and not m.get("bed") \
                and m["start"] < c["start"] < m["end"]:
            sys.exit(f"cue {c['i']} ({c['who']}) starts at {c['start']} inside {m['music']} ({m['start']}-{m['end']})")
ev = {c["what"]: c["start"] for c in cues if c["kind"] == "event"}
down, up, dump = ev.get("playout down"), ev.get("playout restored"), ev.get("dump")
os.makedirs("out/stems", exist_ok=True)
json.dump({"total": round(total, 3), "delay": DELAY, "events": ev, "cues": cues}, open("out/timeline.json", "w"), indent=1)


def mix(items, out, extra=""):
    """items: (file, start, gain). Mixes them on a silent bed of the show's length."""
    args, chains = ["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", f"anullsrc=r=48000:cl=stereo:d={total}"], []
    for n, (f, s, g) in enumerate(items, 1):
        args += ["-i", f]
        ms = int(s * 1000)
        chains.append(f"[{n}:a]aresample=48000,volume={g},adelay={ms}|{ms}[c{n}]")
    ins = "[0:a]" + "".join(f"[c{n}]" for n in range(1, len(items) + 1))
    graph = ";".join(chains) + f";{ins}amix=inputs={len(items) + 1}:normalize=0:duration=first{extra}[o]"
    subprocess.run(args + ["-filter_complex", graph, "-map", "[o]", out], check=True)


speech = [c for c in cues if c["kind"] == "speech"]
music = [c for c in cues if c["kind"] == "music"]
for p in sorted({c["participant"] for c in speech + music}):
    mix([(c["file"], c["start"], 1.0) for c in speech + music if c["participant"] == p], f"out/stems/{p}.wav")
    print("stem", p)

gain = lambda c: 0.35 if c.get("bed") else 0.8
mix([(c["file"], c["start"], 1.0) for c in speech if c["route"] == "air"] +
    [(c["file"], c["start"], gain(c)) for c in music], "out/room.wav")
print("room.wav")

# Listeners: on-air speech and playout, except that room speech is replaced by the fallback while playout is
# down, then the dump removes the DELAY seconds before the press (listeners jump forward).
air = [c for c in speech if c["route"] == "air" and not (down and up and down <= c["start"] < up)]
items = [(c["file"], c["start"], 1.0) for c in air] + [(c["file"], c["start"], gain(c)) for c in music]
if down and up:
    items.append(("music/ident.wav", down, 0.6))  # the fallback mount's loop, as heard
mix(items, "out/listeners-undumped.wav")
if dump:
    a, b = max(0.0, dump - DELAY), dump
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", "out/listeners-undumped.wav", "-filter_complex",
                    f"[0:a]atrim=0:{a},asetpts=N/SR/TB[x];[0:a]atrim={b},asetpts=N/SR/TB[y];[x][y]concat=n=2:v=0:a=1[o]",
                    "-map", "[o]", "out/listeners.wav"], check=True)
    os.remove("out/listeners-undumped.wav")
# Dead air: any silence over 3 s in what listeners hear fails the build (the dead-air rule, ADR-0044).
det = subprocess.run(["ffmpeg", "-i", "out/listeners.wav", "-af", "silencedetect=n=-45dB:d=3", "-f", "null", "-"],
                     capture_output=True, text=True).stderr
gaps = [l.split("silence_start: ")[1] for l in det.splitlines() if "silence_start" in l]
if gaps:
    sys.exit(f"dead air in listeners.wav starting at {', '.join(gaps)} s")
print("listeners.wav", f"show {total:.1f} s; dump at {dump}; playout down {down}-{up}")
