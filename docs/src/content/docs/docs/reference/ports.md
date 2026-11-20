---
title: "Ports"
description: "Every port a station listens on: its setting, default, suggested alternatives, who can reach it, and the Cloudron package's defaults."
type: reference
status: draft
last_verified: 2026-11-20
sidebar:
  order: 6
---

Every listening port is a setting (ADR-0075).
Each has a default chosen clear of ports other software commonly claims, and suggested alternatives.
**Any free port may be used.**
`weavesignal-control check-ports` checks them all: see [Check a station's ports](/docs/how-to/check-ports/).

## The control plane's `[ports]`

| Setting | Protocol | Default | Suggested alternatives | Reached from | What listens |
|---|---|---|---|---|---|
| `proxy_https` | TCP | 443 | 8443 | anywhere | the reverse proxy, the one public web listener |
| `proxy_http` | TCP | 80 | 8080 | anywhere | the proxy's redirects and plain-HTTP listener streams |
| `control` | TCP | 7310 | 17310, 27310 | loopback | the control plane's HTTP API, behind the proxy |
| `chat` | TCP | 7311 | 17311, 27311 | loopback | the chat service, behind the proxy |
| `listeners` | TCP | 7312 | 17312, 27312 | loopback | Liquidsoap's `icecast.server`: listener streams, behind the proxy |
| `srt_ingest` | UDP | 7313 | 17313, 27313 | loopback | the programme over SRT: the desk listens, Liquidsoap connects (ADR-0086) |
| `playout_feed` | TCP | 7315 | 17315, 27315 | loopback | playout's programme to the listener process, which holds the listener mount |
| `desk_feed` | TCP | 7317 | 17317, 27317 | loopback | playout's programme back to the desk, the video stream's sound while nobody is live (ADR-0172): the desk listens, playout connects |
| `desk_sources` | TCP | 7318 | 17318, 27318 | loopback | hosts' own studios from the ingest to the desk (ADR-0280): the desk listens, the ingest connects |
| `ingest_whip` | TCP | 7319 | 17319, 27319 | loopback | WHIP's requests from the control plane to the ingest (17d): the ingest listens while WHIP is on, the control plane connects |
| `source_bridge` | TCP | 7320 | 17320, 27320 | loopback | the bridge from the studio app (Phase 17g): the proxy passes `/api/v1/sources/bridge` on the public address to the ingest here ([Send through the bridge](/docs/how-to/send-through-the-bridge/)) |
| `livekit_http` | TCP | 7880 | 17880, 27880 | loopback | LiveKit's signalling and API, behind the proxy |
| `livekit_media` | UDP | 7882 | 52000, 443 | anywhere | LiveKit's one WebRTC media port (ADR-0023) |
| `livekit_tcp` | TCP | 7881 | 17881, 27881 | anywhere | LiveKit's ICE over TCP, when UDP is blocked |
| `turn_udp` | UDP | 3478 | 3479, 13478 | anywhere | LiveKit's TURN over UDP |
| `turn_tls` | TCP | 5349 | 443, 15349 | anywhere | LiveKit's TURN over TLS, on by default (ADR-0080); 443 where nothing else holds it, so guests on networks that pass only web ports can join. It refuses clients that reach it over IPv6 (508): give its name an A record only ([Known limits](/docs/reference/release-one-status/#known-limits)) |
| `source_icecast` | TCP | 8005 | 18005, 28005 | anywhere | the ingest's Icecast source port for hosts' own studios (ADR-0280, ADR-0284); listens only while a show key exists |
| `source_srt` | UDP | 9710 | 19710, 29710 | anywhere | the ingest's SRT port for hosts' own studios; listens only while a show key exists |
| `source_whip` | UDP | 9712 | 19712, 29712 | anywhere | the ingest's WHIP media port for hosts sending from OBS (17d): ICE-lite, sound only, the offer over HTTPS on the public domain; listens only while a show key exists and the station's public IPv4 address is known ([Send from OBS over WHIP](/docs/how-to/send-from-obs-over-whip/)) |

All nineteen are checked by `weavesignal-control check-ports`, whether or not the station turns them on: the Compose bundle, for one, leaves `livekit_tcp` and `turn_udp` off, and on loopback has no proxy.
`source_icecast`, `source_srt`, `source_whip` and `source_bridge` alone may be 0, which turns that way in off, and a port of 0 is not checked.
When the settings file names one of the first three, it is the installation's: the station's rules for sources (`GET /api/v1/settings/sources`) show it and cannot change it, because the installation is what publishes it. Left out, the rules' own port applies, 8005, 9710 and 9712 unless an admin changes them.
These three reach the ingest directly, not through the proxy, so the ingest counts wrong keys by the address it sees; the bridge comes through the proxy, on loopback.
WeaveSignal's own internal services default to 7310–7320, a block with no entry in the IANA service registry; the hosts' public ports take 8005, the port DJs know for Icecast sources, 9710 and 9712.
`hls_origin`, a port once kept for an HLS origin of its own that was never built, is retired: an old settings file that names it still starts, with a warning.
LiveKit's ports keep LiveKit's own defaults.
VBAN (Phase 17g) is not among these: it is off unless the installation turns it on, never on Cloudron, and its port (6980) and address are the presenter kit's settings ([Connect a studio PC over VBAN](/docs/how-to/connect-a-studio-pc-over-vban/)).
Liquidsoap's control interface and the desk's are Unix sockets, not ports, and PostgreSQL listens on a Unix socket only (ADR-0092).

## The phone line

The telephone line (from 1.2; [Take phone calls](/docs/how-to/take-phone-calls/)) is off unless the installation turns it on, so nothing listens on these ports until then.
They are not under `[ports]`: the first three are the control plane's `[phone]` settings, and Valkey's is the Compose overlay's.
Their defaults are chosen clear of every port above (the control plane's tests check it).

| Setting | Protocol | Default | Suggested alternatives | Reached from | What listens |
|---|---|---|---|---|---|
| `phone.sip_port` (`PHONE_SIP_PORT`) | UDP and TCP | 5060 | 15060, 25060 | the provider's addresses only | LiveKit's SIP service: the provider's calls |
| `phone.sip_tls_port` | TCP | 0 (off) | 5061 | the provider's addresses only | SIP over TLS, when a provider offers it |
| `phone.rtp_port` (`PHONE_RTP_PORT`, to `PHONE_RTP_LAST`) | UDP | 7340–7355 | from 17340, from 27340 | the provider's addresses only | the calls' sound: two ports for each line (`phone.lines`, 8 by default), starting on an even port |
| `PHONE_REDIS_PORT` | TCP | 7321 | 17321, 27321 | loopback | Valkey, which LiveKit and its SIP service share while the line is on, with a password and nothing on disk |

The control plane refuses a phone port below 1024, inside Linux's ephemeral range (32768–60999), or on 3478, 5349 or 50000–51000, which Cloudron's own TURN holds.
`weavesignal-control check-ports` checks the SIP port (UDP and TCP), the TLS port when set, and every RTP port while `phone.enabled` is on.
SIP's usual 5060 is scanned all day by strangers: a port of your own sees fewer of them, if your provider can send to it.

## The edge images

The edge images run on edge machines, not on the station, so their ports are not in the control plane's `[ports]` and `check-ports` does not look at them; each is an environment variable of its image.
Their defaults suit a machine of its own.
A steered player keeps the port of the station's HLS addresses and replaces only the host name, so in a pool every edge serves on the station's port (normally 443); the Compose bundle's local edges (`edges.yaml`) therefore take the control plane's port, which serves the HLS at `/hls/`, on addresses of their own, 127.0.0.2 and 127.0.0.3.

| Variable | Image | Protocol | Default | In `edges.yaml` | Reached from | What listens |
|---|---|---|---|---|---|---|
| `EDGE_HTTP_PORT` | `weavesignal-edge` | TCP | 80 | `control`'s, 7310, on 127.0.0.2 and 127.0.0.3 | anywhere | the HLS edge over plain HTTP: `/health` for the steering checks, and, with no certificate, `/hls/` and `/listen/` (with one, everything else redirects to HTTPS) |
| `EDGE_HTTPS_PORT` | `weavesignal-edge` | TCP | 443 | off | anywhere | `/hls/`, and `/listen/` when a relay is beside it, over HTTPS; only when a certificate is mounted |
| `RELAY_PORT` | `weavesignal-relay` | TCP | 8000 | 7323, 7324 | anywhere, or loopback behind the edge's `/listen/` | the Icecast relay's listener streams |
| `RELAY_HOP_PORT` | `weavesignal-relay` | TCP | `RELAY_PORT` + 1 | off (the bundle's relays pull over plain HTTP, with no hop) | loopback only | the encrypted hop's side of the relay: Icecast's own upstream, never a listener's |

Any free port may be used, such as 8080 and 8443 for the edge, or 8001 and 18000 for the relay.
`EDGE_BIND` and `RELAY_BIND` set the address they listen on (every interface by default; loopback in the bundle).
The edge connects out to the station's HTTPS port, and so does the relay, through the encrypted hop in its image (ADR-0162): nginx in the relay image fetches from the station over HTTPS and hands the stream to Icecast on `RELAY_HOP_PORT` (`RELAY_PORT` + 1), on loopback only. With `RELAY_ORIGIN_SCHEME=http` the relay instead connects to the station's plain-HTTP port, `proxy_http`, for a bench or a private link.

## With `weavesignal.sh`

`weavesignal.sh` writes each port into the station's settings file (ADR-0075, [Install a station with weavesignal.sh](/docs/how-to/install-with-weavesignal-sh/)).
A port left unset takes its default, or the first suggested alternative that is free when the default is taken, and the script says which it chose.
A port you set yourself that is taken stops the script, which names the clash and a free alternative.
Ports already in the settings file are kept on every later run.
It does not offer 443 for the media port, which a rootless container engine cannot bind.

The playout feed, the desk's feed, the two ports above the playout feed and above the listener port, and the listener process's `/status.json` are internal: the proxy never publishes them.

With `--public` ([Publish a station from the Compose bundle](/docs/how-to/publish-with-compose/)) the script also checks and writes `PROXY_HTTPS_PORT` (443, then 8443), `PROXY_HTTP_PORT` (80, then 8080) and `TURN_TLS_PORT` (5349, then 15349), and, if set, `TURN_UDP_PORT` and `LIVEKIT_TCP_PORT` (0 is off).
A taken 443 or 80 stops the script rather than moving the proxy, since a certificate authority reaches those two only; it names the alternative and says to forward to it.
A rootless Podman cannot bind a port below 1024 unless `net.ipv4.ip_unprivileged_port_start` allows it, and the script says so before it starts.

`SOURCE_ICECAST_PORT` (TCP), `SOURCE_SRT_PORT` (UDP) and `SOURCE_WHIP_PORT` (UDP), for hosts' own studios, are 0, off, unless you set them; a set one is checked as a public port and kept.
`INGEST_WHIP_PORT` (7319) is the ingest's loopback port for WHIP's requests, checked like the other loopback ports.

With `PHONE=true` the script adds `phone.yaml` and checks the [phone line's](#the-phone-line) ports the same way: `PHONE_SIP_PORT` free on UDP and TCP on every address, every port of the RTP range from `PHONE_RTP_PORT` (two for each of `PHONE_LINES`) free on UDP, and `PHONE_REDIS_PORT` free on loopback, each moved to the next suggestion when taken unless you set it; it writes `PHONE_RTP_LAST` and makes `PHONE_REDIS_PASSWORD`.
An odd `PHONE_RTP_PORT`, or a phone port another setting has chosen, stops it.
With `PHONE=false` it writes nothing of the line's but `PHONE`, and keeps any `PHONE_*` settings already in the file.
The `ingest` service binds them on every address on a public station (`INGEST_BIND`, derived), and on loopback otherwise.

A loopback port counts as taken when something listens on loopback or on every address; a listener bound only to another interface's address does not take it.
The media port counts as taken when anything listens on it.
`icecast.server`'s legacy source port and the playout feed's harbor each also open the port above them (the `listeners` port + 1 and `playout_feed` + 1), so the script checks those too.

## On Cloudron

The Cloudron package maps its ports through Cloudron's own settings, which the administrator chooses at install (ADR-0075).
Inside the container every port above is on loopback, behind the package's nginx.

| Cloudron setting | Protocol | Default | On at install | Notes |
|---|---|---|---|---|
| The app's domain | HTTPS | Cloudron's 443 | yes | the studio: `/health` and `/api/`; TURN is reached at this name |
| Listener stream domain | HTTPS | Cloudron's 443 | yes | serves `/programme.opus` only |
| Live room signalling domain | HTTPS | Cloudron's 443 | yes | LiveKit's WebSocket |
| Public pages domain | HTTPS | Cloudron's 443 | yes | the player, call-in, the chat, the invitation page, the echo test, feeds, cards and episode files |
| `LIVEKIT_MEDIA_PORT` | UDP | 7882 | yes | keep it out of 50000–51000, Cloudron's own TURN relay range |
| `TURN_TLS_PORT` | TCP | **15349** | yes | Cloudron's own TURN holds 5349 (ADR-0106) |
| `TURN_UDP_PORT` | UDP | 3479 | no | Cloudron's own TURN holds 3478 |
| `LIVEKIT_TCP_PORT` | TCP | 7881 | no | TURN over TLS reaches the same guests |
| `SOURCE_ICECAST_PORT` | TCP | 8005 | no | hosts' own studios over Icecast; disabled is off |
| `SOURCE_SRT_PORT` | UDP | 9710 | no | hosts' own studios over SRT; disabled is off |
| `SOURCE_WHIP_PORT` | UDP | 9712 | no | hosts' own studios over WHIP from OBS, sound only; needs the public pages domain, where OBS posts its offer; disabled is off |

The public ports keep the same number inside the container, so LiveKit advertises the port the administrator chose, and a host's key gives the hosts' ports chosen there.
Cloudron gives the app's domains an AAAA record whenever the server has IPv6, so on such a server TURN refuses guests who reach it over IPv6 ([Install on Cloudron](/docs/how-to/install-on-cloudron/#ipv6-and-turn)).
Every other route keeps 5349 for TURN over TLS, with 443 offered where it is free (ADR-0080).
The control plane's suggested media-port alternative, 52000, stays clear of Cloudron's TURN relay range (50000–51000).

## Ports that are not settings

`icecast.server` also listens on TCP at the `listeners` port plus one (7313 by default), on loopback: the legacy source port for older encoders.
Liquidsoap has no setting to turn it off; it is not published.
The playout feed's harbor does the same at `playout_feed` + 1 (7316).
`weavesignal.sh` checks both ports are free; `check-ports` does not look at them.

Programs that connect out over UDP, such as Liquidsoap's SRT caller and the desk's WebRTC connections, hold sockets on ephemeral ports bound to every interface.
They listen for nothing, but packets from the network reach them, so a host firewall should keep the ephemeral range closed.
The bundle's port scan (`just test-bundle`) reports them as warnings.

LiveKit's TURN allocates relay ports (30000–40000 unless `turn.relay_range_start` and `turn.relay_range_end` say otherwise) for guests who use it.
On loopback, TURN over TLS was measured working end to end (`just test-m3`): it relays to LiveKit's own media port, which LiveKit allows only for a public node IP or one listed in `turn.allow_restricted_peer_cidrs`, and LiveKit tells browsers port 443 whatever its TLS port, which the public pages correct ([The public pages](/docs/reference/public-pages/#turn-over-tls)).
Behind Docker's bridge it has not been measured.
