---
title: "Hand over to the next DJ"
description: "When one DJ's show follows another's: set both shows up back to back, bring the next DJ into the room early with an invitation, and hand over on air with an overlap, so listeners hear a handover and never a gap."
type: howto
status: draft
last_verified: 2026-11-10
sidebar:
  order: 5
---

This is for two DJs whose shows follow each other: Eilidh presents The Morning Kettle until ten, and Finlay presents Steeped from ten, from home.
The handover has an overlap: for a minute or two before the post, both are on air together, then the first takes herself off.
Listeners hear the two of them, then Finlay, with no gap.

One key does it: `⇧⏎` on the Desk tab, pressed twice (ADR-0074).
The first press puts the next DJ on air beside you, the second takes you off.
In the studio app 1.0.0 `⇧⏎` only showed a note; there, use the keys in [By hand](#by-hand).

## Before the day

1. **Both shows are in the schedule, back to back**, each with its own presenter's name and takedown contact ([Set up your show](/docs/how-to/set-up-your-show/), [Schedule the week](/docs/how-to/schedule-the-week/)).
   At the post the schedule moves the station on to the next show by itself, and with it the words a caller agrees to: from ten they name Finlay's contact, not Eilidh's (ADR-0214).
   A caller who rang before the post keeps the words they agreed to.
2. **Each DJ has a staff account** to sign in to the studio app with ([Add staff and sign in](/docs/how-to/add-staff/)).
3. **Agree the times**: when Finlay joins the room (ten or fifteen minutes before the post is plenty), when he comes on air, and who says what in the overlap.

## Bring the next DJ into the room early

1. **Create an invitation for Finlay with the role DJ**: on the Desk tab, **Invite someone…** at the right end of the **Not on air** line; give his name, keep **DJ** under **They join as**, press **Create invitation**, then **Copy link**.
   The link is shown once, and works for the station's invitation time; anyone who has it can join as Finlay, so send it to him alone.
   A DJ joins off air: nothing of his reaches listeners until someone puts him on.
   An invitation with the role presenter would put him on air the moment he joined.
   A program can create the same invitation with `POST /api/v1/invites` (`{"role": "dj", "name": "Finlay Grant"}`); the answer's `link` is the invitation ([Control plane API](/docs/reference/api/#post-apiv1invites)).
   The link lasts the station's `join.invite_ttl_s`, four hours by default ([Settings](/docs/reference/settings/)), so send it on the day.
2. **Finlay gets ready** as any presenter does: headphones, the echo test, the invitation link in a browser, **Join the studio** ([Get ready to present](/docs/how-to/get-ready/)).
   He appears under **Not on air** on the desk, on strip 5, standing by.
3. **Hear his level**: press `T`, strip 5's cue key.
   He is in your headphones only; check he is about as loud as you, and ask him to move or turn his microphone if not.
   Press `T` again to take him out of your cue.
4. **Agree the plan off air**: hold `G`, strip 5's talk key.
   While it is held only Finlay hears you, and you are off the programme; let go, and you are back as you were ([Talk to one person off air](/docs/how-to/present-from-the-keyboard/#talk-to-one-person-off-air)).

## Hand over

1. **Your last link.** Back-announce, thank your guests and callers, say who is next.
2. **Count him in**: press `T` to cue him, then hold `G` and count: "In three, two, one."
3. **Press `⇧⏎`.** Finlay is on air beside you, and both tiles say ON AIR. This is the overlap: say hello, let him say what is in his show, say goodbye.
   The note in the toast slot says the next press hands over.
4. **Press `⇧⏎` again.** You are off air; Finlay holds the air alone.
   Guests and callers on air stay on with him; anyone else presenting comes off with you.
5. **Leave the room**: close your invitation page's tab. The desk keeps your strip for about 30 seconds, then frees it.

The button under Finlay's live buttons does the same as `⇧⏎` and says what the next press does: **Bring Finlay on air**, then **Hand over to Finlay**.
If the rundown has a row whose key is `⇧⏎`, the button is under that row too ([Set the show](/docs/how-to/set-the-show/)).
If no DJ is in the room, `⇧⏎` says so and changes nothing.

From here the desk is Finlay's: he drives it from his own studio app, signed in as himself.
Callers still waiting in the queue stay there for him, and the screener carries on as before.

**Talkback after the handover.**
The desk's talk keys and `⌃T` speak as the presenter's seat, and the seat follows the air: once you are off air, it is Finlay's ([Roles](/docs/reference/roles/#seats-in-the-studio-app)).
His talk keys then reach a guest or a caller off air, and `⌃T` the screener.

### By hand

The same handover with the keys every presenter has, and the way in the studio app 1.0.0:

1. **Press `5`** (strip 5's on-air key) instead of the first `⇧⏎`.
2. **Press `1`** (your own) instead of the second.

On 1.0.0 the presenter's seat does not follow the air: once you have left, a DJ who joined with the role `dj` holds no seat and his talk keys do nothing.
The screener can still talk to him (`⌃T` on the Screener tab reaches presenters and DJs).

## If something goes wrong

- **Finlay is not under Not on air** when it is time: he has not joined, or has joined as someone else. Carry on presenting and talk to him by phone or chat; do not take yourself off until he is on air.
- **You pressed `1` before `5`** (by hand): listeners hear nobody until you press `5`. Press it; Finlay is on. `⇧⏎` never does this: it takes you off only once he is on.
- **His level is far from yours**: he moves closer to his microphone or further from it, or changes his input level, while you listen in cue.

## What has been tried

- **Against a real desk** (2026-11-10, the M2 stack on one machine, `tests/m2/handover.cjs`): `⇧⏎` put the DJ on air beside the presenter with the seat staying hers, `⇧⏎` again took her off and passed the seat to him, the guests stayed on, a third press changed nothing, and the talk keys then spoke as the DJ.
- **Unit tests** for the steps and the seat, and the sandbox's browser test against its simulated station.
- **Not yet:** two real people handing over on a real station over real networks, and the invitation link opened on a phone.

## Practise first

The [sandbox desk](https://demo.weavesignal.top/) has both halves as training scenarios, with Kettle FM's Eilidh and Finlay: *Set up the next DJ* (`#next-dj`: the invitation, the level, the plan) and *Hand over to the next DJ* (`#handover`: the count, the overlap, the goodbye).
They take three minutes each.
