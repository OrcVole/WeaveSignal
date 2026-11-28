# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# One ear for the soak's keyed shows (`soak.py`): a stream, decoded by ffmpeg to 8 kHz mono, and
# a listen for the keyed shows' signatures, each a pair of tones that no musical scale holds
# (1017 and 2531 Hz for one show, 1433 and 3061 Hz for the other). In 100 ms Hann windows every
# 50 ms, a signature is present when both its tones are over 0.01 and each is four times the
# louder of the two points 60 Hz either side (outside the window's main lobe: a tone is narrow,
# music and a cut's click are not). Runs of it are written as they end. A process of its own, so
# that the soak's sampler and held listener never wait on it. Standard library only.
#
#   keyed_ear.py public URL OUT            follow a public mount until stopped (SIGTERM),
#                                          connecting again after a drop (each drop is written)
#   keyed_ear.py holder URL OUT FROM UNTIL a key holder: try from FROM until UNTIL (Unix s),
#                                          again each second while refused, follow the stream to
#                                          its end, then try once more (the answer is written)
#   keyed_ear.py file PATH                 a local file: print its runs (to check the detector)
#
# OUT is JSON lines: {"ev": "try"|"connect"|"run"|"end"|"after"|"progress", "t": Unix s, ...}.
import http.client
import json
import math
import operator
import signal
import subprocess
import sys
import threading
import time
import urllib.parse

RATE, N, HOP = 8000, 800, 400
SIGS = {"A": (1017.0, 2531.0), "B": (1433.0, 3061.0)}
FLOOR, RATIO, GUARD = 0.01, 4.0, 60.0
STOP = threading.Event()

_hann = [0.5 - 0.5 * math.cos(2 * math.pi * i / (N - 1)) for i in range(N)]
_norm = 2 / sum(_hann) / 32768


def _tables(f):
    w = 2 * math.pi * f / RATE
    return ([h * math.cos(w * i) for i, h in enumerate(_hann)], [h * math.sin(w * i) for i, h in enumerate(_hann)])


TABLES = {f: _tables(f) for pair in SIGS.values() for t in pair for f in (t, t - GUARD, t + GUARD)}


def amp(seg, f):
    c, s = TABLES[f]
    re, im = sum(map(operator.mul, seg, c)), sum(map(operator.mul, seg, s))
    return math.sqrt(re * re + im * im) * _norm


def present(seg):
    """The signatures in one window."""
    out = []
    for name, pair in SIGS.items():
        ok = True
        for t in pair:
            a = amp(seg, t)
            if a < FLOOR or a < RATIO * max(amp(seg, t - GUARD), amp(seg, t + GUARD)):
                ok = False
                break
        if ok:
            out.append(name)
    return out


class Detector:
    """Windows in, runs out: `emit(dict)` for each run of a signature as it ends."""

    def __init__(self, emit, clock):
        self.emit, self.clock = emit, clock
        self.buf = []
        self.carry = b""
        self.samples = 0
        self.open = {}  # signature: [start sample, last window's sample, windows]

    def feed(self, raw):
        raw = self.carry + raw
        cut = len(raw) - len(raw) % 2
        self.carry = raw[cut:]
        self.buf.extend(memoryview(raw[:cut]).cast("h"))
        while len(self.buf) >= N:
            seg = self.buf[:N]
            at = self.samples
            found = present(seg)
            for name in found:
                run = self.open.get(name)
                if run and at - run[1] <= 2 * HOP:
                    run[1], run[2] = at, run[2] + 1
                else:
                    self._close(name)
                    self.open[name] = [at, at, 1]
            for name in [n for n, r in self.open.items() if n not in found and at - r[1] > 2 * HOP]:
                self._close(name)
            del self.buf[:HOP]
            self.samples += HOP

    def _close(self, name):
        run = self.open.pop(name, None)
        if run:
            end_s = (run[1] + N) / RATE
            lag = (self.samples - run[1]) / RATE  # how long ago, in the stream, the run ended
            self.emit({"ev": "run", "sig": name, "t": round(self.clock() - lag, 3), "start_s": round(run[0] / RATE, 3),
                       "end_s": round(end_s, 3), "windows": run[2], "seconds": round(run[2] * HOP / RATE, 2)})

    def flush(self):
        for name in list(self.open):
            self._close(name)

    @property
    def decoded_s(self):
        return self.samples / RATE


def decoder():
    return subprocess.Popen(["ffmpeg", "-hide_banner", "-nostats", "-loglevel", "error", "-probesize", "65536", "-analyzeduration", "1000000",
                             "-i", "pipe:0", "-ac", "1", "-ar", str(RATE), "-f", "s16le", "pipe:1"],
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)


