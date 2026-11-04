---
title: "The desk's control socket"
description: "The Unix socket the desk listens on: its requests (people, the dump, the carts, the picture, the mode), its answers, and the status it reports."
type: reference
status: draft
last_verified: 2026-11-04
sidebar:
  order: 9
---

The desk listens on a Unix socket, set by `[control] socket` in its settings, readable and writable by the desk's own user only (mode `0600`).
It is a socket, not a network port, so there is nothing to expose (ADR-0088).
Each request is one line of JSON, and each answer is one line of JSON.

The control plane is the socket's usual client: the studio app's keys reach the desk through the control plane's [routes](/docs/reference/control-api/), which call this socket.
An operator on the server can use it directly.

## Requests

| Request | Does |
|---|---|
| `{"op": "status"}` | lists everyone the desk knows and the playout link |
| `{"op": "on_air", "identity": "callum", "on": true}` | puts a person on air, or takes them off |
| `{"op": "mute", "identity": "gregor", "on": true}` | mutes a person on every mix they air on, keeping their on-air state |
| `{"op": "cue", "identity": "finlay", "on": true}` | cues a person: heard by presenters and DJs, never by listeners |
| `{"op": "talk", "identity": "moira", "on": true}` | holds or releases a screener's or producer's talkback to presenters and DJs |
| `{"op": "talk_to", "identity": "moira", "to": "callum", "on": true}` | holds or releases talk from one person to one other; while held, the talker is off the programme |
| `{"op": "dump"}` | empties the profanity delay and answers `{"ok": true, "delay": {…}}`, the delay as it is then; allowed while it rebuilds, refused only when nothing is held (no delay, or just after a dump) |
| `{"op": "cart_fire", …}`, `cart_stop`, `cart_stop_all`, `cart_plays` | fire, fade and list carts, and read their play events: see [Carts in the desk](/docs/reference/desk-carts/) |
| `{"op": "picture", "mode": "auto"}`, `caption`, `now_playing`, `thumbs` | the picture's mode, a caption over it, the slate's text, a small JPEG of each camera (video on; with video off they answer the code `video_off`): see [Video in the desk](/docs/reference/desk-video/) |
| `{"op": "mode", "mode": "auto"}` | the station's mode, `live` or `auto` (any other word answers `invalid`): see [The station's mode](#the-stations-mode) |

A request that succeeds answers `{"ok": true}`, or `ok: true` with what it asked for (`status`, `dump`, the cart words, `thumbs`).
A request for someone the desk does not know answers `{"ok": false, "error": "…"}`, as does a line that is not a request; some errors also carry a `code`, such as `invalid` or `video_off`.

## Status

`status` answers with everyone the desk knows, whether playout is connected, the station's mode ([below](#the-stations-mode)), the delay, the mixing pass, and the carts sounding (`carts`, described in [Carts in the desk](/docs/reference/desk-carts/)):

```json
{"ok": true, "people": [], "playout": {"connected": true, "for_ms": 5120}, "mode": "live",
 "delay": {"delay_ms": 8000, "target_ms": 8000, "rebuilding": false, "dumps": 0, "dumped_ms_ago": null},
 "mix": {"blocks": 9000, "late": 0, "max_pass_us": 410, "realtime": false},
 "carts": {"ducked": false, "mic_open": false, "sounding": [], "play_seq": 0, "started_ms": 1762214400000}}
```

| Field | Meaning |
|---|---|
| `delay` | the profanity delay: how far behind listeners are now (`delay_ms`), its length (`target_ms`, 8 s by default), whether it is building back up, the dumps since the desk started, and how long ago the last one was |
| `mix` | the mixing pass: blocks mixed, blocks mixed without a voice that came too late, the longest pass in microseconds, and whether the mixing thread runs with real-time scheduling |

Each entry in `people` carries:

| Field | Meaning |
|---|---|
| `identity`, `name`, `role` | who they are, as their token says |
| `on_air`, `mute`, `cue` | their switches |
| `talk` | holding talkback to presenters and DJs |
| `talk_to` | the identity they hold talk to, or `null` |
| `present` | in the room now |
| `away_ms` | how long they have been out of the room, while they are; their mix-minus is kept for 30 s |
| `linked` | their voice reaches the desk |
| `lufs`, `lufs_peak` | short-term loudness over 3 s, and the highest momentary loudness in the same 3 s; `null` for silence |
| `underruns`, `late_ms`, `ring_ms` | their voice's buffer at the desk: blocks it ran out within, audio that arrived after its place was read and was dropped, and how much it holds now |
| `camera` | their camera has a picture at the desk (video on) |

`playout.connected` says whether Liquidsoap is connected to the desk's programme output; `for_ms` is how long it has been in that state, or `null` before playout has ever connected.
The studio app's lamp reads it: see [States](/docs/reference/states/).

With video on, the status also carries `video` and `sound`, the video stream's sound: `source` (`desk` or `playout`), `live`, `feed_connected` and `feed_connections` ([Video in the desk](/docs/reference/desk-video/#the-sound)).

## The station's mode

The station runs in one of two modes, which playout follows (`programme.mode`): `live`, the desk first with automation when the desk is away or silent too long, or `auto`, automation only, the desk never on air.
The mode is a playout setting (`GET`, `PUT /api/v1/playout/settings`).

The desk starts in `live` and reports the mode it was last told as `mode` in its status.
The control plane compares it with the setting every 2 s, and at once when the setting changes, and sends `mode` only when they differ: a desk that started again is told within a few seconds, and a control plane that started again sends nothing until it has read the setting.
A desk too old to report a mode is not sent one.

What the desk does with it: in `auto` the video stream's sound is playout's, whoever is live at the desk ([The sound](/docs/reference/desk-video/#the-sound)).
The control plane also puts the mode in the studio view (`mode`, `null` until it has read the setting); the studio app's master lamp says AUTO from it (ADR-0183, [The state vocabulary](/docs/reference/states/#the-master-lamp-desklamp)).

## From a shell on the server

The path is the desk's `[control] socket`.
In the Compose bundle the socket is in the `desk-control` volume, which the desk and the control plane mount at `/run/weavesignal`; `socat` is not in the images, so run it where it is installed and the socket is reachable.

```sh
echo '{"op": "status"}' | socat - UNIX-CONNECT:/run/weavesignal/desk.sock
```
