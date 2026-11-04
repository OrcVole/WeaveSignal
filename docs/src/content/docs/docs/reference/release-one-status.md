---
title: "What release one does, and what it leaves out"
description: "WeaveSignal 1.0.0: what each part of release one brings, what is not in it, the known limits, and what was measured. Every 'planned' mark on this site points here."
type: reference
status: draft
last_verified: 2026-11-04
sidebar:
  order: 1
---

WeaveSignal's release one ships as **1.0.0**. It was built in milestones (M1 to M11), each a working slice, and this site was written as each one landed (ADR-0072).
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
| **M10 · Packaging**, Cloudron | the Cloudron package 1.0.0 (`top.weavesignal.cloudronapp`), amd64, installed from its published image ([Install on Cloudron](/docs/how-to/install-on-cloudron/)); it is not in Cloudron's app store yet (ADR-0077) |
| **The public API** | scoped tokens, open read routes for a station's own website, webhooks with signed deliveries, and an OpenAPI description generated from the code ([The API](/docs/reference/api/), [Integrations](/docs/reference/integrations/)) |

## Not in release one

| Part | What is left out |
|---|---|
| **M2**, the rest | a 30-minute show driven from the keyboard alone; the webview tests on Debian and the GNOME runtime; the studio app joining the room itself (ADR-0013); the Chat tab (`⌃4`) and the desk's chat glance (the chat is on the Screener tab); a fader on the desk's tiles, and a camera picture in them; the desk's rundown and show heading from the schedule, and the banned list from the moderation log, which are still settings; a measured listener delay, which is still a setting; a screen for webhooks (they are made through the API) |
| **M3**, the rest | a join from a GrapheneOS phone over a real network; WHIP for remote DJs; the DJ overlap handover (`⇧⏎`, which shows a note) |
| **M4**, the rest | 24 hours unattended, proved (13 hours so far: see [Measured](#measured)); what playout does differently with a pre-recorded episode; studios on shows; in the studio app, restart playout (`⌃R`), restart the encoder, the backup stream and end the show, playout as slot 6, playout's queue and hard posts, listener counts by mount, and the listener server's health and the logger on the desk; the screener's listener count |
| **M5**, the rest | Safari's Opus in fMP4 measured on a device; the GPU in the Compose bundle; a still per episode and the slide schedule (a still per show is built); video for callers |
| **M7**, the rest | an edge on another network, with join time and failover in a real hls.js player; steering through Bunny tried against Bunny itself; the hours-long real-network soak of `icecast.server` against Icecast 2.5.0, which decides the listener server (ADR-0011) |
| **M8**, the rest | a step that publishes an episode's words before it airs; opening an episode's recording and publishing it as a podcast episode from the studio app (files are attached through the API); an episode marked as aired by itself (only **On air now** does it) |
| **M9 · The presenter kit and the Windows server** | moved to release two (ADR-0207): presets, the patchbay, VBAN, and the server on Windows. The studio app's Windows installer stays in release one |
| **M10**, the rest | an update and a rollback run on a real install; the Windows installer run on Windows |
| **M11**, the rest | screenshots of every surface from the running app; the listing in the Cloudron community app store (ADR-0077) |

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
| LiveKit's built-in TURN refuses clients that reach it over IPv6 (an allocation error, 508; reported upstream as LiveKit issue 5131) | Give the TURN name an A record and no AAAA record. A Cloudron app domain gets an AAAA record whenever the server has IPv6, so a phone that can only reach TURN over IPv6 cannot join a Cloudron station through it |
| A busy shared server starves the listener stream: on a host at load 9–22 on 12 cores, a 3 s feed cushion filled to its 10 s cap | Run a station on a server of its own. On a dedicated 4-vCPU server the default 0.5 s cushion held ([Memory and CPU](/docs/reference/resources/)) |
| `weavesignal.sh` refuses `podman-compose`: it ignores the top-level `configs` that carry every service's settings | Use Docker Compose, or Podman with the docker-compose provider ([Install a station with weavesignal.sh](/docs/how-to/install-with-weavesignal-sh/)) |
| Firefox for Android cannot use the microphone until Android allows it | In Android's settings, allow Firefox's microphone permission; the call page says when the microphone was refused |
| An API token with `chat:moderate` is refused at `/api/v1/chat/modes` through the station's address: the proxy sends it to the chat, which takes only a staff sign-in or the station key there | Use a staff sign-in there, or a program that reaches the control plane on its loopback port ([Integrations](/docs/reference/integrations/)) |
| The station's own player cannot follow content steering to an edge or a CDN: its CSP allows only the station and `join.hls_url` | Point `join.hls_url` at one CDN address ([Put Bunny in front of the station](/docs/how-to/use-bunny/#send-viewers-to-bunny)) |
| The desk's show heading, rundown and banned list come from `[studio]` in `control.toml`, which the Compose bundle and the Cloudron package write without that section | On Compose and Cloudron the desk says *No show set*; with the Quadlets, edit the generated file ([Set the show, the rundown and the banned list](/docs/how-to/set-the-show/)) |
| The studio app's Linux webview has no WebRTC | Presenters and guests join the room from a browser; the studio app drives the desk |
| The Windows installer is unsigned, and has not been run on Windows | SmartScreen stops it the first time ([Install the studio app on Windows](/docs/how-to/install-studio-windows/)) |

## Measured

| What | Figure | Where |
|---|---|---|
| A guest's mix-minus after the voice | 69–77 ms (median) | M1, loopback |
| A person's own voice in their headphones | 105 to 110 dB down | M1, loopback |
| A caller's return, concealed by the receiver | 29.8 % before the fix of 2026-11-03 (32.3 % over the internet), 0.0 % after; WebRTC `getStats` on the receiver | the local stack, and a public station |
| A caller's return level | about −23 LUFS, against the stream's −16; the call page tells callers to turn the call's volume up (ADR-0205) | the Compose stack |
| The listener stream's feed cushion, 0.5 s set | 0.20–0.36 s over 13 hours unattended, no container restarted | a dedicated 4-vCPU public server |
| The same, 3 s set | median 8.5 s, at most 10.0 s (its cap) | a heavily shared server |
| The playout chain unattended | 5 hours with no dropout; 2 hours at a 0.5 s cushion with no underrun | the Compose stack on a workstation |
| Two guests, one a phone on LTE, one through TURN over TLS | no echo in 150 trials; round trip median 501 ms with no VPN in the path (about 1 s through two VPNs and a loaded host) | G0's one-person run (ADR-0208) |

A caller's return was choppy until 2026-11-03: the desk stamped each block it sent with the clock at the moment it was encoded, so the timestamps stepped unevenly and receivers concealed a gap at nearly every packet.
Each block is now stamped from its place in the mix.