def follow(resp, det):
    """Feed the response to ffmpeg and ffmpeg's samples to the detector until the stream ends."""
    ff = decoder()

    def pump():
        try:
            while not STOP.is_set():
                chunk = resp.read1(16384)
                if not chunk:
                    break
                ff.stdin.write(chunk)
                ff.stdin.flush()
        except Exception:
            pass
        try:
            ff.stdin.close()
        except Exception:
            pass

    th = threading.Thread(target=pump, daemon=True)
    th.start()
    while True:
        raw = ff.stdout.read1(8000) if hasattr(ff.stdout, "read1") else ff.stdout.read(8000)
        if not raw:
            break
        det.feed(raw)
    if STOP.is_set():
        ff.kill()
    ff.wait()
    det.flush()


def get(url, timeout=10):
    u = urllib.parse.urlsplit(url)
    c = http.client.HTTPConnection(u.hostname, u.port, timeout=timeout)
    c.request("GET", u.path)
    r = c.getresponse()
    return c, r


def writer(path):
    f = open(path, "a", buffering=1)
    lock = threading.Lock()

    def emit(d):
        d.setdefault("t", round(time.time(), 3))
        with lock:
            f.write(json.dumps(d, separators=(",", ":")) + "\n")
    return emit


def progress(emit, det):
    while not STOP.wait(30):
        emit({"ev": "progress", "decoded_s": round(det.decoded_s, 1)})


def public(url, out):
    emit = writer(out)
    det = Detector(emit, time.time)
    threading.Thread(target=progress, args=(emit, det), daemon=True).start()
    while not STOP.is_set():
        try:
            c, r = get(url)
            if r.status != 200:
                raise OSError(f"HTTP {r.status}")
            emit({"ev": "connect", "status": 200})
            follow(r, det)
            if not STOP.is_set():
                emit({"ev": "end", "why": "the stream ended", "decoded_s": round(det.decoded_s, 1)})
        except Exception as e:
            if not STOP.is_set():
                emit({"ev": "end", "why": f"{type(e).__name__}: {e}", "decoded_s": round(det.decoded_s, 1)})
        if not STOP.is_set():
            STOP.wait(1)
    emit({"ev": "stopped", "decoded_s": round(det.decoded_s, 1)})


def holder(url, out, t_from, t_until):
    emit = writer(out)
    det = Detector(emit, time.time)
    while time.time() < t_from and not STOP.is_set():
        STOP.wait(min(1, t_from - time.time()))
    last = None
    while not STOP.is_set():
        if time.time() > t_until:
            emit({"ev": "gave_up", "last": last})
            return
        try:
            c, r = get(url, timeout=30)
            if r.status == 200:
                emit({"ev": "connect", "status": 200})
                follow(r, det)
                emit({"ev": "end", "why": "the stream ended", "decoded_s": round(det.decoded_s, 1)})
                break
            body = r.read(2000).decode("utf-8", "replace")
            try:
                code = json.loads(body).get("code")
            except ValueError:
                code = None
            if (r.status, code) != last:
                emit({"ev": "try", "status": r.status, "code": code})
            last = (r.status, code)
            c.close()
        except Exception as e:
            emit({"ev": "end", "why": f"{type(e).__name__}: {e}", "decoded_s": round(det.decoded_s, 1)})
            break
        STOP.wait(1)
    # After the drop, the same token again: refused, or the stream is still open to it.
    try:
        c, r = get(url)
        body = r.read(2000).decode("utf-8", "replace") if r.status != 200 else ""
        try:
            code = json.loads(body).get("code") if body else None
        except ValueError:
            code = None
        emit({"ev": "after", "status": r.status, "code": code})
        c.close()
    except Exception as e:
        emit({"ev": "after", "status": None, "code": f"{type(e).__name__}: {e}"})


def from_file(path):
    det = Detector(lambda d: print(json.dumps(d)), lambda: 0.0)
    ff = subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-i", path, "-ac", "1", "-ar", str(RATE), "-f", "s16le", "pipe:1"],
                        capture_output=True, check=True)
    for i in range(0, len(ff.stdout), 16000):
        det.feed(ff.stdout[i:i + 16000])
    det.flush()
    print(json.dumps({"ev": "decoded", "seconds": round(det.decoded_s, 1)}))


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, lambda *_: STOP.set())
    signal.signal(signal.SIGINT, lambda *_: STOP.set())
    mode = sys.argv[1]
    if mode == "public":
        public(sys.argv[2], sys.argv[3])
    elif mode == "holder":
        holder(sys.argv[2], sys.argv[3], float(sys.argv[4]), float(sys.argv[5]))
    elif mode == "file":
        from_file(sys.argv[2])
    else:
        sys.exit("usage: keyed_ear.py public URL OUT | holder URL OUT FROM UNTIL | file PATH")
