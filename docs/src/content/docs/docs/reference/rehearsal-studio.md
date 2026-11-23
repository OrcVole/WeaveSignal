---
title: "The rehearsal studio"
description: "The REHEARSAL setting on each install route, the control plane's [rehearsal] section, the desk's [desk] mode, the limits, what the rehearsal desk refuses, what it costs, and what a rehearsal pass buys."
type: reference
status: draft
last_verified: 2026-11-22
sidebar:
  order: 33
---

> **From 1.3.0.**

The rehearsal studio is a second desk, the same program in a mode that has no way to the air, mixing a room of its own for one show at a time.
How to use it: [Rehearse before your show](/docs/how-to/rehearse-before-your-show/).
Why it is built so, and what moves from it: [What moves to the live studio](/docs/explanation/what-moves-to-the-live-studio/).
Its routes and the studio app's store: [The rehearsal studio's data](/docs/reference/studio-rehearsal-data/).

## Turning it on

It is **off by default** on every install route.
Off, the studio app's rehearsal choice says so, `GET /api/v1/rehearsal` answers `enabled: false`, and every route that would open or drive it answers 503.

| Route | Setting | What it does |
|---|---|---|
| Compose (`weavesignal.sh`) | `REHEARSAL=true` in the settings file (`.env.example`) | adds `rehearsal.yaml`, a `desk-rehearsal` service, and writes `[rehearsal] enabled = true` for the control plane |
| Quadlets | the same line, then generate again | `generate.sh` adds `rehearsal.yaml` when the file says `REHEARSAL=true`, which gives one more unit |
| Cloudron | `REHEARSAL=true` in `/app/data/weavesignal.env`, then restart the app | starts the `desk-rehearsal` program beside the live desk; the start script writes its settings on every start, whether or not it is on |

