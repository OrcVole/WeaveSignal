<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
# Changelog

## 1.0.0 (2026-11-04)

- **WeaveSignal, release one**, the first public version. The app id is
  `top.weavesignal.cloudronapp`; the 0.x versions below were private test builds under another
  id, so install 1.0.0 fresh.
- **The listener stream is open by default** (`LISTENERS_AUTH=off`), unlisted, as in the
  Compose bundle; set it to `on` to ask for a password.
- **A caller's return is no longer choppy**: the desk stamps each output block from its place in
  the mix, not from the clock when it is encoded (0.0 % concealed, from 30–50 %).
- The edge passes only its own `Cache-Control`; the unused `hls_origin` port is retired.

## 0.9.3 (for staging, 2026-11-03)

- **A show can have its own slate picture** (ADR-0200): set in the show editor, it is on the
  video slate while the show is on air by the schedule, else the station's. The station's
  existing picture is kept as the station's.

## 0.9.2 (for staging, 2026-11-03)

- **API tokens get a key of their own** (ADR-0202): `CONTROL_TOKEN_KEY` is generated once in
  `secrets.env`, so rotating the station key no longer revokes tokens. **Upgrading revokes the
  tokens and webhook signatures made under the old derived key: make them again.**
- **An aired episode is corrected by an admin or the staff member who made it** (ADR-0198;
  migration 0021 records who made each episode; older episodes are an admin's to correct).
- The docs say what No retention keeps (ADR-0199) and that a webhook delivery waiting at a
  restart is lost (ADR-0201).

## 0.9.1 (for staging, 2026-11-03)

- **The listener process's cushion of playout's feed is a setting, `FEED_BUFFER_S`, 1.5 s by
  default.** 0.9.0 shipped 0.5 s, proven on a quiet workstation; on the shared staging server
  the listener process fell behind, the cushion ran dry, listeners heard the fallback for
  moments, and the backlog grew to 6 s. Each second set is a second more listener delay.

## 0.9.0 (for staging, 2026-11-03)

The package carries `m1` at `810553f` (2026-11-03): `after-0028`, the fixes from review 0028, the public API's
tokens and webhooks (migrations 0019-0020 run at start), the chat's minimum age by
jurisdiction (ADR-0148), the DESK BUSY and LINK DEGRADED glyphs, and holds named by the
signed-in admin.

- **About 3 s nearer live again**: playout names the desk's container to FFmpeg instead of
  letting it probe the stream at every connection, which left about 3 s in the listener
  process's buffer for as long as playout stayed connected.
- **Half a second nearer live again**: the listener process holds 0.5 s of playout's feed
  instead of 1 s (ADR-0182 step one; a 2-hour soak behind the real desk read 0.36-0.66 s,
  no underrun).
- **The gain control aims at -16 LUFS** (ADR-0044, the operator's choice after a listening
  comparison), from -23. A station still at -23 is moved to -16 once, with a marker line in
  `weavesignal.env`; setting -23 again afterwards keeps it, and any other value is left alone.
- The processing chain is its own script (`playout/chain.liq`), copied beside playout's.

## 0.8.0 (for staging, 2026-11-02)

The package carries `w-all2` (2026-11-02): the chat's keyed email hash, consents on air,
extended holds, the glyph refusal, episode kinds and corrections, the address check, the camera
preview, the audience warning, the relay's TLS hop, pins, the default loop, the MP3 stream, the
processing chain, a limiter without lookahead, per-return buffers, the video's sound from
playout, staff sign-in codes by email, stills, thumbnails, the station's mode, the Poster square
and the studio app's updater. Migrations 0013-0018 run at start.

- **The listener process decodes playout's WAV feed itself** (Liquidsoap's own decoder, not
  FFmpeg, which probed for 4-6 s): listeners hear about 4 s nearer live, and the desk is heard
  cleanly again after dead air hands the air to the fallback.
- **Two new secrets**, generated once and added to an existing `secrets.env`:
  `CHAT_EMAIL_KEY` (the chat refuses to start without it) and `CONTROL_EMAIL_KEY` (staff
  sign-in codes by email). Each keys the hash of email addresses; keep them with the backups.
  Chat accounts made before 0.8.0 keep their old hash until their owner next signs in with a
  code, when the chat re-keys them; sign-in codes in flight are dropped.
