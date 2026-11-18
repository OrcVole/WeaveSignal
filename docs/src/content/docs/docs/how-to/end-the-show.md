---
title: "End the show, or hand the air to automation"
description: "End the show from the desk with one confirmation: everyone goes off air, the carts fade and automation takes the air once the profanity delay has played out. Or hand the air to automation, and take it back, without ending anything."
type: howto
status: draft
last_verified: 2026-11-17
sidebar:
  order: 6
---

At the end of a show, listeners should hear your last words, then the station's music, and nothing odd in between.
**End show** does that in one step, from the Desk tab (ADR-0272).

## What automation is

Automation is the station playing by itself, with nobody at the desk.
It plays the log: the music planned for this hour from the station's library, with carts such as idents, stingers or adverts between the tracks ([Schedule the week](/docs/how-to/schedule-the-week/)).
With nothing in the log, listeners hear the station's fallback files, or the loop WeaveSignal ships until the station has some ([Playout](/docs/reference/playout/)).

The studio app says the same wherever it names automation (from 1.1): in the tooltip of the button under the lamp, in the AUTO lamp's tooltip, and in the dialog that asks before the air goes to automation.
Point at the button or the lamp, or reach it with `Tab`, and the tooltip shows; `Esc` hides it.

## When to use which

Both take you off the air. They are for different reasons.

| | **Hand to automation** | **End show** |
|---|---|---|
| What it is for | A break: the show is not over | The end: the show is over |
| Listeners hear | The station's log at once, without waiting for the profanity delay | Your last words first, then the log, once the delay has played out |
| At the desk | Nobody is taken off air; whoever was open stays open (and says OPEN) | Everyone on air goes off air, and the carts fade |
| To carry on | **Take the air back** | Put someone on air again |

**Hand to automation: a break in the middle.**
Eilidh is half-way through The Morning Kettle when her guest, Tavish, has to take a phone call he cannot put off.
She presses **Hand to automation**.
Listeners hear the station's log from that moment, and Eilidh and Tavish stay in the room with their places kept.
Ten minutes later she presses **Take the air back**.
The dialog asks first if anyone is open, because they would go straight to listeners: she mutes or takes off air whoever she does not want heard, then confirms.

**End show: the end of the slot.**
At ten o'clock Eilidh has said her goodbyes and Finlay's Steeped is next.
She presses **End show** and confirms.
The delay plays out, so listeners hear her last words whole, the carts fade, everyone goes off air, and automation plays the log until the next presenter takes the air.
She is not left open on the desk, and nothing is waiting to be taken back.

If the next presenter is a DJ who takes the seat from the desk, hand over to them instead ([Hand over to the next DJ](/docs/how-to/hand-over-to-the-next-dj/)).

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

Under the lamp on the stage there is a button as wide as the lamp: **Hand to automation** while the station is live, **Take the air back** while it is on automation (ADR-0183). Its tooltip says what it does, and then [what automation is](#what-automation-is). To leave the air yourself and stay in the room, this is the button; to leave the room, press **Leave** on your own tile; to end the show, **End show**.
It sets the station's mode and changes nothing else; use it for a break when you want automation to hold the air and leave the show open in the room.

An example: half-way through a two-hour show a guest's microphone starts to crackle.
Press **Hand to automation**: listeners hear the station's music from the log, and you and the guest stay in the room, open to each other, to sort it out.
Nobody has to join again.
When the microphone is right, press **Take the air back** and carry on with the show.
The same goes for a planned break: ten minutes of music while you brief the next guest.

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
