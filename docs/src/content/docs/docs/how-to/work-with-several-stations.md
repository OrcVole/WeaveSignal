---
title: "Work with several stations"
description: "Keep more than one station in the studio app and switch between them in one window: add a station, switch, what happens to a station left on air, give a station a name of your own, sign out of one, remove one, and what a station on another version gets."
type: howto
status: draft
last_verified: 2026-11-15
sidebar:
  order: 13
---

The studio app keeps a list of stations and works with one at a time.
Each station is a separate install with its own address and its own sign-in.
Nothing is shared between them: not accounts, not shows, not the chat.

This is for the packaged studio app.
In a browser the page's own address is the station, so there is nothing to switch.

## Before you start

- You need the address of each station, and a way in to each: your username and password there, a code by email, or the station key ([Add staff and sign in](/docs/how-to/add-staff/)).
- A station you already use carries over. After the update it is the first station in the list, and you are still signed in.

## See which station you are on

The station's name is the first thing in the frame, at the top left of every tab.
With two or more stations it has a badge of two letters before it, such as `KF` for Kettle FM.
The window's title says the station's name too.

## Add a station

1. Press the station's name in the frame, or `⌃⇧S` on any tab. The **Stations** dialog opens.
2. Press **Add a station…**. The sign-in screen opens over the studio. The station you are on carries on underneath, and no key you type reaches its desk.
3. Type the other station's address, then sign in there.
4. The station joins the list and the window switches to it.

**‹ Back to …** closes the sign-in screen and changes nothing.
A station is added only when its sign-in succeeds.
One address is one row: signing in to an address already in the list signs that row in again.

## Switch to another station

1. Press the station's name in the frame.
2. Press the station you want. `↑` and `↓` move between the rows and `⏎` opens the one in focus.

The window empties itself and loads again on the other station.
A note in the desk's toast slot says where you are and who you are signed in as.
Nothing carries over from the station you left: no selected caller, no armed key, no half-typed message.
A form you had not saved is lost.

### When the station you are leaving is on air

The studio app is a control surface.
The sound of a show is in the room and on the server, so a switch cuts no audio.
What it takes away is the controls: until you switch back, nobody at this window can mute a strip, dump the delay or take a call on that station.

- A talk key you are holding is let go first, always.
- If a strip is open on the desk, or a caller is in screening, the app asks once: **Leave Kettle FM while it is on air?** `⏎` switches and `Esc` stays.
- A window that is **Preparing** is not asked, because it changes nothing on air ([Prepare your show while another is on air](/docs/how-to/prepare-your-show/)).
- When the window cannot see the desk at all, it says so and asks the same way.

## Give a station a name of your own

Two installs of one station, such as a live one and a test one, call themselves the same.

1. In the **Stations** dialog, press **Manage** on the station's row.
2. Type a name under **Name on this computer** and press **Save**.

Only this computer shows the name. The station is not told.
Empty the field and save to go back to the station's own name.

## Sign out of a station

In the **Stations** dialog, press **Manage** on the row, then **Sign out**.

- The station ends your sign-in and this computer deletes the key it kept.
- The row stays, marked *signed out*, with its address. Press it to sign in again.
- **Sign out** under your name in the frame does the same for the station you are on.

## Remove a station

In the **Stations** dialog, press **Manage** on the row, then **Remove…**, then **Remove**.

- Your sign-in there ends and its key is deleted from this computer.
- The row is forgotten, with the name you gave it.
- The station itself is not changed.

A station you are signed out of can also be removed from its sign-in screen: **Remove … from this computer**.

Signing out of, or removing, the station you are on is leaving it.
If it is on air, the app asks first.

## A station on another version

The studio app is one version for every station.
A station may be older or newer.

| The station is | What you see | What works |
|---|---|---|
| The same version | nothing | everything |
| Older | *1.0.3, older than this app* on its row, and once in the note on arrival | everything that station has. What it does not have yet is hidden or switched off. |
| Newer | *1.1.0, newer than this app* | everything this app knows. Update the app to use the rest ([Update the studio app](/docs/how-to/update-studio-app/)). |
| A different major version | *a different major version* | some screens may not work until the two match |

The app does not refuse a station because of its version.
A station refuses, in its own words, anything it does not understand.

## Where the list and the keys are kept

- The list is a file in the app's own settings folder, `stations.json`. It holds each station's address, the name you gave it, and what the app last saw there. It holds no key and no password.
- Each station's key is one entry in your computer's keychain, under that station's address. On a Linux desktop without a keyring, each station's key is a file of its own that only you can read.
- Removing a station deletes its entry.

## If something goes wrong

| What you see | What to do |
|---|---|
| *Control plane not connected* after a switch | The station does not answer. Press the station's name and go back, or wait. |
| *Your sign-in has ended* | Press **Sign in again**. The sign-in screen for that station opens, with your other stations listed under it. |
| The wrong station opened | Press the station's name and pick the other. The badge and the address on each row tell two stations with one name apart. |
