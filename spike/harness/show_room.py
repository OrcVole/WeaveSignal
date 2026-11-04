# The Blether through the room: the talk-show fixture (spike/talkshow) played into LiveKit, one participant per voice,
# mixed by a desk, sent over SRT to Liquidsoap and icecast.server, and every link recorded. Python by exception, like
# desk_spike.py: a measurement harness, not the desk (the desk proper is gstreamer-rs). Runs in the desk image.
#
# usage: show_room.py <talkshow dir> <out dir> [jitterbuffer latency ms] [--live]
#   Each voice's stem (out/stems/<participant>.wav) is published by its own livekitwebrtcsink. Playout's stem (music
#   and news) joins the desk locally, as Liquidsoap's would. The desk subscribes to every voice and builds:
#     the programme: every voice except the screener; a caller only from his "caller to air" event;
#     one mix-minus per voice: the programme without that voice; callers also hear the screener.
#   It publishes each mix-minus as desk-mm-<participant>, and an "ear" per voice subscribes to it, so what each
#   person hears is recorded after a real round trip. The programme goes over SRT to playout.liq, and a listener
#   records icecast.server's stream.
#   --live: the operator presents from the browser page as "host"; the host stem is not published.
#     (Turn-taking for the other voices in the live run is not built yet: they play on the script's clock.)
# Writes <out dir>/<tap>.wav (48 kHz stereo) and report.json (start times, links, latencies, own-voice leakage).
import json, os, sys, time, wave
import numpy as np
import gi
gi.require_version("Gst", "1.0")
from gi.repository import Gst, GLib

show_dir, out_dir = sys.argv[1], sys.argv[2]
jb_ms = int(sys.argv[3]) if len(sys.argv) > 3 and sys.argv[3].isdigit() else 50
live = "--live" in sys.argv
os.makedirs(out_dir, exist_ok=True)
WS, KEY, SECRET = "ws://127.0.0.1:7880", "devkey", "secret-secret-secret-secret-secret0"
ROOM = "blether" if live else f"blether-{int(time.time())}"
RATE, FR = 48000, 480  # 10 ms pushes
SETTLE, LISTEN_AT, SHOW_AT = 5, 30, 40  # seconds after start: links settle, listener joins, the show begins
Gst.init(None)

tl = json.load(open(f"{show_dir}/out/timeline.json"))
show = json.load(open(f"{show_dir}/show.json"))
VOICES = sorted({c["participant"] for c in tl["cues"] if c["kind"] == "speech"} - {"playout"})  # the newsreader is playout
SCREENER = "screener-moira"
CALLERS = sorted({c["participant"] for c in tl["cues"] if c["kind"] == "speech" and c["who"] in ("caller", "farmer")})
# A caller goes on air at the "caller to air" event that precedes his first on-air line.
on_air_at = {}
for c in tl["cues"]:
    if c["kind"] == "speech" and c["participant"] in CALLERS and c["route"] == "air":
        on_air_at.setdefault(c["participant"], max(e["start"] for e in tl["cues"]
                                                   if e["kind"] == "event" and e["what"] == "caller to air" and e["start"] <= c["start"]))
PUBLISH = [v for v in VOICES if not (live and v == "host")]


def load(p):
    with wave.open(f"{show_dir}/out/stems/{p}.wav") as w:
        assert w.getframerate() == RATE and w.getnchannels() == 2
        return np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16)


stems = {p: load(p) for p in PUBLISH + ["playout"]}
RAW = f"audio/x-raw,format=S16LE,rate={RATE},channels=2,layout=interleaved"


def sig(identity, producer=None):
    s = (f'signaller::ws-url={WS} signaller::api-key={KEY} signaller::secret-key={SECRET} '
         f'signaller::room-name={ROOM} signaller::identity={identity} signaller::participant-name={identity}')
    return s + (f' signaller::producer-peer-id={producer}' if producer else "")


