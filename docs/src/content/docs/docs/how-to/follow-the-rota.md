---
title: "Follow the rota and the handover window"
description: "See who is down for each show of the coming twelve hours, where they present from and whether they are ready; set how long before a show's start the station names the next host and gives them one heads-up; and read what the window says when a host is late, running over or held."
type: howto
status: draft
last_verified: 2026-11-20
sidebar:
  order: 40
---

Two hosts follow each other: Heather presents Steeped in the room until two, and Fiona presents Kettle Drive from two, from her own studio with a show key ([Set up a host in their own studio](/docs/how-to/set-up-a-host-in-their-own-studio/)).
Nobody should find out at two that Fiona is not there.

From 1.2 the station keeps two things for this:

- **The rota**: every show of the coming twelve hours, who is down for it, where from, and whether they are ready.
- **The handover window**: the five minutes (by default) before each show's start, in which the station names the next host, says how far they have got, and gives them **one heads-up**.

Neither changes what goes on air.
The schedule still hands over at the show's start exactly as before: the grace, the late count, a hold and a lost link work as they did ([Set up a host in their own studio](/docs/how-to/set-up-a-host-in-their-own-studio/)).

## See them in the studio app

- **The rota** is the Schedule tab's fourth view: press `R` on the tab, or **Rota** beside Coming up on the desk ([Keymap](/docs/reference/keymap/#schedule-tab)). It lists the next twelve hours, with the show on air marked; `D` or `W` goes back to the calendar.
- **The window** is under the post line of the desk's clock (`desk.clock`): who is next, where they come from, and **READY**, **CONNECTING** or **NOT HERE YET**. When the show under way still waits for a late host and the next show's window has opened, both are said, the late one first, each by name (from 1.2.1 a presenter who never came to the room is named too, not only a host with a show key).
- **The heads-up** is a quiet note in the desk's toast slot, and the incoming host's own tile says they are on next until their post. On their monitor link it is a line above the lamp.
- **The window's length** is in Settings, Own studios, The rules: Off, 5, 10, 15 or 30 minutes.

Everything below is also in the API, for scripts.

## Read the rota

Any member of staff, or an API token with `sources:read`, reads it:

```sh
curl -sS https://station.example/api/v1/rota \
  -H "Authorization: Bearer rss_…"
```

Each row is one airing, in start order, the show on air now first (`"now": true`):

| Field | What it says |
|---|---|
| `show_name`, `start_ms`, `end_ms` | the show and its times |
| `hosts` | who is down for it: the names on its show keys, else the show's presenter; empty when nobody is |
| `from` | `room` (at the desk) or `push` (their own studio, with a show key) |
| `state` | `not_connected`, `connecting`, `ready` or `on_air` |
| `ready` | ready or on air |

A host in their own studio is `ready` once their software has sent steady sound for the rules' steady time (5 s by default); a presenter in the room is `ready` once they are at the desk, signed in as themselves.
The rota answers while the desk is down: everyone then reads `not_connected`, or `connecting` while the station holds their software's connection.
The full description is in the [API reference](/docs/reference/api/#get-apiv1rota).

## Set the window's length

The window is a rule for sources, beside the grace: `window_s`, in seconds.
300 (five minutes) by default, 0 turns it off, at most 1,800 (half an hour).

An admin changes it with the rest of the rules for sources:

```sh
curl -sS https://station.example/api/v1/settings/sources \
  -H "Authorization: Bearer rss_…" > rules.json
# change "window_s" in rules.json, then:
curl -sS -X PUT https://station.example/api/v1/settings/sources \
  -H "Authorization: Bearer rss_…" -H "Content-Type: application/json" \
  --data @rules.json
```

A value outside 0 to 1,800 is refused (422, naming `window_s`).
A body without `window_s`, from a script or a studio app older than 1.2, keeps the station's stored length.

## What the window says

The window is in `post.window` of the studio's view (`GET /api/v1/studio`) and of `GET /api/v1/sources`.
It is about the next show to take the air: while Heather is on air, it is Kettle Drive's.
If a show's start has come and its host never arrived, the window stays on that show until the next show's window opens, then moves on to it, so a host who did not come never hides the next one.

| `state` | When | What the desk is to say |
|---|---|---|
| `quiet` | before the window opens (`opens_ms`) | nothing yet |
| `announced` | in the window; Fiona has not connected, or a room presenter is not at the desk | *Next: Fiona, Kettle Drive at 14:00, not here yet* |
| `cueing` | in the window; Fiona's software is connected but not steady yet | *… connecting* |
| `ready` | in the window; Fiona is standing by (or the presenter is at the desk) | *… ready* |
| `overrun` | past the start, Heather still on air, no hold | *Heather is running over* |
| `held` | a producer holds this handover | *Held by Shona* |

`host` names who is down, `from` where from, and `heads_up_ms` when they were told.
A show that nobody is down for (no show key, no presenter) has no window.

## The heads-up

When the window opens, the station tells the incoming host once:

- the desk gets a notice, *Next: Fiona, Kettle Drive*, with how many minutes are left and how far she has got (`kind` `handover.heads_up`; its `subject` is her identity at the desk while she is there, so it can sit on her tile or seat);
- a host in their own studio has a line on their monitor link's view: *You are on next, at the start of Kettle Drive.* (`post.heads_up`, [Use your monitor link](/docs/how-to/use-your-monitor-link/)).

It is given **once an airing**.
The station writes it in the sources' log (`heads_up`) before it tells anyone, so a restart of the station does not tell Fiona twice.
A host already on air is not told; a producer's hold before the start does not stop it, because the host still has to get ready.

## A host not ready, a host running over

- **Not ready at the start.** The window has shown it for five minutes; at the start the post's own rule takes over unchanged: automation keeps the air and the late count runs once Fiona stands by, or Heather keeps the air for the grace ([Set up a host in their own studio](/docs/how-to/set-up-a-host-in-their-own-studio/)).
- **Running over.** Past the start with Heather still on air, the window says `overrun`. Nothing more happens because of it: the schedule hands over at the grace's end, or Heather hands over by a press ([Hand over to the next DJ](/docs/how-to/hand-over-to-the-next-dj/)).
- **Held.** A producer's **Hold** on the handover shows `held` and the name; the handover is then by a press only, as before.

## Turn it off

Set `window_s` to 0. The rota still answers; the window and the heads-up stop.

## See also

- [Set up a host in their own studio](/docs/how-to/set-up-a-host-in-their-own-studio/): the post, the grace, the late count and a lost link.
- [Hand over to the next DJ](/docs/how-to/hand-over-to-the-next-dj/): the overlap, by `⇧⏎`.
- [Use your monitor link](/docs/how-to/use-your-monitor-link/): what a host in their own studio sees.
