# Spike 01 measurement harness, in Python by exception to the house stack: it measures; the desk proper is gstreamer-rs.
# One process runs two test participants (A clicks at 1 kHz every 2 s, B at 3 kHz every 3 s), the desk (one
# livekitwebrtcsrc per participant, a programme mix, a mix-minus for B published back into the room), the
# programme over SRT to Liquidsoap, a "B ear" that subscribes to B's mix-minus, and a listener on icecast.server.
# Every branch ends in an appsink; each buffer is stamped with time.monotonic() on arrival, and onsets are
# found offline, so every latency is measured on one clock.
# Each mix has a silent live bed, so it outputs from the first moment: without it the programme sent Liquidsoap only a
# Matroska header until the participants linked, Liquidsoap timed out after 1 s, and srtsink never reconnected.
# The programme's SRT branch sits behind leaky queues: with no playout listening, srtsink blocked, the programme mixer
# stalled, and through the receive tees every guest stopped hearing every other guest.
#
# usage: desk_spike.py <seconds> <out.json> [jitterbuffer latency ms] [--no-listener] [--video] [--human] [--interview=<dir>]
# --interview=<dir> (with --human): participant A is an interviewer reading <dir>/q01.wav, q02.wav ... in turn; after each
# line it waits until H has spoken and then been quiet for 2 s (or 40 s pass), and the programme is written to <out>.wav.
# --human: a person joins from the browser page as "H" in place of the test participant B; the desk publishes
# H's mix-minus as "desk-mmH" and no "B ear" runs.
import json, sys, threading, time
import numpy as np
import gi
gi.require_version("Gst", "1.0")
from gi.repository import Gst, GLib

secs = float(sys.argv[1]); out = sys.argv[2]
jb_ms = int(sys.argv[3]) if len(sys.argv) > 3 and sys.argv[3].isdigit() else None
listener = "--no-listener" not in sys.argv
video = "--video" in sys.argv
human = "--human" in sys.argv
interview = next((a.split("=", 1)[1] for a in sys.argv if a.startswith("--interview=")), None)
PB = "H" if human else "B"
WS, KEY, SECRET = "ws://127.0.0.1:7880", "devkey", "secret-secret-secret-secret-secret0"
ROOM = "spike" if "--human" in sys.argv else f"spike-{int(time.time())}"  # a fresh room per run: LiveKit keeps departed identities briefly
RATE = 48000
Gst.init(None)
streams = {}  # name -> list of (arrival monotonic seconds, int16 mono samples)

def sig(identity, producer=None):
    s = (f'signaller::ws-url={WS} signaller::api-key={KEY} signaller::secret-key={SECRET} '
         f'signaller::room-name={ROOM} signaller::identity={identity} signaller::participant-name={identity}')
    if producer:
        s += f' signaller::producer-peer-id={producer}'
    return s

def on_sample(sink, name):
    buf = sink.emit("pull-sample").get_buffer()
    t = time.monotonic()
    ok, info = buf.map(Gst.MapFlags.READ)
    if ok:
        a = np.frombuffer(info.data, dtype=np.int16).reshape(-1, 2).mean(axis=1).astype(np.int16)
        streams.setdefault(name, []).append((t, a))
        if interview and name == "recvB":
            if st_i["h_joined"] is None: st_i["h_joined"] = t
            h_level(a)
        buf.unmap(info)
    return Gst.FlowReturn.OK

RAW = f"audio/x-raw,format=S16LE,rate={RATE},channels=2,layout=interleaved"
A_SRC = (f"appsrc name=interviewer is-live=true format=time do-timestamp=true block=false min-latency=10000000 max-latency=40000000 caps=\"{RAW}\"" if interview else
         "audiotestsrc is-live=true wave=ticks tick-interval=2000000000 freq=1000 volume=0.8 marker-tick-period=10 marker-tick-volume=0.3 samplesperbuffer=480")
def tap(name):
    return f"queue ! audioconvert ! audioresample ! {RAW} ! appsink name={name} emit-signals=true sync=false max-buffers=0 drop=false"

