---
title: "Who hears whom"
description: "The desk's routes: which voices reach the programme and each person's headphones, and the gate on each route, as built."
type: reference
status: draft
last_verified: 2026-11-04
sidebar:
  order: 3
---

Each person in the room joins with a role, carried in their room token ([Roles](/reference/roles/)).
The desk makes the programme (what listeners hear) and, for every person, a *mix-minus*: what that person hears in their headphones, which never contains their own voice.
Each voice reaches each mix through a gate.
The rules are decided in ADR-0083, amended by ADR-0095 and ADR-0099, and built in the desk's matrix (`crates/desk/src/matrix.rs`).

## The routes

| Voice of | Programme | Presenter, DJ | Guest, producer | Caller | Screener |
|---|---|---|---|---|---|
| Presenter, DJ, guest | on air | on air or cued | on air | on air | on air |
| Caller | on air | on air or cued | on air | on air | always |
| Screener | never | talkback | never | talking to them | — |
| Producer | never | talkback | never | never | never |

Read a row as "this voice reaches that mix while…".
Nobody hears their own voice.
A listener (the `listener` role) is not at the desk at all: it has no voice and no mix-minus.

## The gates

**On air** passes while the person is on air and not muted.
Presenters and guests are on air from the moment they join; callers and DJs join off air and are put on air at the desk.

**Cued** is pre-fade listen: a cued voice reaches presenters' and DJs' headphones, whatever its fader, and never the programme.
This is how a presenter hears a caller before taking them to air.

**Talkback** passes while the screener or producer holds their talk key.
It reaches presenters and DJs only.

**Talking to them** passes only while the screener holds `T` with that caller selected (ADR-0095).
Nothing the screener says reaches a caller by accident.

**Always** passes whenever the voice is there: the screener hears every caller, waiting or on air.

## Talking to one person

A person can talk to one other person while a key is held (the desk's `talk_to`): the presenter to anyone in slots 2 to 5 with the Desk tab's `S` to `G`, the presenter to the screener with `⌃T`, and the screener to the selected caller with `T` on the Screener tab.
While the key is held, the talker's voice reaches that one person's headphones and leaves the programme and every other mix (ADR-0099).
Releasing the key puts them back where they were.
It works only where the table gives the talker's voice a route to that person: a screener cannot talk to a guest this way, and a producer has only talkback.

`⌃T` on the Screener tab is the screener's talkback instead: every presenter and DJ hears it while it is held.

## What a waiting caller hears

A caller who is neither on air nor cued hears the programme, and the screener only while the screener talks to them.
Only the screener hears the caller; once the presenter cues them, presenters and DJs hear them too.
The screener is never heard by listeners.
Each caller is in a booth room of their own, which only the desk joins ([Rooms](/reference/roles/#rooms)): the desk carries their voice to everyone who hears it, and their mix-minus back to them. A caller in a booth hears the programme and is not shown the programme picture.

## Levels

The desk mixes in 32-bit floating point and ends every mix in a look-ahead limiter with a ceiling of −1 dBFS, so voices that sum past full scale are limited, not clipped (ADR-0087).

Each role can have an input chain.
Presenters and DJs have one by default, in this order: a gate that turns the microphone down by up to 18 dB below −34 dBFS, a compressor, a leveller that steers the gain slowly toward a speech level of −25 dBFS, and a limiter.
Callers have one by default while WeaveSignal runs its diagnostics (ADR-0194): the band of a phone line, 300 to 3,400 Hz, so a caller sounds like a call on air and in every presenter's headphones, then a light compressor. A station turns it off with an empty `[inputs.caller]` table in the desk's settings.
Other roles have none.
Every value is a setting; see [Settings](/reference/settings/).

A caller's return, what they hear on the phone, is about −23 LUFS against the stream's −16, and is kept there (ADR-0205): the call page asks callers to turn the call's volume up.

## A connection that stalls

Every mix is made in one pass of the desk, so nobody's connection can slow anyone else's mix.
What leaves the desk is encoded on the way out: the programme on a thread of its own, each mix-minus by a small shared pool of encoders.
Between each mix-minus's encoder and its connection to the room there is a buffer of 200 ms that drops its oldest audio when full (ADR-0168).
A guest whose connection stops taking audio loses only their own headphone feed while it is stuck, and gets it back, live, when it takes audio again; the programme and every other person's mix-minus go on (`just desk-check`: `a_stuck_return_drops_its_own_audio_and_holds_up_no_other_return`).

Every block that leaves the desk is timestamped from its place in the mix, never from the clock when an encoder reached it.
Stamped by the encoder's clock, the timestamps stepped unevenly and the receiver concealed a gap at nearly every packet: on a local stack 29.8 % of a guest's return was concealment, and 0.0 % since (measured with the receiving browser's WebRTC `getStats`).

The desk's WebRTC sends without congestion control, because its image lacks GStreamer's `rtpgccbwe`: a guest on a poor link may hear more loss than a browser-to-browser call would give ([Known limits](/reference/release-one-status/#known-limits)).
