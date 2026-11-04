---
title: "Content steering"
description: "The control plane's steering manifest: how it orders the edges for each viewer, its settings under [steering], and its two routes."
type: reference
status: draft
last_verified: 2026-11-04
sidebar:
  order: 12
---

HLS content steering lets a station tell each viewer's player which copy of its video to fetch: which edge, or a CDN, and what to try when one fails (ADR-0051).
The control plane serves the steering manifest; the player asks for it again every few minutes, and moves on its own when an edge stops answering.
Steering is off until edges are configured.
The Compose bundle and the Cloudron package write `control.toml` from their own settings, which have no steering keys yet; on the Compose bundle, add `[steering]` to the `control` entry under `configs:` in `deploy/compose/compose.yaml`.

The station's multivariant playlists, `/hls/live.m3u8` and `/hls/audio.m3u8`, carry the line players need, added as they are served while at least one edge is set (ADR-0159):

```
#EXT-X-CONTENT-STEERING:SERVER-URI="/api/v1/steering",PATHWAY-ID="."
```

`PATHWAY-ID` is `clone_from` (left out when it is empty).
`SERVER-URI` is a path on the host the playlist came from, so the station's reverse proxy must publish `/api/v1/steering` beside `/hls/`, and a CDN that serves `/hls/` for the station (Bunny) must pass it through too.
With no edges the playlists have no steering line and players do not ask.
Media playlists are not changed.
Whether Safari's own player applies pathway clones has not been tested.
The station's own player page cannot follow a clone to another host yet: its Content-Security-Policy allows HLS from the station and from `[join] hls_url` only, so a browser refuses an edge's host name. Steering serves players on other pages, and apps.

## How the order is chosen

- **Each viewer gets an order of their own.** The healthy edges are ranked for that viewer by a weighted rendezvous hash: an edge of weight 2 is first for twice as many viewers as an edge of weight 1.
- **A viewer keeps their edge.** The first manifest gives the player a session in its `RELOAD-URI`, and the same session always ranks the edges the same way. When an edge goes down, only its own viewers move; when it comes back, the viewers it had return at their next reload, or, for a player that penalised it (hls.js does, for two minutes by the specification's suggestion), once the penalty lapses.
- **An edge that fails its health check `down_after` times in a row leaves the order.** One passing check brings it back.
- **Weight 0 is a last resort.** Those pathways (Bunny, ADR-0017, or the origin itself) come after the pool, in the order written, for players whose edges have all failed.
- **The manifest is never empty.** If no edge and no last resort is healthy, every edge is listed, and players fall back on their own failure handling.
- The `_HLS_pathway` and `_HLS_throughput` parameters players add are accepted and not used.

An edge with a `host` is sent as a **pathway clone** of `clone_from`: the player copies the playlist's own variants and replaces their host name with the edge's.
The scheme and the port stay the playlist's (RFC 8216bis 7.3; hls.js 1.7.3 sets only the URL's host name), so **every edge serves on the same scheme and port as the station's HLS addresses**, normally HTTPS on 443, and `host` is a host name without a port.
So the edge pool lives in these settings alone, and the playlist names one pathway.
An edge without a `host` must be declared in the playlist under its pathway ID, as a second set of variants.
Pathway clones are in RFC 8216bis (section 7.3) and in hls.js 1.7.3's content-steering controller (read; running it in a browser is [planned](/docs/reference/release-one-status/)); whether Safari's own player applies them has not been tested.

When a player's edge stops answering, the player marks it penalised and moves to the next in its order at once.
A player that does not waits for its next reload, up to `ttl_s` after the control plane drops the edge.
Measured on loopback (`just test-edges`, 1 s checks, `ttl_s = 10`): the control plane dropped a killed edge 1.3 to 1.7 s after the kill; players that penalise had a segment from the other edge 1.5 to 1.8 s later (median), 3.5 s at worst, inside the three segments they hold; players that wait for the manifest took 6 to 9 s (median), 12 s at worst.
With the default `ttl_s = 300`, a player that does not penalise can wait five minutes: the specification's players penalise.

## `[steering]`

