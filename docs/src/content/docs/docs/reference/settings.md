---
title: "Settings"
description: "The control plane's and the desk's settings files, and the environment variables that carry their secrets."
type: reference
status: draft
last_verified: 2026-11-04
sidebar:
  order: 10
---

Three processes read settings files in TOML: the control plane (`control.toml`), the desk (`desk.toml`) and the chat (`chat.toml`, in [The live chat](/docs/reference/chat/#settings)).
Secrets never go in a file; they come from the environment.
`weavesignal-control sample-config` prints a complete `control.toml` with a fresh station identifier; the repository's `crates/desk/desk.example.toml` is the desk's, with a comment on every value.
An unknown key in the control plane's or the desk's file is refused when the program starts, with the key named.
The install routes write these files for you: the Compose bundle from its `.env`, the Cloudron package from its `weavesignal.env`.

## Environment

| Variable | Read by | Holds |
|---|---|---|
| `WEAVESIGNAL_CONFIG`, `WEAVESIGNAL_DESK_CONFIG`, `WEAVESIGNAL_CHAT_CONFIG` | control plane, desk, chat | each one's settings file, if not `--config` (default `control.toml`, `desk.toml`, `chat.toml`) |
| `WEAVESIGNAL_DATABASE_URL` | control plane, chat | PostgreSQL's address; over a Unix socket in the bundles (ADR-0092) |
| `WEAVESIGNAL_LIVEKIT_API_SECRET` | control plane, desk | LiveKit's API secret, at least 32 characters |
| `WEAVESIGNAL_CONTROL_ADMIN_TOKEN` | control plane, chat | the station key: the station's own bearer, with an admin's powers; it makes the first staff account ([Add staff and sign in](/docs/how-to/add-staff/)). Unset, only staff sessions open the studio's routes. The chat reads the studio's view with it |
| `WEAVESIGNAL_PLAYOUT_FEED_PASSWORD` | desk | the password playout sends with its feed back to the desk (`[video.playout_feed]`); the bundles give it the listener source password |
| `WEAVESIGNAL_CHAT_EMAIL_KEY` | chat service | the secret that keys the hash of chatters' email addresses and sign-in codes (ADR-0125): at least 32 characters, for example `openssl rand -hex 32`. Required: the chat exits at start, naming this variable, when it is missing or shorter (and Compose refuses to start without it). Never in the settings file: the Compose bundle keeps it in `.env` (mode 600), the Quadlets in a Podman secret. See [The email key](/docs/reference/chat/#the-email-key) |
| `WEAVESIGNAL_CONTROL_EMAIL_KEY` | control plane | the secret that keys the hash of staff email addresses and their sign-in codes (ADR-0127): at least 32 characters, for example `openssl rand -hex 32`. Needed only when `[mail]` is on: the control plane then exits at start, naming this variable, when it is missing or shorter. Never in the settings file. A different key finds none of the addresses set under the old one, so keep it with the database's backups. It is separate from the chat's key, and the hashes are labelled apart, so one is never valid as the other. Every install route makes one: `weavesignal.sh` as `CONTROL_EMAIL_KEY` in the settings file, the Quadlets as a Podman secret, the Cloudron package in its `secrets.env` ([Add staff and sign in](/docs/how-to/add-staff/#sign-in-with-an-emailed-code)) |
| `WEAVESIGNAL_CONTROL_TOKEN_KEY` | control plane | the secret that keys API tokens and webhook secrets (ADR-0195): at least 32 characters, for example `openssl rand -hex 32`. Every install route makes one (ADR-0202): `weavesignal.sh` and the Quadlets as `CONTROL_TOKEN_KEY` in the settings file, the Cloudron package in its `secrets.env`. Unset (a station installed before that), a key is derived from the station key (`WEAVESIGNAL_CONTROL_ADMIN_TOKEN`) and the control plane's log says to set one; with neither, tokens and webhooks are off. A different key revokes every token and ends every webhook's signatures, so rotating the station key does the same unless this is set. Never in the settings file; keep it with the database's backups. See [Integrations](/docs/reference/integrations/#the-key) |
| `RUST_LOG` | control plane, desk, chat | overrides the log filter |

## `control.toml`

| Section | Key | Default | Meaning |
|---|---|---|---|
| `[station]` | `id` | — | the station's identifier for its whole life (ADR-0047) |
| | `name` | — | the station's name |
| `[listen]` | `internal` | `127.0.0.1` | the address internal services bind |
| `[ports]` | thirteen keys | see [Ports](/docs/reference/ports/) | every listening port (ADR-0075) |
| `[room]` | `public_url` | `wss://station.example` | LiveKit as browsers reach it, through the proxy: an origin, to which LiveKit's client adds `/rtc` |
| | `api_key` | `weavesignal` | LiveKit's API key |
| | `api_url` | `http://127.0.0.1:7880` | LiveKit's server API on this machine, used to drop a caller |
| | `name` | `studio` | the room's name |
| | `desk_identity` | `desk` | the desk's identity prefix; nobody else may join under it |
| | `token_ttl_s` | `600` | how long a room token can be used to join, 1 s to 4 h (ADR-0085) |
| `[desk]` | `socket` | none | the desk's control socket; left out, the studio app shows the desk as not set up |
| | `poll_ms` | `250` | how often the desk is asked for its status, 50 to 5,000 |
| `[playout]` | `socket`, `listeners_socket` | none | playout's control socket, and the listener process's, through which the legal logger is turned on and off; left out, the logger cannot record |
| | `poll_ms` | `250` | how often playout is asked for its state, 50 to 5,000 |
| | `library_dir`, `library_lufs`, `library_max_seconds`, `library_max_upload_mb` | `/var/lib/weavesignal/library`, `-23`, `1800`, `200` | the music library's files (playout reads the same directory), their loudness target, the longest track and the largest upload |
| `[carts]` | `media_dir` | `/var/lib/weavesignal/carts` | the cart files the media worker writes (the desk's `carts.dir`) |
| | `loudness_lufs`, `ceiling_dbtp` | `-23`, `-1` | the carts' loudness target (−40 to −5) and the true peak no cart is raised above (−10 to 0) |
| | `max_upload_mb`, `max_seconds`, `worker_timeout_s` | `64`, `600`, `120` | the largest upload, the longest cart, and how long the media worker may take over one file |
| | `worker`, `demo_pack` | none | the media worker's program (left out, this one), and a cart pack loaded when the station has no banks ([Carts in the desk](/docs/reference/desk-carts/)) |
| `[retention]` | `recordings_dir`, `sweep_every_s` | `/var/lib/weavesignal/recordings`, `3600` | where recordings are kept (the sweeper deletes nothing outside it) and how often it runs, in seconds (60 or more: at most once a minute); the periods are the station's setting, in the database ([Set retention](/docs/how-to/set-retention/)) |
| `[studio]` | `show` | empty | the show on air, as the desk and the screener head it; the schedule does not set it ([Set the show](/docs/how-to/set-the-show/)) |
| | `rundown` | none | "coming up" rows; the schedule does not fill them (ADR-0102) |
| | `banned` | none | callers banned before; the chat's moderation log does not fill it (ADR-0102) |
| | `listener_delay_ms` | `15000` | how far behind the studio listeners hear, not counting the profanity delay, which the studio adds (ADR-0103) |
| `[publishing]` | `public_url` | empty | `https://` and the station's public host; every show card prints it without the scheme (ADR-0056), and the feeds, card images and episode files are reached under it; empty, cards and feeds are off |
| | `place` | empty | where the station is, printed on the cards and written as `podcast:location` |
| | `zone` | `Europe/London` | the station's time zone as first offered: a new station starts with it and an admin confirms or changes it in Settings (ADR-0189); the zone in force is in the database |
| | `language` | `en-gb` | the feeds' language |
| | `owner_name`, `owner_email` | empty | the feeds' owner; without an address neither `itunes:email` nor `podcast:locked` is written |
| | `copyright`, `category`, `subcategory` | empty, `Music`, empty | the feeds' copyright line and Apple category |
| | `locked` | `true` | `podcast:locked`: other platforms may not import the feeds |
| | `cards_dir` | `/var/lib/weavesignal/show-cards` | the rendered cards, served at `/cards/` |
| | `episodes_dir` | `/var/lib/weavesignal/episodes` | the published episode files, served at `/media/episodes/` |
| `[video]` (control plane) | `still_file`, `still_max_upload_mb`, `still_max_width`, `still_max_height` | `/var/lib/weavesignal/carts/stills/slate.png`, `24`, `1280`, `720` | where the control plane writes the slate's picture (the desk's `video.still_file`), the largest upload, and the size it is fitted inside ([The slate's picture](/docs/reference/desk-video/#the-slates-picture)) |
| `[video]` (control plane) | `demo_still` | none | a PNG that becomes the station's picture when it has none, once: taken away, it stays away. The Compose bundle sets the demo station's kettle (`/opt/weavesignal/stills-demo/kettle-fm.png`) through `DEMO_STILL` (ADR-0200) |
| `[join]` | `stream_url` | empty | the listener stream as the player reaches it, through the proxy; empty, the player says the stream is not set up |
| | `call_in` | `true` | whether the player offers call-in (ADR-0006) |
| | `consent_text`, `consent_contact` | the two-sentence words, empty | what a caller agrees to before calling, and where they ask for a call to be taken down: an email address or a link, at most 200 characters. Call-in stays closed while the words name `{contact}` and it is empty ([The consent words](/docs/reference/public-pages/#the-consent-words)) |
| | `call_pass_ttl_s`, `invite_ttl_s` | `14400`, `14400` | how long a caller's pass and an invite link last, 60 s to a day |
| | `calls_per_address_per_hour`, `calls_per_minute` | `6`, `30` | calls one address may place in an hour, and calls the whole station takes in a minute |
| | `forwarded_for` | `false` | take the caller's address from `X-Forwarded-For`'s last entry; set it only behind the station's own proxy, or every caller shares the proxy's address |
| | `hls_dir`, `hls_url` | `/var/lib/weavesignal/hls`, `/hls` | where the desk writes its HLS, and the address the player fetches it from (an edge or a CDN's, or the control plane's own `/hls/`) ([Video in the desk](/docs/reference/desk-video/#the-player)) |
| | `challenge_bits` | `18` | call-in's proof of work, in bits; `0` turns it off ([The public pages](/docs/reference/public-pages/#call-ins-challenge)) |
| | `challenge_bits_busy` | `21` | the difficulty while the station's minute of calls is at least half used |
| | `challenge_ttl_s` | `1800` | how long a challenge can be answered, 60 s to a day |
| `[web]` | `dir` | none | the public pages' build (`apps/web/dist`; `/opt/weavesignal/web` in the server image), served at `/` with their caching and CSP; left out, the API only |
| | `listed` | `false` | whether search engines may list the public address (the player, feeds and cards). Off, every response says `X-Robots-Tag: noindex, nofollow`, the pages carry a robots meta tag and `/robots.txt` disallows everything. On, only the API, the invite page and the echo test stay unlisted ([The public pages](/docs/reference/public-pages/#search-engines)) |
| `[steering]` | `ttl_s` | `300` | how long a player keeps its steering manifest, in seconds; the specification recommends 300 ([Content steering](/docs/reference/steering/)) |
| | `check_every_s`, `check_timeout_ms`, `down_after` | `5`, `2000`, `2` | each edge's health check, and how many failures in a row take it out of the order |
| | `clone_from` | `.` | the pathway the multivariant playlist declares; edges with a `host` are sent as its clones |
| | `edges` | none | the edge pool, `[[steering.edges]]` with `pathway`, `host`, `weight`, `check_url`; none, steering is off |
| `[mail]` | `mode`, `from`, `command`, `outbox_dir` | `off`, `weavesignal@station.example`, `["/usr/sbin/sendmail", "-t", "-i"]`, `/var/lib/weavesignal/staff-outbox` | how staff sign-in codes reach staff (ADR-0127): `command` pipes each email to a sendmail-compatible program (msmtp, `sendmail -t`), `outbox` writes each to a file in `outbox_dir` (development; the files hold the addresses and the codes), `off` leaves staff signing in with a password. Needs `WEAVESIGNAL_CONTROL_EMAIL_KEY` |
| `[api]` | `public_per_minute`, `token_per_minute`, `streams_per_address` | `120`, `600`, `4` | the public API's limits (ADR-0195): requests per minute from one address to the open routes, requests per minute with one API token, now-playing streams one address may hold open; `0` turns one off ([Integrations](/docs/reference/integrations/#limits)) |
| | `webhooks_allow_private` | `false` | let webhooks point at private and loopback addresses (an integration on the station's own network); off, a webhook must be `https` on a public address |
| `[log]` | `format` | `json` | `json` (one object per line) or `pretty` |
| | `filter` | `info` | the log filter |

A rundown row and a ban look like this:

```toml
[[studio.rundown]]
at = "14:00"            # HH:MM or HH:MM:SS, the station's time
title = "News"
detail = "hard post"
hard_post = true        # the clock back-times to the first hard post ahead
key = "F2"              # the cart key it needs, if any

[[studio.banned]]
identity = "tavish"
date = "2026-09-12"
reason = "Abuse on air"
```

## `desk.toml`

| Section | Key | Default | Meaning |
|---|---|---|---|
| `[livekit]` | `url`, `api_key`, `room` | — | the room, as the desk reaches it on the server |
| `[desk]` | `identity` | `desk` | the desk's identity prefix |
| | `jitter_buffer_ms` | `50` | the receive jitter buffer; LiveKit's 200 ms default doubles a guest's delay |
| | `mix_buffer_ms` | `20` | the mixing block: one pass computes every bus for a block this long; 10 or 20 (one Opus frame) |
| | `input_prebuffer_ms` | `50` | the longest the mix waits for a late voice before mixing without it, 20 to 500 |
| | `workers` | `0` | threads encoding the returns; 0 is one per core, at most four |
| | `realtime` | `true` | ask for real-time scheduling for the mix and the encoders, granted only where the host or container allows it (`CAP_SYS_NICE` or an `rtprio` limit) |
| | `return_opus_bitrate` | `128000` | each person's return, in bit/s |
| `[programme]` | `srt_bind`, `srt_port` | `127.0.0.1`, `7313` | where the desk listens for Liquidsoap; loopback only (ADR-0049, ADR-0086) |
| | `srt_latency_ms` | `20` | SRT's latency on loopback |
| | `opus_bitrate` | `96000` | the programme's Opus bitrate to playout |
| | `queue_ms` | `500` | the leaky queues before the encoder and the SRT sink, so a playout that is away never stalls the desk |
| `[programme.delay]` | `target_ms`, `quiet_db`, `rebuild_slowdown` | `8000`, `-50.0`, `0.02` | the profanity delay toward playout, at most 30 s (0 turns it off); after a dump it builds back up through pauses quieter than `quiet_db`, and a few per cent slow otherwise |
| `[programme.limiter]` | `ceiling_db`, `lookahead_ms`, `release_ms` | `-1.0`, `1.5`, `120.0` | the limiter on the programme and on every mix-minus (ADR-0087) |
| `[room]` | `api_url` | empty | LiveKit's server API; empty, `livekit.url` with `ws` turned into `http` |
| | `poll_ms` | `500` | how often the desk looks at the room |
| | `leave_grace_ms` | `30000` | a person who left keeps their mix-minus this long |
| | `link_timeout_ms` | `8000` | a subscription with no audio by then is made again (ADR-0089) |
| `[control]` | `socket` | none | the [control socket](/docs/reference/desk-control/), such as `/run/weavesignal/desk.sock`; left out, it is off |
| `[video]` | `enabled`, `encoder`, `rungs`, `nvenc_sessions`, `camera_codecs`, `lag_ms`, `stale_ms`, `lower_third_ms`, `station`, `still_file` | `false`, `auto`, `["480p20"]`, `12`, `VP8 AV1 VP9`, `100`, `1000`, `6000`, `WeaveSignal`, `/var/lib/weavesignal/carts/stills/slate.png` | the video path, off by default (ADR-0009); `still_file` is the slate's picture, which the desk watches ([The slate's picture](/docs/reference/desk-video/#the-slates-picture)); `[video.switch]` the vision mixer's rule, `[video.hls]` where and how the HLS is written, `[video.room]` the picture in the room ([Video in the desk](/docs/reference/desk-video/)) |
| `[video.playout_feed]` | `enabled`, `bind`, `port`, `cushion_ms`, `crossfade_ms`, `off_air_hold_ms`, `absent_ms` | `true`, `127.0.0.1`, `7317`, `200`, `300`, `3000`, `500` | playout's output back to the desk, the video's sound while nobody is live (ADR-0172); loopback only; the password is in `WEAVESIGNAL_PLAYOUT_FEED_PASSWORD` ([The sound](/docs/reference/desk-video/#the-sound)) |
| `[carts]` | `dir`, `ahead_ms`, `start_wait_ms` | `/var/lib/weavesignal/carts`, `1000`, `500` | the cart files the desk plays (the control plane's `carts.media_dir`), how far ahead each sounding cart is decoded, and how long a fire waits for its first audio ([Carts in the desk](/docs/reference/desk-carts/)) |
| `[inputs.<role>]` | `makeup_db` and five stages (`telephone`, `expander`, `compressor`, `leveller`, `limiter`) | presenter and DJ: their chain; caller: the telephone band (`low_hz` 300, `high_hz` 3400) and a compressor, while we run diagnostics (ADR-0194) | each role's input chain; an empty table turns it off |

The presenter's chain, in order:

| Stage | Default |
|---|---|
| `expander` (the gate) | 1:3 below −34 dBFS, never more than 18 dB down |
| `compressor` | 2.5:1 above −14 dBFS |
| `leveller` | steers toward −25 dBFS of speech, within −6 to +18 dB, 0.5 dB a step |
| `limiter` | −1 dBFS |

## On Cloudron

The package writes every service's settings itself on every start (`control.toml`, `desk.toml`, `chat.toml`, LiveKit's and playout's), from Cloudron's environment and from two files the administrator can open in the File Manager: `/app/data/weavesignal.env` (settings) and `/app/data/secrets.env` (secrets, made on the first start).
Edit those two, not the files the package writes.
See [Install on Cloudron](/docs/how-to/install-on-cloudron/#the-stations-files).