# --- recording: every tap streams to its own WAV, first arrival time noted -----------------------------------
taps = {}


def tap(name):
    return f"queue ! audioconvert ! audioresample ! {RAW} ! appsink name={name} emit-signals=true sync=false max-buffers=0 drop=false"


def on_sample(sink, name):
    buf = sink.emit("pull-sample").get_buffer()
    t = time.monotonic()
    ok, info = buf.map(Gst.MapFlags.READ)
    if ok:
        if name not in taps:
            w = wave.open(f"{out_dir}/{name}.wav", "wb"); w.setnchannels(2); w.setsampwidth(2); w.setframerate(RATE)
            taps[name] = {"w": w, "first": t - len(info.data) / 4 / RATE, "frames": 0}
        taps[name]["w"].writeframes(info.data); taps[name]["frames"] += len(info.data) // 4
        buf.unmap(info)
    return Gst.FlowReturn.OK


# --- the pipeline -----------------------------------------------------------------------------------------------
def src(name):
    return f"appsrc name={name} is-live=true format=time do-timestamp=true block=false min-latency=10000000 max-latency=40000000 caps=\"{RAW}\""


def bed(mixer):  # a silent live bed, so every mix outputs from its first moment (spike 01)
    return f"audiotestsrc is-live=true wave=silence samplesperbuffer=480 ! {RAW} ! queue ! {mixer}.\n"


MIXES = ["prog"] + [f"mm_{p}" for p in VOICES if p != SCREENER]
desc = ""
for p in PUBLISH:
    desc += f"{src('in_' + p)} ! {RAW} ! queue ! livekitwebrtcsink name=pub_{p} {sig(p)}\n"
    desc += f"livekitwebrtcsrc name=src_{p} {sig('desk-' + p, p)}\n"
if live:
    desc += f"livekitwebrtcsrc name=src_host {sig('desk-host', 'host')}\n"
desc += f"{src('in_playout')} ! {RAW} ! queue ! tee name=t_playout\n"
desc += f"audiomixer name=prog output-buffer-duration=10000000 ! {RAW} ! tee name=tP\n" + bed("prog")
desc += f"tP. ! {tap('programme')}\n"
desc += ("tP. ! queue leaky=downstream max-size-buffers=0 max-size-bytes=0 max-size-time=500000000 ! audioconvert ! "
         "opusenc bitrate=96000 frame-size=20 ! matroskamux streamable=true ! queue leaky=downstream max-size-buffers=0 "
         "max-size-bytes=0 max-size-time=500000000 ! srtsink uri=srt://127.0.0.1:9000?mode=caller latency=20 "
         "wait-for-connection=false sync=false\n")
for p in VOICES:
    if p == SCREENER:
        continue
    desc += f"audiomixer name=mm_{p} output-buffer-duration=10000000 ! {RAW} ! tee name=tmm_{p}\n" + bed(f"mm_{p}")
    desc += f"tmm_{p}. ! queue ! livekitwebrtcsink name=pubmm_{p} {sig('desk-mm-' + p)}\n"
    if not (live and p == "host"):
        desc += f"livekitwebrtcsrc name=ear_{p} {sig('ear-' + p, 'desk-mm-' + p)}\n"
pipe = Gst.parse_launch(desc)
pipe.get_by_name("programme").connect("new-sample", on_sample, "programme")

# Gates: a volume per (source, mix) link, so a caller can be held off air and the screener kept out of the programme.
gates = {}  # source -> [volume elements]


