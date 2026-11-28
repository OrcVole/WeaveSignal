---
title: "Carts in the desk"
description: "How a fired cart reaches every ear: the control plane's cart routes, the desk's cart players and its socket words, cue, ducking, the play events, the media worker, and loading a pack."
type: reference
status: draft
last_verified: 2026-11-21
---

A cart is a row in PostgreSQL (`cart_bank`, `cart`) with a FLAC file, a gain, a mode and an optional ramp.
Its bank is the station's, shared by every show, or one show's own (from 1.0.2): the station's banks and each show's are numbered 1 to 5 apart ([Cart wall](/docs/reference/surfaces/carts/#a-cart-wall-per-show)).
When it fires, the control plane tells the desk over the desk's [control socket](/docs/reference/desk-control/), and the desk plays the file as a source of its one mixing pass (ADR-0117).
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

All take a staff bearer: a staff session or the station key ([Add staff](/docs/how-to/add-staff/)).

| Route | Does |
|---|---|
| `GET /api/v1/carts` | the station's banks and carts, each with `missing`; with `?show=<slug>`, that show's own beside them |
| `POST /api/v1/carts/commands` | `toggle`, `fire`, `stop`, `cue` a cart; `stop_last`; `fade_all`; `clear_cue` |
| `PUT /api/v1/carts/banks/{n}` | makes or renames a station bank: `{"name", "colour_slot"}` |
| `PUT /api/v1/carts/banks/{bank}/slots/{slot}?label=&mode=` | an audio file as the body; the media worker makes it the station slot's cart |
| `PATCH /api/v1/carts/banks/{bank}/slots/{slot}` | changes the slot's cart and keeps its sound (from 1.2.1): any of `label`, `detail`, `mode`, `ramp_ms` (`null` takes the ramp away) and `licence`; `bank` and `slot` move it to an empty slot of the station's banks |
| `DELETE /api/v1/carts/banks/{bank}/slots/{slot}` | empties a station slot, and deletes its file |

A show's own banks have the same routes under the show (1.0.2), and one more to remove a bank:

| Route | Does |
|---|---|
| `PUT /api/v1/shows/{show}/carts/banks/{n}` | makes or renames the show's bank `n` |
| `DELETE /api/v1/shows/{show}/carts/banks/{n}` | removes it with its carts and their files; a cart of it that sounds is stopped |
| `PUT /api/v1/shows/{show}/carts/banks/{bank}/slots/{slot}?label=&mode=` | an audio file into the show's slot |
| `PATCH /api/v1/shows/{show}/carts/banks/{bank}/slots/{slot}` | changes the show's slot's cart and keeps its sound (from 1.2.1); a move stays within the show's own banks |
| `DELETE /api/v1/shows/{show}/carts/banks/{bank}/slots/{slot}` | empties the show's slot |

They change only that show's banks, never another show's or the station's.
The people who may change a show's step may change its banks: the station key, an admin, a producer, or the show's presenter; a preparing window may, as its own show is not on air; an API token may not.
Each answers with the station's catalogue and the show's; the station's routes answer with the station's alone.

### Changing a cart without its sound

`PATCH` on a slot (from 1.2.1) changes the cart where it is: a field left out stays as it is.
The cart keeps its id and its file, so one that is sounding goes on sounding and the play log still names it.
A move (`bank`, `slot`) goes only to an empty slot of the same owner's banks: onto a slot that holds a cart it is 409, naming the cart in the way, so a move never replaces anything.
An empty slot is 404, as is a bank nobody has made; a name that is not 1 to 120 characters, a slot off the wall or a ramp longer than the cart is 422.
To change the sound, `PUT` a file to the slot: that makes a new cart there, with a new id, and deletes the old file.
The station's route takes what its `PUT` and `DELETE` take (any staff session, the station key, or an API token with `carts:write`); a show's, those who may change its banks, and never a token.

### What an upload is held to

`GET /api/v1/carts` carries the limits beside `loudness_lufs` (from 1.2.1): `max_upload_mb`, the largest file a slot takes (`carts.max_upload_mb`, 64), and `max_seconds`, the longest cart (`carts.max_seconds`, 600).
The studio app prints them in its cart dialog and refuses a larger file before sending it.
A body over the limit is 413 without a sentence; a sound too long, silent or not a sound is 422 with the media worker's sentence, *the file was refused: …*; a ramp (`ramp_ms` in the query) longer than the sound is 422, and nothing is written.
On Cloudron the proxy in front of the control plane passes bodies up to 200 MB, above the default 64.

### Loading the default pack again

A station loads its pack (`carts.demo_pack`) by itself only while it has no station banks. From 1.2.1 two routes load it on asking:

| Route | Does |
|---|---|
| `GET /api/v1/carts/pack` | the pack: `available` (false, with `problem`, on a station that has none), its `banks` with each sound's `slot`, `label` and `mode`, whether one is `loading`, and `untouched`, the ids of the station's carts a load may replace or remove |
| `POST /api/v1/carts/pack` | `{"loads": [{"bank": 1, "into": 1}], "replace": false}`: loads the pack's bank `bank` into the station's bank `into` (1 to 5; made under the pack's name if it has not been); several at once, no station bank twice |

A station bank that holds carts is refused (409) unless `replace` is true, and the refusal says what would happen: *bank 1 holds 14 carts: 12 default carts nobody changed would be replaced or removed, and 2 carts of yours would stay; send `replace: true` to go on*.
With `replace`:

- a cart is replaced only if it is **a default cart nobody changed**: it came from a pack (`origin` is `demo:<id>`), and still has the name, slot, mode and licence that pack gave it, with no second line and no ramp. The bank is not part of the test, as one of the pack's banks may be loaded into another: a default cart moved through the API to the same slot of another bank still counts. The pack the station has now counts, and so does the pack shipped from 1.0.0 to 1.2.0, which the control plane lists (`crates/control/src/carts/pack.rs`), since a newer pack takes its files' place;
- a default cart nobody changed whose slot the pack has no sound for is removed;
- every other cart stays: one a person uploaded, renamed, moved to another key, or whose mode, second line, ramp or credit they changed. The pack's sound for its slot is left out. A cart that comes into a slot while the load runs stays too;
- a cart that is sounding is never stopped by a load: it stays as it is, and the pack's sound for its slot is left out this time.

The answer is 202 with the plan for each bank (`load`, `replace`, `remove`, `keep`, `left_out`); the sounds then go through the media worker one by one, counted in the studio view's `carts.importing`. A second load while one runs is 409; a station with no pack is 503.
Who may: as the station's slot routes (any staff session, the station key, or an API token with `carts:write`; reading needs `carts:read`). The pack goes into the station's banks only.

Every bank and cart in the catalogue names its show (`show`, the show's slug), or `null` for the station's.
A cart in a show's bank starts (`toggle`, `fire` or `cue` when it is not sounding) only while that show is the one the station's step comes from: the show on air, or between shows the show last on air ([Choose how much of the studio a show uses](/docs/how-to/choose-a-step/)).
Otherwise the answer is 409, *“Woof” is in another show's own bank: it fires only while that show is the one on air*.
Stopping it, `stop_last`, `fade_all` and `clear_cue` work on anything sounding, so a bed left over from the show before can always be faded.

The studio's view (`GET /api/v1/studio` and its event stream) carries `carts`: what is sounding (each with its `label`, `cue` and `play`), FILE MISSING, whether the desk answers (`desk`, `desk_error`), whether beds are dipped and a microphone is open, and the catalogue's version.

## The media worker

Every upload goes through `weavesignal-control media-worker`, a separate process that restricts itself (Landlock: no files, no network; no new processes; CPU and memory limits) before it reads a byte.
It reads WAV, FLAC, MP3, Ogg Vorbis, AAC in MP4 and Ogg Opus (surround folded to stereo), measures EBU R128 loudness and true peak, and writes 24-bit FLAC at the source's own rate.
The gain that brings the cart to the station's cart loudness without pushing its true peak over `carts.ceiling_dbtp` (−1) is stored with the cart and applied by the desk when it fires.
The control plane writes the files to `carts.media_dir`; the desk reads the same directory (`[carts] dir`), read-only in the Compose bundle.

### The carts' loudness

Every cart is brought to one loudness, in LUFS (EBU R128), when its sound is loaded:

- **A station installed with 1.3.3 or later: −16 LUFS**, the level internet radio streams at (and playout's processing aims for).
- **A station from an earlier release: −23 LUFS, as before.** Upgrading does not change it. 1.3.3's first start marks every station already in the database as an older one (migration 0073), and that station keeps −23 for as long as nothing sets a level.
- **A level that is set wins over both:** `carts.loudness_lufs` in the settings file, or `CARTS_LOUDNESS_LUFS` on the Compose bundle, the Quadlets and the Cloudron package (−40 to −5). Empty or left out means the station's own default above.

The control plane's log says the level at start (*carts are brought to this loudness*), and `GET /api/v1/carts` gives it as `loudness_lufs`.
At −16 a short, sharp sound (a rimshot, a click) is more often held below the level by the −1 dBTP ceiling than at −23.

**To move a station from an earlier release to −16** (or any station to another level):

- **Compose bundle and Quadlets:** add `CARTS_LOUDNESS_LUFS=-16` to the settings file (`deploy/compose/.env`), then run `weavesignal.sh` again, which keeps the line.
- **Cloudron:** in `/app/data/weavesignal.env`, set `CARTS_LOUDNESS_LUFS=-16` (the package writes the line empty), then restart the app.
- **Your own `control.toml`:** set `loudness_lufs = -16.0` under `[carts]`, then restart the control plane.

**Carts already on the wall are not re-levelled.** A cart's gain is worked out once, when its sound is loaded, and kept with it, so after a change the wall holds carts at two levels until each is loaded again:

- a cart of your own: load its sound again, in the studio with the cart's **Replace the sound…** and the same file ([Add your own cart](/docs/how-to/add-your-own-cart/)), or through the API by putting the file to its slot with its words;
- the default pack's carts nobody changed: load the pack again with `replace` ([Loading the default pack again](#loading-the-default-pack-again)), which brings each in at the new level and leaves your own carts as they are.

The phone line's own recording of the phone words (17f) keeps −23 on a new station, as before, unless a cart level is set; a set level applies to it too.

## Loading a pack

```sh
weavesignal-control --config control.toml carts load-pack assets/carts-demo
```

Or set `carts.demo_pack`: a station with no banks of its own loads it at first start (a show's own banks do not count).
A pack always loads into the station's banks.
