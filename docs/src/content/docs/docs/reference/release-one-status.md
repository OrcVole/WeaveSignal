---
title: "What release one does, and what it leaves out"
description: "WeaveSignal 1.0.0 and 1.0.1: what each part of release one brings, what is not in it, what 1.0.1 added, the known limits, and what was measured. Every 'planned' mark on this site points here."
type: reference
status: draft
last_verified: 2026-11-12
sidebar:
  order: 1
---

WeaveSignal's release one shipped as **1.0.0** on 2026-11-04, and **1.0.1** on 2026-11-12 adds what [Since 1.0.0](#since-100) lists. It was built in milestones (M1 to M11), each a working slice, and this site was written as each one landed (ADR-0072).
Where a page says **planned**, the feature is not in release one; [Not in release one](#not-in-release-one) says where it stands.

## In release one

| Part | What it brings, as of 2026-11-04 |
|---|---|
| **M1 · On air** | the control plane (Axum, PostgreSQL); room tokens with roles; the desk: a programme and a mix-minus for each person, gates by role, talkback, 32-bit float mixing with a limiter on every mix, the presenter's input chain; SRT to playout with the desk listening; the fixed Liquidsoap 2.5 scripts and `icecast.server`; the Compose bundle; every port a setting with a free-port check |
| **M2 · The studio app** | the desk and the screener, wired to the real desk through the control plane; the keymap, with the dump (`Del Del`), the carts (`F1`–`F6`, `⌃F1`–`⌃F5`) and the picture (`F7`); ON AIR, OPEN, AUTO and PLAYOUT DOWN, with a sentence on every state word ([The state vocabulary](/docs/reference/states/)); every panel named; cue, mute and held talk; the presenter's gate; the cart wall; the profanity delay and Dump; staff sign-in by password or an emailed code; the Schedule tab and its editors; Settings for retention, staff, API tokens and updates; the light and dark theme. The desktop app signs in to a station over https, or plain http on this computer and private networks |
| **M3 · Call-in** | the player with call-in and the invite page, served by the control plane with their caching and CSP ([The public pages](/docs/reference/public-pages/)); consent to record before any caller token; call-in's challenge, a proof of work in place of a CAPTCHA; each caller in a booth room of their own; a telephone-line sound for callers, on by default; a camera preview before joining when video is on; the single-pass mixer; TURN over TLS, tested on loopback with a browser that blocks WebRTC's UDP, as GrapheneOS's does; calls from a LineageOS phone over LTE and Wi-Fi |
| **M4 · Playout and the schedule** | the listener process holding the mount through playout and desk restarts; library import; automation from clocks with rotation, separation and the US statutory rule; the dead-air fallback with notices; the default loop for an empty fallback folder; the per-play log; listener hours and sessions; the legal logger following retention; now playing, with up-next off for US statutory webcasters; the processing chain (a gain control towards −16 LUFS and a peak limiter); an MP3 compatibility stream, off by default and checked by the Compose bundle test ([Playout](/docs/reference/playout/)); the week grid drawn by Event Calendar (ADR-0185) with the episode, series, show, log and card editors ([Schedule](/docs/reference/surfaces/calendar/)) |
| **M5 · Video** | cameras, the vision mixer, the slate with now playing and a still per show or the station's (ADR-0200), captions, AV1 to CMAF HLS behind the delay, the audio-only toggle, camera thumbnails in the studio app; the video's sound is the desk's while someone is live and playout's otherwise ([Video in the desk](/docs/reference/desk-video/)) |
| **M6 · The chat** | the chat service, accounts with a keyed hash of each address, moderation with statements of reasons and its keys on the Screener tab, the screener's lanes, the legal toolset, retention by jurisdiction, a minimum age that follows the station's jurisdiction (ADR-0148) ([The chat](/docs/reference/chat/), [Moderate the chat](/docs/how-to/moderate-the-chat/)) |
| **M7 · Delivery at scale**, the local parts | content steering in the control plane, with health-checked edges and the steering tag in the playlists ([Content steering](/docs/reference/steering/)); the HLS edge and the audio relay images, the relay reaching the station over TLS ([Edge images](/docs/reference/edge-images/)); two local edges for the Compose bundle; Bunny in the guide ([Put Bunny in front of the station](/docs/how-to/use-bunny/)) |
| **M8 · Shows, cards and feeds** | show cards in the Poster and Slate layouts at three sizes, and the Poster square; seasons that fill in their own episodes; episodes live or pre-recorded; corrections to an aired episode, logged; the podcast feed with an Opus alternate |
| **M10 · Packaging** | the studio app's AppImage, `.deb` and `.rpm` for Linux amd64 and its unsigned NSIS installer for Windows, with the signed updater configured at build time, its update screen, the feed's script and the AppImage rollback; `weavesignal.sh` for the Compose bundle, with start at boot on Podman; Quadlets generated from the bundle; the bundler's downloaded tools pinned; the 1.0.0 packages published as the assets of [release v1.0.0 on the Forgejo](https://forgejo.weavesignal.top/WeaveSignal/weavesignal/releases/tag/v1.0.0) and in the updater's tree, with the update feed at `https://weavesignal.top/updates/latest.json`; the server, edge and relay images published, and the Compose bundle's defaults ([Packages and install files](/docs/reference/packages/)) |
| **M10 · Packaging**, the public route | `weavesignal.sh --public`: a Caddy reverse proxy with a certificate for the station's name in front of the bundle, LiveKit's TURN over TLS using it, `forwarded_for` on, the free-port checks ([Publish a station from the Compose bundle](/docs/how-to/publish-with-compose/)). Run on a dedicated public server with a real certificate on 2026-11-03: a real caller called in through it from a phone |
| **M10 · Packaging**, Cloudron | the Cloudron package 1.0.0 (`top.weavesignal.cloudronapp`), amd64, installed from its published image ([Install on Cloudron](/docs/how-to/install-on-cloudron/)); listed on `ca.cloudron.io`, the Cloudron community app store, since 2026-11-04 (ADR-0077) |
| **The public API** | scoped tokens, open read routes for a station's own website, webhooks with signed deliveries, and an OpenAPI description generated from the code ([The API](/docs/reference/api/), [Integrations](/docs/reference/integrations/)) |

## Not in release one

| Part | What is left out |
|---|---|
| **M2**, the rest | a 30-minute show driven from the keyboard alone; the webview tests on Debian and the GNOME runtime; the studio app joining the room itself (ADR-0013); the Chat tab (`⌃4`) and the desk's chat glance (the chat is on the Screener tab); a fader on the desk's tiles, and a camera picture in them; the desk's rundown and show heading from the schedule, and the banned list from the moderation log, which are still settings; a measured listener delay, which is still a setting; a screen for webhooks (they are made through the API) |
| **M3**, the rest | a join from a GrapheneOS phone over a real network; WHIP for remote DJs; in 1.0.0, the DJ overlap handover (`⇧⏎` showed a note; built since, below) |
| **M4**, the rest | what playout does differently with a pre-recorded episode; studios on shows; in the studio app, restart playout (`⌃R`), restart the encoder, the backup stream and end the show, playout as slot 6, playout's queue and hard posts, listener counts by mount, and the listener server's health and the logger on the desk; the screener's listener count |
| **M5**, the rest | Safari's Opus in fMP4 measured on a device; the GPU in the Compose bundle; a still per episode and the slide schedule (a still per show is built); video for callers |
| **M7**, the rest | an edge on another network, with join time and failover in a real hls.js player; steering through Bunny tried against Bunny itself; the hours-long real-network soak of `icecast.server` against Icecast 2.5.0, which decides the listener server (ADR-0011) |
| **M8**, the rest | a step that publishes an episode's words before it airs; opening an episode's recording and publishing it as a podcast episode from the studio app (files are attached through the API); an episode marked as aired by itself (only **On air now** does it) |
| **M9 · The presenter kit and the Windows server** | moved to release two (ADR-0207): presets, the patchbay, VBAN, and the server on Windows. The studio app's Windows installer stays in release one |
| **M10**, the rest | an update and a rollback run on a real install; the Windows installer run on Windows |
| **M11**, the rest | screenshots of every surface from the running app |

## Since 1.0.0

Released in 1.0.1 (2026-11-12):

| Part | What it brings |
|---|---|
| **Avatars in the chat** | each message shows its sender's avatar: one of 72 animals and monsters that come with WeaveSignal (83 KB, MIT), or a picture of their own, which they centre and zoom on the player page and which nobody else sees until staff or a moderator approves it in the screener's Needs you (one nobody decides on in 30 days is deleted, an admin can switch uploads off at once, a listener can report one, and a suspected child-abuse picture is kept as evidence for one named person, ADR-0254, 0255, 0257, 0259, 0260); refusing or removing one sends a statement of reasons; erasure deletes them (ADR-0235; migration 0026; [Choose your avatar](/docs/how-to/choose-your-avatar/), [The chat service](/docs/reference/chat/#avatars)) |
| **Pictures and documents in the chat** | off unless the station turns it on: a trusted chatter attaches a picture (PNG, JPEG, WebP; rewritten at about 1,600 pixels with nothing but its pixels, a PNG for a screenshot, a JPEG for a photograph) or a document (PDF, OpenDocument text or spreadsheet, Markdown, plain text; passed on unchanged, only ever a download), checked by its bytes, not its name; each waits for staff or a moderator in the same queue as avatars, with the same evidence path, 30-day wait and Report this; it goes when its message goes. An approved upload keeps only its clean copy; a suspected-CSAM statement says only "the rules on illegal content" (ADR-0262, ADR-0258; migrations 0031 and 0032; [Share a picture or a document](/docs/how-to/share-a-picture-or-document/), [Review pictures and documents](/docs/how-to/review-pictures-and-documents/), [The chat service](/docs/reference/chat/#pictures-and-documents)) |
| **Install wizards and the station's look** | a page on the website that asks a new station's questions and gives back its settings file, DNS records and commands, in the browser and sending nothing ([Install wizard](/docs/install-wizard/)); a setup page on the station for its admin: the name, the takedown contact, the jurisdiction, and on Cloudron the stream's password; and the station's own accent colour, brand icon and banner on the player and public pages, the colour kept readable in both themes and uploads re-encoded by the station (ADR-0237; [Set up the station and its look](/docs/how-to/set-up-the-station/), [The station's look](/docs/reference/branding/)) |
| **A show's presenter and contact** | a presenter name and a takedown contact for each show; while it is on air the consent words name them first, then the station as the host (ADR-0214, ADR-0215); a DJ's first sign-in asks three short steps, kept as the defaults for them and their show (ADR-0218; [Set up your show](/docs/how-to/set-up-your-show/)) |
| **Icecast out** | the programme sent as MP3 to another Icecast server, such as a DJ's AzuraCast, with the delay and Dump in it (ADR-0226; [Send your programme to AzuraCast](/docs/how-to/send-to-azuracast/)) |
| **RTMP out** | the delayed picture and sound sent to Rumble, Odysee, PeerTube or any RTMP server, as H.264 and AAC or as AV1 and Opus over Enhanced RTMP (ADR-0230; [Send your show to Rumble, Odysee or PeerTube](/docs/how-to/send-to-video-platforms/)) |
| **Calls on a stream from OBS** | a guide for streamers who keep OBS (ADR-0224; [Take calls on a video stream with OBS](/docs/how-to/take-calls-with-obs/)) |
| **Video in the Cloudron package** | `VIDEO` in `weavesignal.env`, off by default, AV1 on the CPU; the app's memory limit is 2 GiB (ADR-0227; [Install on Cloudron](/docs/how-to/install-on-cloudron/#video)) |
| **Open files in the Cloudron package** | the services' open-file limit raised from Cloudron's 1,024 to 65,536, so the listener stream is no longer capped below 1,024 listeners ([Open files and large audiences](/docs/how-to/install-on-cloudron/#open-files-and-large-audiences)) |
| **The one-key DJ handover** | `⇧⏎` puts the next DJ on air beside the presenter, then takes the presenter off; the presenter's seat, with the desk's talk keys and `⌃T`, follows the air to the DJ; an invite can be made from the desk (ADR-0074, ADR-0231; [Hand over to the next DJ](/docs/how-to/hand-over-to-the-next-dj/)) |
| **`chat:moderate` through the station's address** | an API token with `chat:moderate` switches the chat's modes at `/api/v1/chat/modes` through the station's address: the chat passes it on, and the control plane checks it as on its own routes. In 1.0.0 the chat refused it with 401 ([Integrations](/docs/reference/integrations/#scopes)) |
| **Two MP3 streams, and the listener chooses** | a second MP3 mount beside 1.0.0's, 320 kbit/s by default for music, each off until turned on; 1.0.0's `COMPAT_*` settings are the first, unchanged; the player offers Opus and each MP3 stream that is on, and keeps the listener's choice (ADR-0232; [Playout](/docs/reference/playout/#the-mp3-streams)) |
| **The Opus stream's rate as a setting** | `OPUS_BITRATE`, 64 to 256 kbit/s, 96 by default, still constant bit rate (ADR-0233; [Playout](/docs/reference/playout/#the-opus-streams-rate)) |
| **The chat open during shows only** | `CHAT_HOURS=shows`: posting open while a scheduled episode is on air, and for the station's staff at any time; between shows the player shows the chat read-only and says when the next show starts. `always`, the default, is 1.0.0's (ADR-0234; [The chat service](/docs/reference/chat/#hours)) |
| **The website's sandbox desk** | training scenarios beside the show: running everything alone, a busy queue, the dump, banned before, carts and a bed, playout taking over, a line going bad, and your own microphone |
| **Browser cameras** | a guest's camera from a Chromium browser (AV1) reaches the desk: the images carry the RTP plugin that unpacks it, the join pages send one picture size, the desk asks for a keyframe after a loss and gives cameras a 300 ms jitter buffer (`desk.video_jitter_buffer_ms`). Seen on a station on Cloudron through the desk's picture to an RTMP server; the picture can run up to 0.3 s behind the voices ([Video in the desk](/docs/reference/desk-video/)) |

## Decisions that changed release one

| Decision | What it changed |
|---|---|
| ADR-0206 | The software is called WeaveSignal; everything was renamed before the first public release |
| ADR-0207 | The presenter kit and the Windows server move to release two; the studio app's unsigned Windows installer stays |
| ADR-0208 | The two-person test (G0) stands on a one-person run: two guests on different networks, one a phone on LTE and the other relayed through TURN, for ten minutes. Two people's conversation by ear over two real networks is not proved |

## Known limits

| Limit | What to do |
|---|---|
| The desk's WebRTC sends without congestion control: its image lacks GStreamer's `rtpgccbwe` | Nothing yet. A guest on a poor link may hear more loss than a browser-to-browser call would give |
| LiveKit's built-in TURN refuses clients that reach it over IPv6 (an allocation error, 508; reported upstream as LiveKit issue 5131) | Give the TURN name an A record and no AAAA record (ADR-0217). On Cloudron the TURN name is the app's own domain, and Cloudron's install waits for an AAAA record on every name: add it for the install and delete it after, on a domain with manual DNS ([IPv6 and TURN](/docs/how-to/install-on-cloudron/#ipv6-and-turn)) |
| A busy shared server starves the listener stream: on a host at load 9–22 on 12 cores, a 3 s feed cushion filled to its 10 s cap | Run a station on a server of its own. On a dedicated 4-vCPU server the default 0.5 s cushion held ([Memory and CPU](/docs/reference/resources/)) |
| `weavesignal.sh` refuses `podman-compose`: it ignores the top-level `configs` that carry every service's settings | Use Docker Compose, or Podman with the docker-compose provider ([Install a station with weavesignal.sh](/docs/how-to/install-with-weavesignal-sh/)) |
| Firefox for Android cannot use the microphone until Android allows it | In Android's settings, allow Firefox's microphone permission; the call page says when the microphone was refused |
| The station's own player following content steering to an edge has not been tried in a browser: 1.0.1's CSP allows the edges' hosts (1.0.0's allowed only the station and `join.hls_url`) | Try it against your edges before relying on it, or point `join.hls_url` at one CDN address ([Put Bunny in front of the station](/docs/how-to/use-bunny/#send-viewers-to-bunny)) |
| The desk's show heading, rundown and banned list come from `[studio]`, which the Compose bundle cannot set: it writes the control plane's settings without that section, and has no place for the station's own file | On Compose the desk says *No show set*. On Cloudron (from 1.0.1) put them in `/app/data/station.toml`; with the Quadlets, edit the generated file ([Set the show, the rundown and the banned list](/docs/how-to/set-the-show/)) |
| The studio app's Linux webview has no WebRTC | Presenters and guests join the room from a browser; the studio app drives the desk |
| The Windows installer is unsigned, and has not been run on Windows | SmartScreen stops it the first time ([Install the studio app on Windows](/docs/how-to/install-studio-windows/)) |
| The Cloudron package 1.0.0 runs with Cloudron's soft limit of 1,024 open files, so its listener stream takes fewer than 1,024 listeners | Update to 1.0.1, which raises it to 65,536 ([Open files and large audiences](/docs/how-to/install-on-cloudron/#open-files-and-large-audiences)) |
| RTMP out has not been tried from a Cloudron install: the package's image carries the encoders (Ubuntu's OpenH264 and vo-aacenc, checked to load and encode in the image), but no Cloudron station has sent to an RTMP server yet | Send a short test stream with a test key before the first show, or send from a station on the Compose bundle ([What has been tried](/docs/how-to/send-to-video-platforms/#what-has-been-tried)) |
| RTMP out has been tested against servers on the test machine only (MediaMTX, OvenMediaEngine, PeerTube): not against Rumble, Odysee or another real platform, not over RTMPS, and not with NVENC's H.264 | Send a short test stream with a test key before the first show ([What has been tried](/docs/how-to/send-to-video-platforms/#what-has-been-tried)) |
| Icecast out has been tested against Icecast 2.5, not against an AzuraCast itself | Try it with **Always** before the first show ([What has been tried](/docs/how-to/send-to-azuracast/#what-has-been-tried)) |

## Measured

| What | Figure | Where |
|---|---|---|
| A guest's mix-minus after the voice | 69–77 ms (median) | M1, loopback |
| A person's own voice in their headphones | 105 to 110 dB down | M1, loopback |
| A caller's return, concealed by the receiver | 29.8 % before the fix of 2026-11-03 (32.3 % over the internet), 0.0 % after; WebRTC `getStats` on the receiver | the local stack, and a public station |
| A caller's return level | about −23 LUFS, against the stream's −16; the call page tells callers to turn the call's volume up (ADR-0205) | the Compose stack |
| The listener stream's feed cushion, 0.5 s set | 0.20–0.38 s over 24 hours, no container restarted | a dedicated 4-vCPU public server |
| A station unattended for 24 hours (M4) | no container restarted, no error in playout, the listener process, the desk or the chat; memory flat (playout 248 MB, the listener process 160 MB); the public stream playing at the end | a dedicated 4-vCPU public server, 2026-11-03 to 11-04 |
| The same, 3 s set | median 8.5 s, at most 10.0 s (its cap) | a heavily shared server |
| The playout chain unattended | 5 hours with no dropout; 2 hours at a 0.5 s cushion with no underrun | the Compose stack on a workstation |
| A server's port, measured outward | 3.4 Gbit/s to London, 4.4 Gbit/s to Amsterdam; iperf3, 8 streams for 10 s, not a sustained hour ([A large audience on one server](/docs/reference/resources/#a-large-audience-on-one-server)) | a 12-core server in London, 2026-11-06 |
| A Cloudron station just after start | 383 MB, against a 6 GB limit (package 1.0.0, video off) | the same server, 2026-11-06 |
| Two guests, one a phone on LTE, one through TURN over TLS | no echo in 150 trials; round trip median 501 ms with no VPN in the path (about 1 s through two VPNs and a loaded host) | G0's one-person run (ADR-0208) |

A caller's return was choppy until 2026-11-03: the desk stamped each block it sent with the clock at the moment it was encoded, so the timestamps stepped unevenly and receivers concealed a gap at nearly every packet.
Each block is now stamped from its place in the mix.
