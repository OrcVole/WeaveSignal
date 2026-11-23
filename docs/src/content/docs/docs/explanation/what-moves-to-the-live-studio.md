---
title: "What moves to the live studio"
description: "What a DJ sets up in the rehearsal studio, where each part goes when the rehearsal ends, why the move is a press and not automatic, why every guest now arrives standing by, and why the rehearsal desk cannot reach the air."
type: explanation
status: draft
last_verified: 2026-11-22
sidebar:
  order: 4
---

> **From 1.3.0.**

A DJ rehearses a show with their guests, and then it is time to go live.
The question is what they have to do again.
The answer should be nothing, and for nearly everything it is.
This page says what carries over, what does not, and why the move works as it does.
How to do it: [Rehearse before your show](/docs/how-to/rehearse-before-your-show/).

## What a DJ sets up, and where it lives

Most of what a DJ prepares already belongs to the show, not to a room.
So it needs no carrying: the live studio simply has it.

| What they set up | Where it lives | At the move |
|---|---|---|
| The show's carts and banks | the show's own rows, and the station's | nothing to do: the live desk fires the same carts |
| The episode's log, card, rundown and the show's step | the show and its episode | nothing to do: in force when the show is on air |
| Their on-air name | their account | nothing to do |
| Their microphone and each guest, connected | a link each, and the room they are in | **move**, on the same links |
| Who is in the mix and who is muted | the rehearsal desk's switches | **the opening state**, carried and applied at the handover |
| Levels | none: the desk has no faders; there is an input chain for each role and an automatic leveller | not carried: the live leveller learns a moved person's level while they stand by |
| `cue`, `talk` and `talk_to` | momentary keys | not carried |

That leaves two things that live only in the rehearsal room: the people, and who was on and who was muted.

## The people move; nothing is redone

Making the guests close their pages and open new links is the "redo" a DJ would hate.
So the move keeps their links.
Each person's page is sent a new pass for the live room, on the connection it already has, and swaps it into its address so a reload keeps it.
The page leaves the rehearsal room keeping its microphone, and joins the live room.
Nobody hears the join, because they arrive standing by, before the post.

The control plane does this on routes that already exist, so the public proxy's list of routes does not change.

## Why a press, and not the clock

The move is an explicit button, **Move to the live studio**, for three reasons.

1. **Joining the live room used to put a presenter or guest on air at once.**
   A move that happened by itself could put a rehearsing guest's "can you hear me?" into the show on air before.
   The press is where the station marks everyone it moves as standing by.
2. **The handover already decides who goes on air.**
   The post's rule and the two presses of `⇧⏎` do that ([Hand over to the next DJ](/docs/how-to/hand-over-to-the-next-dj/)).
   The opening state joins that act; it never makes an act of its own.
3. **The DJ chooses when to stop rehearsing.**
   A move timed by the clock would cut a run-through in the middle of a sentence.
   The handover window tells the rehearsal when it is time (*On air at 14:05: move by then*); it does not do the moving.

A DJ who is still rehearsing at the post meets the rule that already exists: the late count, then the grace, then automation covers.

## Everyone arrives standing by

This is a change in 1.3.0 that reaches beyond rehearsal.
Until 1.2 a guest went on air the moment their link joined, and so did a presenter who came by an invitation.
That put a guest's first test words into whoever's show was on air.

From 1.3 **every guest, and everyone who comes by an invitation, arrives standing by**.
They are at the desk, in **Not on air**, heard on cue and by talk, and off the air until a press puts them on: their slot's key, the handover, taking the air back from automation, or the schedule's bring-on.
A person moved from rehearsal stands by in the same way, until a press puts them on, and the opening state is that press.

