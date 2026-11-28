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
# `soak.py --memory-report <run folder>` recomputes the memory section of a finished run from its
# saved samples (see `memory_report`); it needs none of the run's environment.
REPORT_ONLY = sys.argv[1:2] == ["--memory-report"]
if REPORT_ONLY:
    OUT = os.path.abspath(sys.argv[2]) if len(sys.argv) > 2 else sys.exit("usage: soak.py --memory-report <run folder>")
    PROJECT, CONTROL_PORT, LISTENERS_PORT, TOKEN, D, SAMPLE_S = "", 0, 0, "", 0, 60
else:
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
LOGF = open(os.devnull if REPORT_ONLY else os.path.join(OUT, "run.log"), "a", buffering=1)


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
            ["podman", "unshare", "python3", "-I", os.path.join(HERE, "sockhelper.py"), vol("listeners-control"), vol("desk-control"), vol("playout-control")],
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
    """A tone, or a chord of tones (a tuple: the keyed shows' signatures), at 0.3 in all."""
    freqs = freq if isinstance(freq, tuple) else (freq,)
    one = array("h")
    for i in range(48000):
        v = int(sum(math.sin(2 * math.pi * f * i / 48000) for f in freqs) * 0.3 / len(freqs) * 32767)
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


CGROUP_ROOT = "/sys/fs/cgroup"


def read_cgroups(containers):
    """Each running container's memory, read from outside the stack, on the host: `memory.current`
    and every line of `memory.stat` (bytes) of the container's own cgroup scope, which rootless
    Podman with systemd puts at `/sys/fs/cgroup/<State.CgroupPath>` (user.slice/.../libpod-<id>.scope).
    `anon` is the processes' own memory; `file` is page cache (files read and written, reclaimable
    unless dirty or mapped); `shmem` is shared memory and tmpfs (counted in `file` too); `kernel` is
    kernel memory the container holds. Podman's own "MemUsage" is `memory.current` minus
    `inactive_file`. Returns ({container: {...}}, {container: why it could not be read})."""
    got, err = {}, {}
    out = sh("podman", "inspect", "--format", "{{.Name}}|{{.State.Status}}|{{.State.CgroupPath}}", *containers, timeout=60)
    for line in out.splitlines():
        name, status, path = (line.split("|") + ["", ""])[:3]
        if status != "running":
            continue  # a container that has finished (a one-shot step) has no cgroup to read
        n = short(name)
        if not path:
            err[n] = "podman inspect gives no cgroup path"
            continue
        base = os.path.join(CGROUP_ROOT, path.lstrip("/"))
        try:
            d = {"current": int(open(os.path.join(base, "memory.current")).read())}
            for kv in open(os.path.join(base, "memory.stat")).read().splitlines():
                k, _, v = kv.partition(" ")
                if v.strip().isdigit():
                    d[k] = int(v)
            got[n] = d
        except (OSError, ValueError) as e:
            err[n] = f"{type(e).__name__}: {e}"
    return got, err


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
        # the containers' own cgroup counters, read from the host (see `read_cgroups`)
        self.cg_file = open(os.path.join(OUT, "memstat.jsonl"), "w", buffering=1)
        self.cg_rows = []  # {"e", "t", "cg": {container: {"current": bytes, <memory.stat keys>: bytes}}, "err": {container: why}}
        self.cg_failed = 0  # samples in which a running container could not be read
        self.cg_last_err = None
        json.dump({"duration_s": D, "sample_s": SAMPLE_S, "project": PROJECT, "image": IMAGE}, open(os.path.join(OUT, "memory-meta.json"), "w"))
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
        self.sample_cgroups(t)
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

    def sample_cgroups(self, t):
        """A failure here is reported and counted, never raised: the run goes on without the reading."""
        try:
            cg, err = read_cgroups(self.containers)
            if not cg and not err:
                err = {"*": "no running container found"}
        except Exception as e:  # podman not answering, a changed path: a missing reading, not a stop
            cg, err = {}, {"*": f"{type(e).__name__}: {e}"}
        if err:
            self.cg_failed += 1
            self.cg_last_err = err
            log(f"WARNING: cgroup memory not read at {t:.0f}s for {sorted(err)}: {next(iter(err.values()))}")
        row = {"e": round(t, 1), "t": iso(), "cg": cg}
        if err:
            row["err"] = err
        self.cg_rows.append(row)
        self.cg_file.write(json.dumps(row, separators=(",", ":")) + "\n")


# ---- the run --------------------------------------------------------------------------------


def take_away(victim, by_id, control, lib):
    """Move away the file of a track a few items ahead (so it is not yet queued), whichever the
    log rotates to: the track is the victim of the missing-source window."""
    s, a = api("GET", f"/api/v1/schedule/log?from_ms={int(now() * 1000)}&to_ms={int(now() * 1000) + 3600_000}")
    upcoming = [i for i in (a["items"] if s == 200 else []) if i["status"] == "planned" and i["track"]]
    if not upcoming:
        log("the missing source: no planned item ahead to take the file of")
        return
    upcoming = [i for i in upcoming if i["track"] in by_id]  # never a keyed show's signature
    if not upcoming:
        log("the missing source: no planned item ahead to take the file of")
        return
    track = by_id[upcoming[min(3, len(upcoming) - 1)]["track"]]
    victim.update(track)
    sh("podman", "exec", control, "mv", f"{lib}/{track['file']}", f"{lib}/{track['file']}.away")
    log(f"the missing source: took away {track['title']} ({track['file']})")


# ---- keyed shows (1.3.3's run) --------------------------------------------------------------
#
# Two keyed shows, A and B, set to `key` through the studio's route and given episodes through
# the show routes, as staff would; their windows recur through the run (`plan_windows`), A and B
# in turn, one pair back to back. A keyed show's programme here is automation with a signature:
# shortly before each window the log items that will play wholly inside it (worked out from what
# is playing and queued now, since automation does not wait for planned times) are given the
# show's signature track, a chord of two tones that no scale holds (`keyed_ear.py`). Separate
# ears follow the three public mounts for the whole run and must never hear a signature; a key
# holder per window (the show's own key, its device kept) must hear its show's and be dropped
# after the window. Each show also has an aired episode with a file, which the feed must not
# list and the file route must not serve while the show is keyed (1.3.3); a public show's
# episode, served throughout, shows that check can see a file. NEGATIVE=1 plays a signature
# outside any window and opens playout's public gate in the middle of the last window: the
# public ears must then hear both, or the run fails.

