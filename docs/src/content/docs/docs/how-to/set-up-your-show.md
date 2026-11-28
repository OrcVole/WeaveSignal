---
title: "Set up your show"
description: "A DJ's first sign-in: four short parts that tell the station your on-air name, your takedown contact, your show, how much of the studio to start with and when it is on, or start it now; the answers become the defaults for you and your show."
type: howto
status: draft
last_verified: 2026-11-27
sidebar:
  order: 6
---

You present a show on a station someone else runs.
The station's admin has made you a staff account with a password ([Add staff and sign in](/docs/how-to/add-staff/)), and the first time you sign in to the studio app as a presenter it asks four short parts (ADR-0218).
Nothing is emailed, and nothing is asked that an online station does not need (ADR-0219).

The answers are kept in the station as the defaults for you and your show:

- while your show is on air, the words a caller agrees to before calling name you and give your takedown contact first, then the station's own as the host (ADR-0214, ADR-0215);
- new episodes of your show start from its usual day and time.

## Before you start

- Your staff account, with the `presenter` role, and its password from the station's admin.
- The studio app, signed in to the station as yourself.
- Where someone should write to have a call of yours taken down: an email address or a link.
  You can leave it empty; the station's own contact is then the only one said.

## The four parts

The wizard opens by itself the first time a presenter with no show of their own signs in.
Run it again at any time from **Settings** › **Your show** › **Change…**.

1. **About you.**
   Your on-air name (your account's name is offered), and your takedown contact.
   **Skip for now** closes it; it does not open again on this computer, and **Settings** › **Your show** still has it.
2. **Your show.**
   Its name; links to your channels, one per line (Rumble, Odysee, AzuraCast, wherever your audience already is); whether the show plays commercial music; and **how much of the studio to start with**, with Solo picked.
3. **When it is on.**
   Its first date and its times, in station time, and whether it comes back (once, every week, and the other choices New show has: [Schedule the week](/docs/how-to/schedule-the-week/)), with the next dates listed as you choose.
   **Save my show** keeps the show in the station and drafts its first episode; **No dates yet** keeps the show with none.
   A first date that is another show's is refused, with that show named.
4. **Done.**
   What the answers now do, the first episode, where the studio starts, and where to change them.

Start at **Solo** and the studio shows only what one DJ needs: your microphone, the music and carts, the listener count and the chat.
Guests, callers and the rest are switched off, so nobody can call in or join before you are ready.
Move up a step whenever you like, at the top of the studio ([Choose how much of the studio a show uses](/docs/how-to/choose-a-step/)).

The music question is a reminder for the station: a UK webcast of commercial music needs PPL and PRS for Music licences.
It blocks nothing.

## Start now

From 1.3.3 the third part opens with **Start now**, for a show that should be on air in a minute.
One press saves your show to start at the station's next whole minute, once, and drafts its first episode.
It runs for the length the times **from** and **until** make, an hour unless you changed them (15 minutes to 8 hours; anything else is an hour).
**Done** then says first when it starts, in the station's time.
When that minute comes, the step at the top of the studio becomes your show's. **ON AIR** is the lamp, not the schedule: it lights when listeners can hear someone, which is the **On** key's doing.

If another show is on the schedule at that minute, **Start now** says which and that the station does not let you schedule over it: give your show a later time below, or ask the station.
(An admin or a producer running the setup for themselves is asked once instead, with **Start now anyway** and **Not now**.)
The minute is the station's own, on its clock and in its zone.
Give the show more dates later: **Settings** › **Your show** › **Change…**, or the Schedule tab.

**Start now** is offered the first time the show is set up, on a station of 1.0.4 or later. Run again from Settings for a show that exists, the third part shows the show's rule and has no **Start now**: use the Schedule tab's dates there.

## What the station keeps

The answers are the show's own fields.
The schedule's show editor has the same fields under **Presenter** ([Schedule](/docs/reference/surfaces/calendar/#calendarshow-presenter)), so an admin or a producer can set or correct them for any show, and the show's usual time is its weekly rule, which series and episodes inherit.

A show is tied to the account that set it up.
That account can change the show's details from Settings.
The tie itself is set by the wizard, or in the show editor: under **Presenter**, **Staff account** picks the member of staff who presents the show (an admin may pick anyone; anyone else may pick themselves. The list of the team is a producer's and an admin's, so a presenter or a screener is offered only themselves). Through the API an admin sends `presenter_account` in a `PUT` to `/api/v1/shows/{show}` ([Control plane API](/docs/reference/control-api/)). Before release 1.1 the show editor set the presenter's name and contact, not the tie.
If the station turns an account off, the show stays and the tie is cleared.

## What callers see

While your show is on air by the schedule, the call page's consent words read:

> You'll be live on Kettle FM, and we may keep your call in our archive and podcast for two years. To have it taken down, contact shona@example.org. This station is hosted by Kettle FM: info@kettle.example.

With no contact of your own, the last sentence names the station's contact alone.
Each call's consent record keeps the show that was on air, the presenter's name and the contact exactly as the words said them, so changing your details later never rewrites what a caller agreed to ([The consent words](/docs/reference/public-pages/#the-consent-words)).

## Your show in Settings

**Settings** › **Your show** shows what the station keeps about you and your show, with **Change…** to run the wizard again.
A member of staff with no show of their own finds **Set up your show…** there, whatever their role.
The station key has no account to tie a show to, so it has no **Your show** panel; an admin sets a show's presenter name and contact in the show editor instead.
Only an admin or the show's own presenter may change its presenter name and takedown contact, since the consent words name them to callers (ADR-0241, from 1.0.1).

## What your show's step switches off

A show the wizard makes starts at **Solo** (ADR-0271): while it is on air, the studio holds only what one DJ needs.
The call page says "The lines are closed for this show.", and the station refuses invitations and passes for guests, the next DJ and the screener's seat.
Callers and guests come back as you move the show up a step: **Guests**, then **Callers**, then **Full**.
The steps, what each switches on, and how to change them are in [Choose how much of the studio a show uses](/docs/how-to/choose-a-step/).
Shows that existed before steps arrived stay at Full.

## What has been tried

- **The four parts**: unit tests of the wizard's rules and its screens (`wizard.test.ts`, `rule.test.ts`), and a browser review against a stand-in station (2026-11-10; the third part and Start now in `tests/newshow/shoot.mjs`, 2026-11-27).
- **Start now** (1.3.3): unit tests of the press and its screens (`start-now.test.ts`) and the browser run above, against a stand-in station. Not tried on a real station from the DJ's setup.
- **The consent words naming the show's contact**: the control plane's database test (`consent_db.rs`): the words name the show on air, and a caller's record keeps the words they agreed to when the contact changes. The change at the post itself was checked by reading the code, not by a test.
- **Not yet:** a DJ following this on a real station.
