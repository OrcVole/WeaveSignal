---
title: "Calendar files: import and export"
description: "The station's schedule and each show's as iCalendar (.ics) files a person can subscribe to, and a calendar file read into new shows: Subscribe… and Import a calendar… in the studio, the routes, what each carries, how a calendar's rule becomes a show's rule, and what is refused."
type: reference
status: draft
last_verified: 2026-11-15
sidebar:
  order: 9
---

From WeaveSignal 1.0.5 the schedule goes out as a calendar file, and a calendar file comes in as new shows (ADR-0281).
A show's [rule](/docs/reference/show-rules/) is already a small part of iCalendar (RFC 5545), so both ways are a translation, not a second model.
The control plane does all of it; a client sends a file and draws what comes back.
Every route here is in the generated [API reference](/docs/reference/api/).

## In the studio

Both are on the Schedule tab's toolbar ([`calendar.toolbar`](/docs/reference/surfaces/calendar/#calendartoolbar)): two buttons in a wide window, and the two entries of the **⋮** menu beside **New show** in a narrower one.
[Schedule the week](/docs/how-to/schedule-the-week/#give-out-the-schedule-as-a-calendar) has the steps.

### Subscribe…

**Subscribe…** shows the link to the schedule's calendar file, and **Subscribe…** beside **When it airs** in a show's **Details** shows that show's.
Anyone signed in to the studio may open it.

| Part | What it is |
|---|---|
| **Calendar link** | `webcal://station.example/api/v1/public/schedule.ics`: the one to give out. Apple's Calendar and Outlook open it and offer to subscribe. **Copy link** |
| **Web address** | the same file as `https://…`, to paste where a calendar asks for an address (Google Calendar's *From URL*), and for where a `webcal` link does nothing, as on many Android phones. **Copy** |
| **Download .ics** | saves the file: a copy as the schedule is now, which does not follow changes |

The link is made from the station's **public address** (`publishing.public_url`), never from the address the studio talks to.
On Cloudron the studio's own domain is behind Cloudron's login, and a calendar sent there would be given a login page; the public domain is outside it.
The studio learns the address from the week's reply (`public_url` in `GET /api/v1/schedule/episodes`).

A station with **no public address** shows no link: the dialog says so, and that an admin sets `publishing.public_url` in the station's settings file and restarts the station.
**Download .ics** still works there, because the studio reads the file through its own sign-in.

In the studio app, **Download .ics** saves into your Downloads folder and says where: `schedule.ics`, or `the-late-kettle.ics` for a show, and `schedule (2).ics` if that name is taken (a file there is never written over).
In a browser it is an ordinary download.

### Import a calendar…

**Import a calendar…** is offered to an admin and a producer.
It is a dialog in three parts.

1. **The file**: an `.ics` of up to 1 MB and 200 events, given in one of three ways (below). A larger file, or one that is not text in UTF-8, is refused before it is sent; one the station cannot read is refused in the station's sentence, with the file's name.
2. **The shows**: one row for each show the file proposes, in the file's order, with what [the preview](#the-previews-reply) says of it. **Make 4 shows** makes the ticked ones.
3. **Done**: the shows made, each with its path, its rule in short and its next date, **Open** on each, and which of them overlap other shows.

The three ways to give the file:

| Way | What it does |
|---|---|
| **Choose a file…** | the system's file chooser |
| drop the file | anywhere on the dialog, in its first part or over the listed shows (as **Choose another file…**). The dialog shows where it will land. Of several files dropped, the first whose name ends `.ics` is read; a file with another ending is said and not read |
| **Or paste the file's path** | in the studio app only: a field for the file's path and **Read the file** (`⏎` in the field does the same). The path may be as a file manager copies it or as **Download .ics** said it: quotes around it, `~` for your home folder, *Saved as* before it and a `file://` address are all read. It must be a whole path |

A page in a browser cannot read a path, so the browser build has no field for one.
In the studio app the path is read by the app's own side, which reads one kind of file and no other: a regular file whose name ends `.ics` (where a link leads, that file's name too), of at most 1 MB, that is text in UTF-8 and begins `BEGIN:VCALENDAR`.
Anything else is refused in a sentence that holds nothing of the file, so the field cannot be used to look at another file.

Each row of the second part has:

| Part | What it is |
|---|---|
| the tick | whether to make it. Everything that can be made starts ticked, except a show the station already has; **Tick all** and **Untick all** are above the list, and **Tick all** ticks what is new to the station |
| the name | the calendar's, to change if you like. Two ticked shows may share a name: both are made, their paths told apart by a number, and the second says so |
| **EXACT**, **CHOSEN DATES** or **NOT A SHOW** | how it was read (`mapping`), with the reason under it for the last two |
| the sentence | the rule in words and its last date, as New show says them |
| **CONFLICT** | when its dates clash with a show the station has, in the sentence New show uses: *Every date overlaps The Late Kettle (Tue, 21:00–23:00) by an hour.* A second line names the other ticked shows of the same file it overlaps |
| **ALREADY ON THE STATION** | when the station has a show of the same name with the same dates and times, as when its own calendar file is read back. It says which show (*It is there as Kettle Drive*), starts unticked so that it is not made twice, and is not a clash with itself; a clash with another show is still said. Ticked on its own row, it is made as a second show, and the row says the two will clash |
| the notes | how many of the file's events went into it, a zone converted, dates left out |
| the dates | its next four, with CONFLICT, MOVED and the clocks changing on the rows |

A row that cannot be a show has no tick and no name to type.
Nothing is made until **Make the shows**, and they are made together or not at all: when the station refuses one (a name that does not fit the card), its sentence is put under that row and nothing is made.
A clash never stops the import.
When every show in the file is already on the station, the dialog says there is nothing to make, and **Make the shows** waits for a tick.
`⏎` in a name does not make the shows.

## Export

| Route | What |
|---|---|
| `GET /api/v1/public/schedule.ics` | every show with a rule, one event each |
| `GET /api/v1/public/shows/{show}.ics` | one show (`/api/v1/public/shows/the-late-kettle.ics`) |

Both are open routes, as the public schedule is ([Integrations](/docs/reference/integrations/#open-routes)): no bearer, CORS open to any origin, limited per address, and cached for 30 seconds.
They answer `text/calendar; charset=utf-8`.
A calendar app can **subscribe** to either address (with `webcal://` in place of `https://`) and looks again every hour; the file can also be downloaded and opened once.

On a Cloudron station the two routes are on the public domain, outside Cloudron's login.
On the Compose bundle with a public name they are on that name, as everything else is.

### What a show's event carries

One event per show, for its current series' rule (that series' own start date and planned episodes win, as on the Schedule tab).
The calendar works the dates out itself, so a subscribed calendar moves when the rule changes.

| The rule | In the file |
|---|---|
| `starts_on` and `start` | `DTSTART`, with `TZID` the station's zone: the first date the rule gives |
| `minutes` | `DTEND`, that many minutes later in elapsed time |
| `weekly` | `RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,FR` |
| `monthly_nth` | `RRULE:FREQ=MONTHLY;BYDAY=1TU` (or `-1FR` for the last) |
| `monthly_day` | `RRULE:FREQ=MONTHLY;BYMONTHDAY=15` (or `-1`) |
| `yearly` | `RRULE:FREQ=YEARLY;BYMONTH=12;BYMONTHDAY=31` |
| `dates` | `RDATE`, the dates after the first |
| `skip` | `EXDATE` |
| each of `moves` | a second event with the same `UID`, its `RECURRENCE-ID` the date's own time and its `DTSTART` the new one |
| `end`: `until` | `UNTIL`, the start of the last date, in UTC |
| `end`: `count` | `COUNT`, with the skipped dates inside the run added |

A show counts **episodes**, and a skipped date does not use one up; iCalendar counts dates and takes the skipped ones away afterwards.
So a show of 6 episodes with one date skipped is written `COUNT=7`, and the calendar shows the same six dates.

The file also carries the station's zone as a `VTIMEZONE`, so the clocks changing come out as the station's wall clock says.
It is made from the time zone database the control plane carries: the zone's yearly rule where it has one (the last Sunday of March, the second Sunday of March, the first Sunday on or after 2 April), and each change of offset before it.
A zone with no yearly rule is written change by change for ten years ahead.

A weekly slot from before 1.0.4 has no start date; its event begins at the show's first episode.
A show with no rule has no event, and its own file is a calendar with no events.

### Public words only

The event carries the show's **name** (`SUMMARY`) and its **description** (`DESCRIPTION`).
Nothing else: no episode title, guest, host or presenter.
Its page (`URL`) is left out for now: a show's card and feed name `https://station.example/the-late-kettle`, but no page answers there yet.
A drafted episode is not something the station has announced, as on the public schedule.

Each show's `UID` is `{show id}@weavesignal` and stays the same for the show's life, so a calendar keeps one event per show across refreshes.

## Import

Importing makes **new** shows only.
It is two steps: read the file and propose shows, then make the ones chosen.
An admin or a producer may import; a presenter or a screener may not (403).
An API token may read a file with `shows:read` and make the shows with `shows:write`, as it may make one show.

### Reading a file

`POST /api/v1/schedule/import/preview` takes the file itself (`Content-Type: text/calendar`), or JSON:

```json
{ "ics": "BEGIN:VCALENDAR\r\n…", "limit": 5 }
```

It writes nothing.
The file may be up to **1 MB** and **200 events** (413 above either).
A file that is not a calendar is refused with one sentence (422, `field` `ics`).

It proposes **one show per recurring event** (with its changed and cancelled dates), and **one per run of single events with the same name**, on chosen dates.
Cancelled events are left out, and the reply's `notes` say how many.

Each item says how it was read, in `mapping`:

| `mapping` | Meaning |
|---|---|
| `exact` | The rule gives exactly the calendar's dates, times and lengths. |
| `dates` | The calendar's rule is not one a show can have. These are its dates for the next twelve months (from its first, if that is later), as chosen dates; `reason` says why. |
| `refused` | It cannot be a show; `reason` says why, and there is no `rule`. |

An `exact` mapping is judged, not guessed: the calendar's own dates are worked out from its `RRULE`, `RDATE`, `EXDATE` and changed dates for two years, and the proposed rule must give the same instants.

What goes to `dates`: a rule every 2 days or more, the fifth weekday of a month, several days of the month, a yearly rule on a weekday, a rule every 13 weeks or more, a start the rule itself would not give, or an `RRULE` with `RDATE` beside it.
A rule `FREQ=DAILY` every day is `exact`: a show's rule says *every day* as all seven weekdays.
Chosen dates are at most 120; past that, the first 120 are kept and a note says to which date.

What is refused: an all-day event, an event longer than a day, more than one time a day (`FREQ=HOURLY`, several `BYHOUR`), a time zone that is not known, an `RRULE` that cannot be read, and an event whose dates have all passed.

A run of single events with the same name becomes one show on chosen dates (once, if only one is to come).
Past dates are left out, with a note.
Its rule takes the most usual time and length; a date at another time or of another length is a moved date.
Two on one day cannot both air: only the first is kept, with a note.

### Time zones

A time is read in the zone its `TZID` names: an IANA name (`Europe/London`), a path ending in one (`/mozilla.org/…/Europe/London`), or a Windows name Outlook writes (`GMT Standard Time`).
A time with no zone is the station's clock.
A time in another zone is converted to the station's clock, and the item says so in `zone` and in its `notes`.

When the other zone changes its clocks on the same dates as the station's (Paris and London), the converted rule is still `exact`.
When it does not (New York and London), the time on the station's clock is not the same all year: the item is `dates`, flagged `clock_differs`, and the weeks at the other time are moved dates.

### The preview's reply

| Field | What |
|---|---|
| `zone` | The station's zone: every time is on its clock. |
| `events` | The events read. |
| `notes` | About the file as a whole. |
| `ready` | How many items can be made. |
| `items` | In the file's order. |

Each item has `name`, `description`, `mapping`, `rule` (the [rule's](/docs/reference/show-rules/#the-rule) own shape), `reason`, `notes`, `zone`, `clock_differs`, `events` (how many of the file's events went into it), `uid` and `source_rule` (the `RRULE` as written).
And, as the [rule preview](/docs/reference/show-rules/#the-preview) gives them for a new show: `words` (the sentence, the short form and the last date), `dates` (the next `limit` dates that air, 1 to 20, five by default, each with the other shows' times it overlaps), `more` and `clash_count`.
`slug` is the path the show would take if made now with that name.
`overlaps` lists the other items in the same file whose dates overlap this one's, by their place in `items` (from 0).

`already` is the show the station already has that the item is, as `{ "slug", "name" }`, or `null`.
An item is that show when the names are the same with case and spaces folded (*kettle  drive* is *Kettle Drive*) and both rules give the same dates still to come, at the same times and lengths, for two years from the first of them.
The rules are compared by what they give, not as written: the station's own calendar file starts each rule at the first date its repeat gives, which is often not the date the show's rule holds.
Such an item does not clash with itself: the show's own times are in neither its dates' `clashes` nor its `clash_count`. Other shows' times still are.
The route proposes it all the same; leaving it out is the caller's choice, and the studio leaves it unticked.

### Making the shows

`POST /api/v1/schedule/import` takes the chosen items, each as the preview gave it or renamed:

```json
{ "items": [ { "name": "The Late Ceilidh", "description": "Reels, jigs and requests.", "rule": { "…": "…" } } ] }
```

It makes them all in **one transaction**, or none:

- each a new show at step **Solo**, with **no episode drafted** and no host;
- each path made from its name and made free against the station's shows and the others in the same import (`folk-night`, then `folk-night-2`), as the studio makes a new show's (so it can differ from the preview's `slug`, which did not know the names chosen);
- in the next colours of the palette;
- each show's cards drawn first, so a name that cannot fit refuses the import (422, `field` `items[n].name`);
- a rule that cannot stand refuses it too (422, `field` `items[n].rule`, the show named).

It answers 201 with the shows made (`made`, each as the show routes give a show) and `warnings`: for each show whose dates overlap another show's time, those clashes (the first 60 dates, in the twelve months from its first).
An overlap never refuses an import.
The first episode is drafted later, as for any show.

## Not done here

- Two-way sync, CalDAV, or importing from an address rather than a file.
- Importing into an existing show's rule: an import only makes new shows.
- Exporting the station's private schedule (drafted titles, guests) for staff.