def route(src_tee_or_pad, source, is_pad):
    """Link one source to the programme and to every mix-minus it belongs in, through a gate each."""
    dests = []
    if source == SCREENER:
        dests = [f"mm_{c}" for c in CALLERS]
    else:
        dests = ["prog"] + [f"mm_{p}" for p in VOICES if p not in (SCREENER, source)]
    chain = Gst.parse_bin_from_description(f"audioconvert ! audioresample ! {RAW} ! tee name=t", True)
    pipe.add(chain); chain.sync_state_with_parent()
    if is_pad:
        src_tee_or_pad.link(chain.get_static_pad("sink"))
    else:
        q0 = Gst.ElementFactory.make("queue"); pipe.add(q0); q0.sync_state_with_parent()
        src_tee_or_pad.link(q0); q0.link(chain)
    tee = chain.get_by_name("t")
    targets = ([f"sink:recv_{source}"] if source != "playout" else []) + dests
    for target in targets:
        q = Gst.ElementFactory.make("queue"); chain.add(q); q.sync_state_with_parent(); tee.link(q)
        gp = Gst.GhostPad.new(None, q.get_static_pad("src")); gp.set_active(True); chain.add_pad(gp)
        if target.startswith("sink:"):
            sink = Gst.parse_bin_from_description(tap(target[5:]), True)
            pipe.add(sink); sink.sync_state_with_parent(); gp.link(sink.get_static_pad("sink"))
            sink.get_by_name(target[5:]).connect("new-sample", on_sample, target[5:])
        else:
            vol = Gst.ElementFactory.make("volume")
            if source in CALLERS:
                vol.set_property("mute", True)  # off air until the screener sends him to the desk
            pipe.add(vol); vol.sync_state_with_parent(); gp.link(vol.get_static_pad("sink"))
            vol.get_static_pad("src").link(pipe.get_by_name(target).request_pad_simple("sink_%u"))
            gates.setdefault(source, []).append((target, vol))
    log(f"routed {source} -> {targets}")


linked = set()


def log(m):
    print(f"[{time.monotonic() - t_start:8.2f}] {m}", flush=True)


def on_pad(_el, pad, source, kind):
    if not pad.get_name().startswith("audio"):
        fs = Gst.ElementFactory.make("fakesink"); pipe.add(fs); fs.sync_state_with_parent(); pad.link(fs.get_static_pad("sink")); return
    if kind == "voice":
        route(pad, source, True)
    else:  # an ear: what <source> hears, after the round trip
        sink = Gst.parse_bin_from_description(tap(f"ear_{source}"), True)
        pipe.add(sink); sink.sync_state_with_parent(); pad.link(sink.get_static_pad("sink"))
        sink.get_by_name(f"ear_{source}").connect("new-sample", on_sample, f"ear_{source}")
        log(f"ear {source} linked")
    linked.add((kind, source))


t_start = time.monotonic()
for p in PUBLISH + (["host"] if live else []):
    pipe.get_by_name(f"src_{p}").connect("pad-added", on_pad, p, "voice")
for p in VOICES:
    e = pipe.get_by_name(f"ear_{p}")
    if e:
        e.connect("pad-added", on_pad, p, "ear")
route(pipe.get_by_name("t_playout"), "playout", False)


def deep_set(bin_, prop, val):  # the receive jitter buffer, set inside LiveKit's bins (spike 01)
    def apply(el):
        f = el.get_factory()
        if f and f.get_name() in ("webrtcbin", "rtpbin", "rtpjitterbuffer"):
            el.set_property(prop, val)
    def on_added(_b, _sub, el):
        apply(el)
        if isinstance(el, Gst.Bin):
            it = el.iterate_recurse()
            while True:
                r, child = it.next()
                if r != Gst.IteratorResult.OK: break
                apply(child)
    bin_.connect("deep-element-added", on_added)


deep_set(pipe, "latency", jb_ms)
loop = GLib.MainLoop()
bus = pipe.get_bus(); bus.add_signal_watch()
bus.connect("message", lambda _b, m: (print("ERROR", m.src.get_name(), *m.parse_error(), flush=True), loop.quit())
            if m.type == Gst.MessageType.ERROR else None)

# --- the listener ----------------------------------------------------------------------------------------------
lpipe = None


