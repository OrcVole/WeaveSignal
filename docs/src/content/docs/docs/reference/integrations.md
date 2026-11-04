---
title: "Integrations: tokens, open routes and events"
description: "How other programs use a station's API: scoped API tokens and the scope each route needs, the open read-only routes for a station's website, signed webhooks and the event stream, the limits on each, and the settings."
type: reference
status: draft
last_verified: 2026-11-04
sidebar:
  order: 9
---

`/api/v1` is WeaveSignal's public API (ADR-0195).
This page is what an integration needs: the three ways in, what each may reach, and how each is limited.
Every route, one by one, is in the [API reference](/docs/reference/api/); who may call and how a refusal looks are in [Control plane routes](/docs/reference/control-api/); [Connect an integration](/docs/how-to/connect-an-integration/) is the walk-through.

Within `/api/v1` changes are **additive only**: a route, a field or an event is not removed and does not change its meaning.
Removing or changing one is `/api/v2`, announced a release ahead, with both served for at least one release.
A program should ignore fields it does not know.

## The three ways in

| Way in | Credential | For | Reaches |
|---|---|---|---|
| Open routes | none | a station's own website | read-only: now playing, the coming week, published episodes, the station's public facts |
| API token | `Authorization: Bearer rsa_…` | a program run by the station: a stream deck, a scheduling script, a bot | the scopes the token was made with, and nothing an admin alone may do |
| Webhook | a secret per webhook, used to check what arrives | another system that wants to be told | five events, pushed to its address |

The station key (`WEAVESIGNAL_CONTROL_ADMIN_TOKEN`) and a staff session still work as before and are not for integrations: the key can do everything, including erasure and staff accounts.

## API tokens

An admin makes a token under **Settings → API tokens** in the studio app, or with the routes below.
A token has a name, one or more scopes and, optionally, an end date.

