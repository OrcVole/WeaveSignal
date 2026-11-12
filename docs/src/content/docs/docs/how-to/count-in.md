---
title: "Start on the count-in, and take turns"
description: "How the producer or screener counts a presenter in without listeners hearing, and how to leave room for the delay when talking with remote guests."
type: howto
status: draft
last_verified: 2026-11-06
sidebar:
  order: 4
---

## The count-in

Someone with the `producer` or `screener` role counts the presenter in on talkback.
Talkback reaches presenters and DJs only, never the programme and never a guest ([Who hears whom](/docs/reference/who-hears-whom/)).

1. The screener holds `⌃T` on the Screener tab and counts: "In three, two, one."
   Every presenter and DJ hears it; nobody else does.
2. They let go on "one".
3. The presenter starts.

![The screener's status bar while ⌃T is held: the button reads Talk to Eilidh and is lit, and only the presenter hears.](../../../../assets/screens/how-to-count-in-talkback.png)

The studio app has no producer seat; a producer's talkback is the control plane's `talk` command with the producer's identity ([`POST /api/v1/desk/commands`](/docs/reference/api/#post-apiv1deskcommands)).

The presenter can answer the screener without going on air by holding `⌃T` on the desk: while it is held, only the screener hears them.

The rehearsals found that a show begun with nothing said is a show the presenter leaves: in the first live attempt the presenter left after 31 s.
A count-in, and a word on what is next, is what fixed it.

## Taking turns with remote guests

Everyone hears everyone else a little late, because every voice goes to the desk and back.
On loopback the delay is under a tenth of a second; over the internet, in a test through two VPNs, it was about half a second from one person's voice to another's headphones.

- Leave a beat after a guest finishes before you answer, and expect the same from them.
  In the rehearsals the synthetic guests answered after one second of the presenter's silence; at two seconds the conversation felt "as though they were asleep".
- Do not talk over a remote guest to hurry them: they hear you late, and both of you stop.
- To hand over, finish your sentence, then pause.

When the next show is another DJ's, hand over with an overlap: [Hand over to the next DJ](/docs/how-to/hand-over-to-the-next-dj/).
