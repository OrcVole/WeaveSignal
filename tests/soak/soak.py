# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The run behind `run-24h.sh` (issue 12, PLAN Phase 14d): a station on the Compose bundle, up
# already, is named, given its settings, library, clock and week, and then left alone for
# DURATION while this program samples it, holds a listener on the Opus stream, and at the end
# checks what the station kept (the legal logger's hour files, the per-play log) and writes
# RESULT.md. Standard library only.
#
# The week it seeds, as fractions of the run D (T0 is the start of the log):
#   - a library of six tones, one clock, every hour of the week on that clock, mode `auto`;
#   - a HOLE at 0.05 D: a few planned items deleted, with later ones left. Automation does not
#     wait for an item's planned time (`next_to_queue` takes the next planned item), so the
#     items after the hole should play early; the check says what happened (informational);
#   - a MISSING SOURCE from 0.10 D for two rotations of the library: one track's file is moved
#     away inside the control plane's container and back, so its items are skipped ("missing");
#   - a GAP from 0.40 D: the log ends there (the items after it are deleted, the week emptied),
#     so automation has nothing queued and the fallback must cover, for min(0.15 D, 2 h), until
#     the script puts the clock back and generates again.
# Everything the script does to the station is logged in RESULT.md as script-caused.
import collections
import datetime
import http.client
import json
import math
import os
import queue
import re
import shutil
import signal
import statistics
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from array import array

HERE = os.path.dirname(os.path.abspath(__file__))
E = os.environ
PROJECT = E["SOAK_PROJECT"]
CONTROL_PORT = int(E["CONTROL_PORT"])
LISTENERS_PORT = int(E["LISTENERS_PORT"])
TOKEN = E["SOAK_TOKEN"]
D = int(E["SOAK_DURATION_S"])
SAMPLE_S = max(2, int(E.get("SAMPLE_S", "60")))
OUT = E["OUT"]
IMAGE = E.get("SOAK_IMAGE", "")
CONTAINER_SUFFIX = "-1"
NTRACKS = 6
TRACK_S = max(15, min(120, round(D / 80)))
CLOCK_ID = "11111111-1111-4111-8111-111111111101"
HOUR = 3600
STOP = threading.Event()
T_START = time.time()
NOTES = []  # what the script did to the station, and when
LOGF = open(os.path.join(OUT, "run.log"), "a", buffering=1)


def now():
    return time.time()