The desk knows because the control plane signs it into their room token as `weavesignal.standby_until`.
The desk reads it when it first sees the person.
Because it lives in the room's own record of the person, a live desk that restarts before the handover finds them standing by again.
A desk that restarts later sees everyone afresh, and the control plane puts back on air the people it had on, as the desk finds them present ([The desk's control socket](/docs/reference/desk-control/#arriving-standing-by)).

A presenter who joins by an invitation, including their own link, stands by too.
Taking the air back from automation for their show puts them on with the show's presenter, so the usual press is still the only one.

## The opening state

When the move is pressed, the station reads who is *In the mix* and who is muted at the rehearsal desk, among the people present, and writes that on the session before anything else is done.
That is the **opening state**.

It is applied **the moment the show's host is put on air at the live desk, by any route**: the schedule's bring-on, the handover's first press, taking the air back from automation, or their own **On** key.
Taking the air back counts as the moment when the show on air by the schedule is the show that was rehearsed.
Then, once:

1. the muted are muted;
2. the others who were in the mix are put on air with the station's fade.

Rules around it:

- **Nobody absent is touched.** A person who is not in the live room with their link up is skipped.
- **It is applied once**, and kept until the airing's start plus the grace has passed, so a live desk that restarts in between can be put right.
- **No host in the move?** The opening is applied at the first handover for that show.
- **A new move replaces an old one** for the same show.
- **It lapses with its airing.** An opening never applied is dropped when that airing ends.

The opening never switches anyone on by itself, and never on arrival.
The handover is still two presses of the DJ's own.

## Why the rehearsal desk cannot reach the air

The rehearsal studio is a **second process**, not a second mix inside the live desk.
The live desk's panic hook aborts the whole process: a fault hit by a rehearsing guest would take the show on air down with it.
With a separate process, the worst a rehearsal fault can do is restart the rehearsal desk.

It also lets the wall to the air be a property of what the process is, not of the mixing logic:

- **It makes no programme output.** Its programme bus exists inside the mixer, because each person's return is built from it, and nothing carries it away.
- **It refuses to start with video or hosts' own studios on**, with the reason in the log (exit 2), and never opens the sources port.
- **Its control socket takes only nine requests**: `status`, the switches `on_air`, `mute`, `cue`, `talk`, `talk_to`, and the carts `cart_fire`, `cart_stop`, `cart_stop_all`.
  Everything else, including any added later, is refused before it is read.
- **Its room is named `<room>-rehearsal`**, outside every prefix the live desk looks for in both directions: callers' booths, phone booths and monitor rooms.
  A live desk refuses a room that ends `-rehearsal`.
- **A rehearsal pass buys a rehearsal token only**, whatever its role.
  The room comes from what the pass is signed for, not from a name a page can choose.
- **The station keeps the two apart**: each link drives only a desk that reports its own mode, a rehearsal key goes through its own route, and nothing it plays is written to the per-play log or sent to webhooks, now-playing or the chat.

The words are the ones the lamp keeps: in the rehearsal a person is *in the mix* or *not in the mix*, never on air, and the lamp says REHEARSAL, never ON AIR or OPEN ([What ON AIR promises](/docs/explanation/on-air/)).

## Why not a full second studio

A second desk that also feeds playout, with the handover switching between them, was the other design.
It was not built because:

- **A desk that can be switched to the air can be switched there by mistake.**
  This one has no way to the air at all.
- **It costs what is expensive.** The programme's encode, the delay, HLS with video and RTMP out would all be doubled.
  The Cloudron package peaked at 1,136 MiB of its 2,048 MiB with video on in the 1.2.0 memory gate; a second full desk would not fit beside it.
- **The live studio is built around one live desk**: the post, the seat that follows the air, callers' booths, the phone line, the screener, the monitor links, the profanity delay and the per-play log.
  Making each follow "whichever studio is live" would rewrite every one.

Nothing in this design rules a second live studio out later.
Nothing is redone in this one either: the reconnection costs the people a moment while they stand by, and nobody hears it.

## When something goes wrong

| What happens | What it means |
|---|---|
| The rehearsal desk stops | it is started again by its supervisor within seconds; the live desk is untouched; the view says it is not answering |
| The live desk is down at the move | the move still goes ahead; people join the live room, and the restarted desk keeps them standing by |
| The control plane restarts in the middle | the session and the move are read back from the database; the rehearsal keeps mixing |
| A page was offline for the move | it is offered the move again when it reconnects, until the airing ends |
| A moved person's link fails in the live room | the usual rejoin; they are still standing by |
| The live desk restarts after an early handover | the people it had on air are put back as the new desk finds them present, within a minute; the opening is kept until the airing's start plus the grace for the same reason |
| The server runs short of memory | the rehearsal desk is the first process the system stops (its out-of-memory score is 500); the live studio carries on |

## See also

- [Rehearse before your show](/docs/how-to/rehearse-before-your-show/).
- [The rehearsal studio](/docs/reference/rehearsal-studio/): the setting and the rules.
- [The rehearsal studio's data](/docs/reference/studio-rehearsal-data/): the routes.
- [Roles](/docs/reference/roles/) and [Who hears whom](/docs/reference/who-hears-whom/).
