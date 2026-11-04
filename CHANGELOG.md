<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
# Changelog

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
