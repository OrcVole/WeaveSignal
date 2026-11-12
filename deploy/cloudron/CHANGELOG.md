<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
# Changelog

[1.0.2]

- **Beginner steps** (ADR-0271): each show runs at Solo, Guests, Callers or Full, and the studio and the public pages show only what that step needs; hidden means off (call-in closed, seats refused). New shows start at Solo; shows that exist stay Full. **Migration 0033.**
- **Prepare mode** (ADR-0276): a studio window is Live or Preparing; a preparing window sees the live desk but cannot change what is on air, so the next DJ can get ready during someone else's show.
- **End show and Hand to automation** (issue 15): End show takes everyone off air, fades the carts and hands the air to automation once the delay has played out; a button by the lamp switches auto and live.
- **The listener count by mount and a delay that says what it reads** (issue 17); `studio.listener_delay_ms` now defaults to 13 s (it was 15), so a default station shows about 21 s.
- **RTMP presets for YouTube, Owncast, X and Gab** beside Rumble, Odysee and PeerTube, each with the profile it takes (tried live against Owncast only).
- **Simple guides** for people at `/guides/` in the documentation; the studio's frame no longer clips at a large text size.
- **A cart wall per show**: a show can have cart banks of its own beside the station's, which every show shares. **Migration 0034** gives each bank an id and an owner; the banks a station has stay the station's, with their carts, so an upgrade loses nothing. No new settings; the public domain is unchanged (the new routes are under `/api/v1/shows/{show}/carts/` on the station's domain). Not yet tried on a Cloudron install.

[1.0.1]

- **Browser cameras work** with `VIDEO=true`: the image gains gst-plugins-rs's RTP plugin, whose `rtpav1depay` takes a Chromium browser's AV1 camera (without it that guest's voice failed too), and the desk gives cameras a 300 ms jitter buffer and asks for a keyframe after a loss. Tried on a Cloudron install: a Brave camera through the desk's picture to an RTMP destination (Owncast).
- **nginx serves the station's pages `no-cache`** and `/assets/` with a long cache, so a page open across an update fetches the new one; a page that asks for code the update removed reloads once.
- **Avatars in the chat** (ADR-0235): chatters choose one of 72 shipped avatars or upload a picture of their own, held until staff approve it on the Screener tab. Two new settings in `weavesignal.env`: `AVATAR_UPLOADS` (true) and `AVATARS_DIR` (`/app/data/chat-avatars`, inside the backups; a place outside `/app/data` is refused). The public domain passes the three new public chat routes (`me/avatar`, `me/avatar/stock`, `avatars/{id}`) with a 3 MB body limit; the staff route for held pictures stays on the station's domain. Migration 0026. Not yet tried on a Cloudron install.
- **Pictures and documents in the chat** (ADR-0262), off by default. Two new settings in `weavesignal.env`: `CHAT_ATTACHMENTS` (false) and `ATTACHMENTS_DIR` (`/app/data/chat-attachments`, inside the backups; a place outside `/app/data` is refused). The public domain passes the new routes (`attachments`, `pictures/{id}`, `pictures/{id}/thumb`, `files/{id}`), and both domains take a chat request body up to 33 MB (the chat's own limits are lower, route by route). Migrations 0031 and 0032. Not yet tried on a Cloudron install.
- **The station's setup page** (ADR-0237) at `/setup/` on the app's own domain, for the station key or an admin: the name, the takedown contact, the jurisdiction and the station's look (accent colour, icon, banner), kept in the database, so they win over `STATION_NAME` and `CONSENT_CONTACT` in `weavesignal.env` and need no restart. The public pages' domain passes the three branding routes for reading and answers 404 for `/setup/`.
- **The stream's password from the setup page**: its answer is written to `/app/data/setup.env` and applies at the next restart. `start.sh` reads `LISTENERS_AUTH` and `LISTENERS_PASSWORD` from it over `weavesignal.env` and `secrets.env`, checks each (`on` or `off`; 8 to 64 letters, digits, `.`, `_`, `~`, `-`) and never sources the file.
- The post-install note names the setup page and the website's install wizard.
- **A Night theme in the studio app** (ADR-0270): a third theme beside Dark and Light, warm and with no blue, for presenting late. Nothing changes in the package's settings.
- **Video in the package** (ADR-0227), off by default. The image gains gst-plugins-rs's `isobmff`, `hlssink3` (the CMAF HLS writer, `hlscmafsink`) and `dav1d` plugins, built at the tag of the WebRTC plugin, the pango plugin for the slate's text (`gstreamer1.0-x`), `libdav1d` and Noto Sans; SVT-AV1 is Ubuntu's `libsvtav1enc2` 2.3.0. The build stage checks every video element.
- **New setting `VIDEO` in `weavesignal.env`** (false). `VIDEO=true` turns on the desk's video path with the Compose bundle's defaults (the 480p20 rung, 2 s segments), the desk feed (playout's output back to the desk for the video's sound, ADR-0172), the room's codecs (Opus, VP8, VP9, AV1; no H.264) and the control plane serving the desk's HLS, which nginx passes on the public domain at `/hls/`. The join pages then offer the camera check. Restart the app after changing it. It costs memory and CPU: see the install guide.
- **The slate's picture works on Cloudron**: the control plane wrote it under `/var/lib/weavesignal`, which the package's read-only root does not have; `video.still_file` now points under `/app/data/carts/stills/`, for the control plane and the desk, with video on or off.
- The HLS is written under `/run/weavesignal/hls`, which is not backed up.
- **Memory with video on**: the memory gate with three cameras peaked at 1.2 to 1.26 GiB (idle 0.9 GiB), so the gate's rule asks for 1.7 GiB: **`memoryLimit` is now 2 GiB** (from 1.5 GiB). An app installed earlier keeps its limit; raise it in Cloudron before turning `VIDEO` on. The desk used 22 to 26 % of one core with three cameras at one 480p rung.
- The docs: the install guide's Video section, the Cloudron line in the desk's video reference, and the gate's figures.
- **Send the programme to another Icecast server** (ADR-0226): `ICECAST_OUT`, `ICECAST_OUT_HOST`, `ICECAST_OUT_PORT`, `ICECAST_OUT_MOUNT`, `ICECAST_OUT_USER`, `ICECAST_OUT_TLS` and `ICECAST_OUT_BITRATE` in `weavesignal.env` (off by default), and the password as `ICECAST_OUT_PASSWORD` in `secrets.env`. Migration 0022 keeps the choice made in the studio's Settings.

- **Open files: the services' soft limit is lifted from Cloudron's 1,024 to 65,536** (supervisord's `minfds`), so the listener stream is no longer capped below 1,024 listeners. `test-local.sh` starts the container with Cloudron's limits and checks the listener process's.
- **RTMP out to Rumble, Odysee, PeerTube or any RTMP server** (ADR-0230), with video on; destinations and their stream keys are set in the studio app (Settings, RTMP out). Four new settings in `weavesignal.env`: `VIDEO_CANVAS` (the picture composed; 720p30, 720p25 or 1080p30 lets RTMP send more than 480p20), `RTMP_RUNG` (the picture sent), `RTMP_H264` (`openh264`; the app has no GPU) and `RTMP_VIDEO_KBPS`. H.264 and AAC come from Ubuntu's OpenH264 and vo-aacenc builds, which the image already carried through gstreamer1.0-plugins-bad; the build now checks `openh264enc`, `voaacenc` and `rtmp2sink` load. Migration 0024 keeps the destinations. Tried from a Cloudron install (the test station) against Owncast; not yet against Rumble, Odysee or PeerTube.
- **The show, the rundown, the banned list and steering can be set** in `/app/data/station.toml` (`[studio]` and `[steering]` only), which the control plane adds to the settings the package writes; before, the desk always said *No show set* on Cloudron.
- **The listener domain's bare address sends a visitor to the player** on the public domain (302), where it answered 404.
- **An invite's link is on the public domain**: `POST /api/v1/invites` answered a link relative to the address asked, so an invite made through the studio domain led to Cloudron's login. It is now absolute on `PUBLIC_DOMAIN` (the package's `publishing.public_url`).
- **The one-key DJ handover** (`⇧⏎`) and an invite button on the desk (ADR-0231).
- **A show's presenter and takedown contact**, named first in the consent words while the show is on air (ADR-0214, ADR-0215), and a three-step setup on a DJ's first sign-in (ADR-0218). Migration 0023 keeps them.
- **New settings in `weavesignal.env`**, appended with their defaults: `OPUS_BITRATE` (96; the Opus stream's constant rate, 64 to 256, ADR-0233), `COMPAT_HIGH_STREAM` (false), `COMPAT_HIGH_MOUNT` (`programme-high.mp3`) and `COMPAT_HIGH_BITRATE` (320): a second MP3 stream beside the first (ADR-0232), on the listener domain behind the same password, which nginx passes when it is on; and `CHAT_HOURS` (`always`; `shows` opens the chat while a scheduled show is on air, ADR-0234). `COMPAT_STREAM`, `COMPAT_MOUNT` and `COMPAT_BITRATE` are unchanged: an installed station keeps its one MP3 mount. A rate that is not a whole number in range, a second mount that repeats another, or `CHAT_HOURS` other than `always` or `shows` stops the start with its name in the log.
- **The player offers each stream that is on** (Opus, then each MP3 by rate). `test-local.sh` turns both MP3 streams on for its main run and checks each at its rate behind the password, and the player's list.
- **Memory**: two MP3 encoders in the listener process where there was one. The memory gate with both on (2026-11-10, video off): 531 MiB idle, 684 MiB at peak under load, of the 2 GiB limit; no OOM kill.

[1.0.0]

- **WeaveSignal, release one**, the first public version (app id `top.weavesignal.cloudronapp`).
- **The listener stream is open by default** (`LISTENERS_AUTH=off`), unlisted, as in the Compose bundle; set it to `on` to ask for a password.
- **A caller's return is no longer choppy**: the desk stamps each output block from its place in the mix, not from the clock when it is encoded (0.0 % concealed, from 30–50 %).
- The edge passes only its own `Cache-Control`; the unused `hls_origin` port is retired.

[0.9.3]

- **A show can have its own slate picture** (ADR-0200): set in the show editor, it is on the video slate while the show is on air by the schedule, else the station's. The station's existing picture is kept as the station's.

[0.9.2]

- **API tokens get a key of their own** (ADR-0202): `CONTROL_TOKEN_KEY` is generated once in `secrets.env`, so rotating the station key no longer revokes tokens. **Upgrading revokes the tokens and webhook signatures made under the old derived key: make them again.**
- **An aired episode is corrected by an admin or the staff member who made it** (ADR-0198; migration 0021 records who made each episode; older episodes are an admin's to correct).
- The docs say what No retention keeps (ADR-0199) and that a webhook delivery waiting at a restart is lost (ADR-0201).

[0.9.1]

- **The listener process's cushion of playout's feed is a setting, `FEED_BUFFER_S`, 1.5 s by default.** 0.9.0 shipped 0.5 s, proven on a quiet workstation; on the shared staging server the listener process fell behind, the cushion ran dry, listeners heard the fallback for moments, and the backlog grew to 6 s. Each second set is a second more listener delay.

[0.9.0]

The package carries `m1` at `810553f` (2026-11-03): `after-0028`, the fixes from review 0028, the public API's
tokens and webhooks (migrations 0019-0020 run at start), the chat's minimum age by
jurisdiction (ADR-0148), the DESK BUSY and LINK DEGRADED glyphs, and holds named by the
signed-in admin.

- **About 3 s nearer live again**: playout names the desk's container to FFmpeg instead of letting it probe the stream at every connection, which left about 3 s in the listener process's buffer for as long as playout stayed connected.
- **Half a second nearer live again**: the listener process holds 0.5 s of playout's feed instead of 1 s (ADR-0182 step one; a 2-hour soak behind the real desk read 0.36-0.66 s, no underrun).
- **The gain control aims at -16 LUFS** (ADR-0044, the operator's choice after a listening comparison), from -23. A station still at -23 is moved to -16 once, with a marker line in `weavesignal.env`; setting -23 again afterwards keeps it, and any other value is left alone.
- The processing chain is its own script (`playout/chain.liq`), copied beside playout's.

[0.8.0]

The package carries `w-all2` (2026-11-02): the chat's keyed email hash, consents on air,
extended holds, the glyph refusal, episode kinds and corrections, the address check, the camera
preview, the audience warning, the relay's TLS hop, pins, the default loop, the MP3 stream, the
processing chain, a limiter without lookahead, per-return buffers, the video's sound from
playout, staff sign-in codes by email, stills, thumbnails, the station's mode, the Poster square
and the studio app's updater. Migrations 0013-0018 run at start.

- **The listener process decodes playout's WAV feed itself** (Liquidsoap's own decoder, not FFmpeg, which probed for 4-6 s): listeners hear about 4 s nearer live, and the desk is heard cleanly again after dead air hands the air to the fallback.
- **Two new secrets**, generated once and added to an existing `secrets.env`: `CHAT_EMAIL_KEY` (the chat refuses to start without it) and `CONTROL_EMAIL_KEY` (staff sign-in codes by email). Each keys the hash of email addresses; keep them with the backups. Chat accounts made before 0.8.0 keep their old hash until their owner next signs in with a code, when the chat re-keys them; sign-in codes in flight are dropped.
- **Staff sign-in codes by email** (ADR-0127) go out through Cloudron's mail like the chat's, following `MAIL_MODE`; passwords still work everywhere.
- **The default loop** (ADR-0130, CC0-1.0) is in the image: playout and the listener process play it while the fallback folder holds nothing, so a new station is never silent and the studio app says the loop is on air.
- **`/health` without a login** (ADR-0160), for the edges' pass-through check: served on the public domain, and the manifest exempts it from Cloudron's login on the station's domain (`proxyAuth.path` `!/health`). It says only ok, the version and the database.
- **New settings in `weavesignal.env`**, appended with their defaults: `SEARCH_LISTING` (false, ADR-0133: nginx and the control plane say noindex on every answer, once), `DIRECTORY_LISTING` (false, ADR-0129; the package's fixed `public=false` is gone, the setting says the same), `COMPAT_STREAM` (false; the MP3 stream at `COMPAT_MOUNT` on the listener domain, behind the same password, ADR-0002), `COMPAT_MOUNT`, `COMPAT_BITRATE`, `PROCESSING`, `PROCESSING_TARGET_LUFS`, `PROCESSING_MAX_GAIN_DB` (playout's gain control, ADR-0044), and `DESK_FEED_PORT` (7317, loopback; for video, which the package leaves off). A setting that is not true or false, or not a number, stops the start with its name in the log.
- `X-Forwarded-For`: nginx here sends each service one address (the rightmost that is not private, from Cloudron's header), so the control plane now reading the last entry changes nothing; `join.forwarded_for` and the chat's `forwarded_for` stay on.

[0.7.0]

The package carries `m1` at `a170857`.

- **Callers sound like a phone line** (ADR-0194), by default while we run diagnostics: a 300-3,400 Hz band and a light compressor on every caller, on air and in the presenters' ears. Nothing to set; an empty `[inputs.caller]` in the desk's settings turns it off.
- From the first AI call-in show (live 01): the studio app's keychain works on a real desktop, and a station without staff sign-in no longer upsets the app's frame.

[0.6.0]

The package carries `m1` at `e390ae5`.

- **Each caller is in a booth room of their own** (ADR-0193): only the desk joins it beside them, so a caller sees nobody else and nobody else sees a caller. Nothing to set.
- A caller is not shown the programme picture (it is published in the studio room).
- The public pages allow the camera for the station's own origin.

[0.5.0]

The package carries `m1` at `ea2addf`.

- **Staff accounts** (ADR-0127): each member of staff signs in as themselves with a password, with a station-wide role. An admin alone changes retention, erases people, sees personal data and manages staff. The station key (`CONTROL_ADMIN_TOKEN`) makes the first admin and stays the station's own credential. Sign-in is served on the station's domain only.
- The chat's moderation log names the staff member who acted; an account turned off loses the chat's powers at once.
- Migration 0012 runs at start.

[0.4.1]

The package carries `m1` at `d740331`.

- **The silence after some starts is fixed.** A moment of NaN (likely the first frames of a fresh SRT connection from the desk) left playout's limiter at NaN for good, and listeners heard digital silence until playout restarted; 0.3.0 had it too. Playout now passes the desk's input and the programme through `clip` before the limiter.
- **The consent words are two sentences** (how long a call is kept, from the Jurisdiction setting, and how to have it taken down). `CONSENT_CONTACT` is the takedown contact; call-in stays closed until it is set.
- Also from `m1`: chat limits per address, erasure for guests and callers (migration 0011), the studio app's key in the keychain, and no biometric language.

0.4.0 was tried on staging the same morning, heard silent (this fault), and rolled back.

[0.4.0]

The package carries `m1` at `c9cbf11`: room privacy (ADR-0118) and End pass (ADR-0119).

- The webrtc plugin is built with WeaveSignal's patch (`spike/desk-image/patches`), which lets each mix-minus allow only its own person to subscribe; the desk refuses to start without it. Pages allow no one to subscribe to their microphone or camera; the desk hears them through connections with LiveKit's recorder grant.
- The studio can end anyone's pass (`POST /api/v1/passes/end`, the tile's End pass); migration 0010 keeps ended passes for a day across restarts.
- `.containerignore` lets `spike/desk-image/patches` into the build context.

[0.3.0]

The package carries `m1` at `4a0c2cd`: the Schedule tab's screens, the video path (off unless a
station turns it on), packaging, the studio's station setting, and the 2026-10-31 review's
fixes (the rejoin keeps its microphone, the desk aborts on panic and encodes the programme
alone, uploads check the bearer first, one station time zone, chat ids never reused).

- The public pages' domain now passes `/api/v1/call-in/challenge`, `/api/v1/steering` and `/hls/`, which the player calls; before, call-in from the public domain could not start.
- Migration 0009: a station whose zone was never set in the studio takes `PUBLISHING_ZONE` (Europe/London by default) on start, replacing the UTC row playout created before it.

[0.2.1]

The manifest names the app's icon (`logo.png`, the Boxicons broadcast mark, ADR-0117); 0.2.0 had none, so Cloudron showed its default.

[0.2.0]

The package carries `m1` at `1cd70a1`: retention, carts in the desk's mixer, show cards and
feeds, the live chat and M4's playout layer.

* **The listener process holds the mount.** Playout sends PCM in WAV over loopback to the listener process (`playout/listeners.liq`), which runs `icecast.server`, its own fallback, the encoder listeners hear and the legal logger. A listener stays connected through a playout restart. The feed port (`PLAYOUT_FEED_PORT`, 7315) and `/status.json` stay on loopback; the listener domain serves only the mount. The stream still asks no directory to list it (`public=false`, now in `listeners.liq`).
* **The desk starts with the rest.** `desk-start.sh` is gone: its fault came from playout holding the mount.
* **The chat** (`weavesignal-chat`) runs as its own program on `CHAT_PORT` (7311, loopback), behind nginx at `/api/v1/chat/`. Sign-in codes go out through Cloudron's `sendmail` addon by msmtp (`MAIL_MODE=cloudron`); without the addon's SMTP server they go to `/app/data/chat-outbox`.
* **A public domain** (`httpPorts.PUBLIC_DOMAIN`, container port 8003), outside Cloudron's login: the player page with call-in and the chat, the join and echo-test pages, the podcast feeds, show cards and episode files, and only the public routes behind them. The studio's and the chat's admin routes stay on the app's own domain.
* **Cards and feeds** print `https://` and the public domain (else the app's origin, or `PUBLISHING_URL` when set).
* **The station's files** live under `/app/data`: `carts`, `library`, `fallback`, `recordings` (the logger in `recordings/logger`), `show-cards`, `episodes`, `chat-outbox`. The cart demo pack ships in the image.
* **Settings**: new lines are appended to an existing `/app/data/weavesignal.env` with their defaults; the administrator's own lines are never changed. The control plane's settings and ports are checked before any service starts.
* nginx passes each service one client address (the rightmost public address in `X-Forwarded-For`), and both the control plane and the chat take it (`forwarded_for`).
* Migrations 0004 to 0007 run at start, as before, by the control plane.

[0.1.1]

* The desk contains a person's failure; listen-only tokens; a constant-rate listener stream.

[0.1.0]

* First staging build: the control plane, the desk, Liquidsoap 2.5 playout and `icecast.server`, LiveKit with TURN over TLS on 15349, behind nginx; PostgreSQL addon.
