<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
# Changelog

## 1.0.1 (2026-11-12)

Built since 1.0.0; the page *What is built, and what is planned* has the detail.

- **The desktop studio app on Linux can send again** (found in 1.0.1's test run; in 1.0.0 too):
  its content security policy blocked Tauri's own IPC scheme, Tauri fell back to its postMessage
  route, and every request with a body reached the station as a list of numbers, which the station
  refused (400). Invitations, and anything else with a body, failed with "could not make the
  invite". The policy now allows `ipc:`, and the app's bridge turns such a list back into bytes
  either way. A refusal the station sends as plain text now shows its own words.
- **A guest's browser camera reaches the desk** (found on the test station with Brave): a Chromium
  browser sends its camera as AV1, which the desk could not unpack, so that person's whole
  subscription failed, voice included, and was made again about three times a second. The images now
  carry gst-plugins-rs's RTP plugin (`rtpav1depay`); the join pages send AV1 at one picture size
  (`L1T3`), because the decoder stops when the size changes; and the desk offers the room only the
  codecs it can decode. `just test-browsercam` publishes a camera from headless Chromium with the
  pages' own settings.
- **A browser camera survives packet loss**: one lost packet stopped the AV1 decoder for good. The
  desk now asks for a keyframe when a camera stalls, and finds AV1 keyframes by their sequence
  header. Cameras get a jitter buffer of their own, 300 ms by default
  (`desk.video_jitter_buffer_ms`; voices stay at 50 ms): at 50 ms a real camera through a VPN lost
  2.7 % of its packets as late and flickered to the slate. The picture can run up to 0.3 s behind
  the voices on the video outputs.
- **Pages survive a station update**: the station serves its pages `no-cache` (their assets keep a
  long cache), and a page that asks for code an update removed reloads once instead of breaking.
- **Invitations, not invites, in the dialog**: its button is **Create invitation**.
- **The studio fits its window at any text size**: with A+ above 100 % the frame was the window's
  height times the zoom and its bottom was cut off; it is now pinned to the window. The tabs'
  narrow layouts follow the text size as well as the window (container queries), and the desk's
  narrow layout is one column that scrolls instead of panels squeezed over each other.
- **Tips and panel names stay in the window at any text size**: they were placed as many times too
  far right and down as the zoom, and ran off the window near its right edge.

- **Avatars in the chat** (ADR-0235): every message shows its sender's avatar. A chatter chooses one
  of 72 animals and monsters shipped with WeaveSignal (Fluent Emoji's Flat set, MIT, 83 KB), or
  uploads a picture of their own, which they centre and zoom on the player page. The chat's media
  worker rewrites it as a small PNG with nothing but pixels (the upload as it arrived is kept beside it while it waits, never shown, ADR-0257), and nobody else sees it until staff
  approve it in the screener's Needs you; refusing or removing one sends a statement of reasons.
  New settings `[avatars]` in `chat.toml` (`AVATAR_UPLOADS` on the install routes) and a
  `chat-avatars` volume to back up. Migration 0026. The pages' `img-src` gains `blob:`.
- **Chat follow-ups** (ADR-0253, 0254, 0255, 0257, 0259, 0260). With chat hours set to `shows`, the
  chatters the station made moderators may post between shows (the private lane to the studio stays
  the staff's). Moderators see the avatar queue and approve or refuse, each decision under their
  name. A picture nobody decides on in 30 days (`avatars.wait_days`) is deleted and its owner told.
  A switch in Settings > Chat pictures turns uploads on and off at once, over `AVATAR_UPLOADS`.
  A new refusal reason, suspected child sexual abuse material, keeps the upload as it arrived (if it was still waiting), the
  uploader's details and the decision for at least a year (`avatars.evidence_days`), viewable and
  exportable (a ZIP for a report) by one named staff account only (`evidence_viewer`, the first admin
  until set); staff can delete any kept picture from the server at once, with why, and evidence asks
  to confirm; a runbook is in the how-tos. The chat keeps each upload's original beside the clean
  PNG while it waits, never served. Listeners can report a chatter's picture into the screener's reports. Migration
  0030.
- **Pictures and documents in the chat** (ADR-0262, ADR-0258), off by default. Where a station turns
  it on (`CHAT_ATTACHMENTS`, or the new switch in Settings > Chat pictures), a trusted chatter may
  attach a picture (PNG, JPEG or WebP: the media worker rewrites it at about 1,600 pixels on its long
  side, a lossless PNG for a screenshot and a JPEG at full colour resolution for a photograph, with a
  thumbnail and nothing but pixels) or a document (PDF, OpenDocument text or spreadsheet, Markdown or
  plain text: passed on unchanged and only ever served as a download, `Content-Disposition:
  attachment`, `nosniff`, a sandboxing CSP, under the station's own name). The type is decided by the
  file's bytes, never its name. Each one waits in the screener's Needs you beside avatars and is
  approved or refused there, with the same suspected-CSAM evidence path, 30-day wait, delete-now and
  Report this; removing one takes its message down too, and it goes when its message goes under the
  station's retention. New `[attachments]` settings in `chat.toml`, a `chat-attachments` volume to
  back up (`ATTACHMENTS_DIR` on Cloudron), `POST /api/v1/chat/attachments` and the routes
  `/pictures/{id}`, `/pictures/{id}/thumb` and `/files/{id}`. New Rust crates in the media worker:
  image-webp and jpeg-encoder (whose DCT is the IJG's; `deny.toml` allows `IJG`). Migrations 0031 and
  0032.
- **The operator's answers on chat pictures** (2026-11-10): an approved upload keeps only its clean
  copy (its original is deleted at approval); only the person named to see the evidence, or an admin,
  may delete a picture kept as suspected child sexual abuse material; the uploader's statement for
  that reason says only that it broke the station's rules on illegal content; a chatter deleting their
  own picture deletes it, under an evidence hold too; and a `PUT` of the chat's settings that leaves
  out `avatar_uploads`, `attachments` or `evidence_viewer` keeps the stored value.
- **Install wizards and the station's look** (ADR-0237). A page on the website asks a new station's
  questions and gives back its settings file, DNS records and commands for Compose, Quadlets or
  Cloudron; it runs in the browser and sends nothing. The station gains a setup page at `/setup/` for
  its admin (the station key or an admin's sign-in): the name and the takedown contact, kept in the
  database and in force at once; the jurisdiction; on Cloudron the stream's password. The player and
  public pages take the station's accent colour (each theme's shade derived to 4.5:1), brand icon
  (also the favicon) and banner; uploads are PNG or JPEG only and re-encoded by the media worker.
  With none set the pages are unchanged. Migrations 0028 and 0029.
- **The one-key DJ handover** (ADR-0074, ADR-0231): `⇧⏎` puts the next DJ on air beside the
  presenter, then takes the presenter off. The presenter's seat, which the desk's talk keys and `⌃T`
  speak as, follows the air, so a DJ who took over can talk back. An invite can be made from the desk.
- **An invite's link is on the station's public address** when `publishing.public_url` is set. On
  Cloudron the join page is only on the public domain, so a 1.0.0 invite's relative link, opened
  from the studio domain, met Cloudron's login.
- **A show's presenter and takedown contact**, named first in the consent words while the show is on
  air (ADR-0214, ADR-0215), and three short steps on a DJ's first sign-in (ADR-0218). Migration 0023.
- **Icecast out**: the programme as MP3 to another Icecast server, such as a DJ's AzuraCast, with the
  delay and Dump in it (ADR-0226). Migration 0022.
- **RTMP out** to Rumble, Odysee, PeerTube or any RTMP server: H.264 and AAC by exception, or AV1 and
  Opus over Enhanced RTMP (ADR-0230). Migration 0024. The server image no longer carries fdk-aac.
- **Calls on a video stream from OBS**: a guide for streamers who keep OBS (ADR-0224).
- **Cloudron**: video in the package (`VIDEO`, memory limit 2 GiB, ADR-0227); the services' open-file
  limit lifted to 65,536, so the listener stream is no longer capped below 1,024 listeners.
- **The website's sandbox desk**: ten training scenarios beside the show, two of them a DJ handover.
- **An API token with `chat:moderate` works through the station's address** (issue 2): the chat passes
  it on to the control plane, which checks it as on its own routes; 1.0.0's chat refused it with 401.
- **A voice muted in the room keeps its link** (issue 13): the desk no longer subscribes again to a
  voice whose audio is muted in LiveKit, so one muted after the desk heard it no longer shows LINK
  DEGRADED, and it is waited for afresh once unmuted. A quiet or digitally silent microphone never
  was a fault; `test-quiet` now proves both on the stack.
- **Two MP3 streams, and the listener chooses** (ADR-0232): a second MP3 mount beside 1.0.0's, 320
  kbit/s by default for music (`COMPAT_HIGH_STREAM`, `COMPAT_HIGH_MOUNT`, `COMPAT_HIGH_BITRATE`); 1.0.0's
  `COMPAT_STREAM`, `COMPAT_MOUNT` and `COMPAT_BITRATE` are the first, unchanged. Each is off until
  turned on. The player offers Opus and each MP3 stream that is on, keeps the listener's choice on the
  device, and starts on MP3 where the browser cannot play Ogg Opus (`/api/v1/player` gains `streams`).
  The Compose proxy publishes both mounts by their settings (it published `/programme.mp3` whatever
  `COMPAT_MOUNT` said); the local edges' `RELAY_MOUNTS` is a setting.
- **The Opus stream's rate is a setting** (ADR-0233): `OPUS_BITRATE`, 64 to 256 kbit/s, 96 by default,
  still constant bit rate. A rate outside that stops the listener process with the setting named.
- **The chat can be open during shows only** (ADR-0234): `CHAT_HOURS=shows` opens posting while a
  scheduled episode is on air by the schedule, and for the station's staff at any time; between shows
  the player shows the chat read-only and says when the next show starts, in the station's time zone.
  Signing in and reading stay open. `always`, the default, is 1.0.0's behaviour.
- **Tests**: `test-bundle` runs both MP3 streams at their rates, the Opus stream at a rate set and
  measured, and a listener on each mount through playout's restart (none dropped, no chained Ogg
  stream).
- **Smaller changes**: the studio tells an admin when the station's setup is not finished; after a
  handover the toast says the outgoing window's talk keys now speak as the DJ; the rundown's handover
  button is the quiet one; only an admin or the show's own presenter changes a show's presenter name
  and takedown contact; the DJ wizard asks before taking an overnight slot; Settings' dialogs have
  panel names; Icecast out's hint tells a DJ to ask the admin; only `⌃T` reaches the desk while a
  dialog is open; a failing RTMP destination says what to check.
- **Fixed from the public tracker**: the player's CSP allows the steering edges' hosts (issue 3, not
  yet tried in a browser against a real edge); a CDN's `join.hls_url` counts for the 500-viewer
  warning (4); `[studio]` and `[steering]` can be set beside a generated settings file, on Cloudron in
  `/app/data/station.toml` (5; not yet on the Compose bundle); the edge's viewers' `Cache-Control`
  follows its TTL settings (6); the steering edges' view takes an admin's session (7); stale
  interface sentences, the rundown in the station's time zone, pre-recorded episodes shown as such
  (10); a station with no chat says so on the Screener tab (14). The Cloudron listener domain's bare
  address sends a visitor to the player.
- **The website's sandbox**: the show no longer skips its jingle step, and a laptop's F-keys (often
  volume keys without Fn) are explained.
- **Tests**: `test-dump` no longer counts the second in which its second tone begins (a measurement
  artefact that failed about half of all runs).
- **A Night theme in the studio app** (ADR-0270): the theme toggle offers Night beside Dark and
  Light, for presenting late. Warm near-black grounds, warm off-white text and an amber accent,
  with no blue or cyan on the screen: the focus ring, selected text, links, ticks, the chips, the
  cart colours and default avatars included. ON AIR stays red and OPEN the white lamp (a warm
  white); every pair meets WCAG AA. Dark stays the default, and the choice is remembered on the
  machine. The sandbox desk has it too.

## 1.0.0 — release one (2026-11-04)

The first public release. What each part does is on the documentation site, https://weavesignal.top;
what is built and what is not is on its page *What is built, and what is planned*.

### In this release

- **The live room and the desk.** Presenters, invited guests and callers in one LiveKit room; the
  desk mixes the programme and a mix-minus for each person in one pass per 20 ms block, with a gate
  per role, talkback, cue, a limiter on every mix and the profanity delay with Dump.
- **Call-in and screening.** The player page's call-in button, consent to record before any token,
  a proof-of-work challenge instead of a CAPTCHA, a booth room for each caller, the screener's lanes,
  *Banned before* and *Desk busy*, TURN over TLS for networks that block UDP.
- **The studio app** for Linux (AppImage, `.deb`, `.rpm`) and Windows (an unsigned installer), with desk B,
  the screener, the cart wall, the schedule, settings, staff accounts and signed updates.
- **Playout** through Liquidsoap 2.5: automation from clocks with rotation and separation, the US
  statutory rule, a dead-air fallback, a default loop, the per-play log, listener figures, the legal
  logger, now playing, a loudness control towards −16 LUFS, and an MP3 compatibility stream.
- **Video**: cameras, a cut-only vision mixer, the slate with a still per show, lower thirds, captions,
  AV1 into CMAF HLS behind the delay, and the audio-only toggle.
- **The live chat** with light accounts, moderation with statements of reasons, retention and a minimum
  age by jurisdiction.
- **Shows, cards and feeds**: show cards in two layouts, seasons that fill in their own episodes,
  podcast feeds with an Opus alternate.
- **Delivery at scale**: content steering, an HLS edge and an audio relay image, Bunny in the guide.
- **The public API**: scoped tokens, open read routes, signed webhooks and an OpenAPI description.
- **Packages**: the Compose bundle with `weavesignal.sh` (and `--public` with a certificate),
  Quadlets, and a Cloudron app.

### Known limits

- **Run a station on a server of its own.** On a heavily shared server the listener feed's cushion
  filled from 3 s to its 10 s cap; on an idle 4-vCPU server 0.5 s held for 13 hours unattended.
- **WebRTC from the desk has no congestion control**: the desk's image lacks GStreamer's `rtpgccbwe`.
- **Not run end to end yet**: a 24-hour unattended run from a seeded week; two real people over two real
  networks (one person on two networks was run); an off-site edge in front of a station; Safari's Opus
  in fMP4 on a device; the GPU in the Compose bundle; phones on GrapheneOS over a real network.
- **Firefox for Android** can call in only once its microphone is allowed in Android's settings.
- **LiveKit's TURN refuses clients that reach it over IPv6**: give the TURN name an A record only.
- `podman-compose` is refused by `weavesignal.sh`: it ignores the bundle's settings files. Use Docker
  Compose, or Podman with the `docker-compose` provider.
- Not in release one: the presenter kit, the Windows server, WHIP for remote DJs and the DJ overlap
  handover.
