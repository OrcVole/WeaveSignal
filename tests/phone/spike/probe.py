#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Phone line spike (17f, module 0), the desk's side: does a phone booth's key press reach the
# desk's own LiveKit elements? Run in the desk image (GStreamer with gst-plugins-rs's LiveKit
# signaller, patched for ADR-0118). It joins one phone booth as the desk would: a subscriber
# (livekitwebrtcsrc) on the caller, and a publisher (livekitwebrtcsink, publish-only token) of a
# 440 Hz tone, the stand-in for the phone words, whose allowed subscribers name the caller's
# `sip_` identity only. It logs, with times:
#   - each data channel the subscriber's webrtcbin opens (the signaller's own `_reliable` and
#     `_lossy`, and any the server opens) and every message on them, decoded as LiveKit's
#     DataPacket where it is one: a `sip_dtmf` (field 6) prints its code and digit;
#   - when the caller's audio first reaches the subscriber.
# Python, not Mojo: GStreamer's signal callbacks need a host language with GLib closures.
#
#   probe.py WS_URL SUB_TOKEN PUB_TOKEN SIP_IDENTITY SECONDS
import sys
import time

import gi

gi.require_version("Gst", "1.0")
gi.require_version("GstWebRTC", "1.0")
from gi.repository import GLib, Gst  # noqa: E402

ws, sub_tok, pub_tok, sip_id, secs = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4], float(sys.argv[5])
T0 = time.monotonic()


def log(*a):
    print(f"[{time.monotonic() - T0:7.2f}]", *a, flush=True)


def varint(b, i):
    n = s = 0
    while True:
        c = b[i]
        i += 1
        n |= (c & 0x7F) << s
        s += 7
        if c < 0x80:
            return n, i


def fields(b):
    """Top-level protobuf fields: (number, wire type, value)."""
    i, out = 0, []
    while i < len(b):
        key, i = varint(b, i)
        num, wt = key >> 3, key & 7
        if wt == 0:
            v, i = varint(b, i)
        elif wt == 2:
            n, i = varint(b, i)
            v, i = b[i:i + n], i + n
        elif wt == 5:
            v, i = b[i:i + 4], i + 4
        elif wt == 1:
            v, i = b[i:i + 8], i + 8
        else:
            raise ValueError(f"wire type {wt}")
        out.append((num, wt, v))
    return out


def on_message(channel, data):
    raw = bytes(data.get_data()) if hasattr(data, "get_data") else bytes(data)
    try:
        for num, wt, v in fields(raw):
            if num == 6 and wt == 2:
                code = digit = None
                for n2, _w2, v2 in fields(v):
                    if n2 == 3:
                        code = v2
                    elif n2 == 4:
                        digit = v2.decode()
                log(f"SIP DTMF on {channel.props.label}: code={code} digit={digit!r}")
                return
        log(f"data on {channel.props.label}: {len(raw)} bytes, fields {[f[0] for f in fields(raw)]}")
    except Exception as e:  # noqa: BLE001 (a spike logs what it cannot read)
        log(f"data on {channel.props.label}: {len(raw)} bytes, not a DataPacket ({e})")


def watch_channel(channel, how):
    log(f"data channel {channel.props.label!r} ({how})")
    channel.connect("on-message-data", on_message)
    channel.connect("on-message-string", lambda ch, s: log(f"string on {ch.props.label}: {s!r}"))


def on_webrtcbin(_signaller, _consumer, webrtcbin):
    log("subscriber webrtcbin ready")
    webrtcbin.connect("on-data-channel", lambda _w, ch: watch_channel(ch, "opened by the server"))


Gst.init(None)
sub = Gst.parse_launch(
    f"livekitwebrtcsrc name=src signaller::ws-url={ws} signaller::auth-token={sub_tok}"
)
# A launch line of one element gives the element itself, not a bin around it.
src = sub if sub.get_name() == "src" else sub.get_by_name("src")
if isinstance(sub, Gst.Element) and not isinstance(sub, Gst.Pipeline):
    pl = Gst.Pipeline.new("subscriber")
    pl.add(src)
    sub = pl
sig = src.get_property("signaller")
sig.connect("webrtcbin-ready", on_webrtcbin)
first = {"audio": None}


def on_pad(_el, pad):
    log(f"subscriber pad {pad.get_name()}")
    sink = Gst.ElementFactory.make("fakesink")
    sink.set_property("signal-handoffs", True)

    def handoff(*_a):
        if first["audio"] is None:
            first["audio"] = time.monotonic()
            log("the caller's audio reaches the desk")

    sink.connect("handoff", handoff)
    sub.add(sink)
    sink.sync_state_with_parent()
    pad.link(sink.get_static_pad("sink"))


src.connect("pad-added", on_pad)
pub = Gst.parse_launch(
    "audiotestsrc is-live=true wave=sine freq=440 volume=0.2 ! audioconvert ! audioresample ! "
    f"livekitwebrtcsink name=sink signaller::ws-url={ws} signaller::auth-token={pub_tok}"
)
psig = pub.get_by_name("sink").get_property("signaller")
try:
    arr = Gst.ValueArray([sip_id])
    psig.set_property("allowed-subscribers", arr)
    log(f"publisher allows only {sip_id}")
except Exception as e:  # noqa: BLE001
    log(f"cannot set allowed-subscribers ({e}): the image lacks the ADR-0118 patch?")


def bus(pl, name):
    b = pl.get_bus()
    b.add_signal_watch()
    b.connect("message::error", lambda _b, m: log(f"{name} ERROR {m.parse_error()}"))
    b.connect("message::warning", lambda _b, m: log(f"{name} warning {m.parse_warning()[0]}"))


bus(sub, "subscriber")
bus(pub, "publisher")
log("starting the subscriber")
sub.set_state(Gst.State.PLAYING)
log("starting the publisher (the words' stand-in)")
pub.set_state(Gst.State.PLAYING)
# The signaller's own channels, once it has made them.
GLib.timeout_add(3000, lambda: [watch_channel(sig.get_property(p), "the signaller's")
                                for p in ("reliable-channel", "lossy-channel")
                                if sig.get_property(p) is not None] and False)
loop = GLib.MainLoop()
GLib.timeout_add(int(secs * 1000), loop.quit)
loop.run()
pub.set_state(Gst.State.NULL)
sub.set_state(Gst.State.NULL)
log("probe done")
