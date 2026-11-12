<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
# Changelog

## 1.0.2 (2026-11-12)

Public issues 15 and 17 (ADR-0272).

- **From the operator's run on the test station**: a window signed in with the station key is no
  longer offered Join with my microphone… (the link is made for an account, and the station
  refused it); the step menu says plainly that steps belong to a show and how to make one when no
  show has been on air; the account box says how to sign in as yourself (Sign out, then your
  username; an admin makes accounts in Settings › Staff).
  A step the window cannot change now looks greyed rather than clickable, and with no show the
  menu offers the Schedule tab. The documentation calls the studio's bar along the top the **top
  bar** (it said "the frame"), and names the Desk tab's **left panel** and **right panel**.
- **More from the operator's run**: the desk's empty stage says why and what to do (*Nobody is in
  the room yet*, with Invite someone… and how to join yourself; *Nobody is on air* when people wait
  under Not on air), and a number key on an empty strip says so instead of doing nothing. The
  Schedule tab says how to make the first show, lists shows with no weekly rule with **Set its
  weekly rule…**, opens a new show's rule at once, drafts the slot of the block selected (a slot
  under way included; it used to draft next week's), selects a block on the press with a pointer
  and an outline, and follows the minute instead of the clock's tenth, which made typing lag.
- **The studio's frame no longer clips at a large text size**: at 120-130 % its right end (the link,
  the text size, the theme) was cut off; the frame and its tabs now take a second row when they must.
  The desk's and screener's widest layouts follow the page as the text size draws it, not the
  window's own pixels, so a large window at a large text size no longer squeezes them.
- **Join with my microphone… at every step**: the desk offers the DJ their own microphone link
  whenever nobody holds the presenter's seat, not only at Solo (the operator, 2026-11-12); above
  Solo only to an admin, a producer or the show's presenter, and not while a DJ who took over
  is on air.
- **More places to send the show** (RTMP out): **YouTube** (first in the list), **Owncast**, **X** and **Gab** join Rumble, Odysee and PeerTube. Each platform's note says which profile to use: Compatible (H.264 and AAC) everywhere, and for YouTube a hint to try AV1 and Opus, which it may take over Enhanced RTMP. Owncast was proved live from a Cloudron station on 2026-11-11 with Compatible, and refuses AV1 and Opus. What each takes is to be checked again before each release.
- **A way back from Settings**: the page has **Back to the desk** (or the tab it came from) at its top, with that tab's key; the tabs above still leave it too (the 1.0.1 test run: "we struggled to get out of the settings section").
- **A caller's page names who answers**: at the Callers step there is no screener, so the page
  says the presenter is talking to them or will pick up, not "the producer"; before a call the
  player page says "the studio answers first". The caller's state gains `presenter_answers`.
- **Simple guides** (`/guides/`): eleven short, plain guides for people rather than programs,
  first in the documentation's sidebar: *Start here* asks who you are (presenter, guest, or whoever
  looks after the station) and each answer is one guide, from *Your first show* and *Invite a guest*
  to *Change the station's settings* and *When something goes wrong*. The rules they keep are in
  `docs/WRITING-GUIDES.md`.
- **End show** (issue 15): `⌃⇧E` and the stage's **End show** button, which only showed a note, now
  ask once (`⏎` confirms, `Esc` keeps going) and then take everyone off air, fade the carts and hand
  the air to automation once the profanity delay has played out, so the last words are heard. New
  route `POST /api/v1/show/end`, any staff member's; it answers with the new view, how many were
  taken off air and `automation_in_ms` (0: already on automation; null: the station has no playout
  layer to hand the air to).
- **Hand to automation** (issue 15): a button under the master lamp sets the station's mode to
  `auto` and back to `live`, asking first when someone would be cut off or put on air. New route
  `POST /api/v1/playout/mode` (`{"mode": "auto"}` or `{"mode": "live"}`), any staff member's;
  `PUT /api/v1/playout/settings` stays an admin's. Neither new route is open to an API token.
