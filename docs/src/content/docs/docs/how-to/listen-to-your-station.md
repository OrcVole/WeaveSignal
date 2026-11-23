---
title: "Listen to your station"
description: "The studio app makes no sound. Open the player in your browser from the Desk or Settings, copy its link, and know which address it uses."
type: howto
status: draft
last_verified: 2026-11-23
sidebar:
  order: 42
---

The studio app is a control surface: it shows and drives the desk, and it plays nothing.
What listeners hear, you hear in a browser, on the station's **player**: a web page that plays the stream and takes chat and calls.
It plays behind the desk, as every listener's does, by about twenty seconds on a usual station; the Desk's clock says the figure ([the delay](/docs/reference/surfaces/desk/#deskclock)).

Hearing yourself on air is different.
Your microphone is a browser tab as well: **Join with my microphone…** opens it, and that tab is in the room, so it needs no player and has no delay ([Set up your show](/docs/how-to/set-up-your-show/)).

## Open the player

From 1.3.1 the app opens it for you.

1. On the **Desk**, in the **Listeners** panel, press **Open the player**.
   Until you have opened or copied the player once on this computer, the panel also says, in one line, that sound plays in your browser and not in this app; after that it shows only the two buttons, and the line is in the button's tooltip and in Settings.
2. Or go to **Settings → The player**, which shows the address as well.
3. Your own browser opens on the station's player.
   Press **Listen live**, fire a cart or put a microphone on air, and wait: it is heard about twenty seconds late.

**Copy link** puts the address on the clipboard, to paste into a message, a note or a browser on another computer or phone.
If the clipboard is refused, the link appears in a box already selected, to copy by hand.

## Which address it is

- **The station's public address** (`publishing.public_url`), whenever the station has one.
  It is the address people elsewhere reach, and on Cloudron it is the public pages' domain: the studio's own domain is behind Cloudron's login, and a link to it would show a login page to a listener.
- **The address this window uses**, when the station has no public address (a Compose install on a home network, for one).
  Settings says so.
  It opens the player on this network; people elsewhere need a public address ([Publish a Compose station](/docs/how-to/publish-with-compose/)).
- Until the station has answered, the buttons are dimmed and say *Asking the station for its address…* beside them.

Neither button is drawn in the sandbox, which has no station of its own to listen to.

## If nothing opens

The packaged app hands the address to the system to open in your default browser.
If no browser can be started from the app, it says so and offers the link to copy: paste it into a browser yourself.
A station whose stream has a password (the Cloudron package's) cannot be played by the player page, because a browser does not ask for one for sound; use a radio app or player with the password ([Set up the station](/docs/how-to/set-up-the-station/)).
