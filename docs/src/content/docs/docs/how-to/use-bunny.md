---
title: "Put Bunny in front of the station"
description: "Serve live video and big downloads through Bunny's CDN, the CDN this documentation names (ADR-0017), and why a station streaming video to more than about 500 viewers needs a CDN."
type: howto
status: draft
last_verified: 2026-11-04
sidebar:
  order: 34
---

> **A station streaming video to more than about 500 viewers needs a CDN** (ADR-0017).
> At the default quality each viewer takes about 1 Mbit/s, so 500 viewers are about 500 Mbit/s of upload, more than most homes and many servers have to spare, before chat, guests and listeners.
> The studio app says so: when video is on, more than about 500 are connected and the station has no CDN or edges configured (`steering.edges`), `desk.audience` shows a warning. The count is of listeners on the station's stream, the nearest figure the control plane has; it does not yet count video viewers apart.

Bunny is the CDN this documentation names and tests for live HLS and for large archive downloads.
Nothing in WeaveSignal is Bunny-specific: the HLS output and the archive are plain files that any pull CDN can serve, and a [self-run edge pool](/docs/how-to/run-edges/) can stand beside it or instead of it.
Bunny Standard costs about 0.01 USD per GB in Europe and North America (report 18): about 4.4 to 8.8 USD per programme hour for 2,000 to 4,000 viewers, against 0.8 to 1.4 USD for a self-run pool (ADR-0051).

Audio listeners need no CDN at the expected audience: 2,000 listeners at 96 kbit/s are about 200 Mbit/s, which one server or a couple of [relays](/docs/how-to/run-edges/#the-relay-and-the-stations-password) carry.

## Before you start

- The station's HLS origin running (the control plane serves the desk's HLS at `/hls/`, with video on) and published through its reverse proxy, such as `https://station.example/hls/`.
- A Bunny account.

## Make a pull zone

1. In Bunny's dashboard, add a pull zone whose origin is the station's public address, `https://station.example`.
2. Note the zone's host name. Optionally, give it your own name, such as `cdn.station.example`, with Bunny's certificate.
3. Let the zone follow the origin's cache headers: the control plane marks playlists `no-cache` and segments `public, max-age=30` (the edge image, for its part, caches playlists for a second and segments for a minute).
4. Turn on request coalescing if the zone offers it, so a new segment reaches the station once per Bunny node, as `proxy_cache_lock` does on a self-run edge.
5. Turn on the zone's CORS headers for `m3u8`, `m4s` and `mp4` files. The player fetches from the zone across origins, and the control plane sends no `Access-Control-Allow-Origin` on `/hls/` itself.

Check it:

```sh
curl -si https://cdn.station.example/hls/live.m3u8
```

## Send viewers to Bunny

With Bunny alone, point the player at the zone in `control.toml` and restart the control plane:

```toml
[join]
hls_url = "https://cdn.station.example/hls"
```

The player then fetches every playlist and segment from Bunny, and its page allows that address.
The Compose bundle and the Cloudron package write `control.toml` themselves and have no setting for `hls_url` or steering yet: on the Compose bundle, add the lines to the `control` entry under `configs:` in `deploy/compose/compose.yaml` and start the bundle again; the Cloudron package cannot take them.
The studio app's warning above looks at `steering.edges` only, so it still shows with Bunny set this way.

## Steer viewers to Bunny with content steering

With content steering, add the zone as a pathway instead. As the default CDN, give it a weight like any edge:

```toml
[[steering.edges]]
pathway = "bunny"
host = "cdn.station.example"
```

With a self-run pool for event nights, keep Bunny after the pool instead, so it carries viewers only when the edges fail:

```toml
[[steering.edges]]
pathway = "bunny"
host = "cdn.station.example"
weight = 0
```

A pathway without `check_url` is never checked and always counted up; Bunny's own nodes fail over among themselves.
From 1.0.1 the station's own player page allows each edge's host in its Content-Security-Policy, so it may follow a steered pathway to an edge; in 1.0.0 it allowed only the station and `hls_url`. Not yet tried in a browser against a real edge.
Every setting is in [Content steering](/docs/reference/steering/).

## Archive downloads

A second pull zone over the episode files (`/media/episodes/`) can carry a new episode's first day of downloads.
The feeds do not yet take a CDN address for their enclosures, so today that zone serves only links you hand out yourself.

## Known limits

- Steering through Bunny has not been tested against Bunny itself: [planned](/docs/reference/release-one-status/).
- Pathway clones are in hls.js 1.7.3's content-steering code (read, not yet run in a browser); whether Safari's own player follows them has not been tested.
