---
title: "Run a self-run edge pool"
description: "Put edge machines in front of the station for a big night: each runs the HLS edge and an audio relay, and the control plane steers viewers between them."
type: howto
status: draft
last_verified: 2026-11-04
sidebar:
  order: 33
---

> **An option for event nights, for a station with someone who runs servers** (ADR-0051).
> [Bunny](/how-to/use-bunny/) stays the default CDN; a pool and Bunny can back each other up.
> Measured on one machine over loopback only: an edge on another network is [planned](/reference/release-one-status/).

An edge turns the station's load per viewer into load per edge.
In bench 02, 2,000 video viewers straight from the station took 1,972 Mbit/s from it; through two edges, about 2 Mbit/s: each edge fetches each playlist once a second and each segment once, whatever its audience.
The station's upload stops being the limit as soon as there is one edge.

Each edge machine runs two containers from the repository:

| Image | What it does |
|---|---|
| `weavesignal-edge` | nginx as a pull-through cache of the station's HLS: playlists cached 1 s, segments 60 s, simultaneous misses collapsed into one fetch (`proxy_cache_lock`); serves only the HLS path, its `/health` and the relay's mounts, and passes no cookies or credentials either way |
| `weavesignal-relay` | Icecast 2.5.0 relaying the station's listener mounts, with ICY metadata off (ADR-0052) |

Neither holds a secret of the station's.
The edge fetches what any viewer may fetch; the relay pulls each mount as any listener does.
The relay's own source and relay passwords are random at every start, and it takes no source but its relays, so nobody can push a stream to an edge.

## Before you start

- One machine per edge with a public address, Podman or Docker, and a 1 Gbit/s port or better. Size by the port, not the processor: about 1,000 video viewers per Gbit/s at the default quality, or about 9,000 audio listeners (*inference* from bench 02, where two edges served 4,000 viewers on under 5 % of a core each).
- A host name per edge under a subdomain kept for edges, such as `edge-1.edges.example`, and a wildcard certificate for that subdomain only: its key sits on machines you may throw away.
- The images. They are published as `forgejo.weavesignal.top/weavesignal/weavesignal-edge:1.0.0` and `forgejo.weavesignal.top/weavesignal/weavesignal-relay:1.0.0`; or build them from the repository with `just edge-image`, which tags them `localhost/weavesignal-edge:m1` and `localhost/weavesignal-relay:m1`.
- The station's HLS origin running (the control plane serves the desk's HLS at `/hls/`, with video on) and published through its reverse proxy, with its `/health` published too: each edge's `/health` asks it.

## Run an edge

On the edge machine, with the certificate in `/etc/weavesignal-edge/tls/` as `fullchain.pem` and `privkey.pem`:

```sh
podman run -d --name weavesignal-edge --network host \
  -e EDGE_ORIGIN=https://station.example \
  -v /etc/weavesignal-edge/tls:/etc/weavesignal-edge/tls:ro \
  -e EDGE_RELAY=127.0.0.1:8000 \
  forgejo.weavesignal.top/weavesignal/weavesignal-edge:1.0.0

podman run -d --name weavesignal-relay --network host \
  -e RELAY_ORIGIN_HOST=station.example \
  -e RELAY_MOUNTS=/programme.opus -e RELAY_BIND=127.0.0.1 \
  forgejo.weavesignal.top/weavesignal/weavesignal-relay:1.0.0
```