desc = f"""
  {A_SRC} ! {RAW} ! tee name=tA
  tA. ! {tap('sentA')}
  tA. ! queue ! livekitwebrtcsink name=pubA {sig('A')}
  livekitwebrtcsrc name=srcA {sig('desk-A', 'A')}
  livekitwebrtcsrc name=srcB {sig('desk-' + PB, PB)}
  audiomixer name=prog output-buffer-duration=10000000 ! {RAW} ! tee name=tP
  tP. ! {tap('programme')}
  tP. ! queue leaky=downstream max-size-buffers=0 max-size-bytes=0 max-size-time=500000000 ! audioconvert ! opusenc bitrate=96000 frame-size=20 ! matroskamux streamable=true name=mux ! queue leaky=downstream max-size-buffers=0 max-size-bytes=0 max-size-time=500000000 ! srtsink uri=srt://127.0.0.1:9000?mode=caller latency=20 wait-for-connection=false sync=false
  audiomixer name=mmB output-buffer-duration=10000000 ! {RAW} ! tee name=tM
  audiotestsrc is-live=true wave=silence samplesperbuffer=480 ! {RAW} ! queue ! prog.
  audiotestsrc is-live=true wave=silence samplesperbuffer=480 ! {RAW} ! queue ! mmB.
  tM. ! {tap('mixminusB_at_desk')}
  tM. ! queue ! livekitwebrtcsink name=pubMM {sig('desk-mm' + PB)}
"""
if not human:
    desc += f"""
  audiotestsrc is-live=true wave=ticks tick-interval=3000000000 freq=3000 sine-periods-per-tick=30 volume=0.8 samplesperbuffer=480 ! {RAW} ! tee name=tB
  tB. ! {tap('sentB')}
  tB. ! queue ! livekitwebrtcsink name=pubB {sig('B')}
  livekitwebrtcsrc name=earB {sig('B-ear', 'desk-mmB')}
"""
if video:
    desc += f"""
  videotestsrc is-live=true pattern=ball ! video/x-raw,width=854,height=480,framerate=30/1 ! videoconvert ! nvav1enc bitrate=800 ! av1parse ! queue ! mux.
"""
pipe = Gst.parse_launch(desc)
for n in ("sentA", "sentB", "programme", "mixminusB_at_desk"):
    if pipe.get_by_name(n): pipe.get_by_name(n).connect("new-sample", on_sample, n)

def link_src(src, routes, tag):
    def added(_el, pad):
        if not pad.get_name().startswith("audio"):
            fakesink = Gst.ElementFactory.make("fakesink"); pipe.add(fakesink); fakesink.sync_state_with_parent(); pad.link(fakesink.get_static_pad("sink")); return
        if jb_ms is not None:
            pass  # set below on the inner webrtcbin when it appears
        chain = Gst.parse_bin_from_description(f"audioconvert ! audioresample ! {RAW} ! tee name=t", True)
        pipe.add(chain); chain.sync_state_with_parent(); pad.link(chain.get_static_pad("sink"))
        tee = chain.get_by_name("t")
        for target in routes:
            q = Gst.ElementFactory.make("queue"); chain.add(q); q.sync_state_with_parent()
            tee.link(q)
            gp = Gst.GhostPad.new(None, q.get_static_pad("src")); gp.set_active(True); chain.add_pad(gp)
            if target.startswith("sink:"):
                sink = Gst.parse_bin_from_description(tap(target[5:]), True)
                pipe.add(sink); sink.sync_state_with_parent(); gp.link(sink.get_static_pad("sink"))
                sink.get_by_name(target[5:]).connect("new-sample", on_sample, target[5:])
            else:
                mixpad = pipe.get_by_name(target).request_pad_simple("sink_%u"); gp.link(mixpad)
        print(f"[{time.monotonic():.3f}] {tag}: linked {pad.get_name()} -> {routes}", flush=True)
    src.connect("pad-added", added)

link_src(pipe.get_by_name("srcA"), ["sink:recvA", "prog", "mmB"], "desk-A")
link_src(pipe.get_by_name("srcB"), ["sink:recvB", "prog"], "desk-B")
if not human:
    link_src(pipe.get_by_name("earB"), ["sink:mixminusB_at_B"], "B-ear")

def deep_set(bin_, prop, val):
    # Set the receive latency on webrtcbin, rtpbin and every rtpjitterbuffer inside the LiveKit elements.
    # They arrive inside whole bins, so a bin that is added is searched as well as the element itself.
    targets = ("webrtcbin", "rtpbin", "rtpjitterbuffer")
    def apply(el):
        f = el.get_factory()
        if f and f.get_name() in targets:
            el.set_property(prop, val); print(f"{f.get_name()} {el.get_name()} latency set to {val} ms", flush=True)
    def on_added(_b, _sub, el):
        apply(el)
        if isinstance(el, Gst.Bin):
            it = el.iterate_recurse()
            while True:
                r, child = it.next()
                if r != Gst.IteratorResult.OK: break
                apply(child)
    bin_.connect("deep-element-added", on_added)
if jb_ms is not None:
    deep_set(pipe, "latency", jb_ms)

loop = GLib.MainLoop()
bus = pipe.get_bus(); bus.add_signal_watch()
def on_msg(_b, m):
    if m.type == Gst.MessageType.ERROR:
        err, dbg = m.parse_error(); print("ERROR", m.src.get_name(), err, dbg, flush=True); loop.quit()
bus.connect("message", on_msg)