`REHEARSAL` takes `true` or `false`; the script refuses anything else.
The setting opens **no port** on any route, so no firewall, router or Cloudron port setting changes ([Ports](/docs/reference/ports/)).
The rehearsal desk has no TCP listener; its only listener is its control socket (`/run/weavesignal/desk-rehearsal.sock`, beside the live desk's), and its WebRTC sockets are clients of LiveKit's own ports, as the live desk's are.

## What it costs

- **Memory and processor time of its own, while people rehearse.** With video on, a Cloudron app has less room beside it.
  The release's figures for the rehearsal desk at rest and with six people are not in [Resources](/docs/reference/resources/) yet, so none are claimed here.
- **It is the first process stopped under memory pressure.**
  It runs with an out-of-memory score of 500 (the live desk's is 0), and its supervisor starts it again.
  On Compose a container cannot be given less than its own session's score, so `weavesignal.sh` raises it to that when it is higher.
- **It asks for no real-time priority** and encodes with two workers, so it does not compete with the live desk for them.
- **Threads for each person**, as the live desk uses them, which matters where a host limits processes.

## The control plane: `[rehearsal]`

In `control.toml`; the installation writes `enabled` and `socket` from `REHEARSAL`.

| Key | Default | Meaning |
|---|---|---|
| `enabled` | `false` | the station has a rehearsal studio. Needs `socket` |
| `socket` | none | the rehearsal desk's control socket, `/run/weavesignal/desk-rehearsal.sock` |
| `desk_identity` | `desk-rehearsal` | the rehearsal desk's identity in its room; its returns are `<identity>-mm-<person>`. It may not be empty or the live desk's `room.desk_identity` |
| `room_suffix` | `-rehearsal` | what follows `room.name` to make the rehearsal room's: `studio-rehearsal`. A dash and lower-case letters, digits or dashes, at most 40, never containing `-booth-`, `-phone` or `-monitor-` |
| `max_people` | `6` | the most people in the rehearsal room at once, 1 to 20; a person over it is refused with a sentence that says the studio is full |
| `idle_end_s` | `1800` | an open session with nobody in it for this long ends by itself, 60 to 86,400 |

A bad value stops the control plane at start, and says which key.
The settings table: [Settings](/docs/reference/settings/#controltoml).

## The desk: `[desk] mode`

In `desk.toml`.

| Key | Default | Meaning |
|---|---|---|
| `mode` | `live` | `live`, or `rehearsal`. A rehearsal desk mixes only `<room>-rehearsal`, builds no programme output, and takes only the nine requests below |
| `identity` | `desk` | a rehearsal desk's must be `desk-rehearsal`, not the live desk's |

The install routes write the rehearsal desk's file themselves:

```toml
[livekit]
room = "studio-rehearsal"

[desk]
mode = "rehearsal"
identity = "desk-rehearsal"
realtime = false
workers = 2

[control]
socket = "/run/weavesignal/desk-rehearsal.sock"

[carts]
dir = "/var/lib/weavesignal/carts"
```

It carries no `[video]` and no `[sources]`: with `[video] enabled` or `[sources] enabled = true` a rehearsal desk **refuses to start** (exit 2), and says which.
A live desk, for its part, refuses a room whose name ends `-rehearsal`.
The desk's own settings: [Settings](/docs/reference/settings/#desktoml).

## What the rehearsal desk takes and refuses

Its control socket takes `status`, `on_air`, `mute`, `cue`, `talk`, `talk_to`, `cart_fire`, `cart_stop` and `cart_stop_all`.
Every other request, including any added in a later version, is refused before it is read further, with the code `rehearsal`: `dump`, `picture`, the programme's mode, RTMP, VBAN out and `cart_plays` among them.
So nothing played in rehearsal reaches the per-play log ([The desk's control socket](/docs/reference/desk-control/#the-rehearsal-desk)).

The desk's status says `desk_mode`, and each link drives only a desk that reports its own: a mismatch is reported and nothing is sent.

## The words

On a rehearsal desk `on_air` means *in the mix* of the rehearsal room: the returns are made from it, and no output carries it further.
The studio's view calls the field `on`, never `on_air` (ADR-0062).
The lamp says **REHEARSAL**, in a shape of its own; it is never red and never says **ON AIR** or **OPEN**.

## What a rehearsal invitation buys

- **A link for one session.** An invitation with `studio` = `rehearsal` and the show names the open session, and buys a rehearsal token only.
  Roles: a guest, a remote DJ or a presenter, or the account's own link.
  Refused (409) while no session is open for that show.
- **Only the rehearsal room.** The room is taken from what the pass is signed for, never from its role, and the person's return is `desk-rehearsal-mm-<identity>`.
  A rehearsal pass can never buy a token for the live room.
- **The rehearsed show's step.** At Solo only the show's presenter joins; the show on air's step does not count.
- **Nothing after the session.** When the session ends, by **End rehearsal**, by the idle end or by a move, the pass buys nothing.
  Opening the rehearsal again makes a new session and needs new invitations.
- **At most `max_people`** at once (409, *the rehearsal studio is full*).
- **Who may make one:** whoever may change the show's step (the show's presenter, a producer, an admin, the station key); an API token never.

`GET /api/v1/pass` says where a link leads (`studio`: `live` or `rehearsal`), and `not_now` says why a rehearsal link cannot join yet.

## Who may do what

| What | Who |
|---|---|
| See the rehearsal studio | staff |
| Open it, drive it, end it, move it, invite to it | whoever may change the show's step: the show's presenter, a producer, an admin or the station key; from a live or a preparing window |
| Book it | the same, for the show booked; booking is refused while the studio is off |
| Read the bookings | staff, and an API token with `schedule:read` |

## Where the data is kept

Migration `0070_rehearsal.sql` makes `rehearsal_session` (a session, its move and its opening state, so that a restart of the control plane keeps them) and `rehearsal_booking` (the bookings).
At most one session per station is open, by a unique index, so two people pressing **Open** at once cannot both have the studio.
Passes are signed and not stored, so a pass needs no table.
A session's row is kept after it ends, so the Rehearsal column of the schedule can show past sessions.

## See also

- [Rehearse before your show](/docs/how-to/rehearse-before-your-show/).
- [What moves to the live studio](/docs/explanation/what-moves-to-the-live-studio/).
- [The rehearsal studio's data](/docs/reference/studio-rehearsal-data/).
- [The API](/docs/reference/api/), under *rehearsal*.
