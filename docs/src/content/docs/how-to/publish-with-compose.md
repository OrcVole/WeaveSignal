---
title: "Publish a station from the Compose bundle"
description: "Put the Compose bundle on the internet with one command: a reverse proxy with a certificate for your domain, TURN over TLS, and the ports to open."
type: howto
status: draft
last_verified: 2026-11-04
sidebar:
  order: 34
---

`weavesignal.sh --public` turns the loopback bundle into a station that people off the machine can listen to, call, chat with and sign in to (ADR-0022).
It adds a reverse proxy, **Caddy**, which gets and renews the certificate for your domain by itself, and turns on LiveKit's TURN over TLS for guests whose networks block UDP (ADR-0059, ADR-0080).
The bundle is otherwise the one in [Install a station with weavesignal.sh](/how-to/install-with-weavesignal-sh/).

## Before you start

- **A domain name that reaches this machine**: an A record for, say, `station.example`.
  One name serves everything, TURN included; there is no second name for the stream or the room.
  **Give it no AAAA record.** LiveKit's built-in TURN refuses a client that reaches it over IPv6 (an allocation error, 508), so with an AAAA record a guest who can join only through TURN, on an IPv6 network, cannot join at all ([Known limits](/reference/release-one-status/#known-limits)).
- **A server of its own**, not a busy shared one: on a heavily shared server the listener stream ran dry ([Memory and CPU](/reference/resources/#what-this-means-for-a-host)).
- **Ports open to the internet**, in the machine's firewall and any router in front of it:

  | Port | Protocol | What |
  |---|---|---|
  | 80 | TCP | the proxy: the certificate authority's check, a redirect to HTTPS, and the listener stream for players that cannot do TLS |
  | 443 | TCP | the proxy: the station |
  | 7882 | UDP | LiveKit's one media port (ADR-0023) |
  | 5349 | TCP | LiveKit's TURN over TLS (ADR-0080) |

  TURN over UDP and ICE over TCP are off unless you set their ports (below); open those too if you do.
  Nothing else is published. The control plane, the chat and the listener process stay on loopback behind the proxy.
- **Ports 80 and 443 free**, and bindable by your user. A rootless Podman may not bind ports below 1024 unless the machine allows it. If yours does not, the script stops and prints the line to run:

  ```sh
  sudo sysctl -w net.ipv4.ip_unprivileged_port_start=80
  echo 'net.ipv4.ip_unprivileged_port_start=80' | sudo tee /etc/sysctl.d/50-unprivileged-ports.conf
  ```

  Docker's daemon binds them for you. If something else holds 443 on the machine, the script says so and does not move the proxy on its own, because the certificate and the addresses depend on it: free the port, or set `PROXY_HTTPS_PORT=8443 PROXY_HTTP_PORT=8080` and forward the machine's 443 and 80 to those ports.
- Podman 5 with `docker-compose`, or Docker with the Compose plugin, and the server image, as for [the loopback bundle](/how-to/install-with-weavesignal-sh/#before-you-start). Not `podman-compose`: it would start Caddy without its settings, with no certificate, and the script refuses it.

## Publish

```sh
deploy/compose/weavesignal.sh --public station.example
```

This does everything the plain script does, and then:

1. Checks the new ports (`PROXY_HTTPS_PORT` 443, `PROXY_HTTP_PORT` 80, `TURN_TLS_PORT` 5349, and `TURN_UDP_PORT` and `LIVEKIT_TCP_PORT` if you set them) as it checks the others ([Ports](/reference/ports/#with-weavesignalsh)).
2. Writes the public settings into the settings file, and works out from them the addresses the services are given: the room as `wss://station.example`, the listener stream as `https://station.example/programme.opus`, and `forwarded_for` on in the control plane and the chat, so each caller is counted by their own address and not the proxy's.
3. Starts the bundle with `public.yaml`. **The proxy starts first and LiveKit waits for the certificate**, because TURN over TLS needs it; the first start therefore takes as long as the certificate authority does, usually under a minute.
4. Asks the proxy, on this machine, for `/health`, and prints the addresses and what to open.

If the certificate cannot be obtained (the name does not reach this machine, or port 80 or 443 is blocked), the script prints the proxy's log and stops, and the room does not start without it. Fix the cause and run the script again.

Running it again keeps the route on; `--public station.example` need only be given once.
`weavesignal.sh --public off` goes back to loopback only and removes the proxy.

## What the proxy publishes

| Path | Goes to |
|---|---|
| `/rtc`, `/rtc/*` | LiveKit's signalling: a browser's join and its connection check |
| `/api/v1/chat/*` | the chat, its socket included |
| `/programme.opus` | the listener stream, read only (`GET` and `HEAD`); an `Icy-MetaData` request header is dropped, as ICY blocks would be spliced into the Ogg stream |
| everything else | the control plane: the pages, `/api/`, `/health`, `/hls/`, feeds and cards; an upload may be up to 200 MB (a library track) |

Never published: LiveKit's server API (`/twirp`), the listener process's `/status.json` and admin pages, the playout feed, the desk's feed, the SRT ingest port and the ports next to them.
The control plane checks its own bearer tokens, so there is no login in front of it; the studio app connects to `https://station.example` with the admin token or a staff sign-in (ADR-0139).
The proxy gives each service the connection's own address in `X-Forwarded-For` and never a client's. Behind any proxy, the control plane and the chat count the header's last entry, the one the station's own proxy added, so a client cannot choose its address by sending the header itself.
Plain HTTP on port 80 serves the stream mount and redirects everything else to HTTPS, so a station on other ports keeps its port in the redirect.
Show cards and podcast feeds are on once the station is public: the bundle gives the control plane the public address as `publishing.public_url`, and keeps the cards and episode files in the `show-cards` and `episodes-media` volumes (`/cards/`, `/media/episodes/`); a loopback station has them off.
HTTPS answers carry `Strict-Transport-Security` for a year (not for the name's subdomains), so a browser that has visited once comes back over HTTPS only; players that cannot do TLS keep the plain-HTTP mount.

## Settings

All are in the settings file (`deploy/compose/.env`), or in the environment of the run that first writes them.

| Setting | Default | |
|---|---|---|
| `PUBLIC_DOMAIN` | | the station's name; `--public` sets it |
| `PUBLIC_TLS` | `acme` | `acme`: a certificate from Let's Encrypt. `internal`: Caddy's own certificate authority, for a private network or a test; browsers do not trust it unless you install its root |
| `ACME_EMAIL` | empty | where Let's Encrypt writes about the certificate |
| `PROXY_HTTPS_PORT`, `PROXY_HTTP_PORT` | 443, 80 | a certificate authority reaches 443 and 80 only, so any other pair needs those forwarded to it |
| `TURN_TLS_PORT` | 5349 | 15349 where 5349 is taken; not 443, which is the proxy's |
| `TURN_UDP_PORT`, `LIVEKIT_TCP_PORT` | 0 (off) | TURN over UDP and ICE over TCP; guests who need either are reached by TURN over TLS |
| `NODE_IP` | empty | LiveKit's public address; empty, it finds it by asking a STUN server. Set it where discovery picks the wrong address |
| `TURN_ALLOW_CIDRS` | empty | LiveKit's TURN refuses to relay to private addresses, its own included. A `NODE_IP` that is private (the machine is behind NAT) must be listed here, such as `10.0.0.5/32` |
| `SEARCH_LISTING`, `DIRECTORY_LISTING` | `false` | the station is unlisted until you turn these on: search engines may list the pages (ADR-0133), and the stream asks internet radio directories to list it (ADR-0129) |

LiveKit tells browsers that TURN over TLS is on port 443 whatever port it listens on; the pages replace it with `TURN_TLS_PORT` before the browser sees it.

## Renewal

Caddy renews the certificate about thirty days before it expires, and the proxy picks it up at once.
**LiveKit reads its TURN certificate once, when it starts.** Within those thirty days, between shows, run:

```sh
deploy/compose/weavesignal.sh --renew-turn
```

It restarts LiveKit and drops everyone in the room, so do it off air. Updating the station with the script does not restart LiveKit when nothing about it changed. If you forget, the proxy and the room keep working; only TURN over TLS, for the guests who need it, fails once the old certificate expires.

## Check it

```sh
curl https://station.example/health
curl -I https://station.example/programme.opus
```

Open `https://station.example/` in a browser for the player. Call-in stays closed until `CONSENT_CONTACT` is set ([Open call-in](/how-to/install-with-weavesignal-sh/#open-call-in)).

`just public-check`, from the repository, runs the route's own test with no public domain: it renders the bundle, runs the generated proxy settings with Caddy's own certificate authority in front of stand-in services, and starts LiveKit on the certificate Caddy made.

## Where it has run

On 2026-11-03 the public route ran on a dedicated public server (rootless Podman 5.8 with docker-compose) with a real certificate: a caller rang in from a phone through it, and call-in shows ran on it that night.

## Known limits

- **IPv6 and TURN**: see the AAAA record under [Before you start](#before-you-start).
- **The desk's WebRTC sends without congestion control**: its image lacks GStreamer's `rtpgccbwe`, so a guest on a poor link may hear more loss than a browser-to-browser call would give.
- **Quadlets** are generated with the proxy when the settings name a domain (`deploy/quadlets/generate.sh`), but the generated proxy units have been checked with Podman's generator only, not run.
- **The edges** (`edges.yaml`) pull from loopback and are unchanged by the public route.
