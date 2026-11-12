---
title: "End the show, or hand the air to automation"
description: "End the show from the desk with one confirmation: everyone goes off air, the carts fade and automation takes the air once the profanity delay has played out. Or hand the air to automation, and take it back, without ending anything."
type: howto
status: draft
last_verified: 2026-11-11
sidebar:
  order: 6
---

At the end of a show, listeners should hear your last words, then the station's music, and nothing odd in between.
**End show** does that in one step, from the Desk tab (ADR-0272).

## End the show

1. **Press `⌃⇧E`**, or **End show** at the right of the stage.
   A dialog asks, *End the show?*, and says who goes off air, that the carts fade, and what listeners hear next.
2. **Press `⏎`** to confirm (or **End show**), or `Esc` to keep going.
   Nothing happens until you confirm; the dialog is the only confirmation.

When you confirm:

- **Everyone on air goes off air**: you, a DJ, a guest, a caller.
  They stay in the room.
  A caller who is still in the queue stays in the queue.
- **Every cart that is sounding fades.**
- **Automation takes the air** once the profanity delay has played out.
  If the delay holds 8 s, the station waits 9 s, so listeners hear the end of what you said and are not cut off in the middle of a word.
  A toast says how long.
  If somebody goes back on air during that wait, automation does not take the air, and a notice says so; press End show again when the show is over.
  The lamp says AUTO from then on, and the station plays its log ([Schedule the week](/docs/how-to/schedule-the-week/)) or, with none, its fallback files.

Anyone signed in to the studio app can do this, not only an admin; it is the same trust as taking a caller off air.

If the desk is not answering, nobody can be taken off air from the studio app, and the dialog says so; the air still goes to automation.
On a station without the playout layer (it needs the database and playout's socket) there is no automation to hand the air to: everyone goes off air, the carts fade, and that is all.
The dialog and the toast say so.

## Hand the air to automation, and take it back

Under the lamp on the stage there is a small button: **Hand to automation** while the station is live, **Take the air back** while it is on automation (ADR-0183).
It sets the station's mode and changes nothing else; use it for a break when you want automation to hold the air and leave the show open in the room.

- **Hand to automation** stops listeners hearing the desk at once, without waiting for the profanity delay.
  Nobody is taken off air: whoever is open stays open and says OPEN, not ON AIR.
  If someone is open, the dialog asks first, because they are cut off.
  To finish a show properly, use End show instead.
- **Take the air back** lets whatever is open at the desk reach listeners the moment you confirm.
  If someone is open, the dialog asks first, because they go to air.

On a station without the playout layer the button is greyed out, and says why.

## From a program

`POST /api/v1/show/end` ends the show and `POST /api/v1/playout/mode` (`{"mode": "auto"}` or `{"mode": "live"}`) sets the mode.
Both take a staff member's session or the station key, and neither takes an API token ([Control plane API](/docs/reference/api/)).
The settings route `PUT /api/v1/playout/settings` stays an admin's.
