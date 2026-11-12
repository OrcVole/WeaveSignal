---
title: "Settings"
description: "The control plane's and the desk's settings files, and the environment variables that carry their secrets."
type: reference
status: draft
last_verified: 2026-11-10
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
| `WEAVESIGNAL_CONTROL_EXTRA` | control plane | a second settings file the control plane adds to its own when it exists, holding only `[studio]` and `[steering]` (from 1.0.1): for installs whose `control.toml` is generated. The Cloudron package sets it to `/app/data/station.toml` ([Set the show](/docs/how-to/set-the-show/)) |
| `WEAVESIGNAL_CONTROL_TOKEN_KEY` | control plane | the secret that keys API tokens and webhook secrets (ADR-0195): at least 32 characters, for example `openssl rand -hex 32`. Every install route makes one (ADR-0202): `weavesignal.sh` and the Quadlets as `CONTROL_TOKEN_KEY` in the settings file, the Cloudron package in its `secrets.env`. Unset (a station installed before that), a key is derived from the station key (`WEAVESIGNAL_CONTROL_ADMIN_TOKEN`) and the control plane's log says to set one; with neither, tokens and webhooks are off. A different key revokes every token and ends every webhook's signatures, so rotating the station key does the same unless this is set. Never in the settings file; keep it with the database's backups. See [Integrations](/docs/reference/integrations/#the-key) |
| `WEAVESIGNAL_ICECAST_OUT_PASSWORD` | the listener process | the password of another Icecast server the programme is sent to (ADR-0226). Optional. Never in the playout data: the Compose bundle keeps it in `.env` as `ICECAST_OUT_PASSWORD`, the Quadlets in a Podman secret, the Cloudron package in `secrets.env`. See [The outgoing Icecast source](/docs/reference/playout/#the-outgoing-icecast-source) |
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
| `[publishing]` | `public_url` | empty | `https://` and the station's public host, where its public pages are served; every show card prints it without the scheme (ADR-0056), the feeds, card images and episode files are reached under it, and an invite's link points at its `/join/` page; empty, cards and feeds are off |
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
| | `stream_kbps`, `mp3` | `96`, none | the Opus stream's rate, which the player names (the listener process's `listeners.bitrate`, ADR-0233), and the MP3 streams the player offers beside it (ADR-0232): a list of `{ enabled, mount, kbps }`, each reached at `stream_url`'s address with its own mount; a stream that is off, or a mount that is not a mount name or repeats another, is left out. The install routes write both from `OPUS_BITRATE` and `COMPAT_*`, `COMPAT_HIGH_*` ([Playout](/docs/reference/playout/#the-mp3-streams)) |
| | `call_in` | `true` | whether the player offers call-in (ADR-0006) |
| | `consent_text`, `consent_contact` | the two-sentence words, empty | what a caller agrees to before calling, and where they ask for a call to be taken down: an email address or a link, at most 200 characters. Call-in stays closed while the words name `{contact}` and it is empty. While a show with a contact of its own is on air, the words give the show's first and this one as the host's (ADR-0214, ADR-0215; [The consent words](/docs/reference/public-pages/#the-consent-words)) |
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

### `[setup]`

From 1.0.1. The station's [setup page](/docs/how-to/set-up-the-station/) (ADR-0237) keeps what it stores in the database: the station's name and takedown contact, which then win over `station.name` and `join.consent_contact`, and [the look](/docs/reference/branding/). It needs no setting.
These two are for a package whose own proxy asks listeners for a password; the Cloudron package sets both and the Compose bundle neither.

| Key | Default | Meaning |
|---|---|---|
| `overrides_file` | unset | an absolute path. Set, the setup page offers the stream's password and writes its answer here (`LISTENERS_AUTH`, `LISTENERS_PASSWORD`), mode 600, for the package's start script to read at the next start |
| `stream_auth` | unset | whether the stream asks for a password as the station was started, for the page to show |

## `desk.toml`

| Section | Key | Default | Meaning |
|---|---|---|---|
| `[livekit]` | `url`, `api_key`, `room` | — | the room, as the desk reaches it on the server |
| `[desk]` | `identity` | `desk` | the desk's identity prefix |
| | `jitter_buffer_ms` | `50` | the receive jitter buffer; LiveKit's 200 ms default doubles a guest's delay |
| | `video_jitter_buffer_ms` | `300` | the cameras' own receive jitter buffer, at most 2000: a frame spans many packets, and at 50 ms a browser camera through a VPN lost 2.7 % of its packets as late; the picture can run this far behind the voices |
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
| `[video]` (rtmp-out) | `canvas` | empty | the picture composed; empty is the first HLS rung; `720p30`, `720p25` or `1080p30` for [RTMP out](/docs/reference/rtmp-out/), HLS scaled down from it |
| `[video.rtmp]` (rtmp-out) | `rung`, `keyframe_s`, `retry_min_s`, `retry_max_s`, `backlog_ms`; `[video.rtmp.compatible]` `h264`, `h264_profile`, `video_kbps`, `aac_kbps`, `aac_rate`; `[video.rtmp.open]` `video_kbps`, `opus_kbps` | empty, `2`, `2`, `60`, `4000`; `auto`, `high`, `0`, `128`, `44100`; `0`, `128` | RTMP out's picture and encoders (ADR-0230); the destinations and their keys are in the control plane's database, set in the studio app ([RTMP out](/docs/reference/rtmp-out/#settings)) |
| `[carts]` | `dir`, `ahead_ms`, `start_wait_ms` | `/var/lib/weavesignal/carts`, `1000`, `500` | the cart files the desk plays (the control plane's `carts.media_dir`), how far ahead each sounding cart is decoded, and how long a fire waits for its first audio ([Carts in the desk](/docs/reference/desk-carts/)) |
| `[inputs.<role>]` | `makeup_db` and five stages (`telephone`, `expander`, `compressor`, `leveller`, `limiter`) | presenter and DJ: their chain; caller: the telephone band (`low_hz` 300, `high_hz` 3400) and a compressor, while we run diagnostics (ADR-0194) | each role's input chain; an empty table turns it off |

The presenter's chain, in order:

| Stage | Default |
|---|---|
| `expander` (the gate) | 1:3 below −34 dBFS, never more than 18 dB down |
| `compressor` | 2.5:1 above −14 dBFS |
| `leveller` | steers toward −25 dBFS of speech, within −6 to +18 dB, 0.5 dB a step |
| `limiter` | −1 dBFS |

## Chat avatars

Whether chatters may upload a picture of their own, and where the pictures are kept (ADR-0235; the rest of `[avatars]` is in [The chat service](/docs/reference/chat/#avatars)).
`AVATAR_UPLOADS` is the default: once an admin uses the switch in the studio's Settings > Chat pictures (ADR-0259), the switch decides and survives restarts.
A picture refused as suspected child sexual abuse material is kept at least a year (`avatars.evidence_days`, 366 days or more), with the upload as it arrived if it was still waiting (an approved picture keeps only its clean copy), and only the one staff member named in Settings > Chat pictures can see it (the first admin until you set it): [the runbook](/docs/how-to/handle-suspected-child-abuse-images/) says what to do.
The pictures are small files, not in the database: back them up with it. Without them, chatters with an approved picture show the avatar they chose from the set instead.

| Install route | Uploads on or off | Where the pictures are |
|---|---|---|
| `chat.toml`, by hand | `avatars.uploads` | `avatars.dir` (`/var/lib/weavesignal/chat-avatars`) |
| `weavesignal.sh` and the Compose bundle | `AVATAR_UPLOADS` in the settings file | the `chat-avatars` volume |
| The Quadlets | `AVATAR_UPLOADS`, then generate again | the `<project>_chat-avatars` volume |
| Cloudron | `AVATAR_UPLOADS` in `/app/data/weavesignal.env` | `AVATARS_DIR`, `/app/data/chat-avatars`, inside Cloudron's backups; a place outside `/app/data` is refused, as the rest of the app is read-only |

## Chat pictures and documents

Whether trusted chatters may post a picture or a document in the chat, and where they are kept (ADR-0262; the rest of `[attachments]` is in [The chat service](/docs/reference/chat/#pictures-and-documents)).
It is off unless you turn it on. `CHAT_ATTACHMENTS` is the default: once an admin uses the switch in the studio's Settings > Chat pictures, the switch decides and survives restarts.
Every picture and document waits until staff or a moderator approve it, so your staff will see every one posted; add a line to your risk assessment before turning it on.
Documents are passed on unchanged and can carry macros or scripts; the station does not scan them.
They are files, not in the database: back them up with it.

| Install route | On or off | Where they are |
|---|---|---|
| `chat.toml`, by hand | `attachments.uploads` | `attachments.dir` (`/var/lib/weavesignal/chat-attachments`) |
| `weavesignal.sh` and the Compose bundle | `CHAT_ATTACHMENTS` in the settings file | the `chat-attachments` volume |
| The Quadlets | `CHAT_ATTACHMENTS`, then generate again | the `<project>_chat-attachments` volume |
| Cloudron | `CHAT_ATTACHMENTS` in `/app/data/weavesignal.env` | `ATTACHMENTS_DIR`, `/app/data/chat-attachments`, inside Cloudron's backups; a place outside `/app/data` is refused |

## On Cloudron

The package writes every service's settings itself on every start (`control.toml`, `desk.toml`, `chat.toml`, LiveKit's and playout's), from Cloudron's environment and from two files the administrator can open in the File Manager: `/app/data/weavesignal.env` (settings) and `/app/data/secrets.env` (secrets, made on the first start).
Edit those two, not the files the package writes.
See [Install on Cloudron](/docs/how-to/install-on-cloudron/#the-stations-files).

From 1.0.1 a third file may appear there, `/app/data/setup.env`: the station's setup page writes the stream password's answer to it. The start script reads `LISTENERS_AUTH` and `LISTENERS_PASSWORD` from it over the two files above, checks each, and never runs it; delete the file to go back to them.
