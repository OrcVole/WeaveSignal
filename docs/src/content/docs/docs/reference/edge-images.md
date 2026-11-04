---
title: "Edge images"
description: "The weavesignal-edge and weavesignal-relay images: what each runs, every environment variable, and what neither holds."
type: reference
status: draft
last_verified: 2026-11-04
sidebar:
  order: 13
---

Two images from `deploy/images/edge/`, on Alpine 3.24 (ADR-0051).

| Image | Published | Built locally (`just edge-image`) |
|---|---|---|
| `weavesignal-edge` | `forgejo.weavesignal.top/weavesignal/weavesignal-edge:1.0.0` | `localhost/weavesignal-edge:m1` (`WEAVESIGNAL_EDGE_IMAGE` to change) |
| `weavesignal-relay` | `forgejo.weavesignal.top/weavesignal/weavesignal-relay:1.0.0` | `localhost/weavesignal-relay:m1` (`WEAVESIGNAL_RELAY_IMAGE`) |

Each writes its server's configuration from the environment at start and refuses to start on a value it cannot use.
How to run them is in [Run a self-run edge pool](/docs/how-to/run-edges/).

## `weavesignal-edge`

nginx 1.30, as bench 02's pull-through cache: playlists cached 1 s, segments 60 s, simultaneous misses collapsed into one fetch from the station (`proxy_cache_lock`), stale copies served while one refreshes or when the station does not answer.
From the station it serves the HLS path and nothing else; besides that it answers only `/health` and, with a relay, `/listen/`. Any other path answers 404.
It sends no cookies or credentials to the station and passes none back, and keys the cache on the path alone, so a query string cannot bypass it.
It ignores the station's cache headers and sends viewers its own `Cache-Control` only: `max-age=1` on playlists, `max-age=60` on segments.

