<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
# Changelog

## 1.2.0 (2026-11-20)

- **A show can pull a stream** (issue #20): a partner station's Icecast mount or any HTTP or HTTPS
  audio, an HLS playlist, or an SRT listener. Made with the API as a show key with `hint: pull`
  and the address; the station opens it from before the show's time, and it stands by, goes on
  air at the post and comes back after a drop as a host's own studio does. Addresses inside the
  station's own network are refused unless `WEAVESIGNAL_PULL_ALLOW_PRIVATE` is set, checked again
  at every connection, redirect and segment. HTTPS uses rustls with Mozilla's root certificates.
  The how-to is *Pull a partner's stream into a show*. Migration 0042.
- **The rota and the handover window** (issue #21). `GET /api/v1/rota` lists every show of the
  coming twelve hours with who is down for it, whether from the room or their own studio, and
  whether they are ready. A new rule for sources, `window_s` (300 s by default, 0 off, at most
  1,800), opens a window before each show's start: `post.window` in the studio's view and in
  `GET /api/v1/sources` names the next host and says `announced`, `cueing`, `ready`, `overrun` or
  `held`, and the incoming host gets one heads-up an airing (a desk notice,
  `handover.heads_up`, and a line on their monitor link), written in the sources' log first so a
  restart does not repeat it. The window changes nothing that goes on air. A `PUT` of the rules
  without `window_s` keeps the stored one. Migration 0043. In the studio app the window has rows
  under the post line of the desk's clock (who is next, from where, READY, CONNECTING or NOT HERE
  YET), the heads-up is a quiet notice, the rota is the Schedule tab's fourth view (`R`, or Rota
  beside Coming up), and the window's length is in Settings › Own studios › The rules; the monitor
  page shows the heads-up above the lamp.
- **A host can send from OBS over WHIP**, sound only, with their show key: OBS's *Server* is
  `https://<public domain>/whip/<key id>` and its *Bearer Token* the key's secret. The station
  answers it itself, ICE-lite on one UDP port (`SOURCE_WHIP_PORT`, 9712, **off by default** on
  Cloudron, Compose and the Quadlets), with no STUN or TURN, and the source stands by, tests, goes
  on air and is covered when its link drops exactly as an Icecast or SRT one. The picture OBS
  sends with its sound is taken and thrown away, so OBS needs no change, and it does not count
  against the source's 2 Mbit/s. A key can be made for WHIP, and its details give the WHIP
  address and token. Needs the public domain and the
  station's public IPv4 address. A real OBS has not been tried yet. New internal port
  `INGEST_WHIP_PORT` (7319, loopback). Migration 0044.
- **The bridge, and the presenter kit's station half** (issue #22). A host sends from their own
  software to the studio app on their own computer, and the app carries it to the station over one
  WebSocket on the public address's 443 (Settings › Bridge): no source port on the station, the
  show key in the keychain, the way `bridge` at the desk, Ogg and FLAC heads sent again on every
  reconnect. On Compose and the Quadlets (never Cloudron), with `KIT_VBAN=true`, a studio PC's VBAN
  stands by as a show key's source and the desk sends VBAN out (the programme after or before the
  delay, or a monitor mix) to one address each; presets set the routing (Settings › Presenter kit).
  Every source is now resampled once at the desk at a ratio that follows its clock, instead of
  skipping each time it drifts past the cushion. Migration 0045 adds the two ways and the kit's
  settings.
- **Listeners can ring the studio from an ordinary phone**, on Compose and the
  Quadlets (not yet Cloudron), off until a SIP trunk is set: `PHONE=true` adds `phone.yaml`, with
  LiveKit's SIP service (v1.25.0) and a Valkey that LiveKit shares while the line is on. A caller
  hears a few words (the station's own recording, or a short one that ships in two synthetic voices,
  `PHONE_DEFAULT_VOICE` `alba`, a Scottish woman's and the default, or `edinburgh`, a man's; CC BY
  4.0, made with Piper, which is not shipped), presses any key, and waits in `screener.queue` like a caller from the player page; the desk answers a call
  only when the gate says so and holds the caller's voice out of every mix until then. With the
  words off (`PHONE_CONSENT_WORDS=false`) callers press no key and their record says *not asked*.
  The station keeps a ban key and the last four digits, never the number; a withheld number is
  taken but cannot be banned by number. Bans, a takedown by a typed number, and the station's own
  recording of the words are new routes under `/api/v1/phone`. New ports, checked by
  `weavesignal.sh` and `check-ports` while the line is on: SIP 5060 (UDP and TCP), RTP 7340–7355
  for 8 lines (UDP), Valkey 7321 (loopback). The SIP service logs errors only, to one file of at
  most 512 KB. The how-to is *Take phone calls*, and *Decide whose contact to give callers* walks
  a station through whether it needs the words and a named person. Migration 0046. Not yet tried
  through a provider's trunk. An admin can turn the words off and choose the voice in the studio
  (Settings › Phone line, with Play to hear each voice first): `PUT /api/v1/phone/consent` and
  `GET /api/v1/phone/consent/default/{alba,edinburgh}.opus`; what is chosen there is kept in the
  database and wins over the settings file. Migration 0047.
- **A GPU for the desk on Compose and the Quadlets** (issue #8): with video on, the
  desk can encode on an NVIDIA card with NVENC's AV1 (and RTMP out's H.264), handed to its container
  by the card's CDI name. `GPU=nvidia` adds `gpu.yaml` on Compose and the desk unit's `AddDevice=`
  on the Quadlets; with no `GPU` line, or `GPU=auto`, `weavesignal.sh` looks for an NVIDIA CDI spec
  in `/etc/cdi` and `/var/run/cdi` and writes `GPU=nvidia` or `GPU=none`, and `GPU=nvidia` with
  no spec stops it with the `nvidia-ctk cdi generate` command. New settings: `VIDEO_ENCODER`
  (`auto`, `nvenc`, `svt`; the desk's `[video] encoder`, which had no setting on any install
  route), `GPU`, `GPU_DEVICE` (`nvidia.com/gpu=all`) and `NVENC_SESSIONS` (12); the install wizard
  asks *A graphics card for video*. SVT-AV1 on the CPU stays the fallback for any rung NVENC cannot
  take. Cloudron gives an app no GPU: its `VIDEO_ENCODER` is added as `svt`. Not yet measured on a
  card with this release's image; `tests/gpu/measure.sh` does that, stopping at the first Xid in
  the kernel log. A host's own picture is still not used: passing it through without encoding it
  again is designed in *Video in the desk*, not built.
