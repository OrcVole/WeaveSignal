<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
# The Cloudron package

One Cloudron app holds release one's server (ADR-0022): the control plane, the chat, the desk,
Liquidsoap 2.5 playout, the listener process with `icecast.server`, LiveKit with its built-in
TURN, the station's public pages, and nginx in front of them, all run by supervisord as the
`cloudron` user. PostgreSQL is Cloudron's addon (ADR-0082); the chat's mail is Cloudron's
`sendmail` addon. It is built for amd64 only.
The manifest's `version` is the package's; what each version changed, and which commit it
carries, is in `CHANGELOG.md`.

| File | What it is |
|---|---|
| `Dockerfile` | Built from the repository's root. Every Rust and runtime stage is `cloudron/base` 6.0.1 (Ubuntu 26.04): GStreamer 1.28.2 from Ubuntu, gst-plugins-rs 0.15.4+fixup's webrtc plugin (LiveKit) built against it, libsrt 1.5.7 from source in place of Ubuntu's 1.5.4, Liquidsoap 2.5.0-rc1 from upstream's Ubuntu 26.04 package (it carries libsrt 1.5.7), LiveKit 1.13.10's binary, msmtp from Ubuntu. The public pages (`apps/web`) are built in a Node 24 stage with pnpm 10.30.2. Sources are pinned by hash, except pnpm itself (by version; the pages' dependencies by its lockfile). |
| `CloudronManifest.json` | Ports, domains, addons, the memory limit. The `id` is `top.weavesignal.cloudronapp` since 1.0.0. |
| `CHANGELOG.md` | What each package version changed. |
| `start.sh` | Writes every service's settings from Cloudron's environment on each start, appends settings new to this version to `weavesignal.env`, generates the station identifier and secrets on the first start, and checks the control plane's settings and ports before supervisord starts. |
| `supervisord.conf` | The seven services. |
| `test-local.sh` | The package run locally without a Cloudron, with checks, a room test, a restart test and cold starts. |
| `memory-gate.sh` | The local proxy for the idle-versus-production memory gate. |

## How the processes are wired

The same as `deploy/compose/compose.yaml`, inside one container:

| Program | Listens (loopback unless said) | Talks to |
|---|---|---|
| `livekit` | 7880 (signalling), the media port and TURN over TLS on every address | — |
| `desk` | SRT 7313/udp; its control socket `/run/weavesignal/desk.sock` | LiveKit; reads `/app/data/carts` |
| `playout` (`weavesignal.liq`) | its control socket `playout.sock` | the desk's SRT (caller, every second); the listener process's feed |
| `listeners` (`listeners.liq`) | 7312 (`icecast.server`, `/status.json`, and 7313/tcp beside it), 7315 (the feed); `listeners.sock` | writes the legal logger into `/app/data/recordings/logger` |
| `control` | 7310 | PostgreSQL; the three sockets; `/status.json`; LiveKit's API |
| `chat` | 7311 | PostgreSQL; the control plane's studio view |
| `nginx` | 8000, 8001, 8002, 8003 on every address | the services above |

Playout sends the programme to the listener process as PCM in WAV (M4); the listener process
holds the mount, plays its own fallback while the feed is away, and encodes the one Ogg Opus
stream listeners hear, so a playout or desk restart does not disconnect anyone. The desk now
starts with the rest: 0.1.x's `desk-start.sh` waited for the mount because playout's switch to
the desk began a chained Ogg stream on the mount, and that switch no longer reaches it.

## Surfaces