NEGATIVE = E.get("NEGATIVE", "0") == "1"
SIGS = {"A": (1017, 2531), "B": (1433, 3061)}  # as keyed_ear.py listens for them
KEYED_SLUG = {"A": "soak-keyed-a", "B": "soak-keyed-b"}
PODCAST_SLUG = "soak-podcast"
PUBLIC_MOUNTS = ("programme.opus", "programme.mp3", "programme-high.mp3")
G1, G1_PAIR, G2 = 15, 50, 15  # signature items stay this far inside a window (the second of a pair: its holder starts late)
DROP_BOUND_S = 60  # a key holder is dropped within this after the window's end (tail, delay and the mount's burst)
REPO = os.path.dirname(os.path.dirname(HERE))


def plan_windows(plan):
    """The keyed windows as (show, start, end) offsets from T0, whole seconds, clear of the
    missing source, the end of the log before the gap, and the gap: a pair (A then B) first in
    the last stretch, then A and B in turn, `P` apart. Window `W` holds about two tracks."""
    T = TRACK_S
    W = max(60, math.ceil((2 * T + 30) / 60) * 60)
    P = max(60, 2 * T)
    lead = 3 * T + 40  # the signature items are chosen this long before a window
    segs = [(lead + 20, plan["miss_a"]),
            (plan["miss_a"] + plan["miss_len"] + lead, plan["gap1"] - plan["hole_len"] - 3 * T - 30),
            (plan["gap2"] + lead + 30, D - 120)]  # automation takes the air again a few seconds after the gap
    out, nxt = [], "A"
    for i, (a, b) in enumerate(segs):
        t = int(a)
        if i == len(segs) - 1 and t + 2 * W + 60 <= b:
            out += [dict(show="A", start=t, end=t + W, pair=1), dict(show="B", start=t + W, end=t + 2 * W + 60, pair=2)]
            t += 2 * W + 60 + P
            nxt = "A"
        while t + W <= b:
            out.append(dict(show=nxt, start=t, end=t + W, pair=0))
            nxt = "B" if nxt == "A" else "A"
            t += W + P
    return sorted(out, key=lambda w: w["start"]), dict(W=W, P=P, lead=lead)


def control_name(containers):
    return containers[[short(c) for c in containers].index("control")]