- **A station contacts no outside STUN server unless its operator names one** (ADR-0290), on
  Cloudron, Compose and the Quadlets. Up to 1.1.1 the desk asked Google's server for every
  connection (GStreamer's default), LiveKit asked Twilio's and Google's for its public address when
  `NODE_IP` was empty, and the join pages were handed LiveKit's own default servers. Now the desk
  sets none; LiveKit always starts with an address of its own (`NODE_IP`, else the A record of the
  station's domain, read at start on Cloudron and when `weavesignal.sh` runs on Compose and the
  Quadlets) and `use_external_ip` off; and the pages drop every STUN server that is not in the new
  `STUN_SERVERS` setting (empty by default; up to four `host:port`; LiveKit, the desk and the pages
  read it). A name with no A record stops `weavesignal.sh`. On Cloudron the app starts anyway with
  a warning, on the last address the name gave, or with the room unreachable from outside if it
  has none. Set `NODE_IP` for a name behind a proxy. On Compose and the Quadlets run
  `weavesignal.sh` again to update. The packages reference lists STUN servers run by free-software
  projects, which a station may choose to name.
- **The consent words are a setting on every install route** (issue #26): `CONSENT_TEXT`, in
  `weavesignal.env` on Cloudron and in the settings file `weavesignal.sh` writes for Compose and
  the Quadlets. Empty, the default, keeps the shipped words exactly, so no caller is asked again;
  set, the station's own words may use `{station}`, `{kept}`, `{contact}` and the new
  `{presenter}` (the show on air's presenter, else the station's name). On Cloudron write them in
  double quotes; on Compose and the Quadlets in single quotes with each apostrophe as `\'`, and
  without a backslash, a dollar sign or a per cent sign, which `weavesignal.sh` refuses. There the
  words reach the control plane as `WEAVESIGNAL_CONSENT_TEXT`, which takes the place of
  `join.consent_text` when it is not blank. The Quadlets' generator now refuses any setting with a
  backslash or a per cent sign rather than write a unit that Quadlet would drop or systemd would
  change.
- **The desk's congestion control is a setting**, `CONGESTION_CONTROL`, on every install route:
  `gcc` (the default, as since 1.0.1), `homegrown` or `disabled`, for the desk's WebRTC senders (each
  return and the programme picture). Measured three ways with `just test-cc`; the default stays.
- **A show can come from a stream the station pulls in the studio app**: the show editor's
  *Comes from* has *A stream the station pulls*, which reads the address as it is typed and makes
  the show's key; WHIP's details say it is sound only.
- **The phone line, the presenter kit and the bridge in the studio app.** The screener shows a phone
  caller as *Phone ···0123* or *Withheld number*, with *Ban this number…*; Settings › Phone line
  holds the line's state, the consent words' switch and the voice. Settings › Presenter kit is a
  form (one clock, presets as buttons, VBAN in and out rows) instead of JSON, and Settings › Bridge
  shows what to type into your software.
- **A compact Picture and Callers at 1,440 by 900**: the programme picture's line and the cameras
  are in sight under the switch without scrolling, and three callers and **Take** fit where two did.
- **The install wizard asks two more questions**: an outside STUN server (*None* first) and the
  station's own consent words, written for each route as its install script reads them.
- **The join pages on a phone**: the call panel no longer lays out three times too wide at 390 px,
  which had put *Reconnect through the relay* off the screen. `just test-join-layout` keeps it.
- **The listener count's error says what it means** (issue #17): `audience.error` in the studio's
  view names the two real reasons it has no count; the desk reference says where the count comes
  from on each install route.
- **The desk keeps its controls in view at 1,440 by 900.** End show no longer runs over Dump, the
  stage's tiles on air keep their notes and buttons while Not on air scrolls, and when the desk
  folds to one column a notice has its own row above the status bar. Playout's tile no longer
  repeats how to restart it: the Playout panel, the status bar and the notice each offer that.
- **VBAN's lost packets reach the clock readout.** `GET /api/v1/kit/clock` gave every VBAN source
  `lost: 0`, because the desk, which made the readout, never sees VBAN's packets. The ingest now
  counts each bound stream's packets lost on the way (filled with silence; one dropped late stays
  in `vban_dropped.late`) and reports the count beside the connection every second
  (`lost`, VBAN only), and the readout joins it to the source by its show key.
- **The studio app asks before its window closes while the bridge runs**, since closing stops
  the bridge and the host's source leaves the desk: *Keep the window open* (in focus, and Esc) or
  *Stop the bridge and close*. A second close while the question shows goes through, so a page
  that cannot answer never holds the window. Panel `studio.close-ask`.
- **A phone caller joins the screener's queue only once through the gate** (17f). The desk lists
  a phone caller from the moment it answers, while the words play and their voice is held out,
  and the queue took them in then, before any key; now it waits for the desk to say they are
  admitted. LiveKit's `Phone 4471` no longer becomes the caller's name at the second read of
  the desk. **Drop** on a phone caller in the queue or on air turns them off air and out of the
  cue, ends the call at the desk and frees their line (tested end to end against the fake desk).