# iradio-mode=false: souphttpsrc otherwise asks for ICY metadata, and icecast.server then splices ICY blocks into Ogg (bench 02).
lpipe = None
def start_listener():
    global lpipe
    lpipe = Gst.parse_launch(f"souphttpsrc location=http://127.0.0.1:8010/programme.opus is-live=true do-timestamp=true iradio-mode=false ! oggdemux ! opusdec ! {tap('listener')}")
    lpipe.get_by_name("listener").connect("new-sample", on_sample, "listener")
    lb = lpipe.get_bus(); lb.add_signal_watch(); lb.connect("message", on_msg)
    lpipe.set_state(Gst.State.PLAYING); print(f"[{time.monotonic():.3f}] listener started", flush=True)
    return False

# --- the interviewer -------------------------------------------------------
turns = []
if interview:
    import glob
    # Clips are raw 48 kHz stereo S16LE (q01.s16 ...), converted beforehand: the desk image has no FFmpeg.
    clips = [(f.rsplit("/", 1)[-1], np.fromfile(f, dtype=np.int16)) for f in sorted(glob.glob(f"{interview}/q*.s16"))]
    st_i = {"queue": np.zeros(0, np.int16), "next": 0, "state": "idle", "t_state": time.monotonic(),
            "heard": False, "last_voice": 0.0, "pushed": 0, "t0": None, "h_joined": None, "rms": []}
    appsrc = pipe.get_by_name("interviewer")
    FR = 480  # 10 ms of stereo frames per push

    def h_level(a):
        # Speech detection on H as heard at the desk: above an adaptive floor (10th percentile of the last 5 s) + 12 dB.
        rms = 20 * np.log10(max(np.sqrt(np.mean(a.astype(np.float64) ** 2)), 1) / 32768)
        st_i["rms"] = (st_i["rms"] + [rms])[-500:]
        floor = np.percentile(st_i["rms"], 10) if len(st_i["rms"]) > 50 else -60
        # Only an answer counts: sound while the interviewer is still asking is ignored.
        if rms > max(floor + 12, -50) and st_i["state"] == "listening":
            st_i["last_voice"] = time.monotonic(); st_i["heard"] = True

    def start_clip():
        name, pcm = clips[st_i["next"]]; st_i["next"] += 1
        st_i["queue"] = pcm.copy(); st_i["state"] = "speaking"; st_i["heard"] = False
        turns.append({"clip": name, "start": round(time.monotonic() - t_start, 2)}); print(f"[{time.monotonic() - t_start:7.2f}] interviewer: {name}", flush=True)

    def feed():
        now = time.monotonic()
        if st_i["t0"] is None: st_i["t0"] = now
        due = int((now - st_i["t0"]) * RATE / FR) - st_i["pushed"]
        for _ in range(max(due, 0)):
            q = st_i["queue"]
            chunk = q[:FR * 2] if len(q) else np.zeros(0, np.int16)
            st_i["queue"] = q[FR * 2:]
            if len(chunk) < FR * 2: chunk = np.concatenate([chunk, np.zeros(FR * 2 - len(chunk), np.int16)])
            appsrc.emit("push-buffer", Gst.Buffer.new_wrapped(chunk.tobytes())); st_i["pushed"] += 1
        # turn-taking
        if st_i["state"] == "speaking" and len(st_i["queue"]) == 0:
            st_i["state"] = "listening"; st_i["t_state"] = now; turns[-1]["end"] = round(now - t_start, 2)
        elif st_i["state"] == "idle" and st_i["h_joined"] and now - st_i["h_joined"] > 3 and st_i["next"] < len(clips):
            start_clip()
        elif st_i["state"] == "listening" and st_i["next"] < len(clips):
            answered = st_i["heard"] and now - st_i["last_voice"] > 2.0
            if answered or now - st_i["t_state"] > 40:
                turns[-1]["answered"] = bool(st_i["heard"]); turns[-1]["next_after_s"] = round(now - st_i["t_state"], 2)
                start_clip()
        return True
    GLib.timeout_add(5, feed)

t_start = time.monotonic()
pipe.set_state(Gst.State.PLAYING)
if listener:
    GLib.timeout_add_seconds(35, start_listener)  # icecast.server pre-buffers 12-13 s after each source (re)connect
GLib.timeout_add(int(secs * 1000), loop.quit)
loop.run()
print("buffers per tap:", {k: len(v) for k, v in streams.items()}, flush=True)
pipe.set_state(Gst.State.NULL)
if lpipe: lpipe.set_state(Gst.State.NULL)