| Variable | Default | What |
|---|---|---|
| `EDGE_ORIGIN` | required | the station as the edge reaches it: `https://station.example` (the station's certificate is checked), or `http://host:port` on a test bench; no path |
| `EDGE_HTTP_PORT` | `80` | plain HTTP: `/health` for the steering checks; with a certificate mounted it answers nothing else and redirects every other path to HTTPS, without one it serves viewers too |
| `EDGE_HTTPS_PORT` | `443` | HTTPS, only when a certificate is mounted |
| `EDGE_BIND` | every interface | the address both listen on |
| `EDGE_TLS_DIR` | `/etc/weavesignal-edge/tls` | holds `fullchain.pem` and `privkey.pem`: the edge pool's wildcard certificate |
| `EDGE_HLS_PATH` | `/hls/` | the only path served from the station |
| `EDGE_HEALTH_PATH` | `/health` | the station's own health address, which the edge's `/health` passes through to |
| `EDGE_PLAYLIST_TTL` | `1s` | how long a playlist is cached: half a 2 s segment |
| `EDGE_SEGMENT_TTL` | `60s` | how long a segment is cached; a live window is about 12 s, so longer gains nothing live |
| `EDGE_CACHE_MAX` | `2g` | the cache's size on disk |
| `EDGE_RELAY` | none | `host:port` of a relay beside the edge; its mounts are then served under `/listen/`, unbuffered and never cached |
| `EDGE_WORKERS` | `auto` | nginx's worker processes |

`/health` is the station's health answer fetched through the edge: the edge asks `EDGE_HEALTH_PATH` on `EDGE_ORIGIN` (2 s limits, never cached) and returns what it gets, so it answers 2xx only while nginx, the edge's route to the station and the station are all well, and 502 or 504 when the route is down.
The station's reverse proxy must therefore publish its `/health` to the edges, with no login in front of it: a redirect to a login page counts as a failure.
An edge reads down whenever the station does (ADR-0160).
With a certificate mounted, the plain-HTTP port answers `/health` and redirects every other path to HTTPS (a 301 to the same path on `EDGE_HTTPS_PORT`, ADR-0161): players that cannot use TLS cannot use the edge.
Every answer from the HLS path carries `X-Cache` (`HIT`, `MISS`, `UPDATING`, `STALE`) and `Access-Control-Allow-Origin: *`.

## `weavesignal-relay`

Icecast 2.5.0 relaying the station's listener mounts, with ICY metadata off (ADR-0052), behind an encrypted hop (ADR-0162).
Icecast's relay cannot use TLS, so nginx 1.30 in the same image listens on `127.0.0.1:RELAY_HOP_PORT`, fetches each configured mount from the station's HTTPS address and hands the stream to Icecast, which relays from the hop.
The hop checks the station's certificate against the name in `RELAY_ORIGIN_HOST` (TLS 1.2 or 1.3), sends that name as `Host`, adds the listener login to its own request, buffers and caches nothing, and answers 404 to any path that is not a configured mount.
Its configuration, with the login, is written at start with mode 600.
Both processes run as Icecast's own user; the container stops if either ends, and a stop signal reaches both.
The station's name is resolved when the relay starts (it exits if it cannot be), and again every 30 s, when a changed address makes the hop reload.
Without the hop (`RELAY_ORIGIN_SCHEME=http`) Icecast pulls from the station's plain-HTTP listener port itself.

| Variable | Default | What |
|---|---|---|
| `RELAY_ORIGIN_HOST` | required | the station's public name, the one its certificate is for (letters, digits, `.` and `-`) |
| `RELAY_ORIGIN_SCHEME` | `https` | `https`: the encrypted hop; `http`: no hop, Icecast pulls over plain HTTP from the station's `proxy_http` port |
| `RELAY_ORIGIN_PORT` | `443` (`80` with `http`) | the station's port |
| `RELAY_ORIGIN_CA_FILE` | the image's roots | `https` only: a file of certificate authorities to trust instead (a station with its own authority, a test) |
| `RELAY_HOP_PORT` | `RELAY_PORT` + 1 | `https` only: the loopback port the hop listens on, for Icecast alone |
| `RELAY_MOUNTS` | `/programme.opus` | the mounts to relay, space-separated, each kept under the same name (letters, digits and `. _ ~ - /`) |
| `RELAY_PORT` | `8000` | where the relay's listeners connect |
| `RELAY_BIND` | every interface | the address it listens on; loopback when an edge serves it under `/listen/` |
| `RELAY_RETRY_S` | `15` | how often a missing mount is tried again; 15 is the least Icecast 2.5 accepts |
| `RELAY_MAX_LISTENERS` | `5000` | listeners the relay takes |
| `RELAY_ORIGIN_USER`, `RELAY_ORIGIN_PASSWORD` (or `RELAY_ORIGIN_PASSWORD_FILE`) | none | only for a station whose streams need a listener login: that login (the hop sends it; Icecast sends it itself only with `http`) |
| `RELAY_ADMIN_PASSWORD` (or `RELAY_ADMIN_PASSWORD_FILE`) | random | Icecast's admin password; unset, a random one, so the admin pages are closed |

## What neither holds

Neither image holds a secret of the station's.
The edge fetches what any viewer may; the relay pulls each mount as any listener does, and its own source and relay passwords are random at every start.
A relay for a private station holds one credential of the station's, a listener login, and sends it only over the encrypted hop.
It accepts no source but its relays, so nobody can push a stream to an edge.
The one credential an edge may carry is the edge pool's wildcard certificate, which is why it covers the edges' subdomain only, never the station's own domain: a stolen edge key then cannot speak for the station.
The edge does not check this; the certificate you mount decides.

## Measured

`just test-edges` runs two edges (on the origin's port at 127.0.0.2 and 127.0.0.3, as steering requires) and two relays on loopback against a stand-in HLS origin (2 s segments, about 0.9 Mbit/s) and the listener process, with 400 simulated viewers joining over 50 s (four runs, 2026-10-26, one machine):

| What | Result |
|---|---|
| Edges' egress, averaged over the 80 s run | 154 to 156 Mbit/s |
| The origin's egress | 1.7 to 1.9 Mbit/s (1.1 to 1.2 %): each edge's playlist once a second and each segment once, plus each viewer's one multivariant playlist |
| Late segments | 0 of about 6,800 |
| A killed edge out of the steering order | 1.3 to 1.7 s after the kill (checks every second, two failures) |
| A restarted edge back in the order | 0.3 to 0.7 s after its start |
| A relay listener, directly and through `/listen/` | 98 kbit/s for the whole run, never dropped; the tone decodes at about −6 dB |
| In-band titles through a relay of a plain Icecast | 9 to 10 in 20 s, one every 3 s as sent |

Loopback has no loss, no round trip and no port ceiling: an edge on another network is [planned](/docs/reference/release-one-status/).