- **The STUN servers from the studio, on Cloudron** (ADR-0290, the STUN chooser's Settings half).
  `GET /api/v1/setup` gains `stun`: the servers this start uses (`in_use`; LiveKit, the desk and
  the join pages read one setting), whether this install lets Settings name them, what was
  asked and whether it waits for a restart. `PUT /api/v1/setup/stun` (an admin's) takes a list
  of `host:port`, checked by the same rule as `STUN_SERVERS` (an empty list is none); on
  Cloudron it is written to `setup.env` beside the stream password's answer and `start.sh`
  applies it to all three parts at the next restart, ignoring a line it would not take. On
  Compose and the Quadlets it answers 409 and the settings file decides. No migration.
- **The presenter can talk to a caller from the Screener tab** (issue #25). With no screener in
  the room, `T` there is the presenter's talk to the selected caller, and while it is held the
  desk now carries the caller's answer back to the talker's ear alone (the matrix's new rule: a
  talker hears whoever they talk to, unless that person is talking privately to someone else),
  so the caller need not be cued, stays off the programme and is heard by nobody else. Holding
  it takes the presenter off the programme, as every Talk key does.

## 1.1.1 (2026-11-19)

- **A notice no longer hides the chat.** On the desk a toast stands first in the right column's
  slot and Chat keeps what is left under it, down to its header and two lines. The column gives
  the slot a floor of 7.5 rem: the panel above it (Picture or Callers) gives way and scrolls inside
  itself, with **Take** and the picture's switch kept in sight, so a toast is always drawn. In 1.1.0
  a window of 1,440 by 900 with video on left the slot no height at all. A toast with keys holds
  its message to two lines, then one, in a short slot, so its answer is never cut.
- **Name plates follow what can be seen.** With plates pinned (`?audit=1`), a panel that is not
  drawn has no plate, and one scrolled or squeezed out of sight has none and is listed as not in
  view. Plates had stayed where a closed menu, dialog or hidden panel last was, over the controls
  beneath it.
- **The desk names the parts of the listener delay.** The clock's line says, for example, *8.0 s
  profanity delay, 0.5 s buffer, 12 s held by the stream, about 0.5 s in listeners' players
  (estimated)*. Only the players' part is an estimate. `listener_delay` in the view and API gains
  `mount_hold_ms` and `player_ms`, `null` for a station on an older single figure other than 13 s.
- **A setting for the player allowance** (ADR-0289): `studio.player_allowance_ms`, or
  `STUDIO_PLAYER_ALLOWANCE_MS`, default 500. The stream's 12 s hold is fixed. An existing
  `studio.listener_delay_ms` or `STUDIO_LISTENER_DELAY_MS` keeps working as before; if both are set
  the allowance wins and the control plane logs a warning.
- **Compose with Podman where the session runs above an OOM score of 0.** On such a host (seen with
  Podman 6.1.3, crun 1.31 and systemd 262) `podman exec` failed in every container Compose had made
  (*crun: write to /proc/self/oom_score_adj: Permission denied*), so PostgreSQL's health check never
  passed and the station did not start. Each service now takes `WEAVESIGNAL_OOM_SCORE_ADJ`, which
  `weavesignal.sh` sets from the session on Podman; it is never lowered, and Docker is unchanged.
- **Docs.** What RTMP out has really been tried against (Owncast, YouTube and X, from a Cloudron
  station). The desk has used congestion control since 1.0.1, which adapts the programme picture
  but cannot help a guest on a poor link. Going back below 1.0.5 asks you to sign in again.
- **Tests.** The unattended run records each container's anonymous memory and file cache from
  outside the stack (`memstat.jsonl`), and `just test-cc` measures the desk's sending under loss,
  jitter and limited bandwidth, with congestion control on and off.

## 1.1.0 (2026-11-18)

- **The studio's chat connects on a Cloudron station.** The studio's chat socket now opens on the
  station's public address when it has one (learned from the schedule's reply, `public_url`), since
  a socket cannot send the bearer that Cloudron's login in front of the app's own domain wants and
  that domain answered the upgrade with a redirect to `/login`. The requests stay on the app's own
  domain. An address that never opens is given up for the station's own on the next try; `?chat=`
  and a station with no public address are unchanged. `deploy/cloudron/test-local.sh` checks that
  the public domain answers the upgrade with 101.
- **Per-address limits on Cloudron count each person again.** nginx passes none of the server's
  proxy headers to a location that sets its own, so the chat's and the control plane's locations
  sent no `X-Forwarded-For`, and every chatter and caller counted as 127.0.0.1: the chat's 50
  sockets an address and call-in's per-address limit applied to the whole station. Each such
  location now restates the client's address and the host, and `test-local.sh` checks that none
  drops it.
- **Docs and the not-set-up page.** *End the show* says when to use Hand to automation and when
  End show, with an example of each. The not-set-up page and *Set up the station* say where to find
  the station key on Cloudron, on Linux with Compose or Quadlets, and on any other install.
- **The studio app's version in the top bar, and what automation is, said in place** (the
  operator's click-through of 1.1.0): the top bar says the studio app's version in its spare
  room, between the tabs and who is signed in (*WeaveSignal 1.1.0*, or *v1.1.0* in less room; it
  never moves a control or adds a row, and gives way first), and the station's beside it when
  the two differ by their numbers (*v1.1.0 · station 1.0.5*), with both in full in its tooltip.
  **Hand to automation** keeps its name and gains the studio's own tooltip, on hover and on the
  keyboard's focus: what the press does, then what automation is and plays (the log: the music
  planned for the hour, with carts such as idents, stingers or adverts; with nothing in the log,
  the station's fallback files or the loop WeaveSignal ships). The AUTO lamp's tooltip adds the
  same sentence to its own, and the air dialog says it wherever the press gives automation the
  air (End show, Hand to automation). The lamp's tooltip opens under the button, not on it, and
  a tooltip whose word or button is also a panel's title begins beyond the panel's name plate
  instead of under it.
- **Issue 9's keys** (ADR-0289): `⌃R` pressed twice restarts playout, like
  `Del Del` dumps: the first press says what the second does, the second asks the station, which
  tells playout to shut down so its supervisor (Compose, the Quadlets, the Cloudron app's
  supervisord) starts it again. New route `POST /api/v1/playout/restart` (any member of staff,
  refused to a window that is preparing a show, never an API token) and a `playout.restart` command
  on playout's socket. `⌃4` opens the Screener tab with the focus in its Room lane (the studio has no
  Chat tab, and the greyed one is gone from the tab strip). `⌃⇧R` (the listener process, which drops
  every listener; there is no Settings button or route for it yet, so a restart of it is the server's)
  and `⌃⇧B` (a backup stream, with no design) are no longer keys.
- **The desk after the look's review**: while a host in their own studio
  has the air, the stage no longer headlines *You are not in the room yet* over their tile; the
  offer is one quiet line at the head of Not on air, beside Invite someone… (at Solo, in the
  guest's seat), which says what joining is for and that it puts you on air. Each panel states
  the listener delay once: the clock says *about 23 s behind* with its parts on the one line
  under it, in plain words (*8.0 s profanity delay, 0.5 s buffer, about 14.5 s in listeners'
  players (estimated)*; no *read* or *set*, and no tooltip that repeats them), and the delay
  panel says what it holds (*full · 8.0 s held*), not where listeners are. **End show** is drawn
  as Hand to automation is, an outlined button at 13 px with its key cap.
- **The desk: a count's last seconds, Leave, Hand to automation beside End show, the join words and
  the listener figures** (14a, issue 17): every count on the desk
  (the clock's, the tile's late count, the next show's card, the toasts, a lost link's place) turns
  amber in its last 30 s and red in its last 10, with the count's mark (a clock glyph, directly
  before the figure; never FAULT's triangle), a heavier figure and the words beside the colour; on the presenter's own tile the button is **Leave** (End pass for yourself); **Hand to
  automation** is a button as wide as the lamp and as tall as End show's, with its words on hover
  (it was 12 px); **Join with my
  microphone…** says the browser tab carries the voice and must stay open; the Listeners panel and
  the clock read `audience` and `listener_delay` as the server left them (an estimate with its
  parts while it is one, the plain figure once it is measured; the words are in the entry above).
- **The team's directory is a producer's and an admin's** (ADR-0289):
  `GET /api/v1/staff/directory` answers a producer, an admin or the station key, and refuses a
  presenter or a screener with 403 and the words *Only a producer or an admin can see the team*.
  The show editor does not ask for it from a presenter's or a screener's window: they are offered
  themselves (and no account) as before, and a show tied to someone else's account keeps it with no
  picker for them; a producer is offered themselves, and an admin anyone, as before.
- **A show's presenter picked from the staff accounts, and Comes from in New show**: the show editor's Presenter group gains **Staff account** (`presenter_account`; an admin
  names anyone, anyone else themselves, sent only when changed), read from the new
  `GET /api/v1/staff/directory` (id, name and role of each account that is on; a producer's or an admin's, below);
  New show asks **Comes from** (the room, or the host's own studio) and Done offers **Set up a
  host…**.
- **The show, the rundown, the banned list and content steering as settings on Compose and
  Cloudron** (issue 5): `STUDIO_SHOW`, `STUDIO_LISTENER_DELAY_MS`,
  `STUDIO_RUNDOWN`, `STUDIO_BANNED`, `STEERING_TTL_S`, `STEERING_CHECK_EVERY_S`,
  `STEERING_CHECK_TIMEOUT_MS`, `STEERING_DOWN_AFTER`, `STEERING_CLONE_FROM` and `STEERING_EDGES` in
  `.env` or `weavesignal.env`; the lists are TOML inline tables. Unset, nothing changes; on
  Cloudron `station.toml` still replaces a whole section.
- **The listener count and delay, the server's half** (issue 17): the view's
  `listener_delay` gains `basis` (`estimate` or `measured`), `read_ms` and `set_ms`, and `audience`
  gains `scope` (`origin`: the station's own listener process, every mount; edges and HLS viewers
  are not added). Old fields stay. The count itself has run without the playout layer since 1.0.2;
  a test now puts a stand-in listener process's answer through to the view the desk reads.
- **The presenter's gate keeps the end of a word** (issue 16): the expander in the presenter's and DJ's chain now holds open for 250 ms (`[inputs.<role>.expander] hold_ms`, 0 to 2000) after the voice falls before it closes, so a word's soft last consonant is no longer turned down by up to 14 dB at ordinary speech levels; `hold_ms = 0` is the old gate.
- **The monitor link's counts turn amber, then red, and Start listening is off where nothing can play** (ADR-0280, 14a): every count on the page is amber in its last 30 s and red in its last 10 s, with the words *Last 30 seconds* and *Last 10 seconds* beside the colour; Start listening is dimmed, with its reason beside it, while the page has lost the station, the host's link is lost, or the host is not at the desk yet.
- **The monitor link plays the listeners' stream while the host stands by**
  (ADR-0280, 14a): with nobody on air at the desk, automation holds the air downstream of the desk,
  so a standing-by host's return was silent. The page now plays the station's own listener stream,
  says how far behind it is, and cuts it within one view of the host going on air, of someone else
  going on air at the desk, or of the views stopping. The view gains `monitor_has_programme`.
  The same while the host is OPEN (live to the room, automation holds the air): the card says it
  is the station's output, some seconds behind, with the room's voices not in it, and before
  Start listening is pressed it says *will hear*. No level bar is drawn beside the stream, since
  it would show a silent room.
- **A station not set up yet says so**: on its own address (the app's own
  domain on Cloudron, the one address on Compose) `/` shows a short page, not the player, while
  the station still has a name nobody chose and setup has not been finished: it says where to
  begin (the setup page and the station key, the studio app, connecting it). A new install's name
  is *This station* (it was Kettle FM) on Cloudron and Compose; a station still called Kettle FM
  whose setup was never finished (a 1.0.5 install updating) sees the page too, and any other name
  keeps the player. New open route `GET /api/v1/front`, which the
  Cloudron package's nginx asks; the public pages' domain never changes.
- **Hosts in their own studios, the station's side** (ADR-0280,
  ADR-0284): a show key for each host; their software sends over Icecast (MP3, AAC, Opus, Vorbis,
  FLAC) or SRT (encrypted with the key); outside the show's slot it is a private test, in it the
  source stands by on the desk, levelled, until put on air; see *Set up a host in their own studio*.
- **Hosts in their own studios, the monitor link's page** (ADR-0280): the
  page `monitor/`, opened by a link that carries its secret after the `#`. A host sees their own
  state at the size of the desk's lamp with the station's sentence, counts on the station's clock
  (never the device's), their return with a level bar and a volume control, before you, you and
  after you, the next insert and what the station hears; and may press Go on air now, Not yet,
  Cue, the handover's two presses, End my show (asked once) and Dump (two presses). When views
  stop arriving it says so and shows no state. Light or dark with the device, and a Night switch.
  See *Use your monitor link*. Driven against a stand-in station (`just test-monitor-page`) and
  used by a host on a real station.
- **Hosts in their own studios, the monitor link's controls and live view**
  (ADR-0280): `GET /api/v1/monitor` grows to the design's view (the lamp and its sentence, what
  happens next with its count, the post's grace, hold and late count, before you, you and after you,
  the next insert as far as the log can say, the show's step, the next host, and what the host may
  press with the reason for each they may not), `GET /api/v1/monitor/events` carries it every
  second, and the host's controls act through the desk's own paths: `POST /api/v1/monitor/go`,
  `/not-yet`, `/cue`, `/handover` (two presses: `bring`, then `hand_over`), `/end-show` (End show for
  this host alone) and `/dump`. A key that ends takes its monitor room's two seats out of the room.
- **Hosts in their own studios, the monitor link's sound** (ADR-0280): a host
  with a monitor link hears, through the room's own path, the programme without themselves, the cue
  bus and talk addressed to them, in a room of their own (`<room>-monitor-<key id>`) that only the
  link's two seats may subscribe to. The desk makes the mix only while someone listens. A host's
  monitor secret gives `GET /api/v1/monitor` (their own source's state, way in, format and level, the
  show and the station's clock) and `POST /api/v1/monitor/token` (a 600 s listen-only token; two
  browsers, a third refused), both open on Cloudron's public domain. The page and the controls are to
  come.
- **Hosts in their own studios, the server's leftovers** (ADR-0280): the
  schedule's moves (the post, the late count's end, a grace's end, a return after a lost link, End
  show passing the air) now cross over on the desk in `sources.fade_ms` (2 s) instead of switching,
  the one coming on rising as the one going off falls; a person's press stays instant, and a desk
  from before the fade is told again without it. `POST /api/v1/sources/{key}/off-air` lets a lost
  source's kept place go at once (the desk toast's **Off air**, logged, by whom). The monitor view
  gains `dumped` (DUMPED and the seconds listeners lost, for ten seconds, for the host on air, after
  a Dump from the link or the desk). Deleting a show takes its keys' monitor seats out of the room
  and closes their connections, with no event stream open.
- **Hosts in their own studios, the studio's screens** (ADR-0280): a host's
  source is a tile on the desk (Own studio, STANDBY under Not on air, the station's leveller,
  what their software sends, how long the link has held, **Disconnect**), with a toast when it
  arrives; **Settings › Own studios** sets a host up, shows their details once with **Copy** on
  each field and **Copy everything as text**, lists every key with how it stands, and gives
  **New key…**, **End key…**, **Let back in** and, for an admin, the rules; the show editor's
  **Comes from** and **Your show** offer the same setup. Talk to a host is drawn off until the
  monitor link exists.
- **Hosts in their own studios, after the slot and wrong keys** (ADR-0280): when
  a slot's door closes, a source standing by that is not on air becomes a private test (off the desk,
  connection kept, 15 minutes from then, logged as *test*); one on air is never touched. Five wrong keys
  from one public address in a minute still turn it away for ten minutes, but an address that is
  private, loopback, link-local, unique-local or carrier-grade NAT is only counted and logged. The
  addresses in a key's log are kept 30 days, outside the retention setting, and the retention page says
  so. The Cloudron package gets a first-steps checklist and a plainer post-install note.
