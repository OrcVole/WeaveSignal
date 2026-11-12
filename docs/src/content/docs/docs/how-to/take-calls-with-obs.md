---
title: "Take calls on a video stream with OBS"
description: "Keep OBS sending your stream to Rumble, Odysee or another platform, and take your audience's calls through WeaveSignal: the call in a browser of its own, its sound captured by OBS beside your microphone."
type: howto
status: draft
last_verified: 2026-11-06
sidebar:
  order: 37
---

This is for a streamer who already streams with OBS Studio and wants their audience to call in.
OBS keeps sending your picture and sound to your platform.
WeaveSignal does the calls: your viewers call from the station's page, a screener talks to each one first, and the callers you take to air are heard by you and, through OBS, by your stream.

A streamer has two ways to put calls on a video stream:

- **Keep OBS and take the calls beside it**, as this guide does.
  Your scenes, overlays and platform settings stay in OBS.
- **Let the station send the stream itself**, with no OBS: it sends its own picture and sound to Rumble, Odysee or PeerTube ([Send your show to Rumble, Odysee or PeerTube](/docs/how-to/send-to-video-platforms/)).
  The profanity delay and Dump then act on the platform's stream too (ADR-0230).

## How the sound goes

| Sound | Goes |
|---|---|
| Your voice | your microphone into OBS, as now; and into the station, so callers hear you |
| The callers and guests | from the station to the browser tab you joined in; OBS captures that tab's sound |
| You, back from the station | nowhere: the tab plays everyone but you, so your voice is never doubled on the stream |

**Calls go out live on your stream.**
The station's profanity delay and Dump act on the station's own programme, not on what OBS sends, so the screener is your safeguard: they talk to each caller before sending them to you (ADR-0225).
If your stream needs the delay, let the station send it instead of OBS ([Send your show to Rumble, Odysee or PeerTube](/docs/how-to/send-to-video-platforms/)).

## Before you start

- **OBS Studio 28 or later on Windows 10 (version 2004 or later) or Windows 11.** It has the *Application Audio Capture* source, which takes the sound of one program.
- **Headphones**, never speakers ([why](/docs/explanation/echo-and-headphones/)).
- **A browser just for the call.** OBS captures a whole program, so use one you do not use for anything else during the show: Microsoft Edge, if you browse in Chrome or Firefox.
- **The invitation link** the station gave you, and the studio app if you drive the desk yourself ([Install the studio app on Windows](/docs/how-to/install-studio-windows/)).
- **The station's player page address** for your viewers, where they find **Call the studio**.

## Join the call

1. Open the invitation link **in the call browser** (Edge), press **Join the studio**, and allow the microphone ([Get ready to present](/docs/how-to/get-ready/)).
2. Leave the tab open for the whole show. If you close it, you leave the room and your stream loses the callers.
3. Check you hear a guest or the producer in your headphones, and that you do not hear yourself.

## Add the call to OBS

1. In OBS, under **Sources**, press **+** and choose **Application Audio Capture (BETA)**. Name it *Callers*.
2. For **Window**, choose the call browser's window (`[msedge.exe]` for Edge). Set the window match priority to the one that matches the executable, so the capture follows the browser when the tab's title changes.
3. In **Audio Mixer**, open *Callers*' **Advanced Audio Properties** (the cog) and set **Audio Monitoring** to **Monitor Off**: you already hear the callers in the browser.
4. **Mute Desktop Audio** in the Audio Mixer, or make sure it does not include the call browser. Otherwise OBS takes the callers twice and they echo on your stream.
5. Keep your microphone source as it is.

## Set the level

Callers arrive quieter than a broadcast voice: about −23 LUFS against −16 for the station's own stream ([measured](/docs/reference/release-one-status/#measured)).

1. Ask the producer, or a friend calling in, to talk.
2. On *Callers*, add a **Gain** filter and raise it until the caller's meter in OBS's Audio Mixer sits about where your own voice does: usually 6 to 8 dB.
3. Leave a little headroom; a caller who shouts is louder than one who talks.

## Tell your viewers

- Put the player page's address in your stream's description and in your chat: viewers press **Call the studio** there.
- Say on air that calls are screened, and that a caller is heard on the stream once they are put through.

## Do not

- **Do not put the station's listener stream into OBS.** It is seconds behind, so the callers' voices would drift from your picture and from your own replies.
- **Do not use speakers**, even quietly: the station would hear the callers again through your microphone.
- **Do not capture the browser you chat in or watch your stream in.** OBS would take its sound too.

## On Linux

OBS on Linux captures one program's sound through PipeWire with the *Application Audio Capture* plugin, where your distribution carries it; otherwise route the call browser's output to a virtual sink with `pw-link` or a patchbay and capture that sink in OBS.
The rest is the same.

## What has been tried

Nothing records this guide being followed with OBS from start to end yet (2026-11-10).
Each part rests on what was tried separately: the join page's call and its echo cancelling (the call-in tests and the live runs), and a browser source in OBS is OBS's own feature.
Try it once yourself, with a friend calling, before the first show.
