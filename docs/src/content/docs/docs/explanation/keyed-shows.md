---
title: "What a key does and does not do"
description: "Why a keyed show uses a key per person and a token for one show's window, what that stops and what it cannot, how a leak is traced and what the station keeps about a listener while it does."
type: explanation
status: draft
last_verified: 2026-11-24
sidebar:
  order: 5
---

> **From 1.3.2.** The rules, keys and tokens are built, and so is what keeps a keyed show from the public: playout's gate, the station's keyed paths, the public picture and RTMP out held back ([the reference](/docs/reference/keyed-shows/#how-a-keyed-show-is-kept-from-the-public) lists what is not covered yet), the player's Enter a key and the studio app's Keys tab.

A show that is advertised to everyone and heard only by some is a promise about who may listen.
This page says what the software keeps of that promise and what it cannot.

## What it is for

Someone without a valid key cannot get the keyed show from the station: not from the public stream, which carries a slate meanwhile, not from the stream's address, not from the picture, and, from 1.3.3, not from the show's podcast feed or episode files.
The cut is made in playout, where the programme is chosen, on playout's own clock: nothing after it decides what is keyed, so nothing after it can let a keyed moment through.
While a keyed show is on, the public hears the station's slate: its slate file, else its fallback files, else the loop WeaveSignal ships, so a new station with an empty fallback folder still plays something; a spoken slate that says when the public programme returns is for the station to record and put in its fallback folder (no package names a slate file of its own in 1.3.2 or 1.3.3).
A key can start later, end, be switched off, be limited to a number of devices or listens, and be ended for good.
Switching it off or ending it stops someone already listening at once, at the station and at every edge that reads the station's revoked list; the other changes reach them at their player's next renewal, about a minute later.

## What it cannot stop

A key limits who may listen, not what a listener does with the sound.
A person who has the key can record the show, and read it out to a friend.
The limits (devices, listens, people) and the leak report reduce sharing; they do not stop it, and the station should not promise otherwise.
Some music licences and contracts make restricting a show's audience a condition: that is for the station to check, not something the software can promise.

## Why one key per person

If a station gives every member the same key, a leaked key serves everyone, and ending one membership means changing everyone's key.
So the station issues a **family**: one key per recipient, from one template.
Every copy is its own key, which gives three things for free:

- a key found in a post or a forum says whose it was, and which batch;
- one key can be replaced (revoked, and a new one issued to the same person) while every other goes on working;
- use is counted per key, so a key that suddenly appears on many addresses stands out.

That is the leak report.
It is a hint and not a verdict: a person on a phone changes address, so it sums several signs (addresses beyond what the key allows, devices beyond it, a burst of new addresses in an hour) and says which.

## Why a signed token for the show's window

Checking a key on every request would need the control plane in every listener's path, and 2,000 listeners a minute is a lot to ask of one database.
So the key is checked when it is entered and each time the player renews, about once a minute; in between, the edge and the station check only a signature, a clock and a short list of revoked keys.
The renewal is also the proof of a device in use: a closed browser stops renewing, and after a few minutes its place on the key is free.

The token lasts the show's window rather than a minute or two, because players that cannot change the address mid-play (Safari's own player, and later radio apps, which cannot take a key yet) must keep playing; a revoke reaches them through the revoked list instead.

The signature is ECDSA P-256, so an edge run by someone else holds only the station's public key: it can check tokens and cannot make one.
(Ed25519 would have been the first choice; the edge's nginx cannot check it.)

## What the station keeps

A key holds a label (a name, a group, a ticket number), a note and optionally an email address.
The text and the email are sealed in the database, and a copy of the database alone gives neither.
For the leak report the station keeps, per key, device and hour, a keyed hash of the listener's address (an IPv6 address as its /64) cut to 64 bits: it tells addresses apart and cannot be turned back into one.
Use records are deleted after 90 days (a setting), a key's whole record is erased by an admin on request, and a key's name and email are never in a webhook.
The leak report never shows an address.