def start_listener():
    """Join icecast.server as a listener; retry every 3 s until the mount answers (it opens only after the
    playout source has connected and pre-buffered, and a reconnect at start-up delays it; run of 2026-10-25)."""
    global lpipe
    if lpipe: lpipe.set_state(Gst.State.NULL)
    lpipe = Gst.parse_launch(f"souphttpsrc location=http://127.0.0.1:8010/programme.opus is-live=true do-timestamp=true "
                             f"iradio-mode=false ! oggdemux ! opusdec ! {tap('listener')}")
    lpipe.get_by_name("listener").connect("new-sample", on_sample, "listener")
    lb = lpipe.get_bus(); lb.add_signal_watch()
    def on_lmsg(_b, m):
        if m.type == Gst.MessageType.ERROR and "listener" not in taps:
            log(f"listener: {m.parse_error()[0].message}; retrying in 3 s")
            GLib.timeout_add_seconds(3, start_listener)
    lb.connect("message", on_lmsg)
    lpipe.set_state(Gst.State.PLAYING); log("listener started")
    return False


# --- the show clock: every stem advances together, silence before SHOW_AT ----------------------------------------
apps = {p: pipe.get_by_name(f"in_{p}") for p in stems}
st = {"t0": None, "pushed": 0, "show_t0": None, "events": []}
SILENCE = np.zeros(FR * 2, np.int16)
events = sorted([(c["start"], c["what"]) for c in tl["cues"] if c["kind"] == "event"] +
                [(t, f"{p} on air") for p, t in on_air_at.items()])


def feed():
    now = time.monotonic()
    if st["t0"] is None: st["t0"] = now
    due = int((now - st["t0"]) * RATE / FR) - st["pushed"]
    show_frame0 = SHOW_AT * RATE // FR
    for _ in range(max(due, 0)):
        k = st["pushed"] - show_frame0
        if k == 0:
            st["show_t0"] = now; log("show starts")
        for p, app in apps.items():
            a = stems[p]
            chunk = a[k * FR * 2:(k + 1) * FR * 2] if k >= 0 else SILENCE
            if len(chunk) < FR * 2: chunk = np.concatenate([chunk, np.zeros(FR * 2 - len(chunk), np.int16)])
            app.emit("push-buffer", Gst.Buffer.new_wrapped(chunk.tobytes()))
        st["pushed"] += 1
    if st["show_t0"]:
        t = now - st["show_t0"]
        while events and events[0][0] <= t:
            at, what = events.pop(0)
            st["events"].append({"show_s": at, "what": what, "fired_s": round(t, 3)}); log(f"event: {what}")
            p = what[:-len(" on air")] if what.endswith(" on air") else None
            if p in gates:
                for _target, vol in gates[p]: vol.set_property("mute", False)
    return True


pipe.set_state(Gst.State.PLAYING)
GLib.timeout_add(5, feed)
GLib.timeout_add_seconds(LISTEN_AT, start_listener)
GLib.timeout_add_seconds(SETTLE, lambda: (log(f"linked after {SETTLE} s: {sorted(linked)}"), False)[1])
GLib.timeout_add(int((SHOW_AT + tl["total"] + 22) * 1000), loop.quit)  # listeners are about 15 s behind
loop.run()
pipe.set_state(Gst.State.NULL)
if lpipe: lpipe.set_state(Gst.State.NULL)
for v in taps.values(): v["w"].close()

# --- analysis ----------------------------------------------------------------------------------------------------
def read(name):
    with wave.open(f"{out_dir}/{name}.wav") as w:
        return np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).reshape(-1, 2).mean(axis=1)


def env(a, hop=480):  # 10 ms RMS envelope
    n = len(a) // hop
    return np.sqrt((a[:n * hop].astype(np.float64).reshape(n, hop) ** 2).mean(axis=1))