- **A real listener count** (issue 17): the Listeners panel counts listeners by mount and says why
  when it cannot ("the listener process does not answer"), instead of "not counted yet". The count
  no longer needs the playout layer (the database and playout's socket) to run. The view's
  `audience` gains `by_mount` and `error`. With edges or a CDN the figure is the origin's and the
  panel says so.
- **A listener delay that says what it reads** (issue 17): the profanity delay and the listener
  process's feed buffer (`feed.buffer_length`) are read live; the rest (the stream's own hold, each
  listener's player) stays the `studio.listener_delay_ms` setting, because nothing can mark a
  listener stream. The desk calls the sum an estimate, says what is read and what is not, and
  `measured` stays false. The view's `listener_delay` gains `desk_ms`, `process_ms` and `rest_ms`.
  The setting's default is now 13 s (it was 15 s), so a default station shows about 21 s, as the
  demo station measured (21.2 s); a station that set its own value keeps it.

- **Prepare mode: the station's side** (ADR-0276). A studio window is **Live** or **Preparing**; a
  session starts live. A preparing window can see the live desk but cannot change what is on air:
  the desk's commands (`POST /api/v1/desk/commands`), the cart wall's commands
  (`POST /api/v1/carts/commands`), the screener's commands (`POST /api/v1/screener/commands`),
  ending a pass (`POST /api/v1/passes/end`), End show (`POST /api/v1/show/end`) and the station's mode
  (`POST /api/v1/playout/mode`) answer 409 with "This window is preparing a show, so it cannot
  change what is on air. Switch it to Live first." Everything about the person's own show (shows,
  the schedule, cards, the step, invitations, banks, settings, the chat) stays open to it. New route
  `PUT /api/v1/sessions/me/mode` (`{"preparing": true}` or `{"preparing": false}`) sets the mode of
  the caller's own session and answers as `GET /api/v1/sessions/me` does, which now also says
  `preparing`. The station key and API tokens are never preparing (the route answers them 422).
  The mode is kept in memory: after a restart every session starts live again.
- **Prepare mode: the studio app** (ADR-0276). A staff window says **Live** or **Preparing** in its
  frame, after who is signed in, and switches there; the station key has no switch. A preparing
  window shows the live desk to follow, with a band under the frame on the Desk, Screener and Carts
  tabs, and every control that changes the air is disabled and its key does nothing; the Schedule
  tab, Settings, invitations, the chat and the step work as usual. The presenter of a show that is
  not on air now (by the schedule) starts Preparing; everyone else starts Live, as before, and no
  window switches to Live by itself. **Switch to Live** asks once while someone else's show is on
  air (`⏎` confirms, `Esc` keeps preparing). A refusal from the station (409) is shown in its own
  words, and the window then follows the station. To make room in a frame at most 1,600 px wide,
  the name stands without the role and the link shows only its tick while it is up. New how-to:
  *Prepare your show while another is on air*. The sandbox desk has the switch too.
- **A cart wall per show** (ADR-0276, the operator's answer of 2026-11-12). Beside the station's
  banks, which every show still shares, a show can have up to five banks of its own, numbered apart
  from the station's, so the next DJ sets up their carts in a preparing window without touching the
  show on air. Migration 0034 gives each bank an owner; every bank that exists stays the station's,
  so an upgrade loses nothing. `GET /api/v1/carts?show=<slug>` lists the station's banks and that
  show's, each bank and cart naming its show (`show`, or `null`); new routes under
  `/api/v1/shows/{show}/carts/banks/` make, rename and remove a show's bank and fill or empty its
  slots, for the station key, an admin, a producer or the show's presenter (a preparing window may;
  an API token may not). A cart in a show's bank starts only while that show is the one the
  station's step comes from (409 otherwise); stopping and fading work on anything sounding. A
  sounding cart carries its `label`. In the studio app a live window works with the show the step
  comes from, a preparing window with the show it prepares; tab *N* of the Carts tab (`⌃F1` to
  `⌃F5`) is the show's own bank *N* where it has one, else the station's, and each tab says
  **station** or **show**; `F1` to `F6` fire the top row of the tab shown, as before. The sandbox's
  first bank is The Morning Kettle's own.

