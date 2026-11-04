---
title: "How the parts fit together"
description: "The station's processes, what each does and how they talk, and why the people talking and the people listening run on two different clocks."
type: explanation
status: draft
last_verified: 2026-11-04
sidebar:
  order: 1
---

A station is a handful of processes on one server.
Only the web proxy and LiveKit's media and TURN ports face the network (and LiveKit's ICE over TCP, where a station turns it on).
Everything else listens on loopback or on a Unix socket: PostgreSQL has no TCP port at all.

![A diagram of the station. People in the room reach LiveKit over WebRTC on one UDP port. The desk exchanges audio with LiveKit: each voice in, and a mix-minus back to each person. The desk sends the programme over SRT on loopback to playout, Liquidsoap 2.5, which passes it as PCM in WAV over loopback to the listener process, which holds the mount with icecast.server and serves listeners Ogg Opus through the proxy. The studio app reaches the control plane over HTTPS with live events; the control plane drives the desk over a Unix socket and keeps its data in PostgreSQL. A dashed area around the people, LiveKit and the desk is labelled "Conversation: 69–77 ms on loopback, about 0.25 s each way online"; another around playout, the listener process and the listeners is labelled "Listeners: about 12 to 23 s behind".](../../../../assets/architecture.svg)

## The parts

**LiveKit** holds the live room.
Presenters, guests and callers join it over WebRTC, on one UDP media port, with LiveKit's own TURN for people whose networks block UDP (ADR-0023, ADR-0059).
Each caller is in a booth room of their own, which only the desk joins, so callers never see or hear each other directly (ADR-0193).

**The desk** (Rust and GStreamer, with its own mixer in Rust) joins the room as a hidden participant.
It subscribes to each person's voice, mixes the programme, and makes each person a *mix-minus*: everyone they should hear, never their own voice.
It publishes each mix-minus back to that person alone, from a connection of its own named `desk-mm-<identity>`.
Which voice reaches which mix is decided by each person's role: see [Who hears whom](/docs/reference/who-hears-whom/).
The profanity delay (8 s by default) sits after the desk's mix, on the way to playout: listeners are held back, the room never is (ADR-0003).

**Playout** is Liquidsoap 2.5 running one fixed script, `playout/weavesignal.liq`, that reads a data file and is never generated (ADR-0048).
It takes the desk's programme as Opus in Matroska over SRT, on loopback only (ADR-0049).
When the desk is away, or silent for longer than the dead-air wait, it plays automation, then the station's fallback files, then the default loop WeaveSignal ships, then silence.
Its processing chain brings the programme towards −16 LUFS under a peak limiter.

**The listener process** is a second Liquidsoap process, `playout/listeners.liq`, running Liquidsoap's own listener server, `icecast.server`.
Playout sends it the programme as uncompressed PCM in WAV over loopback; the listener process encodes it once, as Ogg Opus at a constant 96 kbit/s (and as MP3 too, when the station turns that on), and serves every listener.
It also writes the legal logger's hourly files.

**The control plane** (Rust and Axum) holds the station's settings and data in PostgreSQL (ADR-0082).
It signs every room token with the person's role (ADR-0084), drives the desk through the desk's [control socket](/docs/reference/desk-control/) and playout through Liquidsoap's, and serves the studio app its view of the studio as a stream of events.
It also serves the public pages, the HLS video, the podcast feeds and the show cards, and runs the media worker, which cleans every uploaded file in a sandboxed process of its own.

**The chat** is a service of its own beside the control plane, behind the same proxy ([The chat](/docs/reference/chat/)).

**The studio app** shows the control plane's view as the desk, the screener, the cart wall and the schedule, and sends the presenter's and screener's keys back as commands.
It carries no audio: presenters and guests join the room from a browser.

## Two latencies

The people talking and the people listening need different things, so the station keeps two clocks.

**The conversation** must feel like being in one room.
Every person hears everyone else through the desk, so each exchange goes from a voice, through LiveKit, into the desk's mix and back out.
On loopback, a person's mix-minus arrives 69–77 ms after the voice that is in it (median, 2026-10-25).
Over the internet, between a guest on a workstation and a guest on a phone on a mobile network, a tone from one reached the other in a median 237 ms, and the answering tone came back in 501 ms (2026-11-03).
That is what a presenter and a remote guest live with.
It is why the desk holds its receive jitter buffer at 50 ms rather than LiveKit's 200 ms default, which doubled a guest's delay in an early test.