def lag_ms(ref, sig_, max_s=20):
    """Delay of sig_ behind ref by envelope cross-correlation, in ms (10 ms resolution)."""
    r, s = env(ref), env(sig_)
    r, s = r - r.mean(), s - s.mean()
    m = min(len(r), len(s)); r, s = r[:m], s[:m]
    c = np.correlate(s, r, "full")[m - 1:m - 1 + max_s * 100]
    return int(np.argmax(c)) * 10, float(c.max() / (np.linalg.norm(r) * np.linalg.norm(s) + 1e-9))


report = {"room": ROOM, "jitterbuffer_ms": jb_ms, "live": live, "events": st["events"], "links": sorted(map(list, linked)),
          "taps": {k: {"first_s": round(v["first"] - st["show_t0"], 3) if st["show_t0"] else None,
                       "seconds": round(v["frames"] / RATE, 1)} for k, v in taps.items()}, "latency": {}, "own_voice": {}}
sent_offset = {p: SHOW_AT for p in stems}
for p in PUBLISH:
    if f"recv_{p}" in taps:
        rec = read(f"recv_{p}"); first = taps[f"recv_{p}"]["first"] - st["show_t0"]
        ref = stems[p].reshape(-1, 2).mean(axis=1)
        # Align by wall clock: recv sample 0 arrived at <first> s of show time; the stem's sample 0 is show time 0.
        pad = int(first * RATE)
        aligned = rec[-pad:] if pad < 0 else np.concatenate([np.zeros(pad), rec])
        ms, corr = lag_ms(ref[:RATE * 120], aligned[:RATE * 120])
        report["latency"][f"{p} -> desk"] = {"ms": ms, "corr": round(corr, 2)}
for p in VOICES:
    if f"ear_{p}" in taps and p in stems:
        ear = read(f"ear_{p}"); own = stems[p].reshape(-1, 2).mean(axis=1)
        first = taps[f"ear_{p}"]["first"] - st["show_t0"]; pad = int(first * RATE)
        ear = ear[-pad:] if pad < 0 else np.concatenate([np.zeros(pad), ear])
        m = min(len(ear), len(own)); e, o = env(ear[:m]), env(own[:m])
        # Own-voice leakage: the ear's level while only this voice speaks, against the ear's level overall.
        alone = o > o.max() * 0.05
        others = np.zeros_like(alone)
        for q in VOICES:
            if q != p and q in stems:
                oq = env(stems[q].reshape(-1, 2).mean(axis=1)[:m]); others |= oq > oq.max() * 0.05
        po = env(stems["playout"].reshape(-1, 2).mean(axis=1)[:m]); others |= po > po.max() * 0.02
        solo = alone & ~others
        quiet = ~alone & ~others
        if solo.sum() > 100:
            report["own_voice"][p] = {"ear_db_while_alone": round(20 * np.log10(e[solo[:len(e)]].mean() + 1), 1),
                                      "ear_db_in_silence": round(20 * np.log10(e[quiet[:len(e)]].mean() + 1), 1) if quiet.sum() else None,
                                      "solo_seconds": round(solo.sum() / 100, 1)}
if "listener" in taps and "programme" in taps:
    # Put both on one wall clock, then find the listener's delay behind the programme over two minutes of the show.
    prog, lis = read("programme"), read("listener")
    shift = int((taps["listener"]["first"] - taps["programme"]["first"]) * RATE)
    lis = np.concatenate([np.zeros(shift), lis]) if shift > 0 else lis[-shift:]
    a = int((taps["programme"]["first"] < st["show_t0"]) and (st["show_t0"] - taps["programme"]["first"]) * RATE) + 60 * RATE
    ms, corr = lag_ms(prog[a:a + 120 * RATE], lis[a:a + 120 * RATE], max_s=40)
    report["latency"]["programme -> listener"] = {"ms": ms, "corr": round(corr, 2)}
json.dump(report, open(f"{out_dir}/report.json", "w"), indent=1)
print(json.dumps(report, indent=1))