def iso(t=None):
    return datetime.datetime.fromtimestamp(t or now(), datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def log(msg):
    line = f"{iso()} {msg}"
    print(line, flush=True)
    LOGF.write(line + "\n")


def script_did(t0, what):
    NOTES.append((now() - t0 if t0 else None, what))
    log(f"script: {what}")


# ---- the station's API ----------------------------------------------------------------------


def api(method, path, body=None, raw=None, timeout=30):
    data = raw if raw is not None else (json.dumps(body).encode() if body is not None else None)
    req = urllib.request.Request(f"http://127.0.0.1:{CONTROL_PORT}{path}", data=data, method=method)
    req.add_header("Authorization", f"Bearer {TOKEN}")
    if raw is None and body is not None:
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            status, text = r.status, r.read()
    except urllib.error.HTTPError as e:
        status, text = e.code, e.read()
    except Exception as e:  # the station not answering is a reading, not a crash
        return None, f"{type(e).__name__}: {e}"
    try:
        return status, json.loads(text) if text else None
    except ValueError:
        return status, text.decode("utf-8", "replace")


def must(method, path, body=None, raw=None, ok=(200, 201, 204)):
    status, answer = api(method, path, body, raw)
    if status not in ok:
        raise SystemExit(f"{method} {path}: {status} {str(answer)[:300]}")
    return answer


def sh(*cmd, timeout=60, check=False):
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    if check and r.returncode:
        raise SystemExit(f"{' '.join(cmd)}: {r.stderr.strip()[:300]}")
    return r.stdout


# ---- the sockets in the volumes -------------------------------------------------------------


class Helper:
    """`sockhelper.py` under `podman unshare`, one request at a time."""

    def __init__(self):
        vol = lambda n: sh("podman", "volume", "inspect", f"{PROJECT}_{n}", "--format", "{{.Mountpoint}}", check=True).strip()
        self.lock = threading.Lock()
        self.p = subprocess.Popen(
            ["podman", "unshare", "python3", "-I", os.path.join(HERE, "sockhelper.py"), vol("listeners-control"), vol("desk-control")],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, bufsize=1,
        )

    def ask(self, line):
        with self.lock:
            try:
                self.p.stdin.write(line + "\n")
                self.p.stdin.flush()
                a = json.loads(self.p.stdout.readline())
            except Exception as e:
                return None
        return a["v"] if a.get("ok") else None

    def close(self):
        try:
            self.p.stdin.close()
            self.p.wait(timeout=5)
        except Exception:
            self.p.kill()


# ---- the held listener ----------------------------------------------------------------------


class Listener(threading.Thread):
    """One Opus listener for the whole run: bytes, drops and gaps over 2 s, and `silencedetect`
    on what it hears (the stream's bytes keep coming through silence, so only a decode says)."""

    GAP_S = 2.0

    def __init__(self, t0_ref):
        super().__init__(daemon=True)
        self.t0 = t0_ref
        self.bytes = 0
        self.connects = 0
        self.drops = []  # (elapsed, why)
        self.gaps = []  # (elapsed start, seconds)
        self.silences = []  # (elapsed start, seconds)
        self.stop = threading.Event()
        self.first_byte_at = None
        self.last_rx = None
        self._ff = None

    def _ffmpeg(self):
        self._ffq = queue.Queue()
        self._ff = subprocess.Popen(
            ["ffmpeg", "-hide_banner", "-nostats", "-loglevel", "info", "-probesize", "65536", "-analyzeduration", "1000000",
             "-i", "pipe:0", "-af", "silencedetect=n=-55dB:d=3", "-f", "null", "-"],
            stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True,
        )
        threading.Thread(target=self._feed, args=(self._ff, self._ffq), daemon=True).start()
        threading.Thread(target=self._parse, args=(self._ff,), daemon=True).start()

    @staticmethod
    def _feed(p, q):
        while True:
            chunk = q.get()
            if chunk is None:
                break
            try:
                p.stdin.buffer.write(chunk)
                p.stdin.buffer.flush()
            except Exception:
                break
        try:
            p.stdin.close()
        except Exception:
            pass

    def _parse(self, p):
        for line in p.stderr:
            m = re.search(r"silence_end: ([\d.]+) \| silence_duration: ([\d.]+)", line)
            if m:
                dur = float(m.group(2))
                self.silences.append((now() - self.t0() - dur, dur))
            elif "silence_start" in line:
                self._open = now()

    def _end_ffmpeg(self):
        if self._ff:
            self._ffq.put(None)
            try:
                self._ff.wait(timeout=10)
            except Exception:
                self._ff.kill()
            self._ff = None

    def run(self):
        while not self.stop.is_set():
            conn = None
            try:
                conn = http.client.HTTPConnection("127.0.0.1", LISTENERS_PORT, timeout=5)
                conn.request("GET", "/programme.opus")
                r = conn.getresponse()
                if r.status != 200:
                    raise OSError(f"HTTP {r.status}")
                self.connects += 1
                self._ffmpeg()
                self.last_rx = now()
                while not self.stop.is_set():
                    chunk = r.read1(16384)
                    if not chunk:
                        raise OSError("the stream ended")
                    t = now()
                    if t - self.last_rx > self.GAP_S:
                        self.gaps.append((self.last_rx - self.t0(), t - self.last_rx))
                    self.last_rx = t
                    self.first_byte_at = self.first_byte_at or t
                    self.bytes += len(chunk)
                    self._ffq.put(chunk)
            except Exception as e:
                if not self.stop.is_set():
                    self.drops.append((now() - self.t0(), f"{type(e).__name__}: {e}"))
                    log(f"the held listener was dropped: {e}")
            finally:
                self._end_ffmpeg()
                if conn:
                    conn.close()
            if not self.stop.is_set():
                time.sleep(1)

    def finish(self):
        self.stop.set()
        self.join(timeout=15)
        self._end_ffmpeg()


# ---- seeding --------------------------------------------------------------------------------


def tone_wav(freq, secs):
    one = array("h")
    for i in range(48000):
        v = int(math.sin(2 * math.pi * freq * i / 48000) * 0.3 * 32767)
        one.extend((v, v))
    body = one.tobytes() * secs
    hdr = b"RIFF" + (36 + len(body)).to_bytes(4, "little") + b"WAVEfmt " + (16).to_bytes(4, "little")
    hdr += (1).to_bytes(2, "little") + (2).to_bytes(2, "little") + (48000).to_bytes(4, "little") + (192000).to_bytes(4, "little")
    hdr += (4).to_bytes(2, "little") + (16).to_bytes(2, "little") + b"data" + len(body).to_bytes(4, "little")
    return hdr + body


def week_all():
    return {"hours": [{"weekday": d, "hour": h, "clock": CLOCK_ID} for d in range(7) for h in range(24)]}


def seed_station():
    for _ in range(90):
        s, a = api("GET", "/health")
        if s == 200 and isinstance(a, dict) and a.get("database") == "ok":
            break
        time.sleep(1)
    else:
        raise SystemExit("the control plane did not come up with its database")
    must("PUT", "/api/v1/setup", {"name": "Soak FM"})
    must("PUT", "/api/v1/playout/settings", {"time_zone": "UTC", "artist_separation_min": 0, "title_separation_min": 0,
                                             "album_separation_min": 0, "mode": "auto", "default_clock": None})
    must("PUT", "/api/v1/retention", {"preset": "uk_internet", "by": "soak"})
    tracks = []
    for i, f in enumerate((220, 247, 277, 330, 370, 415)[:NTRACKS]):
        q = f"title=Soak%20tone%20{f}&artist=Soak%20Artist%20{i + 1}&album=Soak%20Tones&label=Soak&category=a"
        tracks.append(must("PUT", f"/api/v1/library/tracks?{q}", raw=tone_wav(f, TRACK_S)))
    must("PUT", f"/api/v1/schedule/clocks/{CLOCK_ID}", {"name": "Soak playlist", "slots": [{"category": "a"}]})
    # The week grid is put on at T0, not here: with a clock on any hour automation makes the log
    # 26 hours ahead by itself within seconds, and a later generate for a nearer span would find
    # every track "last used" in the future and pick one track over and over.
    return tracks


def seed_shows(t0):
    """Scheduled programmes back to back over the run and its gap, best effort: the shows are the
    calendar (what a presenter's seat follows); what airs unattended is the log."""
    gap1, gap2 = 0.40 * D, 0.40 * D + min(0.15 * D, 2 * HOUR)
    segments = [(0, gap1), (gap2, D)]
    made = 0
    for seg, (a, b) in enumerate(segments):
        t = t0 + a
        while t < t0 + b - 60:
            dt = datetime.datetime.fromtimestamp(t, datetime.timezone.utc)
            start_min = dt.hour * 60 + dt.minute
            minutes = max(5, min(int((t0 + b - t) // 60), 1440 - start_min))
            made += 1
            slug = f"soak-show-{made}"
            s, a_ = api("PUT", f"/api/v1/shows/{slug}", {
                "name": f"Soak Show {made}", "colour_slot": made % 10 + 1, "layout": "slate",
                "slot": {"weekday": dt.isoweekday(), "start": start_min, "minutes": minutes},
                "hosts": ["Soak Host"], "tags": ["soak"], "description": "A programme of the soak run.",
                "explicit": False, "step": "full"})
            if s != 200:
                log(f"shows: {slug} was refused ({s} {str(a_)[:120]}); the run goes on without the calendar")
                return made - 1
            api("PUT", f"/api/v1/shows/{slug}/series/1", {"starts_on": dt.strftime("%Y-%m-%d")})
            api("POST", f"/api/v1/shows/{slug}/series/1/episodes", {"count": 1})
            t += minutes * 60
    return made


def wait_mount():
    for _ in range(200):
        try:
            c = http.client.HTTPConnection("127.0.0.1", LISTENERS_PORT, timeout=4)
            c.request("GET", "/programme.opus")
            r = c.getresponse()
            ok = r.status == 200 and len(r.read1(4096)) > 0
            c.close()
            if ok:
                return True
        except Exception:
            pass
        time.sleep(1)
    return False


# ---- sampling -------------------------------------------------------------------------------


def parse_size(text):
    m = re.match(r"([\d.]+)\s*([kKMGT]?i?B)", text.strip())
    if not m:
        return None
    mult = {"B": 1, "kB": 1e3, "KB": 1e3, "KiB": 1024, "MB": 1e6, "MiB": 1024**2, "GB": 1e9, "GiB": 1024**3}[m.group(2)]
    return float(m.group(1)) * mult / 1048576  # MiB


def short(name):
    n = name[len(PROJECT) + 1:] if name.startswith(PROJECT + "-") else name
    return n[: -len(CONTAINER_SUFFIX)] if n.endswith(CONTAINER_SUFFIX) else n


class Observer:
    def __init__(self, helper, t0, containers):
        self.helper, self.t0, self.containers = helper, t0, containers
        self.rows = []
        self.timeline = []  # (elapsed, on_air, np_on_air, mode, desk_connected)
        self.buf = []  # (elapsed, feed.buffer_length)
        self.fallback_status = collections.Counter()
        self.started = self.inspect()
        self.restarts = []
        self.health_bad = 0
        self.samples_file = open(os.path.join(OUT, "samples.jsonl"), "w", buffering=1)
        self._last = None

    def elapsed(self):
        return now() - self.t0

    def inspect(self):
        out = sh("podman", "inspect", "--format", "{{.Name}}|{{.State.Status}}|{{.State.StartedAt}}|{{.RestartCount}}|{{.State.OOMKilled}}", *self.containers)
        d = {}
        for line in out.splitlines():
            n, status, started, rc, oom = line.split("|")
            d[short(n)] = (status, started, int(rc), oom == "true")
        return d

    def fast(self):
        """Every second: the feed's cushion; every two: what is on air."""
        i = 0
        while not STOP.is_set():
            t = self.elapsed()
            v = self.helper.ask("L feed.buffer_length")
            try:
                self.buf.append((t, float(v)))
            except (TypeError, ValueError):
                pass
            if i % 2 == 0:
                s, a = api("GET", "/api/v1/playout", timeout=5)
                n_s, n = api("GET", "/api/v1/now-playing", timeout=5)
                if s == 200 and isinstance(a, dict) and a.get("playout") is not None:
                    key = (a["playout"]["on_air"], (n or {}).get("on_air") if n_s == 200 else None, a["playout"]["mode"], a["playout"]["desk_connected"])
                else:
                    key = ("away", None, None, None)
                if key != self._last:
                    self.timeline.append((t, *key))
                    self._last = key
            i += 1
            STOP.wait(max(0.0, 1.0 - (self.elapsed() - t)))

    def sample(self):
        t = self.elapsed()
        row = {"t": iso(), "e": round(t, 1)}
        s, h = api("GET", "/health", timeout=10)
        row["health"] = "ok" if s == 200 and isinstance(h, dict) and h.get("status") == "ok" and h.get("database") == "ok" else f"bad:{s}"
        if row["health"] != "ok":
            self.health_bad += 1
        s, p = api("GET", "/api/v1/playout", timeout=10)
        if s == 200 and isinstance(p, dict):
            pl = p.get("playout") or {}
            au = p.get("automation") or {}
            row.update(on_air=pl.get("on_air"), mode=pl.get("mode"), desk_conn=pl.get("desk_connected"), auto=au.get("current"),
                       queued=len(au.get("queued") or []), listeners=p.get("listeners"), default=p.get("default_fallback"),
                       logger_on=(p.get("logger") or {}).get("on"), gaps=len(p.get("gaps") or []))
        s, n = api("GET", "/api/v1/now-playing", timeout=10)
        if s == 200 and isinstance(n, dict):
            row["np"] = n.get("on_air")
            row["np_title"] = (n.get("now") or {}).get("title")
        s, lg = api("GET", "/api/v1/logger", timeout=15)
        if s == 200 and isinstance(lg, dict):
            recs = lg.get("recordings") or []
            row.update(rec_files=len(recs), rec_last_bytes=recs[-1]["bytes"] if recs else None, missing_hours=len(lg.get("missing_hours") or []))
        d = self.helper.ask("D")
        if d:
            row.update(desk_link=(d.get("playout") or {}).get("connected"), desk_for_ms=(d.get("playout") or {}).get("for_ms"),
                       mix_blocks=d["mix"]["blocks"], mix_late=d["mix"]["late"], mix_max_us=d["mix"]["max_pass_us"],
                       delay_ms=d["delay"]["delay_ms"], dumps=d["delay"]["dumps"])
        fb = self.helper.ask("L fallback.status")
        if fb:
            try:
                row["fb"] = json.loads(fb).get("on_air")
                self.fallback_status[row["fb"]] += 1
            except ValueError:
                pass
        mem = {}
        for line in sh("podman", "stats", "--no-stream", "--format", "{{.Name}}|{{.MemUsage}}", *self.containers, timeout=60).splitlines():
            n_, _, u = line.partition("|")
            v = parse_size(u.split("/")[0])
            if v is not None:
                mem[short(n_)] = round(v, 1)
        row["mem"] = mem
        now_state = self.inspect()
        for name, st in now_state.items():
            was = self.started.get(name)
            if was and (st[0] != "running" or st[1] != was[1] or st[2] != was[2] or st[3]):
                self.restarts.append((round(t), name, st[0], st[1], st[2], st[3]))
                self.started[name] = st
        row["restarts"] = len(self.restarts)
        self.rows.append(row)
        self.samples_file.write(json.dumps(row, separators=(",", ":")) + "\n")
        return row


# ---- the run --------------------------------------------------------------------------------


def take_away(victim, by_id, control, lib):
    """Move away the file of a track a few items ahead (so it is not yet queued), whichever the
    log rotates to: the track is the victim of the missing-source window."""
    s, a = api("GET", f"/api/v1/schedule/log?from_ms={int(now() * 1000)}&to_ms={int(now() * 1000) + 3600_000}")
    upcoming = [i for i in (a["items"] if s == 200 else []) if i["status"] == "planned" and i["track"]]
    if not upcoming:
        log("the missing source: no planned item ahead to take the file of")
        return
    track = by_id[upcoming[min(3, len(upcoming) - 1)]["track"]]
    victim.update(track)
    sh("podman", "exec", control, "mv", f"{lib}/{track['file']}", f"{lib}/{track['file']}.away")
    log(f"the missing source: took away {track['title']} ({track['file']})")


def main():
    signal.signal(signal.SIGINT, lambda *_: STOP.set())
    signal.signal(signal.SIGTERM, lambda *_: STOP.set())
    containers = [c for c in sh("podman", "ps", "-a", "--filter", f"label=com.docker.compose.project={PROJECT}", "--format", "{{.Names}}").split() if c]
    if not containers:
        raise SystemExit("no containers in the project")
    log(f"soak: {D}s, sampling every {SAMPLE_S}s, project {PROJECT}, {len(containers)} containers, tracks {NTRACKS} x {TRACK_S}s")
    tracks = seed_station()
    log(f"seeded: {len(tracks)} tracks, one clock on every hour of the week, retention uk_internet, mode auto")
    # the legal logger is on once the setting has taken
    retained = None
    for _ in range(60):
        s, p = api("GET", "/api/v1/playout")
        if s == 200 and p["retained"]["logger"] and (p.get("logger") or {}).get("on"):
            retained = p["retained"]
            break
        time.sleep(1)
    if not retained:
        raise SystemExit("the legal logger did not come on after the retention setting")
    log(f"retention: logger on, per-play log kept for {retained['per_play_days']} days")
    if not wait_mount():
        raise SystemExit("the Opus mount did not answer")
    helper = Helper()
    t0_box = [now()]
    listener = Listener(lambda: t0_box[0])
    listener.start()
    time.sleep(3)

    # ---- T0: the log starts ----
    t0 = now()
    t0_box[0] = t0
    T_end = t0 + D
    gap1 = 0.40 * D
    gap2 = gap1 + min(0.15 * D, 2 * HOUR)
    hole_a, hole_len = 0.05 * D, 4 * TRACK_S
    miss_a, miss_len = 0.10 * D, 2 * NTRACKS * TRACK_S
    ms = lambda t: int(t * 1000)
    seed_plan = f"T0 {iso(t0)}; hole {hole_a:.0f}s+{hole_len}s; missing source {miss_a:.0f}s+{miss_len}s; gap {gap1:.0f}s to {gap2:.0f}s; end {iso(T_end)}"
    log(seed_plan)
    must("PUT", "/api/v1/schedule/week", week_all())
    must("POST", "/api/v1/schedule/generate", {"from_ms": ms(t0), "to_ms": ms(t0 + gap1)})
    items = must("GET", f"/api/v1/schedule/log?from_ms={ms(t0) - 60000}&to_ms={ms(t0 + gap1) + 30 * HOUR * 1000}")["items"]
    cut = [i for i in items if i["status"] == "planned" and (i["planned_at_ms"] >= ms(t0 + gap1) or ms(t0 + hole_a) <= i["planned_at_ms"] < ms(t0 + hole_a + hole_len))]
    for i in cut:
        api("DELETE", f"/api/v1/schedule/log/{i['id']}")
    n_hole = sum(1 for i in cut if i["planned_at_ms"] < ms(t0 + gap1))
    script_did(t0, f"generated the log from T0 to the hour after the gap's start, deleted {n_hole} planned items for the hole and {len(cut) - n_hole} past the gap's start")
    must("PUT", "/api/v1/schedule/week", {"hours": []})
    script_did(t0, "emptied the week grid so nothing refills the log (the gap)")
    kept = [i for i in items if i not in cut and i["track"]]
    rotation = len({i["track"] for i in kept[: 2 * NTRACKS]})
    log(f"the log's first {min(len(kept), 2 * NTRACKS)} items use {rotation} of {NTRACKS} tracks")
    if rotation < min(3, NTRACKS):
        script_did(t0, f"WARNING: the generated log does not rotate ({rotation} tracks in its first items)")
    shows = seed_shows(t0)
    log(f"shows: {shows} seeded over the run")

    obs = Observer(helper, t0, containers)
    obs.sample()
    threading.Thread(target=obs.fast, daemon=True).start()
    victim = {}
    by_id = {t["id"]: t for t in tracks}
    control = containers[[short(c) for c in containers].index("control")]
    lib = "/var/lib/weavesignal/library"
    events = [
        (miss_a, "the missing source: moved a track's file away", lambda: take_away(victim, by_id, control, lib)),
        (miss_a + miss_len, "the missing source: moved the file back", lambda: sh("podman", "exec", control, "mv", f"{lib}/{victim['file']}.away", f"{lib}/{victim['file']}") if victim else None),
        (gap2, "the gap ends: put the clock back on the week and generated the log", lambda: (
            api("PUT", "/api/v1/schedule/week", week_all()),
            api("POST", "/api/v1/schedule/generate", {"from_ms": ms(now()), "to_ms": ms(T_end + 2 * HOUR)}))),
    ]
    resumed_at = None
    next_sample = SAMPLE_S
    while not STOP.is_set() and obs.elapsed() < D:
        e = obs.elapsed()
        if events and e >= events[0][0]:
            at, what, fn = events.pop(0)
            fn()
            script_did(t0, what)
            if "gap ends" in what:
                resumed_at = obs.elapsed()
        if e >= next_sample:
            r = obs.sample()
            next_sample += SAMPLE_S
            log(f"{r['e']:.0f}s: on air {r.get('on_air')}/{r.get('np')} now {r.get('np_title')} listeners {r.get('listeners')} health {r['health']} restarts {r['restarts']} "
                f"control {r['mem'].get('control')} MiB desk {r['mem'].get('desk')} MiB playout {r['mem'].get('playout')} MiB")
        STOP.wait(1)
    interrupted = STOP.is_set()
    for what_at, what, fn in events:  # put back anything half done
        if "moved the file back" in what:
            fn()
            script_did(t0, "interrupted: " + what)
    t_stop = obs.elapsed()
    obs.sample()
    STOP.set()
    listener.finish()
    time.sleep(1.5)
    finish(obs, listener, helper, containers, tracks, victim, t0, t_stop, interrupted, dict(
        gap1=gap1, gap2=gap2, hole_a=hole_a, hole_len=hole_len, miss_a=miss_a, miss_len=miss_len, resumed_at=resumed_at, seed_plan=seed_plan, shows=shows, retained=retained))
    helper.close()


# ---- the end: checks and RESULT.md ----------------------------------------------------------


def pct(vals, p):
    v = sorted(vals)
    return v[min(len(v) - 1, int(len(v) * p))] if v else None


def fmt_t(sec):
    sec = int(sec)
    return f"{sec // 3600}:{sec % 3600 // 60:02d}:{sec % 60:02d}"


def analyse_logger_files(containers, t0):
    """Copy the logger's hour files out, decode each (length, silence), and delete the copies."""
    listeners = containers[[short(c) for c in containers].index("listeners")]
    dest = os.path.join(OUT, "logger-copy")
    shutil.rmtree(dest, ignore_errors=True)
    os.makedirs(dest)
    r = subprocess.run(["podman", "cp", f"{listeners}:/var/lib/weavesignal/recordings/logger/.", dest], capture_output=True, text=True)
    files = []
    for day in sorted(os.listdir(dest)):
        for f in sorted(os.listdir(os.path.join(dest, day))):
            if not f.endswith(".opus"):
                continue
            path = os.path.join(dest, day, f)
            start = datetime.datetime.strptime(f"{day} {f[:-5]}", "%Y-%m-%d %H-%M-%S").replace(tzinfo=datetime.timezone.utc).timestamp()
            dur = None
            pr = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", path], capture_output=True, text=True)
            try:
                dur = float(pr.stdout.strip())
            except ValueError:
                pass
            ff = subprocess.run(["ffmpeg", "-hide_banner", "-nostats", "-i", path, "-af", "silencedetect=n=-55dB:d=3", "-f", "null", "-"], capture_output=True, text=True)
            sil = [float(x) for x in re.findall(r"silence_duration: ([\d.]+)", ff.stderr)]
            if "silence_start" in ff.stderr and len(sil) < ff.stderr.count("silence_start"):
                sil.append(-1.0)  # silent to the end of the file
            files.append(dict(name=f"{day}/{f}", start=start, bytes=os.path.getsize(path), duration=dur, silences=sil))
    shutil.rmtree(dest, ignore_errors=True)
    return files, r.returncode


def finish(obs, L, helper, containers, tracks, victim, t0, t_stop, interrupted, plan):
    log("the run is over: collecting what the station kept")
    T_end = t0 + t_stop
    ms = lambda t: int(t * 1000)
    checks = []  # (name, True/False/None for information, detail)

    def check(name, ok, detail):
        checks.append((name, ok, detail))
        log(f"{'PASS' if ok else 'FAIL' if ok is False else 'INFO'} {name}: {detail}")

    rows = obs.rows
    # 1 containers
    check("every container up the whole run, none restarted", not obs.restarts and all(v[0] == "running" for v in obs.inspect().values()),
          "no restarts, none stopped, none out of memory" if not obs.restarts else f"changed: {obs.restarts}")
    # 2 health
    check("/health ok at every sample", obs.health_bad == 0, f"{len(rows) - obs.health_bad} of {len(rows)} samples ok")
    # 3 the held listener
    secs = (L.last_rx or T_end) - (L.first_byte_at or t0)
    kbps = L.bytes * 8 / 1000 / secs if secs > 0 else 0
    check("the held Opus listener was never dropped and never stalled over 2 s",
          not L.drops and not L.gaps and L.connects == 1,
          f"{L.connects} connection(s), {len(L.drops)} drops, {len(L.gaps)} gaps over 2 s ({', '.join(f'{fmt_t(a)} for {d:.1f}s' for a, d in L.gaps[:5])}), {L.bytes / 1e6:.1f} MB, {kbps:.1f} kbit/s")
    sil = [(a, d) for a, d in L.silences if a > 60]
    check("the listener heard no silence of 3 s or more (decoded, `silencedetect` -55 dB)", not sil,
          "none" if not sil else "; ".join(f"{fmt_t(a)} for {d:.1f}s" for a, d in sil[:8]))
    mp3 = {}
    for mount in ("programme.mp3", "programme-high.mp3"):
        try:
            c = http.client.HTTPConnection("127.0.0.1", LISTENERS_PORT, timeout=6)
            c.request("GET", "/" + mount)
            r = c.getresponse()
            got, until = 0, now() + 5
            while now() < until and got < 20000:
                got += len(r.read1(8192))
            c.close()
            mp3[mount] = got
        except Exception as e:
            mp3[mount] = f"{type(e).__name__}"
    check("both MP3 mounts still answer with audio", all(isinstance(v, int) and v >= 20000 for v in mp3.values()), f"{mp3}")
    # 4 the legal logger
    s, lg = api("GET", "/api/v1/logger")
    lg = lg if s == 200 else {}
    files, rc = analyse_logger_files(containers, t0)
    first_hour = int(t0 // HOUR) * HOUR
    hours = [h for h in range(first_hour, int(T_end // HOUR) * HOUR + HOUR, HOUR) if h < T_end - 60]
    have = {int(f["start"] // HOUR) * HOUR for f in files}
    missing = [iso(h) for h in hours if h not in have]
    check("the legal logger was on at every sample", all(r.get("logger_on") for r in rows), f"{sum(1 for r in rows if r.get('logger_on'))} of {len(rows)} samples")
    check("the logger has an hour file for every hour of the run, and `missing_hours` is empty", not missing and not (lg.get("missing_hours") or []),
          f"{len(files)} files for {len(hours)} hours; missing hours {missing or 'none'}; the route's missing_hours {lg.get('missing_hours')}")
    bad = []
    for i, f in enumerate(files):
        nxt = files[i + 1]["start"] if i + 1 < len(files) else None
        if nxt is not None and f["duration"] is not None and abs(f["duration"] - (nxt - f["start"])) > 10:
            bad.append(f"{f['name']} is {f['duration']:.0f}s of audio for {nxt - f['start']:.0f}s")
        if f["bytes"] <= 0 or f["duration"] is None:
            bad.append(f"{f['name']} unreadable")
        if f["silences"]:
            bad.append(f"{f['name']} has silence {f['silences']}")
    check("each logger file is real audio of its hour's length with no silence of 3 s (decoded)", not bad and bool(files),
          "; ".join(f"{f['name']} {f['bytes'] / 1e6:.1f} MB {f['duration'] or 0:.0f}s" for f in files) if not bad else "; ".join(bad[:6]))
    # 5 per-play log
    from_ms, to_ms = ms(t0) - 60000, ms(T_end) + 120000
    s, lgitems = api("GET", f"/api/v1/schedule/log?from_ms={from_ms}&to_ms={to_ms + 3 * HOUR * 1000}")
    items = lgitems["items"] if s == 200 else []
    s, pl = api("GET", f"/api/v1/reports/plays?from_ms={from_ms}&to_ms={to_ms}")
    plays = pl["plays"] if s == 200 else []
    by_item = collections.defaultdict(list)
    for p in plays:
        if p.get("item") is not None:
            by_item[p["item"]].append(p)
    due = [i for i in items if i["planned_at_ms"] < ms(T_end) - 90000]
    played = [i for i in due if i["status"] == "played"]
    skipped = [i for i in due if i["status"] == "skipped"]
    open_ = [i for i in due if i["status"] in ("planned", "queued")]
    no_play = [i["id"] for i in played if not by_item.get(i["id"])]
    twice = [i for i, v in by_item.items() if len(v) > 1]
    vid = victim.get("id")
    missing_victim = [i for i in skipped if i["track"] == vid]
    other_skips = [i for i in skipped if i["track"] != vid]
    in_flight = [i for i in open_ if i["track"] == vid and abs(i["planned_at_ms"] - ms(t0 + plan["miss_a"])) < 6 * TRACK_S * 1000]
    check("every scheduled item due by the end has a per-play entry, or was skipped for the missing source",
          bool(played) and not no_play and not twice and not other_skips and len(open_) == len(in_flight),
          f"{len(due)} items due: {len(played)} played with an entry each, {len(skipped)} skipped, {len(open_)} still planned/queued; "
          f"items played without an entry {no_play[:5] or 'none'}, played twice {twice[:5] or 'none'}, skipped for another track {[i['id'] for i in other_skips][:5] or 'none'}"
          + (f"; {len(in_flight)} of the still-open were in flight when the file went (queued, then lost: see findings)" if in_flight else ""))
    check("the missing source was skipped, with its reason, and the track played again after it came back",
          len(missing_victim) >= 1 and all("missing" in (i["note"] or "") or "gone" in (i["note"] or "") for i in missing_victim)
          and any(p["track"] == vid and p["started_at_ms"] > ms(t0 + plan["miss_a"] + plan["miss_len"]) for p in plays),
          f"{len(missing_victim)} skipped items, notes {sorted({i['note'][:60] for i in missing_victim})}; played after the file came back: "
          f"{sum(1 for p in plays if p['track'] == victim['id'] and p['started_at_ms'] > ms(t0 + plan['miss_a'] + plan['miss_len']))}")
    # 6 the gap
    tl = obs.timeline
    log_plays = [p for p in plays if p.get("item") is not None]
    before = [p for p in log_plays if p["started_at_ms"] < ms(t0 + plan["gap2"] - 5)]
    gap_from = (max((p.get("ended_at_ms") or p["started_at_ms"] + p["duration_ms"]) for p in before) / 1000 - t0) if before else plan["gap1"]
    lo, hi = gap_from + 25, (plan["resumed_at"] or plan["gap2"]) - 10
    inside = [r for r in tl if lo <= r[0] <= hi]

    def state_at(t):
        cur = None
        for r in tl:
            if r[0] <= t:
                cur = r
        return cur

    on_air_in = {state_at(t)[1] for t in range(int(lo), int(hi) + 1, 2) if state_at(t)} if hi > lo else set()
    np_in = {state_at(t)[2] for t in range(int(lo), int(hi) + 1, 2) if state_at(t)} if hi > lo else set()
    check("the fallback covered the gap: playout and now-playing read fallback, never silence or away",
          hi > lo and on_air_in and on_air_in <= {"fallback", "default"} and np_in <= {"fallback"},
          f"the log's last play ended at {fmt_t(gap_from)}; the script resumed it at {fmt_t(plan['resumed_at'] or 0)}; between them (from +25 s to -10 s) playout read {sorted(on_air_in)}, now-playing {sorted(x for x in np_in if x)}")
    anyone = [r for r in tl if r[1] in ("silence", "away")]
    check("playout never read silence or away in the whole run", not anyone, "never" if not anyone else f"{[(fmt_t(r[0]), r[1]) for r in anyone[:5]]}")
    after = [p for p in log_plays if p["started_at_ms"] >= ms(t0 + (plan["resumed_at"] or plan["gap2"]))]
    first_after = (min(p["started_at_ms"] for p in after) / 1000 - t0 - (plan["resumed_at"] or plan["gap2"])) if after else None
    check("automation took the air again within 30 s of the log being generated", first_after is not None and first_after < 30,
          f"first item {first_after:.1f}s after the script's generate" if first_after is not None else "no item played after the gap")
    # 7 the feed's cushion and the desk
    bufs = [b for _, b in obs.buf]
    over = sum(1 for b in bufs if b > 1.5)
    check("the listener process's feed buffer stayed under 1.5 s (0.5 s set)", bool(bufs) and over <= len(bufs) * 0.01,
          f"{len(bufs)} readings a second: min {min(bufs):.2f}, p50 {statistics.median(bufs):.2f}, p99 {pct(bufs, .99):.2f}, max {max(bufs):.2f}; {over} over 1.5 s" if bufs else "no readings")
    nonfeed = {k: v for k, v in obs.fallback_status.items() if k != "feed"}
    check("the listener process was on playout's feed at every sample (`fallback.status`)", not nonfeed, f"{dict(obs.fallback_status)}")
    link = [r for r in rows if r.get("desk_link") is False]
    late = rows[-1].get("mix_late", 0) - rows[0].get("mix_late", 0)
    blocks = rows[-1].get("mix_blocks", 0) - rows[0].get("mix_blocks", 0)
    for_ms = [r["desk_for_ms"] for r in rows if r.get("desk_for_ms") is not None]
    resets = sum(1 for a, b in zip(for_ms, for_ms[1:]) if b < a)
    check("the desk stayed linked to playout and its mixer kept time", not link and resets == 0 and (blocks == 0 or late / blocks < 0.001),
          f"link down at {len(link)} samples, {resets} resets of the link's age, {late} late blocks of {blocks}, longest pass {max(r.get('mix_max_us', 0) for r in rows)} us")
    # 8 memory
    names = sorted({k for r in rows for k in r["mem"]})
    base_at = min(180, 0.15 * D)
    grow, mem_lines = [], []
    for n in names:
        series = [(r["e"], r["mem"][n]) for r in rows if n in r["mem"]]
        start = next((v for e, v in series if e >= base_at), series[0][1])
        peak, end = max(v for _, v in series), series[-1][1]
        flag = end > start * 1.2 and end - start > 20
        if flag:
            grow.append(n)
        mem_lines.append((n, start, peak, end, flag))
    check("no container's memory grew by more than 20 % (and 20 MiB) from its start reading to its end", not grow,
          f"baseline at {base_at:.0f} s; grew: {grow or 'none'}")
    # 9 information: plan against play, warnings
    pairs = [(i["planned_at_ms"], (by_item[i["id"]][0]["started_at_ms"] - i["planned_at_ms"]) / 1000)
             for i in played if by_item.get(i["id"]) and i["planned_at_ms"] < ms(t0 + plan["gap1"])]
    pre_hole = [d for t, d in pairs if t < ms(t0 + plan["hole_a"])]
    post_hole = [d for t, d in pairs if t >= ms(t0 + plan["hole_a"] + plan["hole_len"])]
    drift_pre = [d for _, d in pairs]
    if pre_hole and post_hole:
        check("(information) the hole: items after it played early by about the hole's length",
              None, f"hole {plan['hole_len']} s; played minus planned before the hole median {statistics.median(pre_hole):.1f} s, after it median {statistics.median(post_hole):.1f} s "
                    f"(min {min(post_hole):.1f}, max {max(post_hole):.1f}): automation does not wait for an item's planned time")
    # logs
    os.makedirs(os.path.join(OUT, "logs"), exist_ok=True)
    noise = []
    for c in containers:
        out = subprocess.run(["podman", "logs", "--tail", "20000", c], capture_output=True, text=True)
        text = out.stdout + out.stderr
        open(os.path.join(OUT, "logs", short(c) + ".log"), "w").write(text)
        cnt = collections.Counter()
        for line in text.splitlines():
            try:
                j = json.loads(line)
                if j.get("level") in ("WARN", "ERROR"):
                    cnt[(j["level"], re.sub(r"\d+", "N", str(j.get("message")))[:90])] += 1
            except ValueError:
                if re.search(r"\b(error|critical|severe|panic)\b", line, re.I):
                    cnt[("text", re.sub(r"\d+", "N", re.sub(r"^\S+ \S+ ", "", line))[:90])] += 1
        if cnt:
            noise.append((short(c), cnt))
    write_result(obs, L, checks, mem_lines, files, noise, plan, victim, t0, t_stop, interrupted, items, plays, drift_pre)


def write_result(obs, L, checks, mem_lines, files, noise, plan, victim, t0, t_stop, interrupted, items, plays, drift):
    fails = [c for c in checks if c[1] is False]
    out = [f"# Soak run {iso(t0)}", "",
           f"- Station image: `{IMAGE}`; project `{PROJECT}`; ran {fmt_t(t_stop)} of {fmt_t(D)} asked{' (INTERRUPTED)' if interrupted else ''}; sampled every {SAMPLE_S} s ({len(obs.rows)} samples in `samples.jsonl`).",
           f"- Plan: {plan['seed_plan']}. Shows seeded: {plan['shows']}.",
           f"- Library: {NTRACKS} tones of {TRACK_S} s on one clock, mode auto, separations off, retention `uk_internet` (per-play log kept {plan['retained']['per_play_days']} days).",
           f"- **Result: {'FAIL (' + str(len(fails)) + ')' if fails or interrupted else 'PASS'}**", "", "## Checks", ""]
    for name, ok, detail in checks:
        out.append(f"- **{'PASS' if ok else 'FAIL' if ok is False else 'INFO'}** {name}: {detail}")
    out += ["", "## What the script did to the station (script-caused)", ""]
    out += [f"- {fmt_t(t) if t is not None else '-'} {w}" for t, w in NOTES]
    out += ["", "## What was on air (playout, now-playing, mode, desk connected), from the 2 s poll", ""]
    for t, a, n, m, d in obs.timeline:
        out.append(f"- {fmt_t(t)} {a} / {n} / {m} / desk {d}")
    out += ["", "## Memory (MiB)", "", "| container | start | peak | end | grew |", "|---|---|---|---|---|"]
    out += [f"| {n} | {s} | {p} | {e} | {'**yes**' if f else 'no'} |" for n, s, p, e, f in mem_lines]
    out += ["", "## Listener", "", f"- {L.connects} connection(s), {L.bytes / 1e6:.1f} MB, drops {L.drops}, gaps over 2 s {[(fmt_t(a), round(d, 1)) for a, d in L.gaps]}, silences {[(fmt_t(a), round(d, 1)) for a, d in L.silences]}"]
    out += ["", "## Logger files", ""] + [f"- {f['name']}: {f['bytes'] / 1e6:.1f} MB, {f['duration'] and round(f['duration'])} s, silences {f['silences']}" for f in files]
    out += ["", "## Warnings and errors in the containers' logs (last 20,000 lines each; `logs/`)", ""]
    for n, cnt in noise:
        out.append(f"- **{n}**: " + "; ".join(f"{c}x {k[0]} {k[1]}" for k, c in cnt.most_common(6)))
    if not noise:
        out.append("- none")
    out += ["", "## Not sampled", "",
            "- The desk's `feed_underruns` and `feed_late_ms` exist only with video on (`VIDEO=true`, the video sound's feed ring); this run leaves video off. The desk's own per-voice `underruns`/`late_ms` cover people in the room, and there are none. "
            "Sampled instead: the desk's link to playout and its age, the mixer's `late` blocks and longest pass, the profanity delay, and the listener process's `feed.buffer_length` every second and `fallback.status` each sample.", ""]
    open(os.path.join(OUT, "RESULT.md"), "w").write("\n".join(out))
    json.dump({"checks": [[n, ok, d] for n, ok, d in checks], "interrupted": interrupted}, open(os.path.join(OUT, "summary.json"), "w"), indent=1)
    log(f"RESULT.md: {os.path.join(OUT, 'RESULT.md')} ({len(fails)} failed)")
    sys.exit(1 if fails or interrupted else 0)


if __name__ == "__main__":
    main()