| Setting | Default | What |
|---|---|---|
| `ttl_s` | `300` | how long a player keeps a manifest before asking again, in seconds (1 to 86,400); the specification recommends 300 |
| `check_every_s` | `5` | how often each edge's health check runs (1 to 300) |
| `check_timeout_ms` | `2000` | how long one check may take (100 ms up to `check_every_s`) |
| `down_after` | `2` | failed checks in a row before an edge leaves the order (1 to 20) |
| `clone_from` | `.` | the pathway the multivariant playlist declares, which edges with a `host` are cloned from; `.` when its variants carry no `PATHWAY-ID`; empty, no clones are sent |
| `edges` | none | the pool, in `[[steering.edges]]` tables; with none, steering is off |

Each `[[steering.edges]]`:

| Key | Default | What |
|---|---|---|
| `pathway` | — | the pathway ID: letters, digits, `.`, `-` and `_`; each used once |
| `host` | empty | the host name that replaces the playlist's in this edge's clone, such as `edge-1.edges.example`, or an address; never a port, which players keep from the playlist; empty, the playlist declares the pathway |
| `weight` | `1` | the edge's share of new viewers; `0` puts it after the pool |
| `check_url` | empty | a plain-HTTP address that answers 2xx while the edge is healthy, such as the edge image's `http://edge-1.edges.example/health`; empty, never checked and always counted up |

```toml
[steering]
ttl_s = 300
check_every_s = 5
down_after = 2
clone_from = "."

[[steering.edges]]
pathway = "edge-1"
host = "edge-1.edges.example"
check_url = "http://edge-1.edges.example/health"

[[steering.edges]]
pathway = "edge-2"
host = "edge-2.edges.example"
weight = 2                     # a bigger machine: twice the viewers
check_url = "http://edge-2.edges.example/health"

[[steering.edges]]
pathway = "bunny"              # after the pool, never checked
host = "kettle-cdn.example"
weight = 0
```

Health checks speak plain HTTP only: the control plane has no TLS client.
The edge image answers `/health` on its plain-HTTP port for this, by asking the station's own `/health` through the edge (ADR-0160).
A passing check proves the edge's nginx runs, that it reaches the station and that the station answers, so an edge cut off from the station leaves the order within `down_after` checks, and every edge reads down while the station does.
The station's proxy must publish `/health` to the edges: a login redirect counts as a failure.

## Routes

| Route | Who | What |
|---|---|---|
| `GET /api/v1/steering` | public | the steering manifest, `application/json`, `Cache-Control: no-store`, `Access-Control-Allow-Origin: *`; 404 while steering is off |
| `GET /api/v1/steering/edges` | the station key only (not a staff session) | `enabled`, `ttl_s`, and every edge with its `pathway`, `host`, `weight`, `checked` (whether it has a `check_url`) and health: `up`, `failures` in a row, `last_check`, `last_error`, `last_ms`, `since` (when `up` last changed), and `first`, the manifests since start that put it first |

A manifest:

```json
{
  "VERSION": 1,
  "TTL": 300,
  "RELOAD-URI": "steering?session=4f0c2a9e6d7b4e1f9a3c5b8d2e6f1a07",
  "PATHWAY-PRIORITY": ["edge-2", "edge-1", "bunny"],
  "PATHWAY-CLONES": [
    { "BASE-ID": ".", "ID": "edge-2", "URI-REPLACEMENT": { "HOST": "edge-2.edges.example" } },
    { "BASE-ID": ".", "ID": "edge-1", "URI-REPLACEMENT": { "HOST": "edge-1.edges.example" } },
    { "BASE-ID": ".", "ID": "bunny", "URI-REPLACEMENT": { "HOST": "kettle-cdn.example" } }
  ]
}
```

The reverse proxy publishes `/api/v1/steering`; `/api/v1/steering/edges` answers only the station key (`WEAVESIGNAL_CONTROL_ADMIN_TOKEN`), and 503 when none is set.

## See also

- [Run a self-run edge pool](/docs/how-to/run-edges/) and [Put Bunny in front of the station](/docs/how-to/use-bunny/).
- [Ports](/docs/reference/ports/), for the edge images' own ports.
