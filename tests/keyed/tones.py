# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Which test tone sounds when, in a decoded stream (mono s16le), for the keyed-show tests.
# Hann-windowed Goertzel over 10 ms windows every 5 ms. A window "has" a tone when its
# amplitude is over 0.005 and three times the amplitude 300 Hz above it, outside the window's
# main lobe (a click from a hard cut is broadband; a tone is not). Prints JSON:
#   seconds        decoded length
#   tone_s         {freq: seconds in which that tone leads}
#   hits           {freq: number of windows in which that tone is present at all}
#   first          {freq: first time it is present, or null}
#   longest_gap_s  the longest run of windows with no tone at all, between the first tone and
#                  the last (a stream starts and ends on whatever its buffers held)
#   runs           the timeline (start, end, leading tone or "-"), first 80
#   python3 -I tones.py FILE.s16le RATE 440,1000,2500
import json, math, struct, sys

path, rate = sys.argv[1], int(sys.argv[2])
freqs = [float(f) for f in sys.argv[3].split(",")]
raw = open(path, "rb").read()
n = len(raw) // 2
x = struct.unpack("<%dh" % n, raw[: 2 * n])
N, H = int(rate * 0.010), int(rate * 0.005)
win = [0.5 - 0.5 * math.cos(2 * math.pi * i / (N - 1)) for i in range(N)]
wsum = sum(win)


def amp(seg, f):
    k = 2 * math.cos(2 * math.pi * f / rate)
    s1 = s2 = 0.0
    for v, w in zip(seg, win):
        s0 = v * w + k * s1 - s2
        s2, s1 = s1, s0
    return 2 * math.sqrt(max(s1 * s1 + s2 * s2 - k * s1 * s2, 0.0)) / wsum / 32768


labels, hits, first = [], {f: 0 for f in freqs}, {f: None for f in freqs}
for st in range(0, n - N, H):
    seg = x[st : st + N]
    t = st / rate
    present = []
    for f in freqs:
        a = amp(seg, f)
        if a > 0.005 and a > 3 * amp(seg, f + 300):
            present.append((a, f))
            hits[f] += 1
            if first[f] is None:
                first[f] = round(t, 3)
    labels.append(str(int(max(present)[1])) if present else "-")

runs, prev, start = [], None, 0
for i, s in enumerate(labels + ["END"]):
    if s != prev:
        if prev is not None:
            runs.append((round(start * H / rate, 3), round(i * H / rate, 3), prev))
        prev, start = s, i
tone_s = {str(int(f)): round(sum(e - b for b, e, l in runs if l == str(int(f))), 3) for f in freqs}
inner = runs[1:-1] if runs and runs[0][2] == "-" else runs
inner = inner[:-1] if inner and inner[-1][2] == "-" else inner
gaps = [e - b for b, e, l in inner if l == "-"]
print(
    json.dumps(
        {
            "seconds": round(n / rate, 3),
            "tone_s": tone_s,
            "hits": {str(int(f)): hits[f] for f in freqs},
            "first": {str(int(f)): first[f] for f in freqs},
            "longest_gap_s": round(max(gaps), 3) if gaps else 0.0,
            "runs": runs[:80],
        }
    )
)