- **Beginner steps: the control plane** (ADR-0271, ADR-0274). A show runs at one of four steps, `solo`,
  `guests`, `callers` or `full`; what a step leaves out is switched off while the show is on air.
  Shows that exist stay `full`, a new show starts at `solo`. `PUT /api/v1/shows/{show}/step` changes it
  (an admin, a producer or the show's presenter; the show's own `PUT` carries `step` under the same rule),
  logs who and when, and tells every studio window. The studio's view gains `step` (the station's step,
  the show it comes from and whether that show is on air, the next show with its step, and `may_change`
  for the account asking) and the chat's counts of held messages and open reports (`chat.held`,
  `chat.open_reports`). The station's step is the show on air's, the show last on air's between shows
  (kept across a restart), and `full` if none ever aired. Below `callers` call-in is closed ("The lines
  are closed for this show.", on the player and for the call-in request itself), waiting callers are
  released with the cause for their page (`released`), and the lines open at once when the step rises.
  Invitations and passes for hidden seats are refused (the pass stays good for later; a person already
  in the room is never removed); `dump` is refused below `callers`; the handover and the picture's modes
  below `full`, where the picture is held on the slate and its mode returns at Full. The Solo DJ's own
  link is `POST /api/v1/invites` with `{"for_me": true}`. Migration 0033. Product docs: *Choose how much
  of the studio a show uses*.
- **Beginner steps: the studio** (ADR-0271, ADR-0274). The show's step is at the top of the studio,
  beside the station's name: four pips and a word. Press it for the four steps; picking another first
  says what will change at once (what appears, what stays off, who stays in the room), then the
  button names the move. **Show everything** draws the whole studio in one window for the moment
  and switches nothing on: a row under the frame says so, and each part the step keeps off carries
  a dashed tag, *off at Solo*.
  - **Solo** is one tile, the music and carts, the listener count and the chat to read; an empty
    seat beside the tile says why the desk is quiet and offers the way up. Before the DJ has
    joined, **Join with my microphone…** makes their own link and opens it in a browser (the
    packaged app hands it to the system's browser).
  - **Guests** brings Not on air and **Invite someone…** (a guest or a presenter; the next
    show's DJ comes with Full). **Callers** brings the delay with Dump and the queue on the desk,
    **Callers**: `P`, or **Take … to strip 4**, brings the caller who has waited longest to strip
    4, off air (ADR-0275, proposed). **Full** is the studio of 1.0.1.
  - A person already in the room keeps their tile at every step. **End show** and the button
    under the lamp are at every step.
  - The key of a part the step keeps off does nothing, and its first press in a session says so.
- **The chat to read, on the desk**: the Chat panel shows the room's last messages at every
  step, and says in one line when held messages or reports wait for a moderator. It used to say
  only that the chat was on the Screener tab.
- **A show's step in the show editor and the wizard**: **Step** under the show's name, and the
  DJ's setup asks *How much of the studio to start with*, with Solo picked. The wizard's pages
  are *parts* now ("Part 2 of 3"), so "step" means one thing.
- **The public pages**: under Solo and Guests the player says *The lines are closed for this
  show.*, and *You can still write in the chat below.* while the chat is open; a caller who was
  waiting when the lines closed is thanked, told why, and told they were not on air; the
  invitation page says when the show on air is not taking guests, and offers **Look again**.
- **Invitations**: the interface and the documentation say *invitation* for the noun
  (**Make the invitation**, *the invitation link*). **Invite someone…** keeps its verb, and the
  route is still `POST /api/v1/invites`.
- **The frame says Connected** while the link to the station is good, in place of *Control plane
  connected*; the full words return whenever it is not.
- **The sandbox** has the same steps: **Learn**, a ladder of the four at the start of its guide
  bar, opens the scenarios of a step, and the chooser lists them by step, Solo first. A link may
  name a step (`#solo`, `#guests`, `#callers`, `#full`). New scenario *A guest by link*, at Guests;
  *Playout takes over* no longer has the next DJ standing by, and *Running everything alone* takes
  its caller from the desk's Callers panel. The simulated station keeps the station's rules.

- **One caller at a time at Callers** (ADR-0271): the station refuses to send a second caller to the desk while one is on the callers' strip ("At Callers the desk takes one caller at a time…"); at Full the screener may send more, as before. `tests/m2/run-studio.sh` takes `STEPS=1` for a public address and a takedown contact, so the steps can be tried on the real desk.

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