**Listeners** can be seconds behind, and they are.
The listener server keeps about 12 s of the stream in hand for each new listener, so a player starts at once and rides out a poor network; the profanity delay adds its own length on top.
The rehearsals of October 2026 measured 12 to 23 s; this release does not measure the delay itself.
The studio app's clock shows *Listeners hear* from a setting (`studio.listener_delay_ms`, 15 s by default), so set it to what your station measures.

The programme passes through playout, an encoder and a listener server built for buffering, so any ordinary player and any stream directory can take it.
Making the listener stream "real time" would buy nothing a listener can notice and lose that compatibility.

## Why the desk listens and playout connects

On the SRT link between them, the desk is the listener and playout is the caller, trying again every second while the desk is away (ADR-0086).
The other way round failed: when the desk connected to playout, it sometimes came back after a restart without Matroska's header, and once never came back at all.
As the listener, the desk sends the header to every connection, so each one starts clean.

Every mix at the desk also has a silent bed under it, so it sends audio from its first moment.
A desk that sent Liquidsoap only a stream header while nobody had joined was dropped after a second.

## Why the listener process holds the mount

`icecast.server` disconnects every listener when its source leaves.
It also drops its source when a new chained Ogg stream starts, which Liquidsoap does at every switch between the desk and a fallback.
So playout never holds the listener mount.
It sends plain PCM, with metadata and track marks removed, to the listener process, which holds the mount with a fallback of its own and outlives a restart of playout or the desk.
A listener hears the fallback for a few seconds, then the programme again, and is never disconnected.
Now playing reaches the control plane over playout's socket instead of travelling in the stream.

Between the two processes sits a small cushion (`listeners.feed_buffer_s`, 0.5 s by default; `FEED_BUFFER_S` in the Compose bundle).
On a dedicated 4-vCPU server it held 0.20–0.36 s for 13 hours unattended.
On a heavily shared server even a 3 s cushion filled to its 10 s cap, and listeners heard the fallback for moments.
So run a station on a server that is not overloaded ([Memory and CPU](/docs/reference/resources/)).

## When a part stops

The parts are separate processes so that one can stop without taking the others with it.

- **Playout stops**: the room carries on, and every person still hears everyone else.
  Listeners hear the listener process's fallback, so the lamp says PLAYOUT DOWN and on-air sources say OPEN ([What ON AIR promises](/docs/explanation/on-air/)).
  When playout starts again it connects to the desk within about a second.
- **The desk stops**: it is started again by its supervisor, and playout connects again.
  One person's failed connection stays that person's (tested by `just test-contain`); it does not stop the whole desk.
- **Either one restarts**: no listener is disconnected; the listener process plays its fallback until the programme returns ([Playout](/docs/reference/playout/)).

The stream is encoded at a constant 96 kbit/s, so silence still sends a steady stream of bytes: with variable bitrate it trickled about 1.4 KB every 6 s, and a player that buffers by bytes waited for minutes.

## Rules and limits the shape imposes

- **The desk sends WebRTC without congestion control.** Its image lacks GStreamer's `rtpgccbwe`, so each mix-minus goes out at its set bitrate whatever the person's link can carry. A guest on a poor link may hear more loss than a browser-to-browser call would give.
- **A return's timing comes from its place in the mix.** Each block the desk sends is stamped with its position in the mix, never with the clock at the moment it is encoded. Stamped at encode time, the timestamps stepped unevenly and receivers concealed a gap at nearly every packet: 29.8 % of a caller's return was concealed before the fix and 0.0 % after, measured with WebRTC `getStats` on the receiver (2026-11-03).
- **LiveKit's built-in TURN refuses clients that reach it over IPv6** (an allocation error, 508): give the TURN name an A record and no AAAA record.
- **The studio app's Linux webview has no WebRTC**, so the app drives the desk and presenters join from a browser.

WHIP for remote DJs is **[planned](/docs/reference/release-one-status/)**.
For everything else release one leaves out, see [What release one does, and what it leaves out](/docs/reference/release-one-status/).