| Surface | Manifest | Inside the container |
|---|---|---|
| The station: `/health`, `/api/` (the studio's routes and the chat's), the pages, feeds, cards | `httpPort` 8000 | nginx → control plane 7310, chat 7311 (`/api/v1/chat/`) |
| The listener stream, `/programme.opus` only | `httpPorts.LISTENERS_DOMAIN` → 8001 | nginx → `icecast.server` 7312 |
| LiveKit's signalling | `httpPorts.ROOM_DOMAIN` → 8002 | nginx → LiveKit 7880 |
| The public pages: the player with call-in and the chat, `/join/`, `/echo-test/`, `/feeds/`, `/cards/`, `/media/episodes/`, and only their public routes | `httpPorts.PUBLIC_DOMAIN` → 8003 | nginx → the pages, control plane 7310, chat 7311 |
| Room media | `udpPorts.LIVEKIT_MEDIA_PORT`, 7882 | LiveKit's one UDP port (ADR-0023) |
| TURN over TLS | `tcpPorts.TURN_TLS_PORT`, 15349 | LiveKit's TURN, with the `tls` addon's certificate (ADR-0059, ADR-0106: Cloudron's own TURN holds 5349) |
| TURN over UDP (off by default) | `udpPorts.TURN_UDP_PORT`, 3479 | LiveKit's TURN |
| ICE over TCP (off by default) | `tcpPorts.LIVEKIT_TCP_PORT`, 7881 | LiveKit |

The public ports have no `containerPort`, so Cloudron maps each to the same number inside the
container, and `start.sh` gives LiveKit the number the administrator chose: LiveKit advertises the
port it binds. The SRT link, the feed, `icecast.server`'s `/status.json` (ADR-0115), the control
plane, the chat, LiveKit's API and the control sockets stay on loopback inside the container
(ADR-0048 rule 3, ADR-0049, ADR-0086). The one socket on every address that is not a published
port is playout's SRT caller's ephemeral UDP port, an outgoing connection's.

Cloudron's own TURN service holds 3478, 5349 and 50000–51000/udp on the server (reported on
Cloudron's forum, not measured here). Where it runs, choose 15349 for TURN over TLS, and keep the
media port out of 50000–51000.

## Why a public domain

`proxyAuth` puts the station's own domain behind Cloudron's login when the app uses Cloudron's
users, and the addon cannot be removed or narrowed after install. Chatters, callers, invited
guests, podcast apps and anyone reading a show card have no Cloudron account, so their routes
are served on a domain of their own, as the listener stream and the room already were. The
public domain serves the built pages and an allow-list: the chat's `info`, `socket`, `signin`,
`guest`, `notices`, `me`, `signout`, `reports` and statement reviews; the control plane's
`station`, `player`, `call-in`, `pass` routes and now playing; and `GET` on feeds, cards and
episode files. Everything else under `/api/` is 404 there, even with the bearer: the studio,
the chat's moderation and admin routes, retention, carts, `/health`. Cards and feeds print
`https://` and the public domain (`publishing.public_url`); without one they fall back to the
app's origin, which a podcast app cannot pass when the login is on, and `start.sh` says so.
`PUBLISHING_URL` in `weavesignal.env` overrides both.

Every surface still answers `noindex` while the station is private (ADR-0078).

## Privacy and access

- The stream is published with `public=false` (`playout/listeners.liq`): it asks no directory
  to list it. The listener domain serves only the mount, answers `noindex`, refuses `PUT`,
  `icecast.server`'s admin pages and `/status.json`, and drops `Icy-MetaData` from requests
  (bench 02's trap).
- The listener stream asks for a password by default (`LISTENERS_AUTH=on` in
  `/app/data/weavesignal.env`; the user is `listener`, the password `LISTENERS_PASSWORD` in
  `/app/data/secrets.env`). `off` leaves it open but unlisted.
- `optionalSso` with `proxyAuth`: installed with Cloudron's user management, the station's own
  domain sits behind Cloudron's login; a `Bearer` token still reaches the admin routes
  (`supportsBearerAuth`). Cloudron's login does not cover the `httpPorts` domains: the room is
  guarded by short-lived room tokens (ADR-0085), the stream by its password, the public domain
  by serving only public routes.
- The client's address: nginx takes the rightmost address in `X-Forwarded-For` that is not
  private (Cloudron's proxy appends the one it saw) and gives each service that one address;
  the control plane (`join.forwarded_for`, call-in's per-address limit) and the chat
  (`listen.forwarded_for`) read it. Under No retention the chat stores no address (ADR-0115).

## The chat's mail

The chat sends sign-in codes by piping each email to a sendmail-compatible program (`MAIL_MODE`
`command`); it has no SMTP client. Cloudron's `sendmail` addon gives an SMTP relay, so the image
carries msmtp, and `start.sh` writes `/run/weavesignal/msmtprc` (0600, never backed up) from
`CLOUDRON_MAIL_*` on each start: PLAIN authentication without TLS on the relay's port inside the
server (msmtp picks no plain-text method by itself without TLS), and `CLOUDRON_MAIL_FROM` as
the sender. `MAIL_MODE` in `weavesignal.env`: `cloudron` (the default; the outbox when the addon
gives no server), `outbox` (files in `/app/data/chat-outbox`), or `off`.

## The station's files

All under `/app/data`, so they are backed up: `carts` (the media worker's FLAC; the desk and
playout read it), `library`, `fallback` (played by playout and by the listener process while
the programme is away; empty, they play silence), `recordings` (retention's directory; the
legal logger writes `recordings/logger` and the sweeper deletes there, ADR-0113), `show-cards`,
`episodes`, `chat-outbox`. The cart demo pack is in the image (`/app/code/carts-demo`).
Migrations (now 0001–0007) run at every start, by the control plane, before it listens.

Settings this version added (`CHAT_PORT`, `PLAYOUT_FEED_PORT`, `MAIL_MODE`, `PUBLISHING_URL`,
`PUBLISHING_PLACE`, `PUBLISHING_ZONE`) are appended to an existing `weavesignal.env` with their
defaults on the first start of 0.2.0; the administrator's lines are left as they are.

## Build and run locally

```sh
podman build -t localhost/weavesignal-cloudron:0.2.0-local -f deploy/cloudron/Dockerfile .
deploy/cloudron/test-local.sh          # needs localhost/weavesignal-desk-dev:m1 for the tone guest
deploy/cloudron/memory-gate.sh         # then, with the app still running
deploy/cloudron/test-local.sh cold 3   # three cold starts, each with the room test
deploy/cloudron/test-local.sh down
```

`test-local.sh` runs the image read-only with tmpfs `/run` and `/tmp`, a volume for
`/app/data`, the manifest's memory limit, PostgreSQL 16 on a private network for the addon, a
small SMTP sink for the `sendmail` addon, and a self-signed certificate in `/etc/certs`. Its
containers are named `weavesignal-cloudron-local-*` and its host ports are 27400–27403, 27449/tcp
and 27482/udp (each a setting; it refuses a port already in use). It checks:

- health and the database; the listener domain (401 without the password; 404 for `/`, the
  admin pages, `/status.json` and `/feed`; 403 for `PUT`); a late listener decoding Opus with
  no ICY metadata;
- that nothing listens off loopback but nginx and LiveKit's public ports (and playout's SRT
  caller), and that 7310, 7311, 7312 and 7315 are on loopback;
- the public domain: the three pages, the public routes (200), the studio's, the chat's admin,
  retention's, carts' and staff routes (404 even with the bearer), `/health` with no bearer
  (200, ADR-0160), `/feeds/` reaching the control plane; the chat's admin route on the station
  domain with the bearer;
- unlisted (ADR-0133): exactly one `X-Robots-Tag: noindex, nofollow` on a page, the invite
  page, a public route, a feed and a station route, and `/robots.txt` disallowing `/`;
- the two email keys added to a `secrets.env` that lacked them, as a station installed before
  0.8.0 has, and the chat starting with its key;
- the default loop (ADR-0130) on the stream while the room is empty (not silence, not the tone);
- with `COMPAT=true` (the main run; cold starts leave it off, as installed), the MP3 stream:
  401 without the password, MP3 at 44.1 kHz, no ICY metadata;
- the chat's sign-in email through msmtp to the SMTP sink, authenticated, with the right `To`
  and `From`;
- the room test: a guest publishing a 1 kHz tone through the room domain with a publish-only
  token (ADR-0090), and the tone in the listener stream;
- the restart test: a listener connected for 110 s while playout is killed by its PID at 45 s;
  one connection, a clean decode, at most one gap with the programme back after it, the
  listener process untouched (since 0.8.0 the listener process decodes the feed natively and
  playout is back in about 0.3 s: one run heard a single 100 ms dropout, the next none);
- the faults in the log (`non-monotonic DTS`, `Generator max buffered length exceeded`,
  `EPIPE`) and the cgroup's `oom_kill`.

The guest runs in the app's network namespace: rootless Podman does not carry WebRTC's UDP into
the container as Docker's published ports do, so the port mapping, TURN and LiveKit's public
address are checked on a real Cloudron.

## What the local checks show (2026-10-26)

Every check above passed on the final image (`localhost/weavesignal-cloudron:0.2.0-local`).
Three cold starts of three, each from a fresh volume (the first start migrates an empty
database; the start measured is the restart that follows, every process cold):

| Cold start | Health with the database | The listener mount answers | The room test | Faults logged, `oom_kill` |
|---|---|---|---|---|
| 1 | 1.7 s | 17.3 s | the tone heard 20 s into the recording, −10.5 dBFS, 100 % at 1 kHz | none, 0 |
| 2 | 1.5 s | 17.0 s | the same, −10.6 dBFS | none, 0 |
| 3 | 1.5 s | 17.0 s | the same, −10.5 dBFS | none, 0 |

With the desk started beside the rest, no run logged `non-monotonic DTS`, `Generator max
buffered length exceeded` or `EPIPE` (0.1.x needed `desk-start.sh` to reach that). Two earlier
runs of the full test and three earlier cold starts, on images differing only in the scripts'
details, gave the same figures.

The restart test: playout was killed 45.5 s into a listener's connection and supervisord had it
running again 0.1 s later; the listener stayed on one connection for the whole 110 s
(1,315,556 bytes, 108.8 s decoded, no decoder errors), heard one 1.6 s gap in the tone, the
listener process's fallback, and then the programme again. The listener process kept its PID.
In 0021 (M4, the bare stack) the programme came back 5.6 s after playout was stopped, 5 s of
it the stop itself; here the kill is instant. A new listener hears the programme about 20 s
behind live (the tone heard from 20 s into the recording), as 0021 measured.

Two things seen and not fixed here:

- **Stopping the container takes about 10.5 s**: supervisord stops the programs by priority,
  and the two Liquidsoaps take 2–6 s each to exit. LiveKit drained its rooms past
  supervisord's 10 s every time, so it now has 3 s (`stopwaitsecs`); `podman stop -t 10`
  still ended in SIGKILL at 10.3–10.9 s. Nothing is lost but the last partial page of a
  logger hour; Cloudron's own stop timeout is to be seen on the server.
- **The log is busy**: the listener process and playout log every control-socket connection
  (the control plane polls them every 250 ms), and LiveKit logs each `ListParticipants` at
  info, two a second. Cloudron rotates logs; the scripts' log level is in `m1`'s fixed scripts.

## The memory limit

**0.8.0 (2026-11-02)**: the limit stays 1.5 GiB. `memory-gate.sh` on the 0.8.0 image, with
the MP3 stream on (a second encoder in the listener process) and the processing chain on:
idle 526 MiB current, 527 MiB peak; loaded (300 s) peak 699 MiB, 45 % of the limit;
`oom_kill` 0 throughout. Loaded RSS: listener process 334 MB, playout 198 MB, LiveKit 83 MB,
desk 62 MB, control 38 MB, chat 37 MB. The labs Cloudron, at rest after the update, read
355 MiB peak in the app's cgroup. The chat's share is understated: since its per-address
limits (0.4.1) the load generator's 1,000 sockets, all from one address, are mostly closed
(958 of them), so the chat held about 40 readers; 0020 measured 97 MiB resident for 4,000
sockets, which the headroom covers (about 800 MiB, 52 %). The load generator needs addresses
of its own before the next gate.

The figures below are 0.2.0's.

1.5 GiB (`memoryLimit` 1610612736), unchanged. Measured locally with `memory-gate.sh` on
2026-10-26, cgroup v2, read-only root, PostgreSQL outside the container as Cloudron's addon is:

| Invariant | Idle (60 s after the full test) | Loaded (300 s) |
|---|---|---|
| `memory.current` / peak | 660 MiB / 663 MiB | 908 MiB / 909 MiB (59 % of 1.5 GiB) |
| `oom_kill` | 0 | 0 |
| Per process (RSS) | listener process 271 MB, playout 254 MB, desk 53 MB, LiveKit 52 MB, supervisord 28 MB, control 11 MB, chat 11 MB, nginx 8 MB | listener process 339 MB, playout 280 MB, chat 105 MB, LiveKit 70 MB, desk 54 MB, nginx 2 × 15 MB, control 13 MB |
| Verdict | | PASS: peak 59 % of the limit |

The load: 3 tone publishers through LiveKit and the desk (a presenter and two guests; a
caller's token now needs consent, M3), 50 listeners through nginx for 300 s (3.6 MB each, the
constant 96 kbit/s), one SSE client on the studio events (3.1 MB), and the chat's own load
generator through nginx three times over: 1,000 sockets each (10 never reading, 50
reconnecting), 8 posters at 7, 20 and 60 messages a second; every message acknowledged
(p99 21–62 ms) and delivered, every reconnect recovered.

The gate's rule (peak ÷ 0.7) gives 1.27 GiB. The limit stays at 1.5 GiB because the listener
process grows under listeners (271 to 339 MB here) and `icecast.server` grew at rest in 0.1.x (about 1.5 MB a minute), a cold
start without the script cache needs about 1.3 GB, the chat was measured at 97 MiB resident
for 4,000 sockets (0020) beside 1,000 here, the load was gentle beside 2,000 listeners (who
reach the station through edges or a CDN, ADR-0051, not this container), and video is not in
the image yet. A test install repeats the gate on a Cloudron itself, over hours.

## Not tested here, for the server

- Docker's published UDP and TCP ports, LiveKit's public address (`use_external_ip`, with
  `NODE_IP` in `weavesignal.env` as the override) and TURN over TLS from outside.
- TURN relays: LiveKit's TURN allocates relay ports (30000–40000) on its node address; behind
  Docker's bridge the server's own traffic to them may not come back in (M3's measurement).
- `proxyAuth` with `supportsBearerAuth`: that a Bearer request passes Cloudron's login, and
  what the studio app's requests carry. The addon cannot be added or removed after install.
  Since 0.8.0 its `path` is `!/health` (ADR-0160), which Cloudron 10.1 applied on an update of
  an installed app (checked on a test Cloudron and on staging: `/health` 200 with no bearer,
  everything else 302). The exclusion is a prefix (`/healthz` also passes the login and gets
  this nginx's 404), harmless while the station serves only `location = /health`.
  `SEARCH_LISTING=true` was rendered and passed `nginx -t`, but no listed station has run.
- That `cloudron update` from 0.1.1 adds the new `PUBLIC_DOMAIN` httpPort and the `sendmail`
  addon to the installed app (if it does not, cards and feeds fall back to the app's origin
  and the chat's codes to the outbox, and `start.sh` says so in the log).
- The real relay's authentication: Cloudron's mail server with msmtp's `auth plain` and
  `tls off`, and that `CLOUDRON_MAIL_FROM` is accepted as the sender.
- That Cloudron's proxy appends to `X-Forwarded-For` from a private address, as nginx's
  real-address rule here assumes; and its upload size limit for library tracks (200 MB).
- The player page's stream in a browser. With `LISTENERS_AUTH=on` (off by default since 1.0.0) the page's
  stream address is on the password-protected listener domain, and a browser does not ask
  for a password for audio another domain serves, so the page will not play it; players and
  `curl` can. Set `LISTENERS_AUTH=off` (open but unlisted) to hear it in the page. No local
  check plays the page in a browser; the pages' scripts and styles are checked to be in the
  image.
- Whether `enabledByDefault: false` is honoured for `udpPorts` (it is documented for TCP).
- An hours-long soak: `icecast.server` grew slowly at rest in 0.1.x; the legal logger's hourly
  files under retention's sweeper.

## Publish a version

The image is built here and pushed to the public registry; Cloudron installs it by its tag.

1. Build and check it: `./test-local.sh` (it builds `localhost/weavesignal-cloudron:<version>-local`).
2. Tag and push it to the project's registry:
   `podman tag localhost/weavesignal-cloudron:<version>-local forgejo.weavesignal.top/weavesignal/weavesignal-cloudron:<version>`,
   then `podman push` it.
3. Install or update with the server named explicitly, from this folder (where the manifest is):
   `cloudron install --server <server> --location <app domain> --image forgejo.weavesignal.top/weavesignal/weavesignal-cloudron:<version>`
   (or `cloudron update --server <server> --app <app domain> --image …`).
4. After the first install, in Cloudron's dashboard: give the **Public pages** domain a location if
   Cloudron did not, check that Mail (the `sendmail` addon) is listed for the app, open the
   public domain's player page, and read the start line in the app's log (`public pages …;
   cards and feeds …; chat mail command`).

The Dockerfile uses BuildKit cache mounts and needs the repository's root as its context, so it
is not built by an on-server `cloudron install` or `cloudron update` without `--image`.
