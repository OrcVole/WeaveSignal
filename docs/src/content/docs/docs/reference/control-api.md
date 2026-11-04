---
title: "Control plane routes"
description: "The control plane's HTTP API: who may call it, how a refusal looks, what the API promises, and where the generated route reference is."
type: reference
status: draft
last_verified: 2026-11-04
sidebar:
  order: 8
---

The control plane is an Axum service (`weavesignal-control serve`) listening on loopback at the `control` port, 7310 by default, behind the station's reverse proxy.
Bodies are JSON.

`/api/v1` is WeaveSignal's public API (ADR-0195): the studio app, the chat and any integration a station writes use the same routes.
This page is the narrative: who may call, how a refusal looks, what the API promises.
Each route (what it needs, takes and answers) is in the generated [API reference](/docs/reference/api/).

## Authorisation

The routes the [API reference](/docs/reference/api/) marks *Needs: nobody* take no bearer: the station, the description, sign-in, and the public routes (the player's facts, call-in and its challenge, now playing, `/api/v1/public/…`, the steering manifest).
Every other route needs one:

```text
Authorization: Bearer <a staff session, the station key, an API token or a pass>
```

- **A staff session** (ADR-0127): a member of staff signs in with `POST /api/v1/sessions` and sends the token it answers, which starts `rss_`.
  An account's role decides the rest: an `admin` reaches every route; any other role reaches every route not marked *admin only* in the [API reference](/docs/reference/api/).
- **The station key** (ADR-0085): `WEAVESIGNAL_CONTROL_ADMIN_TOKEN`, from the environment and never from the settings file.
  It is the station's own credential with an admin's powers; it makes the first admin account, and the chat reads the studio's view with it.
- **An API token** (ADR-0195): `rsa_…`, made by an admin for one program. It reaches only the routes its scopes open, and is never an admin ([Integrations](/docs/reference/integrations/#api-tokens)).
- **A pass** (ADR-0119): the join pages' own bearer, from an invite or a call; it opens only the `/api/v1/pass…` routes.

A missing, wrong or ended bearer answers 401 (an ended session says "Your sign-in has ended. Sign in again."); a role that may not answers 403, and so does a token without the scope.
With no station key set, staff sessions still open the routes, and so do API tokens when `WEAVESIGNAL_CONTROL_TOKEN_KEY` is set; a request with any other bearer, or none, answers 503.

Routes only an admin may use, besides the staff list: retention, holds, recordings, consents and erasure ([Set retention](/docs/how-to/set-retention/)); `GET /api/v1/people` and its erasure; `PUT /api/v1/playout/settings`; a room token with the `listener` role (a monitor hears every return, ADR-0118); API tokens and webhooks; and the chat's `/api/v1/chat/admin/…` routes.
`GET /api/v1/steering/edges` takes the station key only.

## The description and the reference

The API is described by an OpenAPI 3.1 document made from the handlers themselves, so it cannot drift from the routes.

- **`GET /api/v1/openapi.json`** serves it, with no bearer: the description is not secret.
  A tool that generates a client, or checks a request, reads it there.
- **The [API reference](/docs/reference/api/)** is made from the same document: every route with who may call it, its parameters, its body, its answers and the shapes they use.
  Routes only an admin may call say so, and carry `x-admin: true` in the document.
- **The repository commits the document** as `docs/openapi/weavesignal-v1.json`, and `just openapi-check` (part of `just ci`) fails when it differs from what the handlers generate, so a route cannot change without its description changing in the same commit.
  `just openapi` writes it.
- **The document's `info.version` is `1`**, the API's version, not the release's.
- **The chat service's routes are not in it.** The chat is its own process: the station's proxy sends everything under `/api/v1/chat/` to it, with a chatter's session as a bearer of its own and its own error body ([The chat service](/docs/reference/chat/#routes)). The one exception in the document is `POST /api/v1/chat/modes`: the control plane keeps the room's modes and serves that route, and the chat passes the screener's calls on to it with the caller's own bearer. The chat has no description of its own in WeaveSignal 1.0.

The document names the four bearers `staff_session`, `station_key`, `api_token` and `pass`; a route lists the ones it takes, and a route open to tokens names its scope.

## Stability

Within `/api/v1`, changes are additive only (ADR-0195): a new route, a new optional field, a new value where a field already allows several.
Removing a route or a field, or changing what one means, is `/api/v2`, announced a release ahead, with both served for at least one release.
A client should ignore fields it does not know, and treat an unknown `kind`, `op` or enum value as one it cannot act on rather than as an error.

## Errors

Every refusal is JSON with one plain sentence:

```json
{"error": "a valid bearer token is needed"}
```

A refused field adds its name, so a form can mark it:

```json
{"error": "A name is 1 to 80 characters.", "field": "name"}
```

| Status | Means |
|---|---|
| 401 | No bearer, a wrong one, a staff session that has ended (sign in again), or an API token that is unknown, revoked or expired. |
| 403 | The bearer may not do this: a staff session whose role is not `admin` on an admin-only route, or an API token without the scope the route needs, or on a route no token reaches. |
| 404 | There is no such thing. |
| 409 | It does not fit the current state: an aired episode is frozen, a hold is already released, the desk refused. |
| 410 | A pass has expired, or the studio has ended it. |
| 413 | An upload is larger than its limit. |
| 422 | Well formed, not acceptable: the sentence says why, and `field` names the field. |
| 429 | Too many: sign-in tries, calls, codes asked for; requests to the open routes from one address, or from one API token, with `Retry-After` ([Integrations](/docs/reference/integrations/#limits)). |
| 502 | LiveKit or the desk, behind the station, refused. |
| 503 | The part that answers is not set up or does not answer: the database, the room's secret, the desk, mail. |

A `curl` test of a refusal through the station domain on Cloudron sees a 302 to `/login` instead of a 401 when the request carries no bearer: Cloudron's login answers first.

## Staff and sign-in

An account is `{"id", "username", "name", "role", "disabled_ms", "created_ms", "has_email"}`; the password is kept only as an Argon2id hash and never answered, and `has_email` says whether an address is set for the emailed code (the address is kept only as a keyed hash and never answered).
Asking for a code answers alike whatever the username and address are, and the lookup and the sending follow the answer, so neither the words nor the time tell a stranger who has an account.
A code is six digits, good for ten minutes, five tries and one use; a new code, a new address, a new password and turning the account off each spend the codes asked for before.
An account may have three codes made in ten minutes; further asks are answered 202 and send nothing.
A refused sign-in answers 401 with the same sentence whether the username exists or not, after the same work; five failures for a username within 15 minutes, or twenty for an address, answer 429 until the window passes.
A refused field answers 422 with `{"error": "…", "field": …}`, the field one of `username`, `name`, `password`, `current`, `new`, `email` or `code`.
A session ends 12 hours after it was last used, and never more than 7 days after sign-in.
The live sessions are held in memory and in the database, so a restart keeps them.

How to use these from the studio app: [Add staff and sign in](/docs/how-to/add-staff/).

## Where the routes are

- Every route, generated: the [API reference](/docs/reference/api/).
- The playout layer (automation, the schedule, the logs, now playing): [Playout](/docs/reference/playout/).
- Content steering (`GET /api/v1/steering`, public; the edges' health, the station key only): [Content steering](/docs/reference/steering/).
- The desk's own control socket, which the control plane's desk and cart commands call: [Desk control](/docs/reference/desk-control/).
- Shows, series and episodes are under `/api/v1/shows`; `POST …/episodes/{episode}/corrections` corrects the public text of an aired episode (ADR-0177), and the log, which `GET` on the same path lists, names who made each correction (a username, `station key`, or `API token` and the token's name).
  A save whose card text holds characters the card fonts cannot draw answers 422 with the field named (ADR-0176).
- Routes with no `/api/v1` prefix: `GET /health`, `GET /hls/{path}` and `GET /feeds/{file}`, which the [API reference](/docs/reference/api/) has; and the static `/cards/…` and `/media/episodes/…`, served from the publishing directories with ranges, which it does not.