- **Hosts in their own studios, the desk at the post** (ADR-0280): the desk
  says what the post's rule is doing. The clock's line names the next show's post, counts the grace
  with **Hold** for an admin or a producer (*Handover held by …*, **Let it run**), or says the DJ on
  air keeps the air while the next host has not connected; the rundown has a row for that show with
  STANDBY, NOT CONNECTED or LINK LOST. A late host is counted on air on their tile with **Not yet**,
  and **Go on air now** is the handover's button and `⇧⏎` at Full; `Esc` on the toast stops the count.
  A source whose link is lost keeps its tile on the stage with how long and until when its place is
  kept, a toast once automation covers, and a note when it is back on air. **End show** says where
  the air goes before the press, to the next host standing by or to automation, and after it.
- **Hosts in their own studios, the desk's remaining items** (ADR-0280): a
  grace running is offered in the desk's toast (*Heather goes on air in 0:42*; **On air** on `⇧⏎`
  brings the next host on now, as the handover does, and **Not now** on `Esc` only puts the offer
  away), and `⇧⏎` picks the next show's source standing by before any DJ waiting. **Off air** on a
  lost link's toast and on the kept tile's own On key lets the place go at once, and says when
  automation took the air with it. At Solo the next show's source is one card in the guest's empty
  seat (`desk.next`), with Cue and when the schedule brings it on. A key's details give the host's
  whole monitor link with **Copy**, and so does *Copy everything as text*; a source's tile says
  *Monitor open* or *Monitor not open*, and **Talk** goes to the monitor link while it is open.
  **Open my monitor** opens a presenter's own link from the window that made their key: the station
  keeps only a scrambled form and cannot give the link again.
