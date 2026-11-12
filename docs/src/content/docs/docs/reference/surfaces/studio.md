---
title: "Studio app frame"
description: "The studio app's frame by name: the tabs, Settings, who is signed in, the link to the station, the text size and the theme."
type: reference
status: draft
last_verified: 2026-11-04
sidebar:
  order: 1
---

The frame runs along the top of every tab of the studio app.
Its heading below is its name on screen (ADR-0070, ADR-0097).

## `studio.frame`

The station's name, the tabs with their keys, Settings, who is signed in, the link to the control plane, the text size and the theme.

![The studio app's frame: the station's name, the five tabs with their keys, Settings, who is signed in, the control plane link, text size and the theme.](../../../../../assets/screens/reference-studio-frame.png)

- **The tabs**: Desk `⌃1`, Screener `⌃2`, Carts `⌃3`, Chat `⌃4`, Schedule `⌃5`.
  Desk, Screener, Carts and Schedule are built.
  Chat is not a tab yet (**[planned](/docs/reference/release-one-status/)**): it says it is not in this build, and the chat is on the Screener tab.
- **Settings**: retention, holds, the time zone, staff, API tokens, updates and, for a member of staff signed in as themselves, **Your show**. While Settings is open only the `⌃` chords reach the keymap; a tab's key leaves Settings for that tab.
- **Update … available**: shown in the packaged app when its update feed offers a newer version; it opens Settings, and nothing installs without a yes there.
- **Who is signed in**: the member of staff, or the station key ([Add staff and sign in](/docs/how-to/add-staff/)).
- **Control plane**: *connected*, *connecting* or *not connected*: whether the app's live link to the station is up; hover over it for the reason when it is not.
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
