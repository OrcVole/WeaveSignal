---
title: "Send through the bridge"
description: "Send your show from your own software through the studio app: your software sends to the app on your computer, and the app carries it to the station over the web's usual port, so the station needs no port for sources and your network needs nothing but HTTPS."
type: howto
status: draft
last_verified: 2026-11-20
sidebar:
  order: 40
---

The **bridge** carries what your own software sends (BUTT, Mixxx, OBS, Voicemeeter) to the
station through the studio app.
Your software sends to the app on your own computer, as it would to any Icecast server or SRT
listener; the app sends it on over one connection to the station's public address on port 443.
Use it when the station has no port open for sources (a Cloudron station, by default), or when
your network lets nothing through but the web.

Your source arrives at the desk **standing by** with the way **bridge**, exactly as a push over
Icecast does: the post, LINK LOST and your monitor link work the same
([Set up a host in their own studio](/docs/how-to/set-up-a-host-in-their-own-studio/)).

## What you need

- The studio app on your computer (Linux or Windows). A browser has no bridge.
- A show key for your show: its **Icecast password**, the key's id, a hyphen and its secret, as
  the key's details show it once.
- The station's **public address**, where listeners go. On Cloudron that is the public pages'
  domain, not the studio's, which is behind Cloudron's login; the bridge needs the station to
  have a public domain.

Your software mixes your voice and your music together: the station hears one stream, as it
does from push. You talk to listeners, not with co-hosts; a co-host or callers stay in the room.

## 1. Save the station and the key

1. Open **Settings** and find **Bridge** (`settings.bridge`).
2. Type the station's public address, like `https://radio.example.org`.
3. Paste the key's Icecast password into **Show key**. It is kept in your computer's keychain
   and never shown again.
4. Leave the two ports (8100 for Icecast, 9100 for SRT) unless another program uses them.
5. Press **Save**.

## 2. Point your software at the app

**In your software** shows what to type. For Icecast (BUTT, Mixxx):

| Setting | Value |
|---|---|
| Server | `127.0.0.1` |
| Port | `8100` |
| Mount | any, such as `/live` |
| User | `source` |
| Password | the **local password** the panel shows |
| Format | MP3, AAC, Ogg (Opus or Vorbis) or FLAC |

For SRT (OBS, ffmpeg): `srt://127.0.0.1:9100?passphrase=<local password>&pbkeylen=16`, in
MPEG-TS, sound only.

The local password is made by the app the first time and kept in your keychain. It only opens
the app's ports on your own computer, which nothing outside it can reach; press **New local
password** to change it (with the bridge stopped) and type the new one into your software.

## 3. Start

1. Press **Start the bridge**. **Now** says *Waiting for your software to connect on this
   computer*.
2. Connect from your software. **Now** says *Connecting to the station…*, then the desk's
   answer: *Standing by at the desk*, or *a private test* outside your slot, or why the
   station refused (a wrong key, not your slot).
3. Open your monitor link to hear the programme and the desk's talkback.

If the connection to the station drops, the app tries again after 1, 2, 4 … up to 30 seconds,
and the desk covers the gap as it does for any dropped link. Up to two seconds of your stream
wait meanwhile; older sound is dropped rather than sent late in a burst. For Ogg and FLAC the
app sends the stream's header pages again first, so the station can start reading mid-way.

If **Now** says *Your software sends more than the station takes*, your software is sending
more than about 1.6 Mbit/s, usually a picture: send sound only.

**Stop the bridge** closes your software's connection, and your source leaves the desk.

Closing the studio app's window stops the bridge too, so while it runs the app asks first:
**Keep the window open** (already chosen; Esc does the same) or **Stop the bridge and close**.
Pressing the window's close button again while the question is showing closes the app without
waiting for an answer.

## Questions

**Why not send to the station's own ports?** You can, if the station opens them
([Set up a host in their own studio](/docs/how-to/set-up-a-host-in-their-own-studio/)). The bridge
needs none, works through networks that block other ports, and keeps the key out of your
software's settings.

**Is it slower?** About the same as push over Icecast on a clean line: 0.4 to 0.6 s to the
programme. It goes over TCP, so a lossy line waits rather than drops; the desk's cushion covers
short waits.