- **The Schedule toolbar stays on one line** (issue 28): in a week across two months, or a wider
  system typeface, the legend wrapped onto a second line at 1440 by 900. It now gives way first:
  shorter words (*Automation*, *From the rule*, *Aired*), then its glyphs alone, each with its
  full words as a tooltip and for a screen reader.
- **The step button's name plate lies over no control** (issue 27): with the panel names pinned
  (`?audit=1`) on the Carts tab, the plate of `studio.step` lay over the step button, because the
  row of banks left no room under it. A plate with no room by its title now takes the first clear
  gap along its row, down to one that holds its name alone; a pinned plate with no room even for
  that is left out and listed as `no room`, never drawn over a control. A panel's plate also
  answers only to its own title: pointing at the step button showed the frame's plate too, and
  pinned, the plates of the frame, the stage and the Not on air row lay on top of the plates of
  the panels inside them.
- **Hosts in their own studios, the post's rule** (ADR-0280): the control plane
  works out every second, from the schedule and the source log, who goes on air at a show's post. With
  automation holding the air and the next host's source standing by (steady for `steady_s`), it goes on
  air at the post, to the second; a host who arrives late goes on air after the late count (`Go on air
  now` and `Not yet` are the host's own); with a DJ on air the DJ keeps the air for the grace and then
  the schedule hands over, unless a producer or an admin holds that handover; a source on air whose link
  drops is covered by automation after `silence_s`, its place kept `place_kept_s`, and goes back on air
  once steady again (30 s after three drops in ten minutes); a drop during a grace hands over at once;
  End show with the next host standing by passes the air to them. `/studio` and `GET /sources` carry
  `post`; new routes `POST` and `DELETE /air/post/hold`, `POST /sources/{key}/go` and `/not-yet`;
  migration 0041. The desk fades a source's On over `fade_ms` for these moves; a person's press stays
  instant.
- **The ingest runs wherever the station runs** (ADR-0280, ADR-0284): in the
  server image, as a Compose service and a Quadlet, and under the Cloudron package's supervisor.
  Its two public ports are settings, off until set: `SOURCE_ICECAST_PORT` (TCP, 8005 usual) and
  `SOURCE_SRT_PORT` (UDP, 9710) for `weavesignal.sh` and the install wizard, and two ports of the
  same names in Cloudron's dashboard; the station's rules show them and cannot change them.

## 1.0.5 (2026-11-16)

- **An episode's Move… saves the move at once.** In the 1.0.5 test run nobody could tell how a
  moved show was kept: **Move…** then **Set** only put the new time in the editor's draft, and
  it was saved by **Save episode** at the foot of the column. The button is now **Move**, and
  one press saves it, as a show's Dates do (ADR-0283): the week shows the episode at its new
  time and follows it to another week or day, and the tab's line says where it is, and any
  show it now clashes with, with **Undo**. Only the time is sent, and the length if it was
  changed, so a title or guests typed in the editor stay a draft for **Save episode**. `⏎` in
  a field is **Move**; **Cancel** and `Esc` are as before. A move the station refuses (a
  presenter's onto another show) leaves **Move…** open with what was typed and the reason.
  **Back to the rule** is saved at once too, with **Undo**. An aired episode is not moved, as
  before.
- **Arrows beside every schedule time.** The studio app's own window on Linux draws a time
  field with no arrows, so a time could only be typed. Each time on the Schedule tab now has
  two: in an episode's **Move…**, a date's **Move…** in a show's Dates and in the rule dialog,
  and the rule's **from** and **until** (New show, **Change…**, the presenter's setup). A press
  moves the time to the quarter-hour before or after it (18:07 goes to 18:15, or back to
  18:00); the field is still typed in for an odd minute, and `↑` `↓` in it step as the system
  has them. In a **Move…**, a step past midnight takes the date with it; **from** and **until**
  wrap round the clock. `Tab` passes over the arrows.
- **`Esc` clears the selection on the Schedule tab.** Once a block or a show was open in the
  editor column, the only ways out were another block, **‹ Shows**, or a change of view. `Esc`
  now closes what the column holds and clears the selection, so the list of shows is back; a
  menu or a dialog still takes the press first, an episode's **Move…** is cancelled by it as a
  date's is, and in a text field the first `Esc` leaves the field. It never throws work away: while the episode, series or show has changes that are
  not saved, nothing closes and the tab says so. A press on the grid where no block is does
  the same, though automation fills every gap in time, so that is only the narrow gap beside
  a day's blocks and, in a wide window, the plane beside the Day view's column. **‹ Shows**
  names the key in its tooltip. An `Esc` that closes New show or another dialog on the tab is
  that dialog's alone, and no longer reaches the tab behind it.
- **Studios says that it is not in this build.** The Schedule tab's **Studios** button wrote
  its note where only the Desk and Screener tabs draw one, so nothing showed; it is in the
  tab's own line now.
- **A folded show's tab reads as its show's colour.** The tab's ground was 40 % of the show's
  colour over the surface, which the 1.0.5 test run found too faint: a dark green on
  near-black in Dark, a pale blue in Light. It is now 60 % in Dark and Light and 50 % in
  Night, whose colours are lighter, and the count on a fold stands on the same grounds. The
  chevron and the name on a tab are white in Dark and a warm white in Night (the ink the
  lamps use on a filled colour) and stay dark in Light: 5.9:1, 4.9:1 and 4.7:1 at the least
  over the twelve show colours, worked out from the tokens and checked in the tests.
- **The Day view with nothing selected lists the shows.** A block pressed on one day (the
  hours automation fills, say) still held the editor column after another day was opened, so
  the Day view showed an automation log (*Nothing generated for these hours yet*) beside a day
  with nothing selected in it. The column now shows the selected block only while its day is
  the one open, and lists the shows otherwise, as the week does; the block is selected still
  on its own day and in the week.
- **A changed rule's notice names its show and the save that made it.** The window that saved
  knows the notice for its own exactly, even with two windows of one account or two shows of
  one name, and a rule dialog open on that show says so (*Moira changed when The Folk Club
  airs. Saving puts this rule in its place.*) before a save would replace someone else's
  change. A station older than 1.0.5 is not sent the token, and the sentence is read as before.
- **Several stations in one studio app** (ADR-0285, ADR-0287). The
  station's name in the frame is now the button that lists the stations this computer keeps.
  Each is a separate install with its own address and sign-in; nothing is shared between
  them. **Add a station…** opens the sign-in screen over the studio; pressing a row switches
  to it; **Manage** gives a station a name of your own, signs out of it or removes it.
  A station you already use carries over as the first in the list, and you stay signed in.
  `⌃⇧S` opens the list from any tab (not from a text field); no chord switches by itself.
- **Leaving a station that is on air is asked first.** A switch cuts no audio, but nobody at
  the window can mute, dump or take a call on the station left until they switch back. Held
  talk keys are let go first. A Live window is asked once while a strip is open or a caller is
  in screening; a Preparing window and a quiet desk switch at once.
- **Nothing of one station shows or is sent on another.** A switch closes the event stream
  and the chat's socket, empties every store, tells the app's own side (which stops every
  request still open and refuses any that names the wrong station), and loads the page again.
  Each station's key is its own entry in the keychain, or its own file where there is no
  keyring; removing a station deletes it.
- **A station on another version is said, not refused**: *older than this app* or *newer than
  this app* on its row and in the note on arrival.
- **Fixed: the packaged studio app could not read the station's version.** It asked for
  `/health`, which the app's own side did not let through, and fell back to guessing from the
  week's reply.
- **Fixed: the cart bank the window last showed was never restored** when the app opened: the
  read failed quietly and every window opened on bank 1. It is now kept for each station.
- **Clashing shows fold in the week.** Where shows share time, one block is open and readable
  and the others fold beside it at the day's right edge, in their own colours; press a folded
  block, or `⏎` on it, and it comes forward. Two or more hidden at one time are a count (*+2*).
  Each day's header counts the shows that clash on it. Nothing is drawn through anything else,
  and hover no longer looks like selection. In a very wide window they stand side by side.
- **A Day view**: **Week**, **Day**, **Studios** in the toolbar. One day at the week's scale,
  with a lane for each clashing show and the week's seven days as buttons above it. A day's
  header in the week opens it, and so do **Open the day** and the count. The week never
  changes to it by itself, and the tab opens on **Week** every time.
- **A clash is said where it is.** A block in a clash has a warning triangle before its time,
  and names the other show; a date nobody has drafted says CONFLICT too, which it never did.
  With such a block selected, the editor begins with **Clashes with**: each other show, **Show
  it**, who is on air by the schedule, and **Open the day**. A block dropped onto another
  show's time says so, with **Undo**.
- **A list of shows, and each show's dates.** With nothing selected, the editor column lists the
  station's shows by name, each with its rule in short and its next date; **‹ Shows** at the
  start of every pane's path comes back to it. A show opens at **Dates**: when it airs in a
  sentence, how many are to come, and a row for each date (drafted, not drafted, skipped,
  moved, clashing), with **Draft**, **Skip** or **Put back**, **Move…** and **Back to the
  rule**. Pressing a row shows it in the week. Each change is saved as it is made, with
  **Undo**. What has aired is listed under the rows. **Details** is the show editor as before.
  The list is also reached from **Show** on an episode, **All dates**, **All its dates**,
  **See its dates** when New show is done, and **See them** after **Give it dates…**.
- **The week's blocks have their grounds and colours.** Every block now carries its show's
  colour on its left edge and stands on a solid ground, with automation hatched, as the design
  has had them: a style rule had been overriding both, so the grid's lines showed through
  every block. The selected block has its inner line again.
- Fixed: an episode's **Move…** showed its date and cut its time off: one date-and-time box was
  too narrow for the editor column. It has a date field and a time field now, as a show's dates do.
- Fixed: a style meant for the rule dialog's two columns also reached the desk's *Not on air*
  line, cutting it to 24 rem, and a column of the retention table.

- **Keys for the Day view** (ADR-0283): on the Schedule tab `D` opens the Day view on the day
  its button would choose and `W` returns to the week; in the Day view `←` `→` move a day, as
  the toolbar's arrows do (they moved a week).
- **The public JSON routes on Cloudron** (ADR-0282): `/api/v1/public/station`, `/schedule` and
  `/episodes` are on a Cloudron station's public domain, GET only, as on the Compose edge, so a
  station's own website can read them; they answered 404 there.
- **Large screens** (ADR-0286): the root text no longer jumps to 20 px (18 px on the Screener)
  above 2,200 px wide; the window's growth (1.0.4) and **A+** **A−** set it.
- **The Windows installer has been run on Windows** (Windows 11, 2026-11-15): it installs for
  the user with no administrator prompt, signs in to a station and keeps the key in Credential
  Manager. The install guide now gives Edge's own steps to keep the download, and says the
  SmartScreen window may not appear after them.
- Fixed: the desktop app never read the station's version (it may ask only under `/api/`, and
  the version is at `/health`), so New show took any station to store rules; an older station
  would refuse it. The app may now ask for `/health` too, and nothing else outside `/api/`.
