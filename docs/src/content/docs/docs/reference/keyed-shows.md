---
title: "Keyed shows"
description: "What a listener key is, its kinds, window, limits and states, a show's access rule, the signed token a key earns for one show's window, how the station keeps a keyed show from the public (playout's gate, the origin's keyed paths, the public HLS and RTMP out held back), what an edge checks, the events, the refusal codes, the leak report's score, and what changing the token key does."
type: reference
status: draft
last_verified: 2026-11-25
sidebar:
  order: 34
---

> **From 1.3.2.**
> Keys, rules, tokens and routes, and what keeps a keyed show from the public: playout's public gate, the station's keyed paths, and the public picture and RTMP out held back ([How a keyed show is kept from the public](#how-a-keyed-show-is-kept-from-the-public), with what is not covered yet).
> The player's Enter a key ([Listen with a key](/docs/how-to/listen-with-a-key/)) and the studio app's Keys tab ([Keys tab](/docs/reference/surfaces/keys/)) are built on these.

A keyed show is advertised in public and heard or watched only with a key.
Staff keep keys in the control plane; a listener trades a key's text for a signed token for one show's window, and renews it while listening.
The routes are in the [API reference](/docs/reference/api/) under the `keys` and `listen` tags; how to use them is in [Keep a show for people with a key](/docs/how-to/keep-a-show-for-people-with-a-key/).

## A key

| Field | Meaning |
|---|---|
| `kind` | `personal` (one named person), `group` (many who share it, up to `max_people`) or `machine` (a partner station's software or another program, which redeems the key itself; no devices, no people limit). Public-by-time is not a key: it is a show's `opens_at_ms`. |
| `label`, `note` | who it is for (a name, a group, a ticket number, up to 120 characters), and a note for staff (up to 500). Both are staff's own: the player never shows a key's `label` to its holder, whom it greets with *Welcome* alone |
| email | optional; kept sealed, with a keyed hash for the lost-key request. Shown only by the reveal route |
| `media` | `sound`, `video` or `both` (the default). `sound` is the sound alone. A key for the picture always carries the show's sound, so `video` opens what `both` does (the keyed streams and the HLS sound beside the picture); a key reads back as it was made |
| `all_shows`, `shows` | every keyed show, or the shows named (at most 32) |
| `valid_from_ms`, `valid_until_ms` | the window: inclusive, then exclusive; null for no bound |
| `max_devices` | browsers or apps that may hold it at once (default 2; none for a machine key) |
| `max_listens` | how many times it may start a listen |
| `max_people` | a group key's different devices, ever |
| `state` | `active`, `paused` (switched off and kept) or `revoked` (final) |
| `listens_used`, `first_used_ms`, `last_used_ms` | its use |
| `family_id`, `replaces_id`, `replaced_by_id` | the family that issued it, and the key it replaced or was replaced by |

A key's text is 12 characters from `ABCDEFGHJKMNPQRSTUVWXYZ23456789` (no letter or digit can be read as another), written in three groups: `KETM-7QMP-2D4X`.
Case, spaces and dashes are ignored when it is typed.
It is about 59 bits, found by a keyed hash, so it cannot be guessed within the limits below.

Everything staff change about a key (its shows, window and limits) takes effect for the holder at the next renewal, and the key's text stays the same, so nothing needs sending again.
Every change is in the key's log with who made it: the field names, never their values, and never the text.

### What counts

- A **device** is a browser or app. It holds the key while it has renewed within `keys.device_hold_s`, and until it is signed out. Closing the browser frees its place after that time.
- A **listen** starts when a device is new, or comes back after `keys.listen_gap_s`. A page reload within the gap carries on the same listen. A renewal never starts one.
- A **person** on a group key is a device that has ever used it.
- Limits are checked while the key's row is locked, so two phones taking the last place at once cannot both have it. A listen in progress is not cut when the limit is reached; the next one is refused.

### When it does not work

Redeeming and renewing answer with a `code` and a plain sentence (`error`):

| `code` | Status | Meaning |
|---|---|---|
| `not_recognised` | 404 | no such key; wrong keys are counted per address |
| `not_yet_valid` | 403 | the window has not started; `from_ms` says when |
| `ended` | 403 | the window has ended; `until_ms` says when |
| `paused`, `revoked` | 403 | switched off, or ended by the station |
| `does_not_open_show` | 403 | the key does not open the show asked for, or no longer opens what the token was for |
| `not_scheduled` | 403 | the key works, but none of its shows (or not the one asked for) has a keyed window on the schedule now or in the next fortnight |
| `too_many_devices` | 409 | in use on its most devices; `devices` lists them so one can be signed out |
| `people_limit`, `listens_used_up` | 409 | a group key's people, or the listens, are used up |
| `device_signed_out` | 403 | this device was signed out (renewal) |
| `token_ended`, `token_invalid` | 401 | the token has ended (redeem again) or is not one (renewal) |

The words never say "token": the listener has a key.

## A show's access rule

`GET` and `PUT /api/v1/shows/{show}/access`: `rule` is `public` (the default for a show with none) or `key`.
`opens_at_ms` makes the show public from that moment whatever the rule says (early access for keyholders, or the free part of a show).
`slate` is what the public is to hear while the show is keyed: `announcement` (a slate saying when the public programme returns and how to get a key), `fallback` (the fallback loop) or `teaser`; `slate_note` is a line for the announcement.
In 1.3.2 playout plays the same slate whichever is chosen: the station's slate file when its data names one (`keyed.slate`) and it plays, else the station's fallback files while the folder holds one that plays, else the default loop WeaveSignal ships, else silence (a station with none of the three). Each is tried at every moment, as the programme's own fallback is: an empty fallback folder (a new station's) falls through to the default loop, and a file added to the folder later takes over by itself. No package writes `keyed.slate` in 1.3.2 (Compose, the Quadlets and the Cloudron package leave it out), so on them the public hears the fallback music or the loop; only a station that writes its own playout data can name a slate file. The choice is kept for the player's words and a later release.
`GET /api/v1/public/keyed-shows` and the player read `slate_note`.
`GET /api/v1/public/keyed-shows` lists the keyed shows without a bearer, with the lock, when each opens, where to get a key (`keys.get_a_key_text` and `keys.get_a_key_url`), and `on_now`: the keyed show whose window is open now, if one is.
The player's facts (`GET /api/v1/player`) say the same for the page: `show_id`, the show on now (the keyed show whose window is open, else the schedule's show on air), and `keyed`, whether its keyed window is open; the player finds the keyed show by that id, never by its name.
The studio's view carries `keyed` while a window is open: the show, whether playout keeps it from the public (`kept`, as playout answered at its last look, every two seconds; false is the alarm in the studio), whether playout answered at all (`confirmed`; false while it is away) and what the public hears meanwhile as playout says it (`public_hears`: `slate_file`, `fallback`, `default_loop`, `silence`, or `listener_fallback` while playout is away). With `confirmed` false, `kept` and `public_hears` are what the listener process's fallback does by design, not something playout said: the studio words it as *not confirmed*; the desk's Playout panel (`desk.keyed`) says it in words.

Setting a rule is a producer's or an admin's, never an API token's: which shows are keyed is a person's decision.
`key` is refused (409) while playout answers that it has keyed shows off (its data's `keyed.enabled`), and (503) while playout does not answer: nothing would keep the show from the public.

## Tokens

`POST /api/v1/keyed/redeem` trades a key's text for a token; `POST /api/v1/keyed/renew` trades a token for a fresh one after reading the key again from the database.
A token is for **one show's window**: the show named with `show`, else the keyed show on now that the key opens, else the next one on the schedule (up to a fortnight ahead).
It is valid from the window's lead (`keys.window_lead_s` before it starts) to its tail and the HLS lag after it ends, and never past the key's own end.
A player renews about every `renew_in_s` seconds: the renewal is the device's place on the key, and where a signed-out device, a shortened key or a key that no longer opens the show is stopped.
A key **revoked or paused** is refused at once by the station and by every edge that reads the revoked list, whatever its token's life.
`GET /api/v1/listener-keys/signing` gives a key manager the station's figures: `renew_every_s`, `ends_soon_days` (when the player starts saying a key's end) and `device_hold_s` (how long a device holds its place after its last renewal); `POST /api/v1/keyed/devices` gives a key's holder `device_hold_s` with the devices.

The answer names the show (`show`), the token's `starts_ms` and `expires_ms`, and the keyed addresses with the token in their path (`urls`):

| Address | What |
|---|---|
| `urls.hls`: `<join.hls_url>/k/<show>/<token>/audio.m3u8` for a sound key, `…/live.m3u8` for a key with the picture | the keyed HLS the key opens: the sound alone for a sound key (which is refused `live.m3u8`, `wrong_media`); picture and sound for a `video` or a `both` key, with `audio.m3u8` beside it |
| `urls.opus`: `/listen/k/<show>/<token>/programme-keyed.opus` | the keyed Opus stream at the station; every key opens it, a `video` key too |
| `urls.mp3`: `/listen/k/<show>/<token>/programme-keyed.mp3` | the keyed MP3 stream, when the station's MP3 stream is on |

The token is `ks1.<claims>.<signature>`: the claims as compact JSON and an ECDSA P-256 signature (SHA-256, the 64-byte r then s), both in URL-safe base 64 without padding, about 200 characters.
It goes in the path, never in a query, so a playlist's relative addresses carry it to every segment.

| Claim | Meaning |
|---|---|
| `k` | the key's id (never its text) |
| `s` | the show's id: the one show it opens |
| `m` | `a` sound, `av` sound and picture: what a `video` and a `both` key are both given, so a key for the picture is heard through an edge on an older image too. `v` (the picture alone) is still read, and no key is given it |
| `nbf`, `exp` | valid from, valid until, in Unix seconds |
| `d` | the device's id, for the renewal; an edge ignores it |

**What an edge checks** (`deploy/images/edge/keyed.js`, on every playlist and segment, cache hits included): the signature, with the public key from `GET /api/v1/listener-keys/signing` (`public_key`, 87 characters, the edge's `EDGE_KEYED_PUBLIC_KEY`: it checks tokens and cannot make one); `nbf` and `exp` with `keys.clock_skew_s` of leeway; that the show in the path is `s`; that `m` covers what is asked for (the audio rendition needs sound, the rest video); and that the key is not on the revoked list (`EDGE_KEYED_REVOKED_URL`, `GET /api/v1/keyed/revoked`, read every half-minute).
It needs no call to the control plane per request.
Behind the signature sits a small trait in the control plane (`TokenSigner`), with one implementation, P-256; the edge's njs cannot check Ed25519, which is why it is not used.

A token is for memory, not for storage: the player keeps the key (so the next visit can redeem again) and the device id.

## How a keyed show is kept from the public

A keyed show's **window** is its episode's air time (or up to the show's `opens_at`), from the schedule, with margins: it shuts `keys.window_lead_s` (3) before, and stays shut `keys.window_tail_s` (5) after the end plus the desk's profanity delay, which the control plane adds as the desk reports it.

| Where | What it does |
|---|---|
| playout | while a window is open the **public gate** is shut: the public programme is the slate, and a second feed carries the keyed programme to the listener process. The control plane opens it from the schedule, writes its state beside playout's socket first (so a playout that restarts mid-show starts shut), and asks playout every two seconds how it stands; a refusal is a notice in the studio |
| the listener process | the public mounts take the gated feed and never hold a keyed sample; the keyed mounts (`programme-keyed.opus`, `.mp3`) take the keyed one, are reachable only through the control plane, and are taken down and up at the end of each keyed show, which drops every keyed listener. A listener of the listener process is about 13 s behind playout, so key holders are dropped about 14 s after the window's tail, when they have heard the show to its end; until then no stream of the next keyed show starts (`code: changing_show`), since the mounts' buffer may still hold the last one's end. At a change from one keyed show to the next the keyed feed is silent for a few seconds (`keyed.mute`), so the first show's listeners never hear the next one's start. The legal logger records the keyed feed: what key holders heard, which outside keyed shows is what everyone heard |
| now playing | while a window is open, `GET /api/v1/now-playing`, its events and the `now_playing.changed` webhook name only the show (`now.kind` `show`, `now.title` the show's name, no artist or album) and leave out `next`; the tracks are not told to anyone without a key. The studio's own Playout view still shows them. It follows the window as the stream does, behind playout by the listener delay, so it names the show when listeners hear the slate and the tracks again when they hear the public programme |
| the station's keyed paths | `/hls/k/…` checks the token on every file and serves the public segments and that show's, never another keyed show's; `/listen/k/…` checks it and carries the keyed stream from the listener process, ending it at once when the key is revoked or paused, the token ends or the window closes |
| the public HLS (`/hls/`) | its playlists answer 403 (`code: keyed`) while a window is open and for `keys.hls_lag_s` (30) after; no segment written in that time is served, whatever a playlist says. The times the gate was shut are kept in the database, so this holds across a restart. The player plays the public sound (the slate) in its place meanwhile, and a key's holder the keyed HLS |
| RTMP out | every destination is sent to the desk turned off from `keys.rtmp_lead_s` (15) before a window's lead to its tail and the HLS lag after, and back on by itself after |
| an edge | checks keyed HLS as above. Keyed **sound** is not served by edges in 1.3.2: never name the keyed mounts in a relay's `RELAY_MOUNTS` (a relay cannot end one listener) |

**Not covered in 1.3.2:**

- A keyed show **stays keyed until the end of its slot** in the schedule (or until it opens to everyone, if that is sooner), and no longer: it does not wait for the presenter to finish. A show that **overruns** its slot is not followed: what the desk sends after the end, the tail and the delay is the next show's, and public. End on time, or lengthen the episode.
- **A key holder hears the show about 13 s after playout plays it**, because the listener process holds a 12 s burst on every mount, and a keyed stream starts that much behind. The window's tail covers it, and key holders are dropped about 14 s after it, when they have heard the end; a keyed show straight after another therefore starts its stream up to 14 s late. This is accepted for 1.3.2 and is to be shortened later (a smaller burst on the keyed mounts), which would also shorten the wait.
- `keys.hls_lag_s` is a bound on how long after a moment the desk's HLS may still write it, **not measured on a real desk yet**; it is generous so that the error can go one way only (a public moment held back).
- No personal stream address for radio apps and smart speakers, and no machine key relay (`/listen/p/`): a radio app needs a token in its address, which lasts the window.
- A slate of its own for each show: the rule's `slate` is kept, and every keyed show plays the station's one slate.
- A device signed out, or a key narrowed to other shows, keeps a token it already holds until its next renewal, at an edge until the token ends.

## Limits on guessing

Per address (an IPv6 address counts as its /64, and behind the proxy `join.forwarded_for` must be on, or every listener shares one address): `keys.redeem_per_minute` redeems, and `keys.failures_per_ten_minutes` wrong keys, after which that address is refused with `429` and `Retry-After`.
A device may renew ten times a minute.
Lost-key requests are limited per address (`keys.lost_per_hour`, `429`) and per email address (three an hour, answered the same and sending nothing).

## Which show a page names

Every page that names the show on air takes the schedule's show, as the desk's heading does (1.3.2): the player's `show` (which names the show its `show_id` does, a keyed window's margins included), the join page's, a moved pass's fallback and the desk's slate words.
Between shows they name the station's `[studio] show` setting, never the show that was last on air.
The join page's facts (`GET /api/v1/pass`) carry `show_id`, the show the invitation is for (a rehearsal link's rehearsed show, a moved pass's show, else the show on now), and `keyed`, whether that show needs a key now (its rule says `key` and it has not opened to everyone): the join page says who will hear a guest of a keyed show, and that the guest needs no key.

## The lost-key request

`POST /api/v1/keyed/lost` with an email address always answers `202` and the same words, whether a key matched or not, and does its work after the answer, so it takes the same time either way.
With the station's mail set up it emails the key's link to the address; without it, the request is logged on the matching keys (`lost_key_asked`) for staff to send the key themselves.
The answer is `{ "message", "mail" }`: `mail` says whether the station sends email, and the message follows it ("the station is sending it now", or that the station sends no email and its staff can see the request). `mail` is about the station, never about the address, so the answer is still the same whether a key matched or not. `GET /api/v1/public/keyed-shows` carries the same `mail`, so the player can say which before anyone asks.

## Key families and the leak report

A **family** is a label for keys issued together, one per recipient: a named list, or a number of anonymous keys (`Ticket 001`, `Ticket 002`).
Every copy is its own key, so a key found where it should not be says whose it was: `POST /api/v1/listener-keys/find` with its text names the key and its family.
`POST /api/v1/listener-keys/{id}/replace` revokes one key and issues another to the same recipient in one call (name, note, email, family, shows, window, limits and the listens already used carry over); every other key stays alive.

The **leak report** (`GET /api/v1/listener-keys/leaks`, optionally for a family) lists each key with its first and last use, listens, devices, distinct addresses (ever, in 24 hours, in seven days), the most new addresses in one hour, and a `score` with its `reasons`, highest first.
The score is a hint, a sum of three signs:

- each address beyond what the key allows in the last seven days counts 2 (a personal key allows its device limit, three if there is none; a group key that many times its people);
- each device beyond that allowance counts 5;
- three or more new addresses within an hour counts 10, and 2 for each one beyond the third.

A phone on mobile data changes address, which is why the report sums several signs and says which.
An address is never shown: the station keeps a keyed hash of it, cut to 64 bits, and of the use records only the hour, the device and the hashed address, for `keys.use_records_days` (90 by default; ADR-0122).
The score does not stop anyone from passing a key on; it tells staff which key to replace.

## The token key

A key's text is kept sealed (ChaCha20-Poly1305, the key's id as associated data) and found by a keyed hash; an email address and a listener's address are kept only as keyed hashes; tokens are signed with an ECDSA P-256 key made from a secret of its own; each is derived, under its own label, from the station's token key (`WEAVESIGNAL_CONTROL_TOKEN_KEY`, else one derived from the station key).
**A station with keyed shows sets `WEAVESIGNAL_CONTROL_TOKEN_KEY` itself** (the Cloudron package does): changing the token key, or the station key it was derived from, changes the public key every edge holds, so each edge's `EDGE_KEYED_PUBLIC_KEY` must be set again.
**Changing that key makes every sealed text unreadable and no typed text find its key.**
Staff are told to replace the keys (`409` from the reveal and export routes, and listeners are told "not recognised").
A station that rotates its station key and wants its keys to survive sets `WEAVESIGNAL_CONTROL_TOKEN_KEY` first.
With no key at all, keys fail closed: none can be made and none redeemed (`503`).

## Who may

| Who | Keys | Rules and erasure |
|---|---|---|
| a producer, an admin, the station key | everything under `/listener-keys` | a producer or admin sets a show's rule; erasing a key is an admin's |
| an API token with `keys:manage` | everything under `/listener-keys` except erasing a key | reads a show's rule; never sets it |
| a presenter or a screener | refused (403): keys hold names and emails and the means to listen | |

## Events

`key.first_used` and `key.ended` (reason `expired`, `revoked` or `replaced`) go to [webhooks](/docs/reference/integrations/#events-out), with the key's id and the label staff gave it, so that a shop can find its own record, and never its text or an email address.
`key.ended` for an expired window is sent by a sweep every five minutes, once, across restarts; the same sweep deletes use records older than `keys.use_records_days`.
