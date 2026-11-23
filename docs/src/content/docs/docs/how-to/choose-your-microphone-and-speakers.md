---
title: "Choose your microphone and speakers (audio devices)"
description: "Choose which microphone, speakers or headphones WeaveSignal uses: the audio devices are chosen in the browser and in Windows' sound settings, not in the studio app. Every place sound goes in or out, how to pick a device in Firefox, Chrome, Edge and Brave, and what to do when no device is listed or there is no sound."
type: howto
status: draft
last_verified: 2026-11-23
sidebar:
  order: 2
---

This page is about audio devices: which microphone you speak into, and which speakers or headphones you hear through.
**WeaveSignal has no list of audio devices of its own.**
Its pages use the microphone the browser gives them and play through the output the browser uses; the studio app plays nothing of the station.
So you choose a device in the browser and in Windows' sound settings, as below.

Wear headphones, never speakers, whenever your microphone is open: sound from speakers goes back into the microphone and on air as an echo ([why](/docs/explanation/echo-and-headphones/)).

## Where sound goes in and out

| What you use | Microphone | Speakers or headphones |
|---|---|---|
| **The studio app** (Windows, Linux) | none: it shows the desk and sends your keys | none, except **Settings › Phone line**, which plays its sample voices and can record the line's words |
| **Your microphone link or an invitation** (a browser tab) | your voice to the desk | the studio: everyone else, and the carts |
| **The player's call-in** (a browser tab) | a caller's voice | the programme while the caller waits; the studio once they are on air |
| **The player** (a browser tab) | none | the station, as listeners hear it |
| **The echo test**, `/echo-test/` (a browser tab) | your microphone, played back to you | your own voice, half a second late |
| **Your monitor link**, for a host in their own studio (a browser tab) | none | your monitor mix |
| **Your own software** through the bridge, Icecast, SRT or VBAN | chosen in that software (BUTT, OBS, Mixxx, Voicemeeter) | chosen in that software |

Every browser tab in the table follows the same two rules: the microphone is the one the browser chooses for the page, and the sound plays through the browser's output, which is Windows' default unless you give the browser another.

## Choose the microphone

Choose it **before** you press **Join the studio**: the page takes the microphone as it joins.

**Firefox.**
When the page asks for the microphone, Firefox's question has a list of microphones.
Choose yours there, then **Allow**.
If you allowed the wrong one before, click the microphone icon at the left of the address bar, remove the permission, reload the page and choose again.

**Edge.**
Edge's question has a list of microphones too, with a level bar: it reads *Use available microphones (1)*, shows the microphone that is Windows' default, and has the buttons **Allow while visiting the site**, **Allow this time** and **Never allow**.
Choose the microphone in its list before you allow.
To change it later, open **Settings › Privacy, search, and services › Site permissions › Microphone** and choose it in the list at the top (the page also lists the sites you allowed, each with a bin to remove it), or click the icon at the left of the address bar, then **Reset to default** under **Microphone**, and reload the page.

**Chrome and Brave.**
These have been reported to ask only *allow or block*, and to use the microphone set in their own settings, which is Windows' default unless you change it (not checked on Windows in the run below):

- Chrome and Brave: **Settings › Privacy and security › Site settings › Microphone**, then choose the microphone in the list at the top.

Then reload the page, or press **Try again** if the page offers it.

**Windows' default.**
In Windows 11, **Settings › System › Sound**, then under **Input** choose the microphone; in Windows 10, **Settings › System › Sound › Input**.
A browser that follows the default uses it from the next page it opens.

## Choose the speakers or headphones

No WeaveSignal page has its own output chooser: the browser plays through Windows' default output, or the output Windows gives that browser.

- **The default for everything**: Windows 11, **Settings › System › Sound**, then under **Output** choose your headphones; Windows 10, **Settings › System › Sound › Output**.
- **One browser to the headphones, the rest to the speakers**: Windows 11, **Settings › System › Sound › Volume mixer**, find the browser in the list, and set its **Output device**; Windows 10, **Settings › System › Sound › Advanced sound options** (*App volume and device preferences*). The browser appears in the list once it has played a sound; restart the browser if it keeps the old device.

On Linux, choose the devices in the desktop's sound settings, or per program in PulseAudio Volume Control (`pavucontrol`) on its **Playback** and **Recording** tabs.

## Check what you chose

- **The echo test**: open your station's public address followed by `/echo-test/`, press **Start the echo test** and speak. You hear yourself half a second late, with echo cancelling off, so you hear what the room would. It joins nothing.
- **Check your microphone** on an invitation page: its level bar moves when you speak, before you join.

![The echo test page with its Start the echo test button.](../../../../assets/screens/reference-public-pages-echo-test.png)

## When there is no device, or no sound

| What you see or hear | Do |
|---|---|
| The page says *Your microphone is blocked* (chip: **No microphone**) | Allow the microphone for the page: the lock or the site settings by the address. Then **Try again** |
| *No microphone was found. Plug one in or use a phone, then try again.* | The browser sees no microphone at all: see the next two rows |
| *Another app is using the microphone. Close it, then try again.* | Close the program that holds it (a call or a recorder), then **Try again** |
| *The microphone could not be started.* on an address that begins `http://` | Browsers give a page the microphone only over `https://`, or on `localhost`. Open the station's `https://` address instead |
| The browser never asks, and the page stays blocked | Windows refuses the browser: **Settings › Privacy & security › Microphone**, turn on **Microphone access**, **Let apps access your microphone** and **Let desktop apps access your microphone** |
| The browser lists no microphone, or not yours | Plug or pair it, check it under **Settings › System › Sound › Input** (its level moves when you speak), then restart the browser: some browsers read the list of devices only when they start |
| You joined with the wrong microphone | Choose the right one as above, then reload the page: it joins again and takes the microphone afresh |
| You hear nothing from any page | The browser plays to another output: choose your headphones as above, and check the browser tab is not muted (its speaker icon) |
| You hear the echo test but not the studio | Nobody is talking and no cart is playing, or nobody is on air yet: ask someone to speak, or fire a cart ([No sound?](/guides/no-sound/)) |
| A Bluetooth headset sounds thin once its microphone is in use | Bluetooth headsets switch to a low-quality mode while their microphone is open. Use a wired headset, or a separate microphone with the Bluetooth headphones |
| On a phone, the page stays blocked in Firefox | Allow Firefox's microphone in Android's settings too: Apps, Firefox, Permissions, Microphone |

Verified on Windows 11 (build 26100, en-GB) with Edge 158 · 2026-11-23, on a test page served from `localhost`: **Settings › System › Sound** (the output and input lists, each with **Volume**), **Volume mixer** (an **Output device** and an **Input device**, and the apps that have played sound: at first only *System sounds*, then *Microsoft Edge* once a test page had played a tone, with its own **Output device** and **Input device**, both *Default*), **Settings › Privacy & security › Microphone** (**Microphone access**, **Let apps access your microphone**, **Let desktop apps access your microphone**), and Edge's question, its site-information panel and its Site permissions page, as above.
Not run: Firefox, Chrome and Brave on Windows; a Bluetooth headset; Windows 10; a real WeaveSignal page.

## See also

- [Get your first sound out in ten minutes](/docs/tutorials/first-sound/): the whole road, from a silent studio app to your voice on air.
- [Get ready to present](/docs/how-to/get-ready/): headphones and the microphone's settings before a show.
- [Send through the bridge](/docs/how-to/send-through-the-bridge/): your own software, with its own device choices.