def seed_keyed(t0, windows, control):
    """The keyed shows, their episodes and keys, the public podcast show, and the feed's state
    before the rule (a control: the keyed shows' files are served while they are public)."""
    tag = str(int(t0))
    k = {"shows": {}, "files": {}, "keys": {}, "devices": {}, "before": {}}
    fixture = open(os.path.join(REPO, "crates/feeds/tests/fixtures/tone.mp3"), "rb").read()
    for sid, slug in list(KEYED_SLUG.items()) + [("P", PODCAST_SLUG)]:
        n_eps = 1 + sum(1 for w in windows if w["show"] == sid)
        must("PUT", f"/api/v1/shows/{slug}", {"name": f"Soak keyed {sid}" if sid != "P" else "Soak podcast", "colour_slot": 3,
                                             "hosts": ["Soak Host"], "description": "The soak run's keyed show." if sid != "P" else "A public show."})
        must("PUT", f"/api/v1/shows/{slug}/series/1", {})
        must("POST", f"/api/v1/shows/{slug}/series/1/episodes", {"count": n_eps})
        must("POST", f"/api/v1/shows/{slug}/series/1/episodes/1/air", {})
        name = f"{slug}-{tag}.mp3"
        r = subprocess.run(["podman", "exec", "-i", control, "sh", "-c", f"cat > /var/lib/weavesignal/episodes/{name}"], input=fixture, capture_output=True)
        if r.returncode:
            raise SystemExit(f"could not put the episode file in the control plane: {r.stderr[:300]}")
        must("PUT", f"/api/v1/shows/{slug}/series/1/episodes/1/files", {"enclosure": name})
        k["files"][sid] = name
        k["shows"][sid] = must("GET", f"/api/v1/shows/{slug}")
    k["shows"] = {s: (v.get("show") or v).get("id") for s, v in k["shows"].items()}
    k["before"] = feed_state(k)
    for sid, slug in KEYED_SLUG.items():
        must("PUT", f"/api/v1/shows/{slug}/access", {"rule": "key", "slate": "fallback"})
        made = must("POST", "/api/v1/listener-keys", {"label": f"Soak holder {sid}", "shows": [slug], "media": "sound"})
        k["keys"][sid] = made["text"]
    n = {"A": 1, "B": 1}
    for w in windows:
        n[w["show"]] += 1
        w["episode"] = n[w["show"]]
        must("PATCH", f"/api/v1/shows/{KEYED_SLUG[w['show']]}/series/1/episodes/{w['episode']}",
             {"air_at": iso(t0 + w["start"])[:-1] + "+00:00[UTC]", "minutes": (w["end"] - w["start"]) // 60})
    return k


def feed_state(k):
    """Each show's feed items and its episode file's answer, and whether the public list names it."""
    out = {}
    s, lst = api("GET", "/api/v1/public/episodes?limit=50", timeout=15)
    listed = json.dumps(lst) if s == 200 else ""
    for sid, slug in list(KEYED_SLUG.items()) + [("P", PODCAST_SLUG)]:
        s, xml = api("GET", f"/feeds/{slug}.xml", timeout=15)
        items = xml.count("<item>") if s == 200 and isinstance(xml, str) else None
        s2, _ = api("GET", f"/media/episodes/{k['files'][sid]}", timeout=15)
        out[sid] = {"feed": s, "items": items, "file": s2, "listed": k["files"][sid] in listed if s == 200 else None}
    return out


def plays_since(ms_from):
    s, pl = api("GET", f"/api/v1/reports/plays?from_ms={ms_from}&to_ms={int(now() * 1000) + 60000}", timeout=20)
    return pl["plays"] if s == 200 else []


def predict():
    """The items to come with the times they will play, from what is playing now and what is
    queued and planned after it: [(item, start s, end s)], or [] when automation is not playing."""
    nms = int(now() * 1000)
    s, a = api("GET", f"/api/v1/schedule/log?from_ms={nms - 3 * HOUR * 1000}&to_ms={nms + 6 * HOUR * 1000}", timeout=20)
    items = a["items"] if s == 200 else []
    plays = [p for p in plays_since(nms - 3 * HOUR * 1000) if p.get("item") is not None]
    if not plays or not items:
        return []
    cur = max(plays, key=lambda p: p["started_at_ms"])
    end = (cur["started_at_ms"] + cur["duration_ms"]) / 1000
    if end < now() - 5:
        return []  # nothing playing: the log has run out
    by = {i["id"]: i for i in items}
    after = by.get(cur["item"], {}).get("planned_at_ms", cur["started_at_ms"])
    out = []
    for i in sorted((i for i in items if i["status"] in ("queued", "planned") and i["planned_at_ms"] > after and i["id"] != cur["item"]),
                    key=lambda i: (i["planned_at_ms"], i["id"])):
        out.append((i, end, end + i["duration_ms"] / 1000))
        end += i["duration_ms"] / 1000
    return out


def give_signature(sig_track, lo, hi, limit=99):
    """Give the planned items that will play wholly within [lo, hi] (Unix s) the signature."""
    done = []
    for item, a, b in predict():
        if a >= lo and b <= hi and item["status"] == "planned" and item["kind"] == "track" and len(done) < limit:
            s, r = api("PUT", f"/api/v1/schedule/log/{item['id']}", {"track": sig_track["id"], "force": True})
            if s == 200:
                done.append(dict(item=item["id"], start=a, end=b, seconds=item["duration_ms"] / 1000))
            else:
                log(f"keyed: item {item['id']} kept its track ({s} {str(r)[:120]})")
    return done


def playout_say(helper, cmd):
    return helper.ask("P " + cmd)


class Keyed:
    def __init__(self, t0, windows, shape, k, sig_tracks, helper):
        self.t0, self.windows, self.shape, self.k, self.sig, self.helper = t0, windows, shape, k, sig_tracks, helper
        self.ears = {}
        self.feeds = []  # (elapsed, in a window, state)
        self.planted = None
        self.gate_fault = None
        self.threads = []

    def start_ears(self):
        for m in PUBLIC_MOUNTS:
            path = os.path.join(OUT, f"keyed-ear-{m}.jsonl")
            self.ears[m] = (subprocess.Popen([sys.executable, "-I", os.path.join(HERE, "keyed_ear.py"), "public", f"http://127.0.0.1:{LISTENERS_PORT}/{m}", path],
                                             stdout=subprocess.DEVNULL, stderr=open(path + ".err", "w")), path, now())

    def stop_ears(self):
        for m, (p, path, _) in self.ears.items():
            p.terminate()
        for m, (p, path, _) in self.ears.items():
            try:
                p.wait(timeout=20)
            except subprocess.TimeoutExpired:
                p.kill()
        self.ears_stopped = now()

    def start(self):
        for i, w in enumerate(self.windows):
            th = threading.Thread(target=self.window, args=(i, w), daemon=True)
            th.start()
            self.threads.append(th)
        if NEGATIVE:
            th = threading.Thread(target=self.plant, daemon=True)
            th.start()
            self.threads.append(th)

    def wait_until(self, t):
        while not STOP.is_set() and now() < t:
            STOP.wait(min(1.0, t - now()))
        return not STOP.is_set()

    def in_window(self, t):
        return any(self.t0 + w["start"] - 3 <= t < self.t0 + w["end"] + 15 for w in self.windows)

    def window(self, i, w):
        t0, sid = self.t0, w["show"]
        start, end = t0 + w["start"], t0 + w["end"]
        g1 = G1_PAIR if w["pair"] == 2 else G1
        w.update(items=[], holder=None, mid={})
        if not self.wait_until(start - self.shape["lead"]):
            return
        # Again every few seconds while automation is not playing yet (as just after the gap),
        # as long as the items in the window are some tracks away.
        while True:
            w["items"] = give_signature(self.sig[sid], start + g1, end - G2)
            if w["items"] or now() > start + g1 - 2 * TRACK_S - 15 or not self.wait_until(now() + 5):
                break
        log(f"keyed: window {i + 1} ({sid}, {fmt_t(w['start'])} to {fmt_t(w['end'])}): {len(w['items'])} items given the signature")
        if not self.wait_until(start - 75):
            return
        body = {"key": self.k["keys"][sid], "show": KEYED_SLUG[sid]}
        if self.k["devices"].get(sid):
            body["device"] = self.k["devices"][sid]
        s, g = api("POST", "/api/v1/keyed/redeem", body)
        if s != 200:
            w["holder"] = {"redeem": f"{s} {str(g)[:160]}"}
            log(f"keyed: window {i + 1}: the redeem was refused: {s} {str(g)[:160]}")
        else:
            self.k["devices"][sid] = g.get("device")
            path = os.path.join(OUT, f"keyed-holder-{i + 1}.jsonl")
            p = subprocess.Popen([sys.executable, "-I", os.path.join(HERE, "keyed_ear.py"), "holder", f"http://127.0.0.1:{CONTROL_PORT}{g['urls']['opus']}",
                                  path, str(start - 5), str(start + 75)], stdout=subprocess.DEVNULL, stderr=open(path + ".err", "w"))
            w["holder"] = {"path": path, "proc": p}
        if NEGATIVE and i == len(self.windows) - 1 and w["items"]:
            it = w["items"][0]
            fault = (it["start"] + it["end"]) / 2
            if self.wait_until(fault):
                for _ in range(8):
                    playout_say(self.helper, "keyed.window off")
                    time.sleep(0.5)
                self.gate_fault = dict(at=fault, window=i + 1)
                script_did(t0, f"NEGATIVE: opened playout's public gate in window {i + 1} (keyed.window off, eight times in 4 s)")
        if not self.wait_until((start + end) / 2):
            return
        s, v = api("GET", "/api/v1/studio", timeout=10)
        kv = (v or {}).get("keyed") if s == 200 and isinstance(v, dict) else None
        s, n = api("GET", "/api/v1/now-playing", timeout=10)
        s2, ks = api("GET", "/api/v1/public/keyed-shows", timeout=10)
        hls = {p: api("GET", f"/hls/{p}", timeout=10) for p in ("live.m3u8", "audio.m3u8")}
        w["mid"] = dict(studio=kv, np_kind=((n or {}).get("now") or {}).get("kind") if s == 200 else None,
                        on_now=ks.get("on_now") if s2 == 200 and isinstance(ks, dict) else None,
                        hls={p: (a[0], (a[1] or {}).get("code") if isinstance(a[1], dict) else str(a[1])[:60]) for p, a in hls.items()})
        p = (w["holder"] or {}).get("proc")
        if p:
            try:
                p.wait(timeout=max(5, end + DROP_BOUND_S + 90 - now()))
            except subprocess.TimeoutExpired:
                p.terminate()
                try:
                    p.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    p.kill()

    def plant(self):
        """NEGATIVE: a signature item wholly between two windows, where the public hears it."""
        gaps = [(self.t0 + a["end"] + 20, self.t0 + b["start"] - 10) for a, b in zip(self.windows, self.windows[1:]) if b["start"] - a["end"] >= TRACK_S + 40]
        if not gaps:
            log("NEGATIVE: no stretch between windows long enough to plant a signature")
            return
        lo, hi = gaps[len(gaps) // 2]
        if self.wait_until(lo - self.shape["lead"]):
            got = give_signature(self.sig["A"], lo, hi, limit=1)
            if got:
                self.planted = got[0]
                script_did(self.t0, f"NEGATIVE: gave an item outside any window show A's signature (to play from {fmt_t(got[0]['start'] - self.t0)})")

    def sample_feeds(self):
        self.feeds.append((now() - self.t0, self.in_window(now()), feed_state(self.k)))


def read_jsonl(path):
    rows = []
    try:
        for line in open(path):
            try:
                rows.append(json.loads(line))
            except ValueError:
                pass
    except OSError:
        pass
    return rows


def keyed_checks(check, kd, plays, t0, t_stop):
    """The keyed shows' criteria; each a line of RESULT.md. Returns the per-window table."""
    sig_ids = {t["id"]: s for s, t in kd.sig.items()}
    sig_plays = [p for p in plays if p["track"] in sig_ids]
    wins = kd.windows
    check("(information) keyed windows planned", None,
          f"{len(wins)} windows of {kd.shape['W']} s (the second of the pair {kd.shape['W'] + 60} s), {kd.shape['P']} s or more apart, "
          f"{sum(1 for w in wins if w['show'] == 'A')} of show A and {sum(1 for w in wins if w['show'] == 'B')} of B, one pair back to back"
          f"{' (none: the run is too short)' if not any(w['pair'] for w in wins) else ''}")
    # The script's own placing first: a signature that played outside its window is the test's fault.
    placed_bad, unplanned = [], []
    item_win = {it["item"]: (i, w) for i, w in enumerate(wins) for it in w.get("items", [])}
    planted = kd.planted["item"] if kd.planted else None
    for p in sig_plays:
        a, b = p["started_at_ms"] / 1000, (p.get("ended_at_ms") or p["started_at_ms"] + p["duration_ms"]) / 1000
        if p.get("item") == planted:
            continue
        if p.get("item") not in item_win:
            unplanned.append(f"{fmt_t(a - t0)}")
            continue
        i, w = item_win[p["item"]]
        if a < t0 + w["start"] or b > t0 + w["end"]:
            placed_bad.append(f"window {i + 1}: played {fmt_t(a - t0)} to {fmt_t(b - t0)}, window {fmt_t(w['start'])} to {fmt_t(w['end'])}")
    given = sum(len(w.get("items", [])) for w in wins)
    check("every signature item played wholly inside its window (the script's placing; outside, the run cannot judge the gate)",
          given > 0 and not placed_bad and not unplanned and all(w.get("items") for w in wins),
          f"{given} items given, {len(sig_plays) - (1 if planted else 0)} played; outside their window: {placed_bad[:4] or 'none'}; not the script's: {unplanned[:4] or 'none'}; "
          f"windows with none: {[i + 1 for i, w in enumerate(wins) if not w.get('items')] or 'none'}")
    # The public mounts.
    leaks, cover = [], []
    for m, (p, path, began) in kd.ears.items():
        rows = read_jsonl(path)
        runs = [r for r in rows if r["ev"] == "run"]
        drops = [r for r in rows if r["ev"] == "end"]
        decoded = max([r.get("decoded_s", 0) for r in rows if "decoded_s" in r] or [0])
        span = kd.ears_stopped - began
        cover.append((m, decoded, span, sum(1 for r in rows if r["ev"] == "connect"), drops))
        for r in runs:
            pt = r["t"] - 13  # the public hears playout about 13 s late
            where = next((f"in window {i + 1}" for i, w in enumerate(wins) if t0 + w["start"] - 5 <= pt <= t0 + w["end"] + 20), "outside any window")
            leaks.append((m, r, where))
    coverage_ok = all(d >= 0.98 * s and c == 1 and not dr for _, d, s, c, dr in cover)
    check("the public ears followed every public mount the whole run (decoded nearly all of it, never dropped)", coverage_ok,
          "; ".join(f"{m}: {d:.0f} of {s:.0f} s decoded, {c} connection(s), drops {[x['why'] for x in dr][:3] or 'none'}" for m, d, s, c, dr in cover))
    leak_text = "; ".join(f"{m} heard {r['sig']} for {r['seconds']} s at {fmt_t(r['t'] - t0)} ({where})" for m, r, where in leaks[:10]) or "none"
    if NEGATIVE:
        check("(information, NEGATIVE) signatures the public ears heard", None, leak_text)
        def caught(lo, hi):
            return all(any(lo <= r["t"] <= hi for mm, r, _ in leaks if mm == m) for m in PUBLIC_MOUNTS)
        pl = kd.planted
        check("NEGATIVE: the signature played outside any window was heard on every public mount", bool(pl) and caught(pl["start"], pl["end"] + 60),
              f"planted at {fmt_t(pl['start'] - t0)} to {fmt_t(pl['end'] - t0)}" if pl else "nothing was planted")
        gf = kd.gate_fault
        check("NEGATIVE: the public gate opened mid-window was heard on every public mount", bool(gf) and caught(gf["at"], gf["at"] + 60),
              f"opened at {fmt_t(gf['at'] - t0)} in window {gf['window']}" if gf else "the gate was not opened (the last window had no signature item)")
    else:
        check("no public mount (Opus, MP3, high MP3) ever carried a keyed show's signature (decoded; about 100 ms or more is caught)",
              not leaks and coverage_ok, leak_text if leaks else f"none in {len(sig_plays)} signature items of {sum(TRACK_S for _ in sig_plays)} s")
    # The key holders.
    rows_out, bad_hold, bad_drop, bad_after, heard_other = [], [], [], [], []
    for i, w in enumerate(wins):
        h = w.get("holder") or {}
        rows = read_jsonl(h["path"]) if h.get("path") else []
        con = next((r for r in rows if r["ev"] == "connect"), None)
        end_ = next((r for r in rows if r["ev"] == "end"), None)
        after = next((r for r in rows if r["ev"] == "after"), None)
        tries = [f"{r['status']} {r.get('code')}" for r in rows if r["ev"] == "try"]
        own = sum(r["seconds"] for r in rows if r["ev"] == "run" and r["sig"] == w["show"])
        other = sum(r["seconds"] for r in rows if r["ev"] == "run" and r["sig"] != w["show"])
        expect = sum(it["seconds"] for it in w.get("items", []))
        s, e = t0 + w["start"], t0 + w["end"]
        conn_at = con["t"] - s if con else None
        drop_after = end_["t"] - e if end_ else None
        row = dict(n=i + 1, show=w["show"], pair=w["pair"], start=w["start"], end=w["end"], items=len(w.get("items", [])), expect=expect,
                   heard=round(own, 1), other=round(other, 1), conn=conn_at, drop=drop_after, after=(after or {}).get("status"),
                   after_code=(after or {}).get("code"), tries=tries, redeem=h.get("redeem"), mid=w.get("mid", {}))
        rows_out.append(row)
        if NEGATIVE and kd.gate_fault and kd.gate_fault["window"] == i + 1:
            continue  # the faulted window's own figures are not judged
        if not con or conn_at > 60 or expect <= 0 or own < 0.8 * expect:
            bad_hold.append(f"{i + 1}: " + (h.get("redeem") or f"connected {conn_at if conn_at is None else round(conn_at, 1)} s after the start, heard {own:.1f} of {expect:.0f} s, tries {tries[:3]}"))
        if other > 0:
            heard_other.append(f"{i + 1}: {other:.1f} s")
        if not end_ or drop_after is None or drop_after < 0 or drop_after > DROP_BOUND_S:
            bad_drop.append(f"{i + 1}: " + (f"ended {drop_after:+.1f} s from the window's end ({end_['why']})" if end_ else "never ended"))
        if not after or after.get("status") == 200:
            bad_after.append(f"{i + 1}: {after}")
    drops = [r["drop"] for r in rows_out if r["drop"] is not None]
    check("a key holder heard each window's keyed show (connected within 60 s of its start, at least 80 % of its signature items)", not bad_hold,
          f"{len(wins) - len(bad_hold)} of {len(wins)} windows; " + ("; ".join(bad_hold[:6]) if bad_hold else "ok"))
    check("no key holder heard the other keyed show (the pair's change included)", not heard_other, "; ".join(heard_other) or "none")
    check(f"each key holder was dropped after its window, within {DROP_BOUND_S} s of its end", not bad_drop,
          (f"after the end: min {min(drops):.1f} s, max {max(drops):.1f} s; " if drops else "") + ("; ".join(bad_drop[:6]) if bad_drop else "all"))
    check("after the drop, the same token was refused", not bad_after, "; ".join(bad_after[:6]) or
          f"answers {sorted({(r['after'], r['after_code']) for r in rows_out if r['after']}, key=str)}")
    # The station's own view in each window.
    mids = [(r["n"], r["mid"]) for r in rows_out]
    bad_mid = [f"{n}: {m}" for n, m in mids if not (m.get("studio") or {}).get("kept") or not (m.get("studio") or {}).get("confirmed")
               or (m.get("studio") or {}).get("show") != kd.k["shows"][wins[n - 1]["show"]] or m.get("np_kind") != "show"
               or m.get("on_now") != kd.k["shows"][wins[n - 1]["show"]]]
    check("mid-window, the studio's view said playout kept the right show from the public (kept, confirmed), now playing named only the show, "
          "and the public keyed-show list had it on now",
          not bad_mid and bool(mids), "; ".join(bad_mid[:4]) or f"{len(mids)} windows, public hears {sorted({str((m.get('studio') or {}).get('public_hears')) for _, m in mids})}")
    hls = sorted({str(v) for _, m in mids for v in (m.get("hls") or {}).values()})
    check("(information) the public HLS mid-window", None,
          f"answers {hls}: video is off in this run, so /hls/ serves nothing at any time and its keyed hold is not exercised here (tests/keyed and the edge tests cover it)")
    # The podcast feed and files (1.3.3).
    before = kd.k["before"]
    check("before the rule: each keyed show's feed listed its episode and its file answered 200 (so the next check can see one)",
          all(before[s]["items"] == 1 and before[s]["file"] == 200 for s in ("A", "B", "P")), f"{before}")
    bad = [(round(e), s, st[s]) for e, _, st in kd.feeds for s in ("A", "B") if st[s]["feed"] != 200 or st[s]["items"] != 0 or st[s]["file"] != 404 or st[s]["listed"]]
    pub_bad = [(round(e), st["P"]) for e, _, st in kd.feeds if st["P"]["items"] != 1 or st["P"]["file"] != 200 or not st["P"]["listed"]]
    in_win = sum(1 for _, inw, _ in kd.feeds if inw)
    check("while keyed, the keyed shows' feeds listed no episode, the public list left them out and their files answered 404, at every sample",
          bool(kd.feeds) and not bad, f"{len(kd.feeds)} samples ({in_win} in a window); wrong at {bad[:4] or 'none'}")
    check("the public show's feed, list entry and file were served at every sample (the check is not blind)", bool(kd.feeds) and not pub_bad,
          f"{len(kd.feeds)} samples; wrong at {pub_bad[:4] or 'none'}")
    return rows_out


def keyed_lines(rows):
    out = ["| # | show | from | to | signature items (s) | holder connected (s after start) | heard (s) | other show (s) | dropped (s after end) | then | studio mid-window |",
           "|---|---|---|---|---|---|---|---|---|---|---|"]
    for r in rows:
        st = (r["mid"] or {}).get("studio") or {}
        out.append(f"| {r['n']}{' (pair ' + str(r['pair']) + ')' if r['pair'] else ''} | {r['show']} | {fmt_t(r['start'])} | {fmt_t(r['end'])} | {r['items']} ({r['expect']:.0f}) | "
                   f"{'-' if r['conn'] is None else round(r['conn'], 1)} | {r['heard']} | {r['other']} | {'-' if r['drop'] is None else round(r['drop'], 1)} | "
                   f"{r['after']} {r['after_code'] or ''} | kept {st.get('kept')}, confirmed {st.get('confirmed')}, public hears {st.get('public_hears')} |"
                   + (f" redeem: {r['redeem']}" if r["redeem"] else ""))
    return out


def main():
    signal.signal(signal.SIGINT, lambda *_: STOP.set())
    signal.signal(signal.SIGTERM, lambda *_: STOP.set())
    containers = [c for c in sh("podman", "ps", "-a", "--filter", f"label=com.docker.compose.project={PROJECT}", "--format", "{{.Names}}").split() if c]
    if not containers:
        raise SystemExit("no containers in the project")
    log(f"soak: {D}s, sampling every {SAMPLE_S}s, project {PROJECT}, {len(containers)} containers, tracks {NTRACKS} x {TRACK_S}s")
    tracks = seed_station()
    log(f"seeded: {len(tracks)} tracks, one clock on every hour of the week, retention uk_internet, mode auto")
    sig_tracks = {s: must("PUT", f"/api/v1/library/tracks?title=Soak%20keyed%20{s}&artist=Soak%20Keyed%20{s}&album=Soak%20Keyed&label=Soak&category=k",
                          raw=tone_wav(SIGS[s], TRACK_S)) for s in SIGS}
    log(f"seeded: the keyed shows' signatures {SIGS} as tracks in category k, which no clock names")
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
    control = control_name(containers)
    windows, shape = plan_windows(dict(miss_a=miss_a, miss_len=miss_len, gap1=gap1, gap2=gap2, hole_len=hole_len))
    kd = Keyed(t0, windows, shape, seed_keyed(t0, windows, control), sig_tracks, helper)
    script_did(t0, f"keyed shows: A and B set to `key`, {len(windows)} windows on the schedule ({', '.join(w['show'] + ' ' + fmt_t(w['start']) + '-' + fmt_t(w['end']) for w in windows)}), "
                   f"a key each, an aired episode with a file each, and a public show with one")
    kd.start_ears()
    kd.start()

    obs = Observer(helper, t0, containers)
    obs.sample()
    threading.Thread(target=obs.fast, daemon=True).start()
    victim = {}
    by_id = {t["id"]: t for t in tracks}
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
            kd.sample_feeds()
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
    kd.sample_feeds()
    for th in kd.threads:
        th.join(timeout=5 if interrupted else 120)
    kd.stop_ears()
    STOP.set()
    listener.finish()
    time.sleep(1.5)
    finish(obs, listener, helper, containers, tracks, victim, t0, t_stop, interrupted, kd, dict(
        gap1=gap1, gap2=gap2, hole_a=hole_a, hole_len=hole_len, miss_a=miss_a, miss_len=miss_len, resumed_at=resumed_at, seed_plan=seed_plan, shows=shows, retained=retained))
    helper.close()

# ---- memory: the verdicts and the tables, from saved samples (also `--memory-report`) -------

MIB = 1048576


def grew(start, end):
    """The run's one growth rule for every memory series: more than 20 % and more than 20 MiB."""
    return end > start * 1.2 and end - start > 20


def series_start(series, base_at):
    """The reading at the baseline time (after the stack has settled), else the first one."""
    return next((v for e, v in series if e >= base_at), series[0][1])


def memory_analysis(rows, cg_rows, duration):
    """`rows` are the samples' rows (`mem` is `podman stats`, MiB); `cg_rows` the cgroup readings
    (bytes). Returns the baseline time, the `podman stats` lines, and for each
    container the start, peak and end (MiB) of memory.current (`total`), `anon`, `file`, `kernel`
    and `shmem`, with whether each grew by the rule above."""
    base_at = min(180, 0.15 * duration)
    ws = []
    for n in sorted({k for r in rows for k in r["mem"]}):
        series = [(r["e"], r["mem"][n]) for r in rows if n in r["mem"]]
        start, peak, end = series_start(series, base_at), max(v for _, v in series), series[-1][1]
        ws.append((n, start, peak, end, grew(start, end)))
    cg = {}
    for n in sorted({k for r in cg_rows for k in r["cg"]}):
        cg[n] = {}
        for label, key in (("total", "current"), ("anon", "anon"), ("file", "file"), ("active file", "active_file"), ("kernel", "kernel"), ("shmem", "shmem")):
            series = [(r["e"], r["cg"][n][key] / MIB) for r in cg_rows if key in r["cg"].get(n, {})]
            if series:
                start, peak, end = series_start(series, base_at), max(v for _, v in series), series[-1][1]
                cg[n][label] = (start, peak, end, grew(start, end))
    return {"base_at": base_at, "ws": ws, "cg": cg, "cg_samples": len(cg_rows),
            "cg_failed": sum(1 for r in cg_rows if r.get("err"))}


def memory_verdicts(a):
    """(total flagged, anon flagged, file flagged, flagged by total but not by anon) as name lists."""
    total = [n for n, _, _, _, f in a["ws"] if f]
    anon = [n for n, d in a["cg"].items() if d.get("anon", (0, 0, 0, False))[3]]
    file_ = [n for n, d in a["cg"].items() if d.get("file", (0, 0, 0, False))[3]]
    return total, anon, file_, [n for n in total if n not in anon]


def memory_lines(a, cg_last_err=None):
    """The Markdown for RESULT.md's memory section."""
    out = ["| container | start | peak | end | grew |", "|---|---|---|---|---|"]
    out += [f"| {n} | {s} | {p} | {e} | {'**yes**' if f else 'no'} |" for n, s, p, e, f in a["ws"]]
    out += ["", "Container memory as `podman stats` reports it (`memory.current` minus inactive file cache), MiB: it counts active page cache, so the verdict reads the cgroup table below. "
            f"Baseline at {a['base_at']:.0f} s.", "",
            "### The same, split by the cgroup's own counters (read on the host; `memstat.jsonl`)", ""]
    if not a["cg"]:
        out += [f"**No cgroup readings** ({a['cg_failed']} of {a['cg_samples']} samples failed" + (f"; last: {cg_last_err}" if cg_last_err else "") + ").", ""]
        return out
    out += ["| container | counter | start | peak | end | growth MiB | growth % |", "|---|---|---|---|---|---|---|"]
    for n, d in a["cg"].items():
        for label in ("total", "anon", "file", "active file"):
            if label in d:
                s_, p_, e_, f_ = d[label]
                pc = f"{(e_ - s_) / s_ * 100:+.0f} %" if s_ else "-"
                out.append(f"| {n} | {label}{' (memory.current)' if label == 'total' else ''} | {s_:.1f} | {p_:.1f} | {e_:.1f} | {e_ - s_:+.1f} | {pc}{' **over the rule**' if f_ else ''} |")
    out += ["", "`total` is `memory.current`; `anon` is the processes' own memory (heap, stacks); `file` is page cache, the kernel's to take back under pressure unless it is dirty or mapped; `active file` is the part of it recently used (podman's figure leaves out only the inactive part, so growth in active file cache shows in the first table). MiB.", "",
            "| container | kernel start | kernel end | shmem start | shmem end |", "|---|---|---|---|---|"]
    for n, d in a["cg"].items():
        k, sh_ = d.get("kernel"), d.get("shmem")
        out.append(f"| {n} | {k[0]:.1f} | {k[2]:.1f} | {sh_[0]:.1f} | {sh_[2]:.1f} |" if k and sh_ else f"| {n} | - | - | - | - |")
    out.append("")
    out.append(f"Cgroup readings: {a['cg_samples'] - a['cg_failed']} of {a['cg_samples']} samples complete" + (f"; **{a['cg_failed']} had a failure** (last: {cg_last_err})" if a["cg_failed"] else "") + ".")
    out.append("")
    return out


def memory_check_texts(a):
    """The two lines: (name, ok, detail). The verdict reads each container's own memory, the cgroup's
    `anon` and `shmem`; page cache is the kernel's to take back, and it moving from inactive to
    active made a flat process look as if it had grown in `podman stats` (2026-11-18). That figure
    stays beside it as information. With too few cgroup readings the verdict falls back on it."""
    total, anon, file_, only_total = memory_verdicts(a)
    own = sorted(set(anon) | {n for n, d in a["cg"].items() if d.get("shmem", (0, 0, 0, False))[3]})
    podman = (f"baseline at {a['base_at']:.0f} s; grew: {total or 'none'}; of those, own memory did not grow in: "
              f"{[n for n in only_total if n not in own] or 'none'}; page cache (`file`) grew: {file_ or 'none'}")
    complete = a["cg_samples"] - a["cg_failed"]
    if not a["cg"] or complete < 0.9 * a["cg_samples"]:
        return (("no container's memory grew by more than 20 % (and 20 MiB) from its start reading to its end "
                 "(`podman stats`: too few cgroup readings for the usual test)", not total, podman),
                ("(information) the cgroup's own-memory test could not be made", None,
                 f"readings complete at {complete} of {a['cg_samples']} samples; see the log's WARNING lines"))
    return (("no container's own memory (`anon` and `shmem`) grew by more than 20 % (and 20 MiB) from its start reading to its end", not own,
             f"baseline at {a['base_at']:.0f} s; grew: {own or 'none'}; readings complete at {complete} of {a['cg_samples']} samples"),
            ("(information) the same test on `podman stats`, which counts active page cache (not part of the verdict)", None, podman))


def memory_report(out):
    """`soak.py --memory-report <run folder>`: the memory section again from `samples.jsonl`,
    `memstat.jsonl` and `memory-meta.json`, printed and saved as `memory-recomputed.md`."""
    def load(name):
        rows = []
        try:
            for line in open(os.path.join(out, name)):
                try:
                    rows.append(json.loads(line))
                except ValueError:
                    pass  # a line cut short by a stopped run
        except OSError as e:
            print(f"cannot read {name}: {e}", file=sys.stderr)
        return rows

    rows, cg_rows = load("samples.jsonl"), load("memstat.jsonl")
    if not rows:
        sys.exit("no samples")
    try:
        duration = json.load(open(os.path.join(out, "memory-meta.json")))["duration_s"]
    except (OSError, ValueError, KeyError):
        duration = rows[-1]["e"]
        print(f"no memory-meta.json: taking the run's length as {duration:.0f} s", file=sys.stderr)
    a = memory_analysis(rows, cg_rows, duration)
    last_err = next((r["err"] for r in reversed(cg_rows) if r.get("err")), None)
    t, n = memory_check_texts(a)
    text = ["## Verdicts", "", f"- **{'PASS' if t[1] else 'FAIL'}** {t[0]}: {t[2]}", f"- **INFO** {n[0]}: {n[2]}", "", "## Memory (MiB)", ""] + memory_lines(a, last_err)
    open(os.path.join(out, "memory-recomputed.md"), "w").write("\n".join(text))
    print("\n".join(text))



# ---- the end: checks and RESULT.md ----------------------------------------------------------


def pct(vals, p):
    v = sorted(vals)
    return v[min(len(v) - 1, int(len(v) * p))] if v else None


def fmt_t(sec):
    sec = int(sec)
    return f"{sec // 3600}:{sec % 3600 // 60:02d}:{sec % 60:02d}"


def keyed_mutes(containers):
    """The control plane's `keyed.mute`s: (Unix s, seconds) of each change of keyed show."""
    out = subprocess.run(["podman", "logs", control_name(containers)], capture_output=True, text=True)
    mutes = []
    for line in (out.stdout + out.stderr).splitlines():
        if "silent for a change of show" in line:
            try:
                j = json.loads(line)
                at = datetime.datetime.fromisoformat(j["timestamp"].replace("Z", "+00:00")).timestamp()
                mutes.append((at, float(j["secs"])))
            except (ValueError, KeyError):
                pass
    return mutes


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
            at = [float(x) for x in re.findall(r"silence_start: (-?[\d.]+)", ff.stderr)]
            if "silence_start" in ff.stderr and len(sil) < ff.stderr.count("silence_start"):
                sil.append(-1.0)  # silent to the end of the file
            files.append(dict(name=f"{day}/{f}", start=start, bytes=os.path.getsize(path), duration=dur, silences=sil, silence_at=at))
    shutil.rmtree(dest, ignore_errors=True)
    return files, r.returncode


def finish(obs, L, helper, containers, tracks, victim, t0, t_stop, interrupted, kd, plan):
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
    # The keyed feed, which the logger records, is silent on purpose at a change from one keyed
    # show to the next until the first show's key holders are dropped (`keyed.mute`, about the
    # tail, the desk's delay and the mount's burst: 31 s on 2026-11-27): a silence inside one of
    # those, as the control plane logged it, is set aside and listed; any other fails as before.
    mutes = keyed_mutes(containers)
    excused = []
    bad = []
    for i, f in enumerate(files):
        nxt = files[i + 1]["start"] if i + 1 < len(files) else None
        if nxt is not None and f["duration"] is not None and abs(f["duration"] - (nxt - f["start"])) > 10:
            bad.append(f"{f['name']} is {f['duration']:.0f}s of audio for {nxt - f['start']:.0f}s")
        if f["bytes"] <= 0 or f["duration"] is None:
            bad.append(f"{f['name']} unreadable")
        rest = []
        for j, d in enumerate(f["silences"]):
            a = f["start"] + f["silence_at"][j] if j < len(f["silence_at"]) else None
            m = next((m for m in mutes if a is not None and d > 0 and m[0] - 3 <= a and a + d <= m[0] + m[1] + 5), None)
            if m:
                excused.append(f"{f['name']} {d:.1f} s from {fmt_t(a - t0)} (keyed.mute of {m[1]:.1f} s from {fmt_t(m[0] - t0)})")
            else:
                rest.append(d)
        if rest:
            bad.append(f"{f['name']} has silence {rest}")
    check("each logger file is real audio of its hour's length with no silence of 3 s (decoded), but for the keyed feed's silence at a change of keyed show",
          not bad and bool(files),
          "; ".join(f"{f['name']} {f['bytes'] / 1e6:.1f} MB {f['duration'] or 0:.0f}s" for f in files) if not bad else "; ".join(bad[:6]))
    check("(information) silences in the logger files that were the keyed feed's silence at a change of keyed show (`keyed.mute`, logged by the control plane)",
          None, "; ".join(excused) or f"none ({len(mutes)} changes of keyed show logged)")
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
    # 5b keyed shows
    keyed_rows = keyed_checks(check, kd, plays, t0, t_stop)
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
    # 8 memory: the verdict reads each container's own memory (cgroup anon and shmem)
    mem_a = memory_analysis(rows, obs.cg_rows, D)
    mem_total, mem_anon = memory_check_texts(mem_a)
    check(*mem_total)
    check(*mem_anon)
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
    write_result(obs, L, checks, mem_a, files, noise, plan, victim, t0, t_stop, interrupted, items, plays, drift_pre, keyed_rows)


def write_result(obs, L, checks, mem_a, files, noise, plan, victim, t0, t_stop, interrupted, items, plays, drift, keyed_rows):
    fails = [c for c in checks if c[1] is False]
    if NEGATIVE:  # a negative control: only its own lines decide
        fails = [c for c in checks if c[1] is False and c[0].startswith("NEGATIVE")]
    out = [f"# Soak run {iso(t0)}", "",
           f"- Station image: `{IMAGE}`; project `{PROJECT}`; ran {fmt_t(t_stop)} of {fmt_t(D)} asked{' (INTERRUPTED)' if interrupted else ''}; sampled every {SAMPLE_S} s ({len(obs.rows)} samples in `samples.jsonl`).",
           f"- Plan: {plan['seed_plan']}. Shows seeded: {plan['shows']}.",
           f"- Library: {NTRACKS} tones of {TRACK_S} s on one clock, mode auto, separations off, retention `uk_internet` (per-play log kept {plan['retained']['per_play_days']} days).",
           f"- **Result: {'FAIL (' + str(len(fails)) + ')' if fails or interrupted else 'PASS'}**"
           + (" of the NEGATIVE control (NEGATIVE=1: a signature played outside a window and playout's public gate opened mid-window on purpose; "
              "only the NEGATIVE lines decide, and the others are as they fell)" if NEGATIVE else ""), "", "## Checks", ""]
    for name, ok, detail in checks:
        out.append(f"- **{'PASS' if ok else 'FAIL' if ok is False else 'INFO'}** {name}: {detail}")
    out += ["", "## What the script did to the station (script-caused)", ""]
    out += [f"- {fmt_t(t) if t is not None else '-'} {w}" for t, w in NOTES]
    out += ["", "## What was on air (playout, now-playing, mode, desk connected), from the 2 s poll", ""]
    for t, a, n, m, d in obs.timeline:
        out.append(f"- {fmt_t(t)} {a} / {n} / {m} / desk {d}")
    out += ["", "## Keyed windows (`keyed-holder-N.jsonl`, `keyed-ear-*.jsonl`)", ""] + keyed_lines(keyed_rows)
    out += ["", "## Memory (MiB)", ""] + memory_lines(mem_a, obs.cg_last_err)
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
    memory_report(OUT) if REPORT_ONLY else main()
