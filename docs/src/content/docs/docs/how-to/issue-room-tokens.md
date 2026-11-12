---
title: "Give someone a room token"
description: "Issue a short-lived token that lets one person join the live room with their role, from the command line or over the API."
type: howto
status: draft
last_verified: 2026-11-04
sidebar:
  order: 20
---

Everyone joins the live room with a token from the control plane.
The token names the room, the person and their [role](/docs/reference/roles/), and the desk routes their voice by that role.
A token can be used to join for the station's `room.token_ttl_s`: 10 minutes by default, 4 hours at most (ADR-0085).
Once someone has joined, the token's lifetime no longer matters.

Any member of staff signed in can issue tokens, and so can the station key; a `listener` token (a monitor) needs an admin ([Add staff and sign in](/docs/how-to/add-staff/)).
An API token cannot issue one: a room token can mint a presenter ([Integrations](/docs/reference/integrations/#scopes)).
The control plane needs LiveKit's API secret in `WEAVESIGNAL_LIVEKIT_API_SECRET`.

## From the command line

On the server, with the station's settings:

```sh
weavesignal-control --config control.toml room-token --identity eilidh --role presenter --name "Eilidh Fraser-MacLeod"
```

The command prints the token alone.
`--ttl-s` asks for a shorter lifetime; a longer one than `room.token_ttl_s` is cut to it.
`--config` defaults to `control.toml` in the current directory, or `WEAVESIGNAL_CONFIG`.
A `caller` token signed here carries no consent to record: the desk takes the caller, but the control plane will not send them to the desk or put them on air.

## Over the API

The route needs a staff session or the station key (`WEAVESIGNAL_CONTROL_ADMIN_TOKEN`) as its bearer ([Control plane routes](/docs/reference/control-api/#authorisation)).

```sh
curl -X POST https://station.example/api/v1/room/tokens \
  -H "Authorization: Bearer $WEAVESIGNAL_CONTROL_ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"identity": "finlay", "name": "Finlay Grant", "role": "dj"}'
```

The answer (201) carries the token, the room, the identity, the role, the lifetime granted (`ttl_s`), and the address browsers reach LiveKit at (`url`).
A name is at most 120 characters.
A `caller` token records the caller's consent to record on the station's behalf (`consent`), against the words the station shows now, and is for the caller's own booth.
See the [API reference](/docs/reference/api/#post-apiv1roomtokens) for every field.

## A publisher that is not a browser

A GStreamer `livekitwebrtcsink` that joins with permission to subscribe sends no media.
Ask for a publish-only token with `"subscribe": false` (ADR-0090), and listen to the return on a second connection with a second token.

## Someone who only listens

A monitor, a recorder or a test's ear takes the `listener` role, and only an admin can issue its token.
Its token can subscribe but not publish, and the desk leaves it out: no mix-minus, no slot, no cost at the desk.
Subscribe it to the one track it needs by name, such as `desk-mm-eilidh` for what Eilidh hears.

Each caller is in a booth room of their own ([Roles](/docs/reference/roles/#rooms)), so a monitor on a caller's return names the booth by the caller's identity: `"booth": "c7f3"` in the request.
Without `booth`, a monitor joins the studio room.

## Without handling a token

Guests, remote DJs and presenters follow an invitation link to the join page, and a listener calls in from the player page: in both the station issues the token for them ([The public pages](/docs/reference/public-pages/)).
Presenters and screeners sign in to the studio app as themselves ([Add staff and sign in](/docs/how-to/add-staff/)).
A staff member's role is not tied to the room role their token carries.

Verified on a development workstation · 2026-10-25: the command line, with a throwaway secret, for the `presenter`, `dj` and `listener` roles and an over-long `--ttl-s`; the API route by the control plane's own tests. Checked against the code · 2026-11-04.