- Fixed: the studio app's window opened larger than a small screen's free space (1280 by 800
  less the task bar, on Windows), so a dialog's buttons sat under the task bar; it now opens
  maximised there, and its smallest height is 600 px, so a 1366 by 768 laptop holds it.
- Fixed: every window opened on cart bank 1; the bank last shown on this machine comes back.
- **Calendar files** (ADR-0281). The schedule and each show's go out as iCalendar files a
  listener or a presenter can subscribe to (`/api/v1/public/schedule.ics`,
  `/api/v1/public/shows/{show}.ics`), one event per show's rule with its repeat, skipped and
  moved dates and the station's zone, carrying only the show's name and description. And a
  calendar file from another system comes in as new shows (`POST
  /api/v1/schedule/import/preview`, then `POST /api/v1/schedule/import`): one per recurring
  event or per run of events with one name, each said back with its sentence, next dates and
  clashes, marked exact, as chosen dates for twelve months (a rule no show's rule can say, or
  another zone whose clocks change on other dates), or refused with the reason; made in one go
  at Solo with no episode drafted. On Cloudron the two calendar routes are on the public
  domain. The reference page *Calendar files* has the detail.
- **Subscribe…** on the Schedule tab's toolbar, and beside **When it airs** in a show's
  **Details**: the link to the schedule's calendar file, or the show's, as `webcal://` and as
  `https://`, each with **Copy**, and **Download .ics**. The link is on the station's public
  address, never the one the studio talks to (on Cloudron that one is behind the login); a
  station with no public address shows no link and says who sets one. The studio app saves the
  download into the Downloads folder, says where, and never writes over a file there.
