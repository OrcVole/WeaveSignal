---
title: "Glossary"
description: "The studio words WeaveSignal uses, in plain language: the desk, playout, the screener, mix-minus, cue, talkback, carts, the rundown, clocks, the slate, booths, the dump, the lamp states, WHIP, Quadlets and the rest."
type: reference
status: draft
last_verified: 2026-11-04
sidebar:
  order: 0
---

Radio has its own words, and WeaveSignal keeps them where a station's staff would expect them.
This page says what each one means here.
Where a word is a state the studio app shows, [The state vocabulary](/docs/reference/states/) draws it and says what to do.

## The parts of a station

**The room.**
Where the people talking are: presenters, guests and callers, each joined from a browser over WebRTC.
LiveKit holds it; the desk joins it as a hidden participant.
Each caller is in a **booth** of their own (below).

**The desk.**
The mixing desk, as software: it hears every person in the room, mixes the programme, and gives each person a mix of their own.
The design documents called its decided layout "desk B"; in the studio app and on this site it is simply the desk.
Its panels are named in [The desk](/docs/reference/surfaces/desk/).

**The studio app.**
The desktop program (Linux and Windows) that shows the desk, the screener, the cart wall, the schedule and the settings, and sends your keys to the station.
It carries no audio: presenters and guests join the room from a browser, and the app controls the desk.

**Playout.**
The automation that fills the air between and around live shows: it plays the schedule's music and recorded shows, takes the desk's programme when the desk is on air, and keeps listeners connected through a dead-air fallback.
Liquidsoap 2.5 runs it from one fixed script ([Playout](/docs/reference/playout/)).

**The listener process.**
A second Liquidsoap process that holds the listener stream, encodes it once and serves every listener, so a playout or desk restart disconnects nobody.

**The control plane.**
The station's web service: settings and data in PostgreSQL, room tokens, the public pages, the API, and the commands that drive the desk and playout.

**The screener.**
The person who answers callers off air, talks to them, writes the one line the presenter sees, and sends them to the desk or back to the queue.
Also the tab of the studio app where they work, which holds the chat's moderation as well.

**The public pages.**
The station's player with call-in and the chat, the invite page for guests, and the echo test, served by the control plane for a station's own website to link or embed.

**An edge, a relay.**
For a big night: an **edge** caches the video stream (HLS) near the viewers; a **relay** is an Icecast server that pulls the audio stream and serves listeners of its own.
**Content steering** sends each player to the edge that is up.

## In the room

**Mix-minus.**
The mix a person hears: everyone they should hear, never their own voice.
The desk makes one for each presenter, guest and caller, so nobody hears themselves back late.
[Who hears whom](/docs/reference/who-hears-whom/) lists every route.

**The programme.**
What listeners hear: the desk's mix of everyone on air, then playout's processing, then the stream.

**Cue.**
A source in your headphones only.
Cue a caller or a cart to check it before you put it on air; listeners hear nothing from it.

**Talkback, talk.**
Your voice to one person, off air, for as long as you hold the key: a word to a guest, a count-in from the producer, the screener telling a caller they are next.
Listeners never hear it.

**On, off, muted.**
*On* puts a source on air (or, in auto mode, opens it to the room only).
*Muted* cuts it from every mix.

**The gate.**
A quiet source is held down automatically so room noise and breathing do not reach the mix; a presenter's gate is set for speech.

**The profanity delay, the dump.**
Listeners hear the programme a few seconds (8 s by default) after the room.
If something must not go out, **Dump** throws that held stretch away, so listeners never hear it; the room carries on.
The desk shows the delay building after a dump until it is whole again.

**A booth.**
A room of one caller's own, which only the desk joins.
The caller hears the programme and the screener; nobody else sees or hears them until the desk takes them to air.

**A pass, an invite.**
A short-lived token that lets one person into the room with one role, carried in the link the station sends them.