- **Staff sign-in codes by email** (ADR-0127) go out through Cloudron's mail like the chat's,
  following `MAIL_MODE`; passwords still work everywhere.
- **The default loop** (ADR-0130, CC0-1.0) is in the image: playout and the listener process
  play it while the fallback folder holds nothing, so a new station is never silent and the
  studio app says the loop is on air.
- **`/health` without a login** (ADR-0160), for the edges' pass-through check: served on the
  public domain, and the manifest exempts it from Cloudron's login on the station's domain
  (`proxyAuth.path` `!/health`). It says only ok, the version and the database.
- **New settings in `weavesignal.env`**, appended with their defaults: `SEARCH_LISTING` (false,
  ADR-0133: nginx and the control plane say noindex on every answer, once), `DIRECTORY_LISTING`
  (false, ADR-0129; the package's fixed `public=false` is gone, the setting says the same),
  `COMPAT_STREAM` (false; the MP3 stream at `COMPAT_MOUNT` on the listener domain, behind the
  same password, ADR-0002), `COMPAT_MOUNT`, `COMPAT_BITRATE`, `PROCESSING`,
  `PROCESSING_TARGET_LUFS`, `PROCESSING_MAX_GAIN_DB` (playout's gain control, ADR-0044), and
  `DESK_FEED_PORT` (7317, loopback; for video, which the package leaves off). A setting that
  is not true or false, or not a number, stops the start with its name in the log.
- `X-Forwarded-For`: nginx here sends each service one address (the rightmost that is not
  private, from Cloudron's header), so the control plane now reading the last entry changes
  nothing; `join.forwarded_for` and the chat's `forwarded_for` stay on.

## 0.7.0 (for staging, 2026-11-01)

The package carries `m1` at `a170857`.

- **Callers sound like a phone line** (ADR-0194), by default while we run diagnostics: a
  300-3,400 Hz band and a light compressor on every caller, on air and in the presenters'
  ears. Nothing to set; an empty `[inputs.caller]` in the desk's settings turns it off.
- From the first AI call-in show (live 01): the studio app's keychain works on a real desktop,
  and a station without staff sign-in no longer upsets the app's frame.

## 0.6.0 (for staging, 2026-11-01)

The package carries `m1` at `e390ae5`.

- **Each caller is in a booth room of their own** (ADR-0193): only the desk joins it beside
  them, so a caller sees nobody else and nobody else sees a caller. Nothing to set.
- A caller is not shown the programme picture (it is published in the studio room).
- The public pages allow the camera for the station's own origin.

## 0.5.0 (for staging, 2026-11-01)

The package carries `m1` at `ea2addf`.

- **Staff accounts** (ADR-0127): each member of staff signs in as themselves with a password,
  with a station-wide role. An admin alone changes retention, erases people, sees personal
  data and manages staff. The station key (`CONTROL_ADMIN_TOKEN`) makes the first admin and
  stays the station's own credential. Sign-in is served on the station's domain only.
- The chat's moderation log names the staff member who acted; an account turned off loses
  the chat's powers at once.
- Migration 0012 runs at start.

## 0.4.1 (for staging, 2026-11-01)

The package carries `m1` at `d740331`.

- **The silence after some starts is fixed.** A moment of NaN (likely the first frames of a
  fresh SRT connection from the desk) left playout's limiter at NaN for good, and listeners
  heard digital silence until playout restarted; 0.3.0 had it too. Playout now passes the
  desk's input and the programme through `clip` before the limiter.
- **The consent words are two sentences** (how long a call is kept, from the Jurisdiction
  setting, and how to have it taken down). `CONSENT_CONTACT` is the takedown contact;
  call-in stays closed until it is set.
- Also from `m1`: chat limits per address, erasure for guests and callers (migration 0011),
  the studio app's key in the keychain, and no biometric language.

0.4.0 was tried on staging the same morning, heard silent (this fault), and rolled back.

## 0.4.0 (for staging, 2026-11-01)

The package carries `m1` at `c9cbf11`: room privacy (ADR-0118) and End pass (ADR-0119).

- The webrtc plugin is built with WeaveSignal's patch (`spike/desk-image/patches`), which lets
  each mix-minus allow only its own person to subscribe; the desk refuses to start without it.
  Pages allow no one to subscribe to their microphone or camera; the desk hears them through
  connections with LiveKit's recorder grant.
- The studio can end anyone's pass (`POST /api/v1/passes/end`, the tile's End pass);
  migration 0010 keeps ended passes for a day across restarts.
- `.containerignore` lets `spike/desk-image/patches` into the build context.

## 0.3.0 (for staging, 2026-10-31)

The package carries `m1` at `4a0c2cd`: the Schedule tab's screens, the video path (off unless a
station turns it on), packaging, the studio's station setting, and the 2026-10-31 review's
fixes (the rejoin keeps its microphone, the desk aborts on panic and encodes the programme
alone, uploads check the bearer first, one station time zone, chat ids never reused).

- The public pages' domain now passes `/api/v1/call-in/challenge`, `/api/v1/steering` and
  `/hls/`, which the player calls; before, call-in from the public domain could not start.
- Migration 0009: a station whose zone was never set in the studio takes `PUBLISHING_ZONE`
  (Europe/London by default) on start, replacing the UTC row playout created before it.

## 0.2.1 (installed on staging)

The manifest names the app's icon (`logo.png`, the Boxicons broadcast mark, ADR-0117); 0.2.0 had none, so Cloudron showed its default.

## 0.2.0 (built and checked locally; not pushed, not installed)

The package carries `m1` at `1cd70a1`: retention, carts in the desk's mixer, show cards and
feeds, the live chat and M4's playout layer.

* **The listener process holds the mount.** Playout sends PCM in WAV over loopback to the
  listener process (`playout/listeners.liq`), which runs `icecast.server`, its own fallback,
  the encoder listeners hear and the legal logger. A listener stays connected through a
  playout restart. The feed port (`PLAYOUT_FEED_PORT`, 7315) and `/status.json` stay on
  loopback; the listener domain serves only the mount. The stream still asks no directory to
  list it (`public=false`, now in `listeners.liq`).
* **The desk starts with the rest.** `desk-start.sh` is gone: its fault came from playout
  holding the mount.
* **The chat** (`weavesignal-chat`) runs as its own program on `CHAT_PORT` (7311, loopback),
  behind nginx at `/api/v1/chat/`. Sign-in codes go out through Cloudron's `sendmail` addon
  by msmtp (`MAIL_MODE=cloudron`); without the addon's SMTP server they go to
  `/app/data/chat-outbox`.
* **A public domain** (`httpPorts.PUBLIC_DOMAIN`, container port 8003), outside Cloudron's
  login: the player page with call-in and the chat, the join and echo-test pages, the podcast
  feeds, show cards and episode files, and only the public routes behind them. The studio's
  and the chat's admin routes stay on the app's own domain.
* **Cards and feeds** print `https://` and the public domain (else the app's origin, or
  `PUBLISHING_URL` when set).
* **The station's files** live under `/app/data`: `carts`, `library`, `fallback`,
  `recordings` (the logger in `recordings/logger`), `show-cards`, `episodes`, `chat-outbox`.
  The cart demo pack ships in the image.
* **Settings**: new lines are appended to an existing `/app/data/weavesignal.env` with their
  defaults; the administrator's own lines are never changed. The control plane's settings and
  ports are checked before any service starts.
* nginx passes each service one client address (the rightmost public address in
  `X-Forwarded-For`), and both the control plane and the chat take it (`forwarded_for`).
* Migrations 0004 to 0007 run at start, as before, by the control plane.

## 0.1.1

* The desk contains a person's failure; listen-only tokens; a constant-rate listener stream.

## 0.1.0

* First staging build: the control plane, the desk, Liquidsoap 2.5 playout and
  `icecast.server`, LiveKit with TURN over TLS on 15349, behind nginx; PostgreSQL addon.