- **Import a calendar…** on the toolbar, for an admin or a producer: choose an `.ics` file
  (up to 1 MB and 200 events), read each proposed show with its sentence, next dates and
  clashes, untick or rename any, and **Make the shows**. A file that is not a calendar, or is
  too large, is refused in a sentence; a show the station refuses says so under its own row,
  and nothing is made.
- The two are buttons where the window has room for their words, and the entries of one menu
  (**⋮**, beside **New show**) in a narrower one, so the toolbar stays on one line at 1440 by
  900.
- The week's reply (`GET /api/v1/schedule/episodes`) says the station's public address
  (`public_url`, or `null`), from which the studio makes those links.
- **A show the station already has is not a clash with itself.** Reading a station's own
  calendar file back with **Import a calendar…** proposed every show as a CONFLICT with
  itself. A proposed show with the name of one the station has (case and spaces apart) and the
  same dates and times now says **Already on the station**, starts unticked, and is left out
  of the clashes' count; its dates still clash with other shows as before. The preview's reply
  says which show in `already`.
- **Import a calendar… takes a pasted path and a dropped file.** The system's file chooser was
  hard to paste a path into. A file can now be dropped anywhere on the dialog, and in the
  studio app its path can be pasted into **Or paste the file's path**, as a file manager
  copies it or as **Download .ics** said it (quotes and `~` are fine). The app reads only a
  file ending `.ics`, of at most 1 MB, that begins as a calendar. A browser cannot read a
  path, so there the field is not shown.
- **A folded show in the week is a tab you can see and press.** A show lying behind another
  showed only as a thin line of its colour at the day's right edge. It is now a tab as wide as
  the strip, on a ground filled with the show's colour, with a chevron, the show's name down it
  where the tab is tall enough, and a tooltip; pointing at it rings it and a press brings the
  show forward. A long show with a shorter one over its middle has its name again below. The
  open block says what it is over even when it is an hour tall: *over The Long Lunch*. In the
  studio app the week's scrollbar no longer lies over Sunday's tabs, which could be seen and
  not pressed.

