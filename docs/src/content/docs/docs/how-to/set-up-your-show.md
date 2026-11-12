---
title: "Set up your show"
description: "A DJ's first sign-in: three short steps that tell the station your on-air name, your takedown contact, your show and when it is on; the answers become the defaults for you and your show."
type: howto
status: draft
last_verified: 2026-11-06
sidebar:
  order: 6
---

You present a show on a station someone else runs.
The station's admin has made you a staff account with a password ([Add staff and sign in](/docs/how-to/add-staff/)), and the first time you sign in to the studio app as a presenter it asks three short steps (ADR-0218).
Nothing is emailed, and nothing is asked that an online station does not need (ADR-0219).

The answers are kept in the station as the defaults for you and your show:

- while your show is on air, the words a caller agrees to before calling name you and give your takedown contact first, then the station's own as the host (ADR-0214, ADR-0215);
- new episodes of your show start from its usual day and time.

## Before you start

- Your staff account, with the `presenter` role, and its password from the station's admin.
- The studio app, signed in to the station as yourself.
- Where someone should write to have a call of yours taken down: an email address or a link.
  You can leave it empty; the station's own contact is then the only one said.

## The three steps

The wizard opens by itself the first time a presenter with no show of their own signs in.
Run it again at any time from **Settings** › **Your show** › **Change…**.

1. **About you.**
   Your on-air name (your account's name is offered), and your takedown contact.
   **Skip for now** closes it; it does not open again on this computer, and **Settings** › **Your show** still has it.
2. **Your show.**
   Its name; links to your channels, one per line (Rumble, Odysee, AzuraCast, wherever your audience already is); the usual day, start and end in station time, or none yet; and whether the show plays commercial music.
   **Save my show** keeps it in the station.
3. **Done.**
   What the answers now do, and where to change them.

The music question is a reminder for the station: a UK webcast of commercial music needs PPL and PRS for Music licences.
It blocks nothing.

## What the station keeps

The answers are the show's own fields.
The schedule's show editor has the same fields under **Presenter** ([Schedule](/docs/reference/surfaces/calendar/#calendarshow-presenter)), so an admin or a producer can set or correct them for any show, and the show's usual time is its weekly rule, which series and episodes inherit.

A show is tied to the account that set it up.
That account can change the show's details from Settings.
The tie itself is set by the wizard, or through the API: an admin who wants an existing show tied to a DJ sends `presenter_account` in a `PUT` to `/api/v1/shows/{show}` ([Control plane API](/docs/reference/control-api/)); the show editor sets the presenter's name and contact, not the tie.
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

## What has been tried

- **The three steps**: unit tests of the wizard's rules and its screens (`wizard.test.ts`), and a browser review against a stand-in station (2026-11-10).
- **The consent words naming the show's contact**: the control plane's database test (`consent_db.rs`): the words name the show on air, and a caller's record keeps the words they agreed to when the contact changes. The change at the post itself was checked by reading the code, not by a test.
- **Not yet:** a DJ following this on a real station.