- **The secret** is `rsa_` and 64 hexadecimal digits, shown **once**, in the answer that makes the token.
  The station keeps only a keyed hash of it ([The key](#the-key)); a lost secret is made again, not read back.
- **A token is never an admin.**
  Wherever a route checks for an admin, a token is refused with 403, whatever scopes it holds.
- **A token passes only the routes in the table below, with the scope named.**
  A route that is not in the table is closed to tokens, so a route added later stays closed until it is placed.
  `:write` does not include `:read`.
- **Revoking** takes effect at once, in the running control plane, and for good.
  A revoked or expired token answers 401.
- **`last_used_ms`** is when a request last presented the token, written at most once a minute.
  A request refused for its scope still counts as a use.

| Route | Who | What |
|---|---|---|
| `GET /api/v1/tokens` | admin | every token, revoked and expired included, never a secret; and the scopes one can hold |
| `POST /api/v1/tokens` | admin | `name` (at most 80 characters), `scopes` (at least one), `expires_in_days` (1 to 3650, or left out for never). 201 with `token` and `secret` |
| `DELETE /api/v1/tokens/{id}` | admin | revoke: 204; 404 for a token the station does not have |

A station may have 100 live tokens.
A refused field is named in the answer: `{"error": "…", "field": "scopes"}`.

### Scopes

| Scope | Opens |
|---|---|
| `studio:read` | `GET /studio`, `GET /studio/events`: the studio's view, with callers' names and the topics they gave |
| `desk:control` | `POST /desk/commands`: on air, mute, cue, talk, dump the profanity delay |
| `screener:control` | `POST /screener/commands`; `POST /passes/end` |
| `chat:moderate` | `POST /chat/modes` on the control plane: slow mode, hold new, links, influx. Behind the station's proxy that path reaches the chat service first, which refuses a token (see below). The chat service's own routes are not reachable with a token |
| `carts:read` | `GET /carts` |
| `carts:fire` | `POST /carts/commands`: toggle, fire, stop, cue, stop last, fade all |
| `carts:write` | `PUT /carts/banks/{n}`; `PUT` and `DELETE /carts/banks/{bank}/slots/{slot}` |
| `schedule:read` | `GET /schedule/clocks`, `/schedule/week`, `/schedule/log`, `/schedule/episodes` |
| `schedule:write` | `PUT` and `DELETE` of clocks and log items; `PUT /schedule/week`; `POST /schedule/generate` |
| `playout:read` | `GET /playout`, `/playout/zone`, `/logger`, `/library/tracks` |
| `library:write` | `PUT /library/tracks` (an upload); `DELETE /library/tracks/{id}` |
| `reports:read` | `GET /reports/plays`, `/reports/audience` |
| `shows:read` | `GET /shows`, a show, a series, an episode's corrections |
| `shows:write` | changing shows, series and episodes; putting an episode on air; corrections; publishing files; drawing cards; `POST /cards/preview` and `/cards/words` |
| `video:read` | `GET /video/still`, `/video/still/image`, `/video/thumbs`; `GET /shows/{show}/still`, `/shows/{show}/still/image` |
| `video:write` | `PUT` and `DELETE /video/still`; `PUT` and `DELETE /shows/{show}/still` |

Paths are under `/api/v1`.
There is no `playout:control`: the desk's switches are `desk:control`, and the library is `library:write`.

**`chat:moderate` behind the proxy.** The station's proxy sends every `/api/v1/chat/…` request to the chat service, which passes `POST /api/v1/chat/modes` on to the control plane, but only for a staff session or the station key: an API token is refused there with 401 before it reaches the control plane. In WeaveSignal 1.0 a token with `chat:moderate` works only for a program that reaches the control plane directly, on its loopback port (`ports.control`, 7310), not through the station's address.

**Never reachable with a token**, so that a leaked token cannot do the worst things:
staff accounts and sessions; retention, holds, recordings, consents and erasure; the people list and its erasure; playout's settings and the station's time zone; room tokens (they mint a presenter) and invites; API tokens; webhooks.

## Open routes

No bearer, and `Access-Control-Allow-Origin: *` so a browser on any website can read them (preflight answered; `GET`, `HEAD` and `OPTIONS` only; no credentials).
They carry only what the station's public pages already show.
Nothing in them names a caller, a chat account, a presenter at the desk or a member of staff.
Someone [erased](/docs/how-to/set-retention/) at their request is not in them: their record is deleted and the frozen copy of what aired has their name taken out.
They follow the station's listing setting ([The public pages](/docs/reference/public-pages/)): unlisted, they say `X-Robots-Tag: noindex, nofollow`; listed, they say nothing and `robots.txt` allows them. The rest of `/api/` always says `noindex`.

| Route | Answers |
|---|---|
| `GET /api/v1/now-playing` | `on_air` (`desk`, `automation`, `fallback`, `silence`, `unknown`), `now` (`kind`, `title`, `artist`, `album`, `started_at_ms`), and `next` (two items; left out for a US statutory webcaster) |
| `GET /api/v1/now-playing/events` | the same as server-sent events: a `now_playing` event at once, then one per change; a keep-alive every 15 s |
| `GET /api/v1/public/station` | `station`, `show` (the title on now), `stream_url`, `live` (the show is on air), `call_in_open`, `video` (`live` and `audio` HLS addresses, or null), `zone` |
| `GET /api/v1/public/schedule?days=7` | the episodes airing from now for `days` (1 to 14), in air order: `show` (`slug`, `name`), `series`, `number`, `aired`, `title`, `description`, `kind`, `air_at` (with the station's offset), `air_at_ms`, `minutes`, `hosts`, `guests`, `tags`, `explicit`, `image`. Under `zone`, with `from_ms` and `to_ms` |
| `GET /api/v1/public/episodes?limit=20&before_ms=` | episodes that have aired and have published files, newest first (`limit` 1 to 50; `before_ms` pages back): the schedule's fields and `audio` and `audio_opus` (`url`, `mime`, `length`, `duration_ms`), `link`, `feed` |

An episode with an air time is in the schedule, but until it airs only as its slot: `title` and `description` are `null`, and `guests` and `tags` are empty, and `image` is the show's card. The station has no step that publishes an episode before it airs, so a season drafted from the rule, or a guest pencilled in, is not announced here; once it has aired, the schedule and the episode list show its words as they went out.
`image`, `audio`, `link` and `feed` are set when `publishing.public_url` is.
Caching: `public/station` says `max-age=5`, `public/schedule` and `public/episodes` `max-age=30`.

## Events out

An event is a JSON object: `id` (unique; the same on every retry), `type`, `at_ms` and `data`.

| Event | Fires | `data` |
|---|---|---|
| `now_playing.changed` | what listeners are told is playing changes | the now-playing document above |
| `on_air.changed` | the source on air changes | `on_air`, `previous`, `live` (the desk is on air) |
| `episode.started` | a scheduled episode's air time arrives | `show` (`slug`, `name`), `series`, `episode` (`id`, `number`, `title`, `air_at_ms`, `minutes`, `kind`) |
| `episode.ended` | its length has run out | as `episode.started` |
| `caller.queued` | a caller joins the screener's queue | `identity` (the call's, not the person's), `ordinal`, `queue` (callers waiting). Never their name, topic or consent: a caller agrees to be live and kept, not to have what they typed sent elsewhere; an integration that needs the name reads `GET /api/v1/studio` with a `studio:read` token |

`episode.started` and `episode.ended` follow the schedule: the air time, and the length the episode has, or its slot's, or an hour.
They fire for automation and for the desk alike, and are looked for every 15 s.
What was already airing when the control plane started is not announced again.

### Webhooks

| Route | Who | What |
|---|---|---|
| `GET /api/v1/webhooks` | admin | every webhook with its last delivery, and the events one can ask for |
| `POST /api/v1/webhooks` | admin | `name`, `url`, `events`. 201 with `webhook` and its `secret`, shown once |
| `PUT /api/v1/webhooks/{id}` | admin | any of `name`, `url`, `events`, `enabled` |
| `POST /api/v1/webhooks/{id}/secret` | admin | a new secret; the old one stops signing at once |
| `DELETE /api/v1/webhooks/{id}` | admin | remove it |

A station may have 20 webhooks.
Each shows `last_attempt_ms`, `last_success_ms`, `last_status` (the HTTP status the receiver answered with, 0 when it did not answer), `last_error` and `failures` (attempts failed since the last good one).

**A delivery** is a `POST` of the event with these headers:

| Header | Holds |
|---|---|
| `X-WeaveSignal-Timestamp` | when this attempt was sent, in Unix seconds (each retry has its own) |
| `X-WeaveSignal-Signature` | `sha256=` and the hexadecimal HMAC-SHA-256, keyed with the webhook's secret (the whole `whsec_…` text), of the timestamp, a full stop (`.`) and the exact body |
| `X-WeaveSignal-Event` | the event's `type` |
| `X-WeaveSignal-Delivery` | the event's `id` |

A receiver checks the signature and refuses a timestamp more than five minutes from its own clock, so a delivery seen in transit cannot be sent again later.
Any `2xx` answer is delivery.
Otherwise it is tried again after 5 s, 30 s, 2 min and 10 min, then given up.
Retries are kept in memory: a delivery waiting when the control plane restarts is lost (ADR-0201).
A receiver that needs every event reads the current state from the API after any gap in what it receives.
A receiver that does not answer holds up only its own deliveries: each webhook has 16 places for deliveries waiting, and one that finds them full is dropped for that webhook alone and counted in its `failures`.
A webhook removed or turned off is not tried again.
Deliveries are not ordered and may repeat; a receiver drops an `id` it has handled.
One slow receiver holds up only its own deliveries; at most 32 are in flight, and a webhook that cannot be served loses the event rather than slowing the station.
Redirects are not followed.

**Where a webhook may point.**
Its address must be `https` and must resolve to public addresses only.
Loopback, private, link-local, shared (carrier-grade NAT), unique-local, multicast, documentation and reserved ranges are refused when the webhook is made and again at every delivery, so a name that later changes its answer reaches nothing new.
`api.webhooks_allow_private = true` lifts this for an integration on the station's own network; `http` is then allowed to a loopback address too.

## Limits

| Limit | Default | Setting | Answers |
|---|---|---|---|
| open routes, per address, per minute | 120 | `api.public_per_minute` | 429 with `Retry-After` and the CORS header, so a page can read it |
| now-playing streams one address holds open | 4 | `api.streams_per_address` | 429 |
| a token's requests per minute | 600 | `api.token_per_minute` | 429 with `Retry-After` |

`0` turns a limit off.
An address is the last `X-Forwarded-For` entry the station's proxy added, counted only with `join.forwarded_for = true`; behind a proxy without it every connection is the proxy's and the per-address limits are off.
The Compose bundle's proxy replaces the header with the connection's own address.

## The key

API tokens are kept as an HMAC-SHA-256 of the secret under a key held outside the database, and a webhook's secret is derived under the same key from the webhook's id and a salt; so a copy of the database alone is neither a set of working tokens nor able to sign a delivery.

- `WEAVESIGNAL_CONTROL_TOKEN_KEY`, at least 32 characters (`openssl rand -hex 32`), is the key.
- Every install route makes it once and keeps it (ADR-0202): `weavesignal.sh` and the Quadlets write `CONTROL_TOKEN_KEY` into the station's settings file, and the Cloudron package keeps it in its `secrets.env`.
- Unset (a station installed before that, not yet run through its installer again), the key is derived from the station key, and the control plane's log says at start that one should be set.
- **Upgrading adds the key, which revokes the tokens and webhook signatures made under the derived one**: make them again after the upgrade.
- With neither, tokens and webhooks are off: making one answers 503 naming the variable, and any `rsa_` bearer is refused.
- **Changing the key revokes every token and ends every webhook's signatures.**
  Rotating the station key does the same when `WEAVESIGNAL_CONTROL_TOKEN_KEY` is not set; a station that rotates its station key and wants its tokens to survive sets `WEAVESIGNAL_CONTROL_TOKEN_KEY` first.
  Keep the key with the database's backups.
