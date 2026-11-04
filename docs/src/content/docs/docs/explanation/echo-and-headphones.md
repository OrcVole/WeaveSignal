---
title: "Why headphones, and why the browser cancels echo"
description: "How a presenter's headphones can put a guest's voice back on air a quarter of a second late, and the three things that keep it out."
type: explanation
status: draft
last_verified: 2026-11-04
sidebar:
  order: 3
---

Every person in the room hears everyone else in their headphones, through the desk.
If any of that sound reaches their microphone, it goes back to the desk as part of their voice, and the desk puts it on air.
Listeners then hear a guest twice: once as the guest spoke, and again a moment later, out of the presenter's microphone.

## What the rehearsals heard

In the second live rehearsal (2026-10-25) the presenter heard distinct echoes on three of the voices.
They were second copies in the programme 0.25 to 0.27 s late: the guests' voices leaking from the presenter's headphones into the presenter's microphone.
The browser's automatic gain control made it worse: while the presenter was quiet, it turned the microphone up, and lifted the leak with it.

Run again with the browser's echo cancelling on and its gain control off, no voice had a second copy beyond its own natural self-similarity.
The same held in the third live rehearsal and in the live News and Moos show.

## What keeps it out

1. **Headphones, never speakers.**
   A loudspeaker in the room is the loudest possible leak.
   Closed-back headphones at a sensible level leak least.
2. **The browser's echo cancelling on, its noise suppression on, and its automatic gain control off.**
   These are the station's defaults for every page that sends a voice into the room (`packages/ui/media.ts`), and the invite page and the call-in page use them.
   Only the echo-test page (`/echo-test/`) turns echo cancelling off, so a person can hear what the room would.
   The desk levels voices itself, so gain control in the browser has nothing to add.
3. **The presenter's gate at the desk.**
   First in the presenter's and DJ's input chain, it turns the microphone down by up to 18 dB while the presenter is not speaking (below −34 dBFS, at 1:3), so a quiet leak stays out of the programme.
   It follows the voice's envelope, opening in a millisecond and closing over 150 ms, so it does not chop the ends of words.
   GStreamer's own expander was tried first and rejected: at one setting it turned quiet signals up, at another it distorted speech.

   ![The echo test page on a phone, with its Start the echo test button.](../../../../assets/screens/reference-public-pages-echo-test.png)

## Two side effects

With echo cancelling on, a browser stops sending while the cancelled microphone is silent.
In the rehearsal, the presenter's microphone sent audio for 267 of 669 s, against 630 before.
Nothing is lost on air, but a recording of the microphone alone is shorter than the show, so it must be timed by its packet timestamps, not by counting samples.

The desk's leveller then does what gain control did, slowly and toward the guests' level: in the live rehearsals the leveller lifted the raw microphone by 13.8 to 15.9 dB, so the presenter and the guest sat within half a decibel of each other.

## Callers on phones

A caller on a phone usually has no headphones: the phone plays the call on its loudspeaker.
There the browser's echo cancelling is the only thing between the loudspeaker and the microphone, which is one more reason the call page keeps it on.
In the two-person test, with one guest on a phone, neither page heard its own tone come back in 150 trials; that test injected tones past the microphone, so echo by ear from a phone's loudspeaker is not yet measured.

A caller's return is quieter than the stream (about −23 LUFS, against −16), and a phone call has a volume of its own, apart from music's. So the call page tells callers to press volume up once the call has started (ADR-0205).

On Firefox for Android the microphone must also be allowed for Firefox in Android's own settings. Until it is, the call page says *Your microphone is blocked* and offers *Try again*.

For the steps, see [Get ready to present](/docs/how-to/get-ready/).
