# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# What the public heard around a keyed show's window, for tests/keyed/run-slate.sh: a decoded
# capture (mono s16le) in which the desk's 1 kHz tone is the programme before and after the window
# and the slate is whatever the station has (a file, its fallback files, the default loop, or
# silence when the slate is broken). 20 ms windows. Prints JSON:
#   seconds         decoded length
#   desk_s          seconds in which 1 kHz leads (the programme)
#   shut            [start, end] of the stretch between the first and the last run of 1 kHz
#                   (each at least 2 s long): what the public heard while the gate was shut
#   shut_s          its length
#   shut_db         its level: the median of its windows' RMS, dBFS (60 ms in from each end,
#                   as are the next two)
#   shut_quiet_s    seconds of it below -60 dBFS
#   shut_1khz       windows inside it in which 1 kHz holds over 0.3 of the energy: the desk's
#                   tone in the slate (the default loop's music reaches 0.12 by itself; a leak of a
#                   few milliseconds is tests/keyed/run-switch.sh's to find, with tones only)
#   longest_quiet_s the longest run below -60 dBFS between the first and the last sound
#   edges           for each change (desk to slate, slate to desk): the time, the longest quiet
#                   run within 1 s of it, and the click figure: the largest ratio, in a 5 ms
#                   window within 50 ms of the change, of the first difference's RMS to the
#                   signal's RMS, against the same figure's median over the capture (a hard cut
#                   between two sounds is broadband and raises it; a tone or music does not)
#   python3 -I level.py FILE.s16le RATE
import json, math, struct, sys

path, rate = sys.argv[1], int(sys.argv[2])
raw = open(path, "rb").read()
n = len(raw) // 2
x = [v / 32768 for v in struct.unpack("<%dh" % n, raw[: 2 * n])]
W = int(rate * 0.020)


def goertzel_share(seg, f):
    k = 2 * math.cos(2 * math.pi * f / rate)
    s1 = s2 = 0.0
    e = 0.0
    for v in seg:
        e += v * v
        s0 = v + k * s1 - s2
        s2, s1 = s1, s0
    p = s1 * s1 + s2 * s2 - k * s1 * s2
    return min(1.0, 2 * p / (len(seg) * e)) if e > 0 else 0.0


def db(e, count):
    return 10 * math.log10(e / count) if e > 0 and count else -120.0


wins = []
for st in range(0, n - W, W):
    seg = x[st : st + W]
    e = sum(v * v for v in seg)
    wins.append({"t": st / rate, "e": e, "db": db(e, W), "k": goertzel_share(seg, 1000.0)})

desk = [w["db"] > -50 and w["k"] > 0.5 for w in wins]
runs, i = [], 0
while i < len(desk):
    if desk[i]:
        j = i
        while j < len(desk) and desk[j]:
            j += 1
        if (j - i) * 0.02 >= 2.0:
            runs.append((i, j))
        i = j
    else:
        i += 1
out = {"seconds": round(n / rate, 3), "desk_s": round(sum(desk) * 0.02, 2)}

sounding = [i for i, w in enumerate(wins) if w["db"] > -60]


def longest_quiet(a, b):
    best = cur = 0
    for w in wins[a:b]:
        cur = cur + 1 if w["db"] <= -60 else 0
        best = max(best, cur)
    return round(best * 0.02, 3)


out["longest_quiet_s"] = longest_quiet(sounding[0], sounding[-1] + 1) if sounding else None

# The click figure, in 5 ms windows.
C = int(rate * 0.005)
ratios = []
for st in range(0, n - C, C):
    seg = x[st : st + C]
    e = sum(v * v for v in seg)
    d = sum((seg[i] - seg[i - 1]) ** 2 for i in range(1, len(seg)))
    ratios.append(math.sqrt(d / e) if e > 1e-9 else 0.0)
med = sorted(r for r in ratios if r > 0)[len([r for r in ratios if r > 0]) // 2] if any(ratios) else 0.0


def click_at(t):
    a, b = max(0, int((t - 0.05) / 0.005)), int((t + 0.05) / 0.005) + 1
    return round(max(ratios[a:b] or [0.0]) / med, 2) if med else None


if len(runs) >= 2:
    a, b = runs[0][1], runs[-1][0]
    # The level, the quiet and the 1 kHz are judged inside the stretch, 60 ms from each end: a
    # window at the cut holds some of the programme before it (the encoder spreads it).
    inner = wins[a + 3 : b - 3]
    levels = sorted(w["db"] for w in inner)
    out["shut"] = [round(wins[a]["t"], 3), round(wins[b]["t"], 3)]
    out["shut_s"] = round((b - a) * 0.02, 2)
    out["shut_db"] = round(levels[len(levels) // 2], 1) if levels else -120.0
    out["shut_quiet_s"] = round(sum(1 for w in inner if w["db"] <= -60) * 0.02, 2)
    out["shut_1khz"] = sum(1 for w in inner if w["db"] > -60 and w["k"] > 0.3)
    out["edges"] = [
        {
            "t": round(wins[i]["t"], 3),
            "quiet_s": longest_quiet(max(0, i - 50), min(len(wins), i + 50)),
            "click": click_at(wins[i]["t"]),
        }
        for i in (a, b)
    ]
    out["click_median"] = round(med, 4)
else:
    out["shut"] = None
print(json.dumps(out))