## 1.0.4 (2026-11-15)

- **New show, guided like calendar software** (ADR-0278): **New show** asks for the name, then
  *When it airs*: one date and time, and whether it comes back (once, every week, every month on
  the third Tuesday or the last Friday, every year on a date, or on dates you choose), and when
  it ends. On the right the station says it back in a sentence with its last date, shows the
  dates in small months and as numbered rows, marks where the clocks change and what clashes,
  and lets a date be skipped, moved or (a chosen date) removed. When every date clashes with
  another show, it offers the nearest clear time within three hours (*Try 21:00 to 22:00*).
  **Make the show** drafts the first episode; **Draft all 10** the rest. **Change…** in the
  show editor reopens the questions and, before saving, lists the drafted episodes with their
  old and new dates. The presenter's own setup asks the same in *When it is on*.
- **The studio grows with its window** (ADR-0277): above 1920 by 1080 the whole interface is
  scaled to the window, on top of the text size, and the top bar and tabs wrap instead of
  clipping. Dialogs keep their buttons inside the window at any size and text size.
- **Not-drafted dates in the week** are dashed blocks: select one to **Draft this episode** or
  **Skip this date** (with **Undo**).
- **Delete show** (ADR-0279): an admin or a producer may delete a show none of whose episodes
  has aired or is on air, from the show editor (**Delete show…**, with a confirmation that
  counts what goes with it) or with `DELETE /api/v1/shows/{show}`. In one transaction the show
  goes with its series, drafted episodes, own cart banks and carts, and the logs of its step and
  rule changes; then its card pictures, slate still, carts' files and the audio files only it
  named are deleted, and the open cart wall is read again. A show that has aired is refused
  (409, one sentence naming how many episodes aired) and stays as the station's record. A
  presenter, a screener and an API token may not (403).
- **A show airs by a rule, not a weekly slot** (ADR-0278, migration 0036): once, every N
  weeks on some weekdays, monthly on the nth weekday or on a day, yearly, or on chosen dates,
  with dates skipped, dates moved to another time, and an end (never, until a date, or after
  N episodes). The control plane works the dates out, says the rule back in a sentence and a
  short form (`rule_words`), finds the next date (`next_on`) and the clashes with other
  shows; the studio does none of it. A weekly slot becomes the slot-shaped rule and airs at
  exactly the same instants (a test checks every slot over two years across the clocks
  changing, in four zones); `slot` is still sent while a rule is exactly that, and an older
  studio's save keeps a rule it cannot see. The three slot columns stay for one release,
  unread.
- **New routes**: `PUT /api/v1/shows/{show}/rule` and `PUT …/series/{series}/rule` (the rule
  alone, with `drafted`: `move` or `leave`, in one transaction, logged and told to every
  window), `POST /api/v1/schedule/rule/preview` (the sentence, the next dates with the clocks
  changing and the clashes, what saving would do to the episodes; it writes nothing), and
  `dates` in `GET /api/v1/schedule/episodes` for the week's dotted blocks. `PUT` of a show
  takes `rule` and `draft_first`, which makes series 1 and drafts the first episode in the
  same transaction. [When a show airs: the rule](docs/src/content/docs/docs/reference/show-rules.md).
- **The Schedule tab says when someone else changes a rule** (*Moira changed when The Folk Club
  airs. The week is up to date.*) and reads the week and the shows again; an open show editor
  reads its show again too, keeping what was typed, so its save cannot send the old rule back.
  A window is not told of its own change, and a **Skipped** with its **Undo** stays beside the
  line. Found in the 1.0.4 test run, where the tab neither showed the notice nor reloaded.
- **New show says where a date it did not ask for came from**: opened with a block selected in
  the week, part 2 says under **First on** *From the block selected in the week: Thursday 19
  November, 15:00 to 16:00*, with **Use today instead**. In the test run that date and time went
  unnoticed and shows were made on the wrong day.
- **When it airs, from the keyboard**: the next dates are one stop for `Tab`, then the focused
  row's own buttons (ten rows were twenty stops before **Make the show**); a row's button or a
  chosen date's chip pressed from the keyboard leaves the focus on a row or a chip, not outside
  the dialog; a column that scrolls on a short window fades at the edge where more waits.
- **A changed rule moves the episodes drafted for dates to come**, in order, keeping their
  numbers, titles and guests (or leaves them, if asked). Aired episodes are frozen and an
  episode a person moved or made by hand stays. Each episode now records the rule date it fills
  (`occurrence`); episodes made before are given theirs at the first start.
- **A count counts episodes**: a skipped date does not use one up, so the run is one date
  longer. A series is complete when it has its planned episodes: a further draft answers 409
  (it used to be allowed without limit).
- **Drafting never goes into the past**: the first episode is the date under way, else the next
  on or after the series' start, however long ago that was; and a series left alone for weeks
  goes on from the date under way, not from the day after its last episode.
- **A presenter may not draft an episode over another show's time** (409, naming the other
  show); an admin, a producer, the station key and a token are warned and may go on.
- **Playout sends the desk its copy of the programme on its own clock**, as it sends listeners'
  copy, with a 1 s write timeout; on a clock of its own that copy fell behind real time on the
  test station. The video stream's sound while nobody is live still has gaps on a busy host.
- **The desk's status reports the copy's buffer**: how often a piece was placed afresh after a
  pause or a burst (`feed_replaced`), and `feed_underruns`, `feed_late_ms` and `feed_ring_ms`.

## 1.0.3 (2026-11-13)

- **Saving a YouTube, Owncast or X destination no longer fails** ("the database refused
  that"): 1.0.2 offered their forms, but the database still allowed only Rumble, Odysee,
  PeerTube and another server. Migration 0035 allows every form the studio app offers, and a
  test saves each of them.
- **Drafting a series' first episode through the API takes the slot under way**, as the studio
  app already did: with nothing drafted and no start date, the first episode is the slot on air
  now (its length, or an hour), not next week's.
- **Gab is no longer offered as a video platform**: its live streaming is not open to streamers
  yet (its Go Live Now failed on the test station), and Gab says it is working on it. The
  database still allows a Gab destination, so the form can come back when Gab opens it.
- **The guide to video platforms says what has gone live**: YouTube and X from the test station
  (2026-11-12), with their server URLs.

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