**Roles.**
Presenter, guest, caller, screener, producer, remote DJ, listener: what each may do and hear is in [Roles](/docs/reference/roles/).

**TURN.**
A relay for people whose networks block WebRTC's direct connections; WeaveSignal runs LiveKit's own, over UDP and over TLS.

**WHIP.**
A standard way to send a live stream into a WebRTC server from an encoder such as OBS.
WHIP for remote DJs is planned, not in release one.

## On air

**ON AIR, OPEN, AUTO, PLAYOUT DOWN.**
The master lamp's four states.
*ON AIR*: listeners can hear this now.
*OPEN*: the room hears this source but listeners do not, because automation holds the air.
*AUTO*: automation is playing the log.
*PLAYOUT DOWN*: the room is alive but nothing reaches listeners.
[What the lamp means](/docs/explanation/on-air/) explains them.

**DESK BUSY, BANNED BEFORE, LINK DEGRADED, FAULT, ALL GOOD, STANDBY.**
The other state words: a caller the presenter declined for now; a caller matched to the banned list before they are heard; a person's connection losing packets; something the desk depends on has failed; nothing needs you; nothing is open and the next On key puts you on air.

**The rundown.**
The running order of a show: what comes next and when, shown beside the desk.

**A hard post.**
A time a show must hit, such as the news at the top of the hour; the clock counts down to it.

**A cart.**
A short sound on a key: a jingle, a sting, applause, a bed.
The **cart wall** holds five banks of 24, one per letter key, and every cart is heard in every return.

**A bed.**
A cart that plays under speech, ducked beneath the voice.

**The slate.**
The picture shown when video is on and nobody is on camera: the station's or the show's still, with now playing.

**Lower third.**
A caption over the picture naming who is speaking.

**Count-in.**
The producer's or screener's talkback before a show starts, so a remote guest knows when to speak.

## Playout and the schedule

**A clock.**
The template for an hour of automation: which categories play in which order, with the times a show must hit.

**Rotation.**
The rule that picks the next track: the one rested longest that breaks no other rule.

**Separation.**
How long must pass before the same artist, song or album plays again; relaxed only when nothing else fits, and the log says so.

**An automation block, a live show.**
The week is made of both: blocks that playout fills from a clock, and shows that people present.

**Dead air, the fallback, the default loop.**
Silence on the stream is dead air.
When the desk is away and the schedule has nothing, playout plays the station's fallback folder, then a loop WeaveSignal ships, then silence, and listeners stay connected throughout.

**LUFS.**
The unit of perceived loudness.
Playout brings the programme towards −16 LUFS before the stream, so songs and voices sit at one level.

**The per-play log, the legal logger.**
Every track played, with its time, for reporting; and an hourly recording of the output, kept as long as the station's retention says.

**Now playing.**
The title on air now, served by the API for the station's website and player.

## Listeners

**The stream.**
Ogg Opus by default, and an optional MP3 stream for older players and directories that cannot take Opus.
Served Icecast-style, so existing players and relays can take it.

**HLS.**
The video stream's format: short files a player fetches in turn, cached by edges or a CDN.

**The two latencies.**
The people talking need to hear each other at once; listeners can be, and are, seconds behind.
[How the parts fit together](/docs/explanation/architecture/#two-latencies) explains why.

## Packaging

**The Compose bundle, `weavesignal.sh`.**
The station as a set of containers described in one Compose file, and the script that installs it in one command.

**Cloudron.**
A self-hosting platform that runs apps on your own server with backups and updates.
WeaveSignal has a Cloudron package, installed with Cloudron's command-line tool from its published image.

**Quadlets.**
Podman's way of running containers as systemd units.
WeaveSignal generates them from the Compose bundle.

**The station key.**
The station's own bearer token, with an admin's powers, made at install; it signs the first staff account in.

**Retention, holds, jurisdiction.**
How long each kind of record is kept, from a preset for the station's country; a hold keeps what it names until released.
