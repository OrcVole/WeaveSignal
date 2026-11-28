---
title: "Send from OBS over WHIP"
description: "Let a host send their show's sound from OBS over WHIP with their show key: turn on the station's WHIP port, give the host the WHIP address and the key's secret as OBS's Bearer Token, and their source stands by on the desk like an Icecast or SRT one. Sound only; UDP must reach the station."
type: howto
status: draft
last_verified: 2026-11-21
sidebar:
  order: 40
---

OBS can send to a station over **WHIP**, the WebRTC way in: the host pastes an address and a
token, and OBS does the rest.
WeaveSignal takes WHIP as a third way in for a host in their own studio, beside Icecast and SRT
([Set up a host in their own studio](/docs/how-to/set-up-a-host-in-their-own-studio/)).
The host uses the same **show key**, and their source arrives on the desk **standing by** in their
show's slot, a private test outside it, exactly as over Icecast or SRT: the tile, STANDBY, the
post, the grace, LINK LOST and the monitor link are the same.

WHIP carries **sound only** at this station: OBS sends its picture too, and the station takes it
and throws it away, so OBS needs no change (from 1.2's candidate; the operator's choice). Only
the sound reaches the desk.
The sound is Opus, under half a second to the station, and encrypted on the way.

## What you need

- A station with a **public domain**: OBS posts its offer to
  `https://<public domain>/whip/<key id>`.
- The station's **WHIP port**, UDP, turned on where the station is installed and open to the
  internet: **9712**, or another you choose. It is off until you turn it on.
  - With `weavesignal.sh`: set `SOURCE_WHIP_PORT=9712` and run it with `--public`
    ([Install a station with weavesignal.sh](/docs/how-to/install-with-weavesignal-sh/#hosts-own-studios)).
  - On Cloudron: turn on *Hosts' own studios: WHIP* in the app's Location settings, with the
    public domain on too ([Install on Cloudron](/docs/how-to/install-on-cloudron/)).
- The station's **public IPv4 address**, which it already has for the live room: `NODE_IP`, or the
  A record of the station's domain. The station gives OBS this one address to send to, and asks
  no STUN or TURN server of anyone (ADR-0290). With no address, WHIP stays off and the log says so.
- A show key for the host. Make it as for any host, and choose **WHIP** under **How they will
  send**; the key works every way, so a host may also fall back to Icecast with it.
- On the host's side, OBS 30 or later with WHIP. Ubuntu's own OBS package (the PPA) has no
  WHIP; the Flatpak has it.

## 1. Give the host the WHIP details

When the key is made, its details have a **WHIP** block, with **Copy** on each field:

- **Server**: `https://<public domain>/whip/<key id>`, for example
  `https://kettle.example/whip/mreqm2ys`.
- **Bearer Token**: the key's secret, the same secret as the SRT passphrase. It is shown this once,
  as for every way in. The Icecast password's form, the key id, a hyphen and the secret, is taken as
  well.
- **UDP port**: the station's WHIP port, which the host's network must let through.

**Copy everything as text** puts the WHIP block first when the host said they would send over WHIP.

## 2. The host sets up OBS

In OBS:

1. **Settings** › **Stream**.
2. **Service**: **WHIP**.
3. **Server**: the WHIP address from the details.
4. **Bearer Token**: the secret from the details.
5. **Settings** › **Output**: WebRTC carries Opus, and the station takes Opus only; an audio
   bitrate of 128 kbps or more is right for a show. The station takes up to 2 Mbit/s of sound;
   the picture is not counted against it. The video settings change nothing at the
   station, which discards the picture, but OBS still sends it: on a slow upload, set OBS's video
   bitrate low (a few hundred kbps) so the picture leaves room for the sound.
6. **OK**, then **Start Streaming**.

Outside the show's slot this is a private test, as for any host: nobody hears it, and the host's
row in **Own studios** says TEST with *rtp/opus 48 kHz stereo* and the level.
In the slot, the source stands by on the desk.
**Stop Streaming** ends the session at once.

## When it does not connect

- **401**, *The key is wrong*: the Bearer Token is not the key's secret. Five wrong keys in a
  minute from one address refuse it for ten minutes.
- **409**, *Another connection is using this key*: the host's other software is still sending.
- **503**, *This station has not turned WHIP on*: the WHIP port is off where the station is installed.
- The station answers (the host's row shows a connection), but never hears it, and the source
  ends after 10 s: the sound is not reaching the WHIP port. The station's firewall, or the host's network, blocks UDP.
  WHIP here has no TURN and no TCP fallback, so a host whose network blocks UDP sends over
  **Icecast** instead, with the same key.
- A station reached only over IPv6 cannot take WHIP: its one address is IPv4.
- **OBS on the station's own machine, against a station installed without `--public`**, stays at
  *Connecting* and fails after about 37 seconds, and the station ends the session after 10 s with
  *no sound for 10 s*. Such a station gives OBS one address to send to, `127.0.0.1`, and the
  connection checks never complete: the station's side waits for them, and the machine itself
  carries the packets both ways, so OBS's WebRTC library (libdatachannel with libnice) is the
  likely cause (not proved). It happens only with that loopback address, so it is a limit of
  trying on one machine, not of a station: a real station gives its public address.
  To try OBS on the same machine, give the ingest and LiveKit the machine's own network address
  instead, as the 2026-11-21 tries did, where OBS then connected in about 100 ms: for example
  `INGEST_BIND=192.168.1.20 LIVEKIT_NODE_IP=192.168.1.20 ./weavesignal.sh` (your address in place
  of `192.168.1.20`; the next plain run puts them back on loopback; this way of setting them has
  not itself been tried), then OBS's *Server* is `http://127.0.0.1:<CONTROL_PORT>/whip/<key id>`.

## What has been tried

The station's endpoint has been tried on one machine with two WHIP clients: one of its own (the
ingest's `whip_loopback` test: the answer, the sound at the desk, a wrong key, a second
connection, the end, a host behind a NAT) and GStreamer's `whipclientsink` sending from behind a
NAT (`just test-whip`: standing by, heard at the desk, its end). A real OBS 33.0.2 was tried
against 1.2.0 on one machine on 2026-11-21: it accepted the answer that takes its picture and
discards it, chose Opus by itself, was heard at the desk and on air, and **Stop Streaming**
ended the session; across a real NAT or the internet it has not been tried yet.
From OBS's own pages: WHIP output came in OBS 30.0, and *Settings › Stream* takes *Service* WHIP,
*Server* and *Bearer Token*. Not yet seen with OBS: what it shows when UDP is blocked.
