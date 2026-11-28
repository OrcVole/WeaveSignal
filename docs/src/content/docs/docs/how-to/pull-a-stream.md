---
title: "Pull a partner's stream into a show"
description: "Let the station open a partner station's or a host's stream itself, over HTTP, HTTPS, HLS or SRT, from before the show's time: the source stands by on the desk and goes on air at the post as a host's own studio does. Made in the show editor or with the API; the address is checked against the station's own network."
type: howto
status: draft
last_verified: 2026-11-20
sidebar:
  order: 40
---

A show can come from a stream that already exists: a partner station's Icecast mount, its HLS
playlist, or an SRT listener a host runs.
Instead of the host's software connecting to the station
([Set up a host in their own studio](/docs/how-to/set-up-a-host-in-their-own-studio/)), the
station **pulls** the stream: it opens the address itself from before the show's time.
From there it is a source like a host's own studio: a tile on the desk standing by (STANDBY),
on air at the show's post, the grace, LINK LOST when the stream drops, and a monitor link.

## What you need

- A WeaveSignal station from 1.2, with the ingest running beside the desk.
- An admin, a producer or the show's presenter, signed in, to make the pull. An API token cannot.
- A show with episodes in the schedule: the station opens the stream only around the show's times.
- The stream's address, in one of these forms:

| Address | What the station does | What it reads |
|---|---|---|
| `http://…` or `https://…` | asks for it once and plays what comes (an Icecast mount, any HTTP audio) | MP3, AAC, Opus, Vorbis or FLAC, by the server's `Content-Type` |
| an address ending `.m3u8` | follows the HLS playlist, fetching each new segment (a master playlist's first stream) | MPEG-TS, AAC or MP3 segments; not fMP4 |
| `srt://host:port` | calls the SRT listener, with `streamid=` when the address has one | MPEG-TS with AAC or MPEG audio |

The address may hold no user name or password, and an SRT address no passphrase: the station
does not keep a password for a pull. Ask the partner for an address without one, or have the
host send to the station with a show key instead.

## 1. Make the pull

In the studio app, open the show's editor, choose **A stream the station pulls** under **Comes from**, give the address and a name for the desk, and press **Pull this stream**.
Or make it with the API, as a show key whose `hint` is `pull` and with the address as `url`:

```sh
curl -X POST https://station.example/api/v1/shows/steeped/keys \
  -H "Authorization: Bearer $SESSION" -H 'Content-Type: application/json' \
  -d '{"label": "Elspeth Gordon", "hint": "pull", "url": "https://radio.example.org/live.m3u8"}'
```

- `label` is the name on the desk's tile: the partner station, or the host whose stream it is.
- The station checks the address before it keeps it: the form, then the name resolved, and every
  address it gives must be on the internet, not inside the station's own network (see below).
  A refusal is a 422 that says what is wrong.
- The reply holds the monitor link's secret, **once**, as for any key; send the monitor link to
  whoever presents from that stream if they want to hear the station's return.
- The show's **Comes from** now says **Pulled by the station**, with the address.

## 2. What happens at the show's time

- **From 30 minutes before the show** (the rules' *how early a host may connect*), the station
  opens the stream. When it can, the source stands by on the desk under **Not on air**, as a host
  does. `GET /api/v1/sources` lists it with `"pull": true`, and `way` is `pull` while it is open.
- **At the show's post** it goes on air by the same rules as a host's own studio: at once when
  automation holds the air, after the grace when a DJ is on air
  ([What the station does at the show's post](/docs/how-to/set-up-a-host-in-their-own-studio/#what-the-station-does-at-the-shows-post)).
- **When the stream drops**, the station opens it again after a second, then after 2, 4, 8 and
  up to 30 seconds, and back to a second once it has held for half a minute. While it was on air
  the drop is LINK LOST, and automation covers until it is back.
- **When the slot's door closes** (the next show takes the air, or two hours after this one
  ends), the station closes the stream, unless it is on air: then it stays open until it comes
  off the air. A pull is never a private test.
- **A playlist that has ended** (`#EXT-X-ENDLIST`) is not opened again: the station pulls live
  streams, not recordings.

An HLS stream plays a few segments behind the partner's live edge (often 10 to 30 seconds), and
the station feeds it to the desk at the pace it plays.

## If something goes wrong

- **The source never stands by.** The ingest's log says why the station could not open it: the
  name does not resolve, the server answered 404, a type it cannot read, or an address inside
  the station's own network. The key's log keeps the same words, once for each new reason.
  The station keeps trying while the door is open.
- **Disconnect** on the tile closes it and turns the pull away for the rest of that show, as for
  a host. **Let back in** on the key's row, or `DELETE /api/v1/sources/{key}/disconnect`.
- **To stop pulling for good**, end the key (`DELETE /api/v1/shows/{show}/keys/{key}`). When it
  was the show's last key, the show comes from the room again.

## Addresses inside the station's own network

By default a pull may not open loopback, private (10.x, 172.16 to 172.31, 192.168.x),
link-local, carrier-grade NAT or unique-local IPv6 addresses, nor names such as `localhost` or
anything under `.internal`, `.local`, `.lan` or `.home.arpa`.
The check runs when the address is saved and again at every connection the station makes,
redirects and HLS segments included, after each name is resolved, so a name that changes what it
points to cannot get past it.
A station whose encoder is on its own network, or a test station, can allow it by setting
`WEAVESIGNAL_PULL_ALLOW_PRIVATE=1` in the control plane's environment
([Settings](/docs/reference/settings/)).
On Compose and the Quadlets that is `PULL_ALLOW_PRIVATE=true` in the settings file beside
`compose.yaml`; run `weavesignal.sh` again to apply it (from 1.2.1).

## How it is tested

`just test-pull` runs pulls end to end in containers on one machine, with the setting above on.
The control plane, the ingest and the desk come from the station's own image.
Two partners stand in for other stations: an Icecast 2.5 mount fed with MP3, and an SRT listener
sending AAC in MPEG-TS (Liquidsoap sends both).
Each is pulled for its own show, and the test checks that each pull:

- stands by before its show's time, and is heard at the desk;
- goes on air at the show's post: at once from automation, and after the grace from the pull
  before it;
- shows LINK LOST when the partner drops, is opened again by the station, and goes back on air.

It also checks that the first pull is closed once its door shuts.
HLS, HTTPS and a refused address are covered by the ingest's and the control plane's own tests,
not by this one.

In 1.2.0 an SRT pull from a listener built on libsrt (Liquidsoap, FFmpeg, OBS, GStreamer) got no sound; 1.2.1 fixed it.
