---
title: "Carts in the desk"
description: "How a fired cart reaches every ear: the control plane's cart routes, the desk's cart players and its socket words, cue, ducking, the play events, the media worker, and loading a pack."
type: reference
status: draft
last_verified: 2026-11-04
---

A cart is a row in PostgreSQL (`cart_bank`, `cart`, per station) with a FLAC file, a gain, a mode and an optional ramp.
When it fires, the control plane tells the desk over the desk's [control socket](/reference/desk-control/), and the desk plays the file as a source of its one mixing pass (ADR-0117).
So a cart is heard by listeners and in every return, the presenter's and the guests' alike; it sits inside the profanity delay, and a dump takes it with everything else held; and the limiters on the programme and on every return come after it.

## The path of a key

1. The studio app sends `POST /api/v1/carts/commands` with `{"op": "toggle", "cart": "<id>"}`.
2. The control plane checks the cart's file is on the disk and sends the desk `{"op": "cart_fire", "cart": "<id>", "file": "<uuid>.flac", "gain_db": -7.0, "mode": "one_shot", "cue": false, "duration_ms": 3000}`.
3. The desk checks every word, opens the file from its cart directory (`[carts] dir`), starts a small decoder of its own for it, and gives it a free player from the next 20 ms block.

Measured on loopback (`just test-carts`, the desk's delay at 3 s): the desk starts the cart 4 ms after the key; the listener stream carries it 17.0 s later, 3 s of it the delay and the rest playout and `icecast.server`'s buffer.

## The players

Eight players for one-shots and sweepers, two for beds; a ninth one-shot is refused as busy.
Each sounding cart is decoded from its FLAC by a GStreamer pipeline of its own (converted and resampled to the desk's 48 kHz stereo) about a second ahead of where it plays; a file that cannot be decoded ends that cart and nothing else.
A bed loops without a gap: its decoder goes back to the start while the second it holds plays on.

| Mode | Plays |
|---|---|
| one-shot | once, at its stored gain |
| sweeper | once; beds on air dip while it sounds |
| bed | until stopped; it dips 12 dB over 250 ms while a microphone is open on air (a presenter, DJ, guest or caller on air, present and not muted) or a sweeper sounds, and comes back the same way |

Stopping a cart fades it out over 300 ms.
The dip is worked out by the desk's mixing pass from the same switches it mixes with; the control plane only shows it.

## Cue

`⇧` and a cart's key fire it with `"cue": true`: it plays in the presenters' and DJs' returns only (the same ears a cued person reaches), never in the programme, a guest's or a caller's return, or the delay.
`⇧` and the key again take it out; `Esc` on the Carts tab takes every cued cart out; its key alone takes it out of the cue and fires it on air from its start.
A cart on air cannot be cued as well, a cued bed does not dip, and `Space` fades only carts on air.

## The desk's cart words

On the desk's control socket, one JSON line each way.

| Request | Does |
|---|---|
| `{"op": "cart_fire", "cart", "file", "gain_db", "mode", "cue", "duration_ms"}` | plays `file` (a UUID and `.flac`, in `[carts] dir`) at `gain_db` (−40 to 20); answers `{"ok": true, "pool": "shot", "player": 3, "play": 17}`, or `ok: false` with `code`: `invalid`, `missing`, `busy` or `decode` |
| `{"op": "cart_stop", "cart"}` | fades every player sounding that cart; answers `stopped` |
| `{"op": "cart_stop_all"}` | fades everything; with `"cue": true` only what is cued, `false` only what is on air |
| `{"op": "cart_plays", "after": 41}` | the play events after number 41, oldest first |

The desk's `status` carries `carts`: `ducked`, `mic_open`, `sounding` (each with `cart`, `pool`, `player`, `mode`, `cue`, `play`, `elapsed_ms`, `remaining_ms`, `loop`, `stopping`, `ducked`), `play_seq`, the last play event's number, and `started_ms`, when this desk started (a desk that starts again numbers its play events from 1 again).

## Play events, for the per-play log

Every fire and every end is a play event at the desk: `seq`, `play` (a fire and its end share it), `event` (`fired` or `ended`), `cart`, `mode`, `pool`, `player`, `cue`, `at_ms` (the desk's clock; listeners hear it the delay later), and for an end `played_ms` and `end` (`ran_out`, `stopped` or `failed`, with `error`).
The desk keeps the last 1,024 and numbers them from 1 each time it starts.
The control plane reads the new ones whenever `play_seq` moves, records each once (the last 512 in memory), logs it under `weavesignal_control::plays`, and sends it on `Hub::subscribe_plays`, where the per-play log listens.

## The control plane's cart routes

All take a staff bearer: a staff session or the station key ([Add staff](/how-to/add-staff/)).

| Route | Does |
|---|---|
| `GET /api/v1/carts` | banks and carts, each with `missing` |
| `POST /api/v1/carts/commands` | `toggle`, `fire`, `stop`, `cue` a cart; `stop_last`; `fade_all`; `clear_cue` |
| `PUT /api/v1/carts/banks/{n}` | makes or renames a bank: `{"name", "colour_slot"}` |
| `PUT /api/v1/carts/banks/{bank}/slots/{slot}?label=&mode=` | an audio file as the body; the media worker makes it the slot's cart |
| `DELETE /api/v1/carts/banks/{bank}/slots/{slot}` | empties a slot, and deletes its file |

The studio's view (`GET /api/v1/studio` and its event stream) carries `carts`: what is sounding (each with `cue` and `play`), FILE MISSING, whether the desk answers (`desk`, `desk_error`), whether beds are dipped and a microphone is open, and the catalogue's version.

## The media worker

Every upload goes through `weavesignal-control media-worker`, a separate process that restricts itself (Landlock: no files, no network; no new processes; CPU and memory limits) before it reads a byte.
It reads WAV, FLAC, MP3, Ogg Vorbis, AAC in MP4 and Ogg Opus (surround folded to stereo), measures EBU R128 loudness and true peak, and writes 24-bit FLAC at the source's own rate.
The gain that brings the cart to `carts.loudness_lufs` (−23 by default) without pushing its true peak over `carts.ceiling_dbtp` (−1) is stored with the cart and applied by the desk when it fires.
The control plane writes the files to `carts.media_dir`; the desk reads the same directory (`[carts] dir`), read-only in the Compose bundle.

## Loading a pack

```sh
weavesignal-control --config control.toml carts load-pack assets/carts-demo
```

Or set `carts.demo_pack`: a station with no banks loads it at first start.
