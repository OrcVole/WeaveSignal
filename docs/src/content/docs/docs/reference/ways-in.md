---
title: "Ways in for a host's sound"
description: "The six ways a host's own studio, a partner station or a studio PC reaches the desk in 1.2 (Icecast, SRT, WHIP, the bridge, VBAN, and a stream the station pulls): who connects, the port and setting, the formats, how the key is checked, and on which install routes each works."
type: reference
status: draft
last_verified: 2026-11-21
sidebar:
  order: 30
---

A show whose host presents from their own studio (ADR-0280) gets its sound to the desk one of six ways.
Whichever way, the source is a **show key's** source: outside the show's time it is a private test, in it the source stands by on the desk, levelled, until put on air, and the post, the grace, LINK LOST and the monitor link work the same.
The ingest decodes it to 48 kHz and hands the desk the sound only: a picture that comes with it is not used ([A host's own picture](/docs/reference/desk-video/#a-hosts-own-picture)).
The API and the desk name the way as `way`.

| `way` | Who connects | Port (setting) | Formats | The key | Install routes | How-to |
|---|---|---|---|---|---|---|
| `icecast` | the host's software, as an Icecast source (BUTT, Mixxx, RadioDJ) | TCP 8005 (`SOURCE_ICECAST_PORT`), off unless set | MP3, AAC, Opus, Vorbis, FLAC | the source password: the key's id, a hyphen and its secret, in clear like every Icecast password | all three | [Set up a host in their own studio](/docs/how-to/set-up-a-host-in-their-own-studio/) |
| `srt` | the host's software, as an SRT caller (OBS, vMix, ffmpeg) | UDP 9710 (`SOURCE_SRT_PORT`), off unless set | AAC or MPEG audio in MPEG-TS (Opus or AC-3 in MPEG-TS is refused at the stream's start, with its name) | the stream ID is the key's id; the secret is the passphrase, which encrypts the sound | all three | [Set up a host in their own studio](/docs/how-to/set-up-a-host-in-their-own-studio/) |
| `whip` (1.2) | OBS 30 and later, its WHIP output | UDP 9712 (`SOURCE_WHIP_PORT`), off unless set; the offer over HTTPS at `/whip/<key id>` on the public pages' domain | Opus over WebRTC; OBS's picture is taken and set aside | the key's secret as OBS's *Bearer Token* | all three; needs the public domain and the station's public IPv4 address | [Send from OBS over WHIP](/docs/how-to/send-from-obs-over-whip/) |
| `bridge` (1.2) | the studio app on the host's computer, carrying their software's Icecast or SRT from loopback | none of its own: one WebSocket to `/api/v1/sources/bridge` on the public address's 443, passed to the ingest on loopback (`SOURCE_BRIDGE_PORT`, 7320; 0 turns it off) | what the host's software sends the app: MP3, AAC, Ogg (Opus or Vorbis) or FLAC | the key's Icecast password, kept in the host's keychain | all three | [Send through the bridge](/docs/how-to/send-through-the-bridge/) |
| `vban` (1.2) | a studio PC on the station's own network (Voicemeeter and others) | UDP 6980 on the station's own local address, the presenter kit's settings; off unless `KIT_VBAN=true` | VBAN audio, 48 kHz, 16-bit, 24-bit, 32-bit or float, up to eight channels (a pair taken) | none in the protocol: only a bound sender's address and stream name are taken | Compose and the Quadlets, never Cloudron | [Connect a studio PC over VBAN](/docs/how-to/connect-a-studio-pc-over-vban/) |
| `pull` (1.2) | the station itself, from before the show's time | none: it connects out | HTTP or HTTPS audio (MP3, AAC, Opus, Vorbis, FLAC), HLS (MPEG-TS, AAC or MP3 segments), SRT caller (MPEG-TS) | the key holds the address; addresses inside the station's own network are refused unless `WEAVESIGNAL_PULL_ALLOW_PRIVATE` is set | all three | [Pull a partner's stream into a show](/docs/how-to/pull-a-stream/) |

Every port is a setting and is checked free by `weavesignal.sh` ([Ports](/docs/reference/ports/)); on Cloudron the public ones are chosen in the dashboard.
The three ports a host's software reaches directly (Icecast, SRT, WHIP) listen only while a show key exists; the ingest counts wrong keys by the address it sees, and five from a public address in a minute refuse it for ten minutes.

## Which to choose

- **A host with OBS**: SRT encrypts and carries the most software; WHIP needs no port forwarding on the host's side but only sound is used, and it has been tried with GStreamer's WHIP client and, on one machine on 1.2.0, with a real OBS (33.0.2), not yet from OBS across the internet.
- **A host behind a router who cannot open a port, or a station with no source port open** (a Cloudron station by default): the bridge.
- **A partner station already streaming**: a pull, made in the show editor or through the API.
- **A studio PC beside the station, on the same network**: VBAN (no password, so that network only), or SRT, which encrypts.
