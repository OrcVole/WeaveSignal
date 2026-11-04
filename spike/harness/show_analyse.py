# Analyse a show_room.py run over the whole show (runs in the desk image for numpy).
# usage: show_analyse.py <talkshow dir> <run dir>
# For each voice: its delay to the desk (envelope cross-correlation over the whole show, by FFT), and own-voice
# leakage in its ear: the ear's level while only that voice speaks, against the ear's level while others speak.
# For the listener recording(s): the delay behind the programme, found by searching the programme for each.
import json, subprocess, sys, wave
import numpy as np

show_dir, run = sys.argv[1], sys.argv[2]
RATE = 48000
tl = json.load(open(f"{show_dir}/out/timeline.json"))
rep = json.load(open(f"{run}/report.json"))


def read(path):
    with wave.open(path) as w:
        return np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).reshape(-1, 2).mean(axis=1)


def env(a, hop=480):
    n = len(a) // hop
    return np.sqrt((a[:n * hop].astype(np.float64).reshape(n, hop) ** 2).mean(axis=1))


def xlag(ref, sig, max_lag):
    """Lag (in envelope frames) of sig behind ref, 0..max_lag, by FFT cross-correlation."""
    r, s = ref - ref.mean(), sig - sig.mean()
    n = 1 << int(np.ceil(np.log2(len(r) + len(s))))
    c = np.fft.irfft(np.fft.rfft(s, n) * np.conj(np.fft.rfft(r, n)), n)[:max_lag + 1]
    k = int(np.argmax(c))
    return k, float(c[k] / (np.linalg.norm(r) * np.linalg.norm(s) + 1e-9))


def aligned(tap):  # the tap from show time 0, by its first-buffer time
    a = read(f"{run}/{tap}.wav"); pad = int(rep["taps"][tap]["first_s"] * RATE)
    return a[-pad:] if pad < 0 else np.concatenate([np.zeros(pad), a])


db = lambda x: round(20 * np.log10(max(x, 1e-9) / 32768), 1)
stems = {}
for p in ("host", "guest-gregor", "caller-dougal", "caller-seamus", "dj-finlay", "screener-moira", "playout"):
    stems[p] = env(read(f"{show_dir}/out/stems/{p}.wav"))
out = {"voice_to_desk_ms": {}, "own_voice_leak_db": {}, "listener": {}}
for p in stems:
    if p == "playout": continue
    e = env(aligned(f"recv_{p}")); m = min(len(e), len(stems[p]))
    k, c = xlag(stems[p][:m], e[:m], 100)
    out["voice_to_desk_ms"][p] = {"ms": k * 10, "corr": round(c, 2)}
for p in stems:
    if p in ("playout", "screener-moira"): continue
    e = env(aligned(f"ear_{p}")); m = min(len(e), len(stems[p])); e = e[:m]
    own = stems[p][:m] > stems[p].max() * 0.05
    others = np.zeros(m, bool)
    for q, s in stems.items():
        if q != p: others |= s[:m] > s.max() * (0.02 if q == "playout" else 0.05)
    # Shift masks by the round trip (about 0.2 s) so the ear's lag does not count others' tails as leakage.
    sh = 25
    solo = np.roll(own & ~others, sh); busy = np.roll(others, sh)
    solo &= ~np.roll(others, sh + 30)  # and keep clear of anyone who spoke in the 0.3 s before
    if solo.sum() > 50 and busy.sum() > 50:
        a, b = e[solo].mean(), e[busy].mean()
        out["own_voice_leak_db"][p] = {"ear_dbfs_while_alone": db(a), "ear_dbfs_while_others_talk": db(b),
                                       "leak_db": round(20 * np.log10(max(a, 1e-9) / b), 1), "solo_s": round(solo.sum() / 100, 1)}
prog = env(aligned("programme"))
for name in ("listener-late-1", "listener-late"):
    path = f"{run}/{name}.opus"
    try:
        raw = subprocess.run(["gst-launch-1.0", "-q", "filesrc", f"location={path}", "!", "oggdemux", "!", "opusdec", "!",
                              "audioconvert", "!", "audio/x-raw,format=S16LE,rate=48000,channels=2", "!", "filesink",
                              f"location=/tmp/{name}.raw"], check=True)
        a = np.fromfile(f"/tmp/{name}.raw", dtype=np.int16).reshape(-1, 2).mean(axis=1)
    except Exception as ex:
        out["listener"][name] = str(ex); continue
    le = env(a)
    seg = le[1000:4000] if len(le) > 4000 else le  # 30 s of it, searched for in the whole programme
    k, c = xlag(seg, prog, len(prog) - 1)
    out["listener"][name] = {"seconds": round(len(a) / RATE, 1), "starts_at_show_s": round((k - 1000) / 100, 2), "corr": round(c, 2)}
json.dump(out, open(f"{run}/analysis.json", "w"), indent=1)
print(json.dumps(out, indent=1))
