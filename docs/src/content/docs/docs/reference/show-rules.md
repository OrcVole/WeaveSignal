---
title: "When a show airs: the rule"
description: "A show's recurrence rule as the control plane stores and expands it: once, weekly, monthly, yearly or chosen dates, with skipped and moved dates and an end; how it replaces the weekly slot; which date an episode fills; what a changed rule does to the episodes; and how a clash is judged."
type: reference
status: draft
last_verified: 2026-11-13
sidebar:
  order: 9
---

From WeaveSignal 1.0.4 a show (and a series) airs by a **rule**, which replaces the weekly slot.
A rule only makes dates; episodes are what air.
The control plane works the dates out, says the rule in words and finds the clashes; a client draws what it answers.
Every route named here is in the generated [API reference](/docs/reference/api/).

## The rule

A rule is a small, typed subset of iCalendar: one start time and one length a day, a repeat, an end, dates skipped and dates moved.
All dates and times are the station's wall clock in its one time zone.

```json
{
  "starts_on": "2026-12-01",
  "start": 1200,
  "minutes": 60,
  "repeat": { "kind": "monthly_nth", "every": 1, "nth": 1, "weekday": 2 },
  "end": { "kind": "count", "count": 10 },
  "skip": ["2027-01-05"],
  "moves": [{ "from": "2027-04-06", "to": "2027-04-13T20:00:00", "minutes": null }]
}
```

| Field | Meaning |
|---|---|
| `starts_on` | The first date the rule may give. Left out only for a weekly rule of every week, which is what a weekly slot was. |
| `start` | Minutes after midnight. |
| `minutes` | The length in minutes, 1 to 1,440; left out, the studio's 60. |
| `repeat` | One of the kinds below. |
| `end` | `never` (the default), `until` a date (inclusive), or `count` of episodes. |
| `skip` | Dates the rule gives that do not air. |
| `moves` | A date aired at another time (and, if given, another length). It still fills the date it was moved from. |

| `repeat.kind` | Fields | Dates |
|---|---|---|
| `once` | none | `starts_on` alone |
| `weekly` | `every` (1 to 12), `weekdays` (1 Monday to 7 Sunday) | those weekdays every `every` weeks; weeks start on Monday and week zero holds `starts_on` |
| `monthly_nth` | `every`, `nth` (1 to 4, or -1 for the last), `weekday` | the first Tuesday of every month |
| `monthly_day` | `every`, `day` (1 to 31, or -1 for the last day) | a day of the month; a month without it is passed over |
| `yearly` | `month`, `day` | a date each year; 29 February gives leap years only |
| `dates` | `dates` | chosen dates (at most 120) |

Not in the subset, and refused: more than one start time a day, daily and finer, several month days or ordinals in one rule, and a yearly nth weekday.
Unknown fields are refused with 422.

**`count` counts episodes, not dates.**
A skipped date does not use one up, so a skipped date makes the run one date longer.
An episode that fills no date of the rule (one made by hand, or aired before the rule's start) counts against the total.
This is the one place the subset differs from iCalendar's `COUNT`.
A series' own `starts_on` and `planned_episodes` win over the rule's start and count for that series, and a series is complete when it has its episodes (a further draft answers 409).

## Words

The control plane writes the rule back in British English: `rule_words.sentence` (*Every month on the first Tuesday, 20:00 to 21:00, from 1 December, for 10 episodes. Not on 5 January 2027.*), `rule_words.short` (*First Tue monthly, 20:00–21:00*), `last` (*The last is Tuesday 5 October 2027.*, when the rule ends) and `notes` (*Months without a 31st are skipped.*, *Only in leap years.*).
A year is added to a date only when it is not this year's.

## The clocks changing

A start is resolved as a weekly slot always was: a start in the hour that does not exist airs an hour late, and a start in the hour that happens twice airs the first time.
The length is elapsed minutes.
A date whose start is moved this way carries `clock`: `forward` or `back`.
The date an episode fills is the date of its nominal start, and a show past midnight belongs to the date it starts on.

## Which date an episode fills

An episode has an `occurrence`: the rule date it fills.
Drafting sets it, and dragging the episode in the week keeps it, which is what marks the episode as moved.
An episode made or left by hand has none.
A date is filled when an episode of the series has it.
Drafting takes the next dates that no episode fills and the rule does not skip, after the latest date drafted.
The first is the date under way, else the next on or after the rule's start; with episodes drafted already, drafting goes on after the latest date drafted, or from the date under way if that is later. **Drafting never goes into the past.**
A rule with no more dates answers 409.
Episodes made before 1.0.4 were given the date of their weekly slot in their Monday-to-Sunday week the first time the control plane started on 1.0.4.

## A show, a series and the old slot

A series follows its show's rule until it has one of its own (`null` follows the show again).
Every show and series carries `rule` and, only while the rule is exactly a weekly slot (every week, one weekday, no start date, no end, nothing skipped or moved), `slot` as well, so a studio from before 1.0.4 still reads it.
The three slot columns stay in the database for one release, written while the rule is slot-shaped and no longer read.

A save that sends an old studio's `slot` and no `rule`:

| The body has | The control plane |
|---|---|
| `rule`, a value or `null` | sets or clears the rule; `slot` is ignored |
| no `rule`, and a `slot` | keeps the stored rule if it is that slot, otherwise sets the weekly rule the slot means |
| no `rule`, and no `slot` (or `null`) | clears a slot-shaped rule; keeps any other, since the sender could not see it |

## Making a show and its first episode

`PUT /api/v1/shows/{show}` with `draft_first: true` saves the show, makes series 1 if the show has none and drafts its first episode in one transaction; the reply adds `drafted`.
If anything is refused, a clash among it, nothing is saved.

## Changing a rule later

`PUT /api/v1/shows/{show}/rule` and `PUT /api/v1/shows/{show}/series/{series}/rule` set the rule alone.
`drafted` says what happens to the episodes already drafted:

| Episode | What happens |
|---|---|
| Aired, or on air | Frozen. Never moved, renumbered or rewritten. |
| Drafted, still on its rule date, to come | `move` (the default): it moves to the new rule's dates, in order, keeping its number, title, guests and everything set on it. `leave`: it stays. |
| Drafted, moved or made by hand | Stays where it is. |
| Drafted, and the new rule has no date for it | Stays where it is. |

It is one transaction.
The change is logged with who made it, and every window is told (*Moira changed when The Folk Club airs.*).
The studio app says it on the Desk and on the Schedule tab. The Schedule tab reads the week again, and does not say a change back to the window that made it.
`POST /api/v1/schedule/rule/preview` with `show` says what a change would do (`change`) without saving it.

## The preview

`POST /api/v1/schedule/rule/preview` takes a rule and answers its sentence, its next dates (each with its state, times, the clocks changing and its clashes), the last date, and any problem by field.
It writes nothing.
A skipped date stays in the list, marked, so it can be put back.

## Clashes

A date **clashes** when it overlaps, by a minute or more, another show's drafted episode or a date another show's rule gives that no episode fills.
Automation never clashes: it yields.
A clash is a warning for an admin, a producer, the station key and an API token; the reply carries it as `warnings`.
A **presenter** (and a screener, who runs no schedule) is refused: 409, `field` `rule`, the other show named, and `clashes`.
That holds where an episode comes to exist on a rule date: `PUT /api/v1/shows/{show}` with `draft_first`, `POST …/series/{series}/episodes`, a `PATCH` that sets `air_at`, and a rule change that moves episodes.
A later date of a presenter's rule that clashes is refused only when it comes to be drafted.
