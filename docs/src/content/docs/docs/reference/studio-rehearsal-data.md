---
title: "The rehearsal studio's data"
description: "What the control plane says about the rehearsal studio and the schedule's studio columns, the bookings and their rules, and how the studio app holds it: the calls, the store fed by events, and the rules the screens read."
type: reference
status: draft
last_verified: 2026-11-22
sidebar:
  order: 32
---

> **From 1.3.0.** The rehearsal desk's screens and the schedule's Studios view are drawn from this.

The rehearsal studio is a second desk with no way to the air, for one show at a time.
The schedule shows it as a second column beside the live studio.
This page lists what the control plane gives for both, and how the studio app holds it.
Each route's full description is in the generated [API reference](/docs/reference/api/), under *rehearsal* and *schedule*.

Nothing in the rehearsal studio reaches the air.
So a person there is *in the mix* or *not in the mix*, never *on air* (ADR-0062).
The desk's own command is still called `on_air`.

## The rehearsal studio

| Route | Who | What |
|---|---|---|
| `GET /api/v1/rehearsal` | staff | The rehearsal view (below). A station older than 1.3.0 answers 404. |
| `GET /api/v1/rehearsal/events` | staff | The same view as server-sent events: one `state` event at once, then one for each change. |
| `POST /api/v1/rehearsal/open` `{show}` | whoever may change the show's step | 201 and the view; 200 when it was open for that show already. |
| `POST /api/v1/rehearsal/commands` | as Open, for the session's show | One switch or cart (below); answers with the view. |
| `POST /api/v1/rehearsal/end` | as Open | Everyone out of the rehearsal room; their links are retired. |
| `POST /api/v1/rehearsal/move` | as Open | Moves everyone to the live studio on their links, and closes the session ([The move](#the-move)). |

*Whoever may change the show's step* means the show's presenter, a producer, an admin or the station key.
An API token never may.

Open is refused with 409 in two cases:

- **Another show's session is open:** "Kirsty is rehearsing *The Morning Kettle*…".
- **A booking for another show holds the studio now:** "The rehearsal studio is booked for *Steeped* until 14:30 (Finlay)…".

Open is also refused with 403 to someone without rights over the show, and with 503 when the studio is off.

### The view

| Field | What |
|---|---|
| `enabled` | The station has a rehearsal studio (the installation's `REHEARSAL` setting). Off, the rest is empty. |
| `desk` | `linked`, and `error` when the rehearsal desk is not answering. |
| `session` | The open session or null: `id`, `show` (its slug), `show_name`, `opened_by` ("Eilidh") and `opened_at_ms`. |
| `people` | Each person at the rehearsal desk: `identity`, `name`, `role`, `on` (in the mix), `mute`, `cue`, `talk`, `talk_to`, `present`, `linked` and `lufs`. |
| `window` | The handover window of the session's show, when it has one, as the live desk's post line reads it, so the rehearsal says *On air at 14:05: move by then*. |
| `carts_sounding` | Each cart sounding at the rehearsal desk, with its `label`, `mode`, `cue`, `elapsed_ms`, `remaining_ms`, `loop` and `stopping`. |
| `max_people` | The most people the room takes (`rehearsal.max_people`, 6). |

### Commands

These are the only requests `POST /api/v1/rehearsal/commands` takes; any other `op` is 422:

- `{op: "on_air" | "mute" | "cue" | "talk", identity, on}`: `on_air` puts a person into the rehearsal's mix or takes them out.
- `{op: "talk_to", identity, to, on}`.
- `{op: "cart_fire", cart, cue?}`: the cart comes from the rehearsed show's banks or the station's.
- `{op: "cart_stop", cart}`.
- `{op: "cart_stop_all", cue?}`.

## Bookings

The rehearsal studio is first come, with optional bookings.
A booking holds the studio for its show from its start until its end.
While it does, Open is refused for any other show.

| Route | Who | What |
|---|---|---|
| `GET /api/v1/rehearsal/bookings?from_ms=&to_ms=` | staff; an API token with `schedule:read` | `{enabled, bookings}`: every booking that shares any time with the stretch (at most 62 days), in start order. |
| `POST /api/v1/rehearsal/bookings` `{show, start_ms, end_ms}` | whoever may change the show's step | 201 and the booking. |
| `PATCH /api/v1/rehearsal/bookings/{id}` `{show?, start_ms?, end_ms?}` | whoever may change the step of the booking's show, and of the show it goes to | 200 and the booking; what is left out stays. |
| `DELETE /api/v1/rehearsal/bookings/{id}` | whoever may change the step of the booking's show | 204. |

The rules for a booking:

- **Times.** A booking starts from now on, with a minute's slack for filling in the editor. It lasts at least five minutes and at most a day. Anything else is 422, with `field` naming `start_ms` or `end_ms`. A booking already under way keeps its start when it is changed.
- **Never sharing time.** A booking that would share time with another is 409, with `error` (the sentence) and `clash` (the booking in the way). A booking that starts as another ends does not share time. Two bookings made at the same moment cannot both win: each write locks the station's row while it checks.
- **Over is over.** A booking whose end has passed cannot be changed or deleted (409); it stays in the schedule as it was.
- **Off.** With the rehearsal studio off, no booking can be made or changed (503). Bookings made earlier are still listed and can be deleted.

## The schedule's studio columns

`GET /api/v1/schedule/episodes` gives three things more from 1.3.0:

| Field | What |
|---|---|
| `studios` | The Studios view's columns in order: `{id: "studio", name: "Studio"}`, then `{id: "rehearsal", name: "Rehearsal"}` when the station has a rehearsal studio. |
| each episode's `studio` | Always `studio`. Every show on air is in the Studio column, wherever its host is, because shows on air share one programme. The clash rule must never be keyed on the column. |
| each episode's `from` | Where the host comes from: `room` (at the desk) or `own` (their own studio, with a show key that has not ended). It is worked out as the rota works it out, from the show keys standing now. The rota's `push` is `own` here. |
| `rehearsals` | Every booking in the range, then every session (past or open), each in start order. Empty without a rehearsal studio. |

Each rehearsal block has the same shape, from the week or from the bookings' routes:

| Field | What |
|---|---|
| `id` | The booking's or the session's id. |
| `kind` | `booking` or `session`. |
| `show`, `show_name` | The show's slug and name. |
| `colour_slot` | The show's colour slot, 1 to 12. |
| `start_ms`, `end_ms` | A booking's times. For a session, its opening and its end, or now while it is open. |
| `by` | Who booked it, or who opened the session. |
| `open` | A session still open. Always false for a booking. |
| `ended` | How a session ended: `ended`, `moved` or `idle`. Null while it is open, and for a booking. |

The public calendar file (`/api/v1/public/schedule.ics`) never lists a rehearsal.

## In the studio app

The studio app holds this in three places under `apps/studio/src/lib/`.

### `api.ts`: the types and calls

The types mirror the schemas in `docs/openapi/weavesignal-v1.json`: `RehearsalView`, `RehearsalPerson`, `RehearsalSounding`, `RehearsalCommand`, `RehearsalBlock`, `StudioColumn` and `BookingList`.

The calls:

- `getRehearsal()`, `openRehearsal(show)`, `rehearsalCommand(c)`, `endRehearsal()`;
- `followRehearsal(onView, onLink)`, which stops for good on a 404;
- `listBookings(from, to)`, `makeBooking(b)`, `changeBooking(id, patch)`, `deleteBooking(id)`;
- `bookingClash(e)`: from a refusal, the booking in the way, or null.

Refusals are a `FieldError` carrying the station's sentence, its `status` and its `field`.
`followEvents(path, …)` is the event-stream loop that `follow` and `followRehearsal` share.

### `rehearsal.svelte.ts`: the store

`rehearsal` holds:

- `view`: the last view;
- `absent`: the station is older than 1.3.0;
- `link` and `linkWhy`: the event stream;
- `busy`: a press is on its way;
- `error` and `errorStatus`: the last refusal.

How to drive it:

- `startRehearsal()` once the window is signed in. It reads the view once, then follows the events. It returns its stop.
- `stopRehearsal()` on sign-out.
- `rehearsalPhase()` says where the studio stands.
- `openRehearsalFor(show)`, `sendRehearsal(c)` and `endRehearsalNow()` each resolve to `true`, or to `false` with `error` set.
- `chooseDeskStudio('live' | 'rehearsal')` sets `studio.deskStudio`. That is the Desk tab's choice (`desk.studio`): this window's own, never remembered.

A change of station closes the stream and empties the store, and the desk's studio goes back to live.
Start the store again on the new station.

### `rehearsal.ts`: the rules the screens read

These are pure functions:

- `phaseOf(view, absent)` gives one of these:
  - `unknown`, before the first view;
  - `absent`, a station older than 1.3.0;
  - `off`, turned off at the station;
  - `desk-down`, the rehearsal desk not answering;
  - `closed`, no session open;
  - `open`, a session open.
- `presentPeople`, `inTheMix` and `roomFull`.
- `openingPreview(view)`: who a move would put on at the handover (`on`) and who it would mute (`mute`), worked out as the control plane works it out.
- `rehearsing(view, show)` and `mixCommand(identity, on)`.
- For the Rehearsal column: `bookingsOf`, `sessionsOf`, `holdingBooking(blocks, now)`, `bookingClashAt(blocks, start, end, except)` (to warn before the 409) and `bookingChangeable(block, now)`.

`schedule.ts`'s `weekEpisodes` also gives `studios` (null from a station older than 1.3.0) and `rehearsals`; the Schedule tab's store keeps both as `sched.studios` and `sched.rehearsals` with each week it loads.
`studioOf(e)` and `fromOf(e)` read an episode's column and its host's origin, with an older station's defaults.
The Studios view's blocks, the column's clash rule and the booking editor belong to the Schedule tab (`schedule/model.ts`, `lib/schedule.svelte.ts`, `schedule/RehearsalBooking.svelte`).

### The studio app and the move

The move is the call `moveRehearsal()`, which the desk's move dialog sends, and the handover window's `rehearsing` flag is typed in `api.ts`.

## The move

`POST /api/v1/rehearsal/move` takes no body.
It moves everyone in the rehearsal studio to the live studio, on the links they already have, and closes the session ([What moves to the live studio](/docs/explanation/what-moves-to-the-live-studio/)).
In order:

1. The move is written on the session: who is moved, and the opening state, which is who is in the mix and who is muted among the people present.
2. Each person's page is sent a live pass on its `GET /api/v1/pass/events` stream (`move`), and joins the live room standing by.
3. The session ends, so its rehearsal passes buy nothing more.

It answers 200 with `MoveResult`:

| Field | What |
|---|---|
| `moved` | Everyone moved, by identity. |
| `opening` | The opening state: `on` and `mute` (sorted identities), the show's `host` among those moved, `airing_start_ms`, `standby_until_ms` (the airing's start plus the grace), `applied_at_ms` once applied and `cleared_at_ms` if it was cleared unapplied. |
| `airing_start_ms` | The airing they stand by for. |

It is refused with 409, naming the reason, when no session is open, when nobody is in the rehearsal studio, or when the show has no airing under way or within the next twelve hours.
It is 403 to someone who may not run the session's show, and 503 when the station has no rehearsal studio.
The opening is applied when the show's host is put on air at the live desk, by whatever route; the control plane looks every second, and at once after a press ([Rehearse before your show](/docs/how-to/rehearse-before-your-show/#5-at-the-handover)).

## Invitations and passes

- `POST /api/v1/invites` takes `studio` (`live`, the default, or `rehearsal`) and `show` (needed with `rehearsal`, refused without).
  A rehearsal invitation needs the session open for that show (409 otherwise) and the right to run its rehearsal (403).
  The invitation's `studio` says where the link leads.
- `GET /api/v1/pass` says the link's `studio`, and for a rehearsal link the rehearsed show; `not_now` says why it cannot join yet.
- `POST /api/v1/pass/token` for a rehearsal pass answers 409 while its session is not open, and 409 over `max_people`.
- `GET /api/v1/pass/events` sends the person a `move` (their new pass and link, the show and its airing) once the session has moved, and again to a page that reconnects until the airing ends.
- Every invitation to the live studio arrives standing by ([Roles](/docs/reference/roles/)).
