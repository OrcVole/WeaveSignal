---
title: "Connect a studio PC over VBAN"
description: "On a station you run on your own network (Compose or the Quadlets, never Cloudron), take a studio PC's sound over VBAN as a show key's source, send the desk's programme back as VBAN to monitors, a transmitter or a local guest, and read the one clock's figures."
type: howto
status: draft
last_verified: 2026-11-20
sidebar:
  order: 41
---

**VBAN** is the protocol Voicemeeter and other software use to send sound between computers on
one network. The station can take a studio PC's VBAN as a host's own source, and send VBAN from
the desk: the presenter kit's station half (Phase 17g).

VBAN has **no password and no encryption**: anyone on the network can send to the station or
read what it sends. So:

- it is **off** until the installation turns it on, and **never offered on Cloudron**, which is
  on the internet;
- the station uses one **address of its own on your network**, and refuses a public one unless
  you say otherwise;
- a VBAN input is a **show key's source**: it only ever **stands by** until someone at the desk
  puts it on air, and Dump and the profanity delay apply to it.

For anything beyond one local network, carry VBAN through WireGuard.

## What you need

- A station installed with Compose or the Quadlets on a machine on the same network as the
  studio PC.
- An admin's sign-in, and a show key for the show the studio PC sends to
  ([Set up a host in their own studio](/docs/how-to/set-up-a-host-in-their-own-studio/)).

## 1. Turn VBAN on for the installation

Set `KIT_VBAN=true` in the station's settings (`.env` beside `compose.yaml`) and run
`weavesignal.sh` again, or restart the bundle. It writes `[kit] vban = true` in the control
plane's settings. Settings › **Presenter kit** (`settings.kit`) says why VBAN is off until then.

## 2. Bind the studio PC's stream to a show key

In **Settings** › **Presenter kit**, set in the kit's settings:

- `bind`: the station's own address on your network, such as `192.168.1.10`;
- `port`: VBAN's port, 6980 unless you change it;
- in `vban_in`, one entry: the studio PC's address, its stream's name as it sends it (Voicemeeter's
  `Stream1`, 1 to 16 characters, matched exactly), and the show key's id, with the pair of
  channels to take (`[1, 2]` by default).

Press **Save**. The ingest listens within seconds. On the studio PC, send VBAN to the station's
address and port with that stream name, at 48 kHz (16-bit, 24-bit, 32-bit or float; up to eight
channels).

The source stands by at the desk with the way **vban**, as any host's own studio does. Packets
from any other address or under any other name are dropped and counted; the clock's figures show
the counts by reason.

## 3. Send VBAN from the desk

A **preset** sets the routing in one press; type the receiving machine's address first:

| Preset | What the desk sends |
|---|---|
| Studio PC on the network | the live programme, before the profanity delay, to the studio's monitors |
| Transmitter feed | the programme after the delay, what listeners hear, in 24-bit |
| Local guest | the first VBAN input's own monitor mix (everyone but itself) back to its sender |
| Custom | the rows as you set them |

Each output goes to one address and port, never broadcast or multicast, at 48 kHz stereo, 16-bit
by default (`s24` and `f32` by choice). In the kit's settings an output's `source` may also be a
person's mix-minus (`mix_minus:<identity>`).

## 4. Read the clock

The panel's **One clock** table shows each host's own studio at the desk with its clock against
the desk's, in parts per million. The desk resamples each source once, at a rate steered to keep
it a fixed cushion ahead of the mix, so a sound card a little fast or slow is followed instead of
skipping every hour or so. **Placed afresh** counts the gaps it could not follow (a pause, a
restart).