The edge listens on 80 and, with a certificate, 443; it pulls `/hls/` from the station over HTTPS, checking the station's certificate.
Every edge must serve on the same scheme and port as the station's HLS addresses: a steered player swaps the host name and keeps the port (RFC 8216bis 7.3; hls.js 1.7.3).
With `EDGE_RELAY` set, it also serves the relay's mounts under `/listen/`, so a player on an HTTPS page reaches them as `https://edge-1.edges.example/listen/programme.opus`; the relay itself then stays on loopback.
With a certificate, port 80 answers `/health` only and redirects everything else to HTTPS, so players that cannot use TLS cannot use the edge.
The wildcard certificate covers the edges' subdomain only (`*.edges.example`), never the station's own domain, because its key sits on these machines.
Every port is a setting: see [Ports](/reference/ports/#the-edge-images).

Check it from anywhere:

```sh
curl -si http://edge-1.edges.example/health          # 200: the station's own answer, through the edge
curl -si http://edge-1.edges.example/hls/live.m3u8   # 301 to https, once a certificate is mounted
curl -si https://edge-1.edges.example/hls/live.m3u8  # the playlist, with X-Cache
```

### The relay and the station's password

Icecast's relay cannot use TLS, so the relay image carries a small encrypted hop (ADR-0162): nginx, on loopback inside the image, fetches each mount from the station's HTTPS address (port 443, `RELAY_ORIGIN_PORT` to change it), checks the station's certificate against the name in `RELAY_ORIGIN_HOST`, and hands the stream to Icecast on the same machine.
The station needs no plain-HTTP listener port for relays, and neither the stream nor a listener login crosses the internet in clear.
Give `RELAY_ORIGIN_HOST` the station's public name, the one its certificate is for; the relay never pulls from `icecast.server` on the station's loopback.
A station whose streams are open needs nothing more.
A private station whose streams need a listener login gives the relay that login, and only that: `RELAY_ORIGIN_USER` and `RELAY_ORIGIN_PASSWORD`, or `RELAY_ORIGIN_PASSWORD_FILE` from a container secret.
It is a listener's credential, never the station's source password; the hop adds it to its encrypted request.

The relay's container runs two processes, the hop and Icecast, and exits if either does, so Podman or systemd restarts it.
The station's name is looked up when the relay starts, so the station's name must resolve then (the relay exits otherwise); the relay looks it up again every 30 s and reloads the hop if the addresses changed.
`RELAY_ORIGIN_SCHEME=http` leaves the hop out and pulls from the station's plain-HTTP listener port: for a bench, or a link you already trust.

**Upgrading a relay set up before the hop:** the relay used to pull over plain HTTP from port 80, and now pulls over HTTPS from port 443 unless told otherwise.
A relay started with `RELAY_ORIGIN_PORT=80` and no `RELAY_ORIGIN_SCHEME` now speaks TLS to a plain port and carries nothing (it says so in its log).
Drop `RELAY_ORIGIN_PORT` to pull from the station's HTTPS port, or set `RELAY_ORIGIN_SCHEME=http` to keep the plain pull.
Icecast 2.5.0 appends a stray zero byte to the login it sends, which a strict server may refuse, so on a private station use the hop.

A relay that finds no mount tries again every `RELAY_RETRY_S` (15 s, the least Icecast 2.5 allows), so an edge started before the station's programme picks it up within 15 s.

## Steer viewers to the edges

Add each edge to the station's `control.toml` and restart the control plane:

```toml
[[steering.edges]]
pathway = "edge-1"
host = "edge-1.edges.example"
check_url = "http://edge-1.edges.example/health"

[[steering.edges]]
pathway = "edge-2"
host = "edge-2.edges.example"
check_url = "http://edge-2.edges.example/health"
```

The Compose bundle and the Cloudron package write `control.toml` themselves and have no setting for steering yet: on the Compose bundle, add the lines to the `control` entry under `configs:` in `deploy/compose/compose.yaml` and start the bundle again; the Cloudron package cannot take them.

Viewers are shared between the healthy edges by weight, and each keeps their edge between reloads.
An edge that fails two checks in a row leaves the order; one passing check brings it back.
To keep Bunny or the station itself as a last resort, add it with `weight = 0`.
Every setting is in [Content steering](/reference/steering/).

See what the control plane thinks of the pool. This route takes the station key only, not a staff session:

```sh
curl -s -H "Authorization: Bearer $WEAVESIGNAL_CONTROL_ADMIN_TOKEN" https://station.example/api/v1/steering/edges
```

## Audio listeners

Players and directories that take a plain stream URL do not follow content steering.
Give them an edge's `/listen/` address (over HTTPS once the edge has a certificate; its plain-HTTP port redirects), or one name that resolves to several edges; with DNS alone, a listener stays on a dead edge until their resolver's time-to-live runs out (recce 25).

## Try it on one machine

The Compose bundle has two local edges and relays beside it, on loopback: the edges on the control plane's port (it serves `/hls/`) at 127.0.0.2 and 127.0.0.3, the relays on 7323 and 7324. The station needs video on (`VIDEO=true`) for there to be HLS.
`weavesignal.sh` has no switch for them, so start the overlay by hand, beside an installed bundle:

```sh
cd deploy/compose && podman compose -f compose.yaml -f edges.yaml up -d
```

Use Docker Compose (`docker compose`), or Podman with docker-compose as its compose provider: `podman-compose` ignores the settings files the bundle gives each service.
The overlay runs the published images unless `EDGE_IMAGE` and `RELAY_IMAGE` name others, such as the ones `just edge-image` builds.
On a station published with `--public`, add `-f public.yaml` before `-f edges.yaml`.
Then add the two edges to `[[steering.edges]]` as above, with `host = "127.0.0.2"` and `check_url = "http://127.0.0.2:7310/health"` for the first, and `127.0.0.3` for the second (7310 is the default `CONTROL_PORT`).

They add no capacity on the station's own machine; they are there to try the pool, the relays and steering before renting machines.
`just test-edges` runs the same shape with simulated viewers, kills an edge and checks that they are steered around it.

## Known limits

- **Measured on loopback only.** Real links, join time over a real network, and failover in a real hls.js player are [planned](/reference/release-one-status/).
- **The station's own player does not follow steering to an edge yet.** The player page's Content-Security-Policy lets it fetch HLS only from the station and from `[join] hls_url`, so a browser refuses the edges' host names. Players on other pages, and apps, are not held back.
- **Health checks are plain HTTP.** A pass proves that the edge's nginx answers, that it reaches the station and that the station's `/health` answers; it does not prove that the edge has the newest segments.
- **Icecast 2.5.0 ignores `relay-shoutcast-metadata` inside an `<upstream type="normal">` block** and still asks for ICY metadata, after which it relays nothing; the image sets it in the default upstream block. If you write your own relay configuration, do the same.
- **Now playing does not travel in the stream.** The listener process strips metadata before it encodes (a new title would end the stream for every listener); players get it from the control plane (`/api/v1/now-playing`). A relay does pass in-band titles when its origin sends them.