# --- analysis -------------------------------------------------------------
def onsets(name):
    """Onset times (monotonic s) and pitch class ('A' 1 kHz, 'B' 3 kHz) of each click in a stream.
    A sample's time is its buffer's arrival time minus the samples that follow it in that buffer."""
    chunks = streams.get(name, [])
    if not chunks: return []
    a = np.concatenate([c for _, c in chunks])
    tt = np.concatenate([t - (len(c) - np.arange(len(c))) / RATE for t, c in chunks])
    loud = np.nonzero(np.abs(a.astype(np.int32)) > 3000)[0]
    if len(loud) == 0: return []
    first = loud[np.concatenate(([True], np.diff(loud) > int(0.05 * RATE)))]
    res = []
    for i in first:
        seg = a[i:i + 240].astype(np.int32)
        zc = np.count_nonzero(np.diff(np.signbit(seg))) if len(seg) > 10 else 0
        hz = zc / 2 / (len(seg) / RATE) if len(seg) else 0
        pk = int(np.max(np.abs(a[i:i + 480].astype(np.int32)))) if len(a[i:i + 480]) else 0
        res.append((float(tt[i]), "A" if hz < 2000 else "B", hz, pk))
    return res

def level_db(a):
    m = np.max(np.abs(a.astype(np.int32))) if len(a) else 0
    return 20 * np.log10(max(m, 1) / 32768)

def match(recv, sent_by):
    lat = {"A": [], "B": []}
    for t, who, *_ in recv:
        prior = [s for s in sent_by[who] if s <= t and t - s < 1.9]
        if prior: lat[who].append((t - prior[-1]) * 1000)
    return lat

sentA_on = onsets("sentA")
sent = {"A": [x[0] for x in sentA_on], "B": [x[0] for x in onsets("sentB")]}
# Marker clicks: every tenth A click is quieter (0.3 against 0.8), once every 20 s, which disambiguates delays over 2 s.
def markers(on):
    a_on = [x for x in on if x[1] == "A"]
    if not a_on: return []
    top = max(x[3] for x in a_on)
    return [x[0] for x in a_on if x[3] < 0.6 * top]
sent_markers = markers(sentA_on)
report = {"seconds": secs, "jitterbuffer_ms": jb_ms, "video": video, "streams": {}}
for name in ("recvA", "recvB", "programme", "mixminusB_at_desk", "mixminusB_at_B", "listener"):
    o = onsets(name)
    lat = match(o, sent)
    st = {"onsets": len(o), "classes": {"A": sum(1 for x in o if x[1] == "A"), "B": sum(1 for x in o if x[1] == "B")}}
    for who in ("A", "B"):
        v = sorted(lat[who][2:])  # drop the first two (join transients)
        if v: st[f"latency_ms_{who}"] = {"n": len(v), "p50": round(v[len(v)//2], 1), "p95": round(v[int(len(v)*.95)], 1), "min": round(v[0], 1), "max": round(v[-1], 1)}
    rm = markers(o)
    ml = []
    for t in rm:
        prior = [m for m in sent_markers if m <= t and t - m < 19.5]
        if prior: ml.append((t - prior[-1]) * 1000)
    if ml: st["marker_latency_ms"] = {"n": len(ml), "values": [round(v, 1) for v in ml]}
    report["streams"][name] = st
# Mix-minus leakage: B's own click level in B's return versus A's click level.
mm = streams.get("mixminusB_at_B", [])
if mm:
    allmm = np.concatenate([a for _, a in mm])
    report["mixminusB_peak_dbfs"] = round(level_db(allmm), 1)
    # Leakage, spectrally: energy at B's 3 kHz against A's 1 kHz in B's own return, in dB (more negative is better).
    spec = np.abs(np.fft.rfft(allmm.astype(np.float64) * np.hanning(len(allmm))))
    f = np.fft.rfftfreq(len(allmm), 1 / RATE)
    band = lambda c: spec[(f > c - 20) & (f < c + 20)].max()
    report["mixminusB_leak_3k_vs_1k_db"] = round(20 * np.log10(band(3000) / band(1000)), 1)
    progc = np.concatenate([x for _, x in streams.get("programme", [(0, np.zeros(2, np.int16))])]).astype(np.float64)
    sp = np.abs(np.fft.rfft(progc * np.hanning(len(progc)))); fp = np.fft.rfftfreq(len(progc), 1 / RATE)
    report["programme_3k_vs_1k_db_reference"] = round(20 * np.log10(sp[(fp > 2980) & (fp < 3020)].max() / sp[(fp > 980) & (fp < 1020)].max()), 1)
report["sent_peak_dbfs"] = round(level_db(np.concatenate([a for _, a in streams.get("sentA", [(0, np.zeros(1, np.int16))])])), 1)
if interview:
    import wave
    report["turns"] = turns
    prog = np.concatenate([a for _, a in streams.get("programme", [])]) if streams.get("programme") else np.zeros(0, np.int16)
    with wave.open(out.rsplit(".", 1)[0] + ".wav", "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(RATE); w.writeframes(prog.tobytes())
json.dump(report, open(out, "w"), indent=1)
print(json.dumps(report, indent=1))
