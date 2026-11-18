---
title: "Studio app top bar"
description: "The studio app’s top bar by name: the show's step and Show everything, the tabs, Settings, who is signed in, whether the window is Live or Preparing, the link to the station, the text size and the theme."
type: reference
status: draft
last_verified: 2026-11-16
sidebar:
  order: 1
---

The top bar runs along the top of every tab of the studio app.
Its heading below is its name on screen (ADR-0070, ADR-0097).

## `studio.frame`

The station's name, the show's step, the tabs with their keys, Settings, the app's version, who is signed in, whether the window is Live or Preparing, the link to the station, the text size and the theme.

![The studio app’s top bar: the station's name, the five tabs with their keys, Settings, who is signed in, the control plane link, text size and the theme.](../../../../../assets/screens/reference-studio-frame.png)

- **The tabs**: Desk `⌃1`, Screener `⌃2`, Carts `⌃3`, Schedule `⌃5`.
  `⌃4` has no tab: it opens the Screener tab with the focus in its Room lane, where the chat is moderated (from 1.1; before, the strip drew a greyed Chat tab).
  Below the Full step the Screener tab is not drawn, `⌃4` with it, and each tab keeps its number ([`studio.step`](#studiostep)).
  Desk, Screener, Carts and Schedule are built.
  Chat is not a tab yet (**[planned](/docs/reference/release-one-status/)**): it says it is not in this build, and the chat is on the Screener tab.
- **Settings**: retention, holds, the time zone, staff, API tokens, updates and, for a member of staff signed in as themselves, **Your show**. While Settings is open only the `⌃` chords reach the keymap; a tab's key leaves Settings for that tab.
- **Update … available**: shown in the packaged app when its update feed offers a newer version; it opens Settings, and nothing installs without a yes there.
- **The version** (from 1.1): the studio app's own version, in quiet words between the tabs and who is signed in: **WeaveSignal 1.1.0**, or **v1.1.0** where the bar has less room (a member of staff's window at 1,440 px).
  It is the number **Settings › Updates** shows.
  It takes only the bar's spare room: in a bar with none it is not drawn, and it never moves a control or adds a row.
  When the station runs another version, the station's is said beside it (**v1.1.0 · station 1.0.5**, or **1.1.0 ≠ 1.0.5** in less room); a test build of the same release (`1.1.0-test.4`) is not a difference.
  Point at it, or reach it with `Tab`, and its tooltip gives both versions in full and, when they differ, which way and what follows ([Work with several stations](/docs/how-to/work-with-several-stations/)).
- **Who is signed in**: the member of staff, or the station key ([Add staff and sign in](/docs/how-to/add-staff/)).
  Beside the Live or Preparing control, in a window at most 1,600 px wide, the name stands without the role, which is in its tooltip and its dialog.
- **Live or Preparing**: for a member of staff signed in as themselves ([`studio.prepare`](#studioprepare)).
- **The link**: **Connected** while the app's live link to the station is up (one word, to make room for the step at 1,440 px); *Control plane connecting* or *Control plane not connected* when it is not. Hover over it for the reason.
  Beside the Live or Preparing control, in a window at most 1,600 px wide, only the tick is drawn while the link is up; the words return whenever it is not.
- **Text size**: `A−`, the level (select it to reset) and `A+`; or `Ctrl+−`, `Ctrl+0` and `Ctrl++`. The whole interface scales, and the level is remembered on this machine.
- **Theme**: Dark, Light or Night.
  The app starts dark and never follows the system's setting (ADR-0065); the choice is remembered on this machine.
  Night is for presenting late: warm near-black grounds, warm off-white text and an amber accent, with no blue or cyan anywhere (ADR-0270). The state words keep their meaning and their contrast (WCAG AA): ON AIR stays red, OPEN stays the white lamp in a warm white, CUE becomes a plum and TALK a green.

The desktop app signs in as a member of staff, or with the station key, and keeps either in the keychain ([Add staff and sign in](/docs/how-to/add-staff/)).
The browser build is still given the station key once (as `?token=` in its address, kept for that session) or served behind a proxy that adds it.

## `studio.show-setup`

The presenter's show setup (ADR-0218, ADR-0219), a dialog over the surface: **About you** (the on-air name and the takedown contact), **Your show** (its name, links to the presenter's channels, the usual day and time, whether it plays commercial music) and **Done**.
It opens by itself for a presenter signed in as themselves with no show of their own, and again from Settings; **Skip for now** keeps it away on that computer.
A dialog owns the keyboard: only the `⌃` chords reach the keymap while it is open.
The answers become the show's own fields, tied to the account ([Set up your show](/docs/how-to/set-up-your-show/)).

## `settings.my-show`

**Your show**, in Settings, for a member of staff signed in as themselves: the show tied to their account as the setup left it (name, on-air name, takedown contact, links, usual time, commercial music) with **Change…**, or **Set up your show…** when there is none.
The station key has no account to tie a show to, so it has no such panel.
**Your show** also holds **Broadcast from your own software**: the presenter's own key for their show, with **Details…**, **New key…** and **End key…**, or **Broadcast from your own software…** to make one (`settings.hosts.setup`).
**Open my monitor** opens the presenter's own monitor link in their browser, which is also how they test their connection. It works in the window that made the key, until that window is closed: the station keeps only a scrambled form of the link's secret and cannot give it again, so elsewhere the button is drawn off and says to open the link from the details.

## `settings.hosts`

**Own studios**, in Settings after **Staff**, for an admin or a producer (ADR-0280): a row for each show key.

![Settings › Own studios: rows for Finlay Grant on air over SRT, Catriona MacKenzie-Buchanan in a private test, Elspeth Gordon not connected, and Heather Douglas with her last refusal; Details…, New key… and End key… on each.](../../../../../assets/screens/reference-settings-hosts.png)

- **Show**: its colour, its name and when it is next on.
- **Host**: who the key is for, the key's public id and who made it.
- **Way in**: *Icecast* or *SRT*, as the setup was told. A hint: the key works either way.
- **Status**: one chip and what is known. ON AIR or STANDBY for a source at the desk, with since when it has been connected, whether its level is in the leveller's reach, and *monitor open* while the host has their monitor link open; **TEST** for a connection outside the show's slot, with the station's own reading and what the software says is playing; **NOT CONNECTED** while the slot is open and nobody has connected. A key that is not due has no chip and says when it last connected. Under it, the last refusal in words (*refused today 13:41: the key is wrong*), and for a key the desk turned away, by whom and until when, with **Let back in**.
- **Details…** (`settings.hosts.details`), **New key…** (`settings.hosts.new-key`) and **End key…** (`settings.hosts.end`).

**Set up a host…** opens `settings.hosts.setup`. Ended keys are listed apart, folded.
With no key: *No host has a key yet. Set one up when a presenter wants to use their own software instead of joining the room.*
When the part of the station that takes hosts' software is not running, a callout says **The station is not listening for hosts**; when the station cannot keep keys at all, it says why and the setup is off.
A station older than 1.1 has no such panel.

## `settings.hosts.rules`

Under the table, for an admin: how early a host may connect before their show (30 minutes); whether a connection outside the slot is taken as a private test, and for how long (15 minutes); the grace at the post (*Off*, 1, 2 or 5 minutes) and a late host's count; and the two ports, Icecast on TCP and SRT on UDP.
**Save the rules** sends them all at once.

## `settings.hosts.setup`

**Set up a host**: the **Show** (one that already has a host says so), **Who is it for** (their name on the desk; an admin may pick a member of staff, which ties the key to their account), and **How they will send**, *Icecast* or *SRT*, each with one line on what it is good for.
**Make the key** turns the dialog into the details.
From the show editor and from **Your show** the show is given, not asked.

## `settings.hosts.details`

A host's connection details: a block for SRT and a block for Icecast, the way they said they would send first, with **Copy** on each field.

![A host's details, shown once: the SRT and Icecast fields with Copy on each, the host's monitor link with Copy, what to tell the host, Copy everything as text and I have sent them.](../../../../../assets/screens/how-to-host-details.png)

When a key has just been made the secret is in them, **this once**: **Copy everything as text** gives a block to paste into a message, the host's whole monitor link is given with **Copy** (the station's public address, then `/monitor/#` and the link's own secret; on a station with no public address, the secret and how the link is formed), a person's own key offers **Open my monitor**, and **I have sent them** is the only way out (`Esc` does not close it).
Opened later with **Details…**, they hold everything but the secret, offer no Copy on it, say whether the host's monitor link is open while they are connected, and close as any dialog does.

## `settings.hosts.new-key`

*Make a new key for Finlay?* The key they have stops working, and they need the new details before the next show.
If they are connected it asks when the old key ends: **When Finlay disconnects** (picked) or **Now**, which says what listeners hear if they are on air.
The new key's details follow, shown once.

## `settings.hosts.end`

*End Finlay's key for Steeped?* Their software is refused from now on, and a connection that is open is closed. The show stays in the schedule; with no key left it comes from the station's room again.
If the host is on air it says so first, and the button reads **End the key and take Finlay off air**.

The picture above is from 1.0.1: it lacks the step control, and says *Control plane connected*.
The top bar from 1.0.2, at Solo:

![The studio app’s top bar at Solo: the station's name, the step control with one of four pips filled and the word Solo, Show everything, the Desk, Carts and Schedule tabs, Settings, who is signed in, Connected, text size and the theme.](../../../../../assets/screens/reference-studio-frame-step.png)

That picture was made before Prepare mode: a member of staff signed in as themselves now has **Live** or **Preparing** after their name, and the link shows its tick alone ([`studio.prepare`](#studioprepare)).

## `studio.step`

The show's step as four pips and a word, the way to change it, and **Show everything** (ADR-0271).
It sits beside the station's name, before the tabs it governs, and never changes place: tabs open to its right.
The pips are equal in height, filled up to the step; it is not a state chip and takes no live or status colour.
Its tooltip names the show the step is of; between shows its edge is dashed and the tooltip names the show the step is kept from.
**Show everything** is drawn at Solo, Guests and Callers; at Full nothing is hidden.

## `studio.step.menu`

The step dialog, under the top bar at its left: the four steps for the show named, the one in force marked **now**, and, once another is picked, what will change at once and the button that names the move.
`↑` `↓` move between the steps, `⏎` confirms the move, `Esc` keeps the step.
An account that may not change the step sees the list to read, with who can change it.
With no show ever on air it says the studio shows everything, and offers **Set up your show…**.

## `studio.step.banner`

A row under the top bar while everything is shown (ADR-0274): *Showing everything for now.*, the step that still holds, and **Back to** the step.
How the steps work: [Choose how much of the studio a show uses](/docs/how-to/choose-a-step/).

## `studio.prepare`

Whether this window is **Live** or **Preparing** (ADR-0276), as one word in a pill after who is signed in, and the way to switch.
A live window drives the desk; a preparing window shows the live desk and cannot change what is on air.
It is drawn for a member of staff signed in as themselves, on a station that has Prepare mode; the station key is always live and has no control.
It is not a state chip and takes no live or status colour: *Live* here is the window's, never the red of ON AIR.
While preparing, the pill takes the ground of the band below it.

![The studio app’s top bar with a member of staff signed in: the station's name, the step, the tabs, Settings, the name, the word Live in a pill, the link's tick, text size and the theme.](../../../../../assets/screens/reference-studio-frame-prepare.png)

## `studio.prepare.menu`

The dialog under the top bar at its right: the two choices, the one in force marked **now**, and, once the other is picked, what the switch changes and the button that names it (**Switch to Preparing**, **Switch to Live**).
`↑` `↓` move between the two, `⏎` confirms, `Esc` keeps what is.
It is also the one ask before taking the desk: while someone else's show is on air it names the show and says that this window's keys and buttons will act on it at once.

## `studio.prepare.banner`

A row under the top bar while this window is preparing, on the Desk, Screener and Carts tabs: *This window is preparing.*, what is on air, and **Switch to Live** (with `…` when it asks first; the primary button once the account's own show is on air).
On those tabs every control that changes the air is disabled, with *Off while this window is preparing. Switch it to Live first.* in its tooltip, and its key does nothing; the row deepens for a moment to answer the press.
How to use it: [Prepare your show while another is on air](/docs/how-to/prepare-your-show/).
