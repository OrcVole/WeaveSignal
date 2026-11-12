---
title: "Documentation"
description: "WeaveSignal's documentation: a tutorial, how-to guides for presenters, screeners and self-hosters, the reference, and how the parts fit together."
type: explanation
status: draft
last_verified: 2026-11-04
---

WeaveSignal is open-source software for a self-hosted internet radio station with the live studio built in: a room for presenters, guests and screened callers, the desk that mixes them, playout between shows, streams for listeners, a chat, show cards and podcast feeds, in one installation on your own server.
[The website](/) says what it is in a page; these pages say how it works.

The pages are of four kinds (ADR-0072): a **tutorial** teaches by doing, a **how-to guide** gets one job done, **reference** describes what is there, and **explanation** says why.
Studio words are explained in the [glossary](/docs/reference/glossary/).
What 1.0.0 does and leaves out is [What release one does, and what it leaves out](/docs/reference/release-one-status/).

## Start here

- [The simple guides](/guides/): short, plain guides for presenters, guests and whoever looks after the station; start there if you are not technical.
- [Run a first show](/docs/tutorials/first-show/): the studio app against the real desk on your own machine, with a room of synthetic people from Kettle FM, the sample station.
- [How the parts fit together](/docs/explanation/architecture/): the processes, what each does, and why the people talking and the people listening run on two different clocks.

## Presenters

- [Get ready to present](/docs/how-to/get-ready/): joining, headphones and the microphone.
- [Present from the keyboard](/docs/how-to/present-from-the-keyboard/): on air, cue, mute and talk.
- [Take a caller to air](/docs/how-to/take-a-caller/): accept, decline or drop a caller the screener sends.
- [Start on the count-in](/docs/how-to/count-in/): the count-in, and taking turns with remote guests.
- [When the lamp says PLAYOUT DOWN](/docs/how-to/playout-down/): what it means, and what to do.
- [Set up your show](/docs/how-to/set-up-your-show/): a DJ's on-air name, takedown contact, show and usual time, kept as defaults.
- [Echo and headphones](/docs/explanation/echo-and-headphones/): why presenters and guests wear headphones.

## Screeners and station staff

- [Screen callers](/docs/how-to/screen-callers/): talk to callers off air and send them to the desk.
- [Moderate the chat](/docs/how-to/moderate-the-chat/): reports, held messages and statements of reasons.
- [Schedule the week](/docs/how-to/schedule-the-week/): shows, automation blocks and corrections.
- [Set the show clock and the rundown](/docs/how-to/set-the-show/): the show, what is coming up, who was banned before.
- [Add staff and sign in](/docs/how-to/add-staff/): accounts, roles and sign-in codes.
- [Set retention](/docs/how-to/set-retention/): the jurisdiction presets, holds and the sweeper.
- [Who hears whom](/docs/reference/who-hears-whom/): every route through the desk, by role.

## Self-hosting

- [Install a station with weavesignal.sh](/docs/how-to/install-with-weavesignal-sh/): the Compose bundle in one command.
- [Publish a station from the Compose bundle](/docs/how-to/publish-with-compose/): a reverse proxy with a certificate, TURN over TLS, and the ports to open.
- [Install on Cloudron](/docs/how-to/install-on-cloudron/): the 1.0.0 package from its published image.
- [Run a station as Podman Quadlets](/docs/how-to/run-with-quadlets/): systemd units generated from the bundle.
- [Install the studio app on Linux](/docs/how-to/install-studio-linux/) and [on Windows](/docs/how-to/install-studio-windows/).
- [Check a station's ports](/docs/how-to/check-ports/): every port is a setting; find the free ones.
- [Memory and CPU](/docs/reference/resources/): what a station uses, and why live audio needs a server that is not overloaded.
- [Run edges](/docs/how-to/run-edges/) and [Put Bunny in front of the station](/docs/how-to/use-bunny/): for a big night.

## Reference and explanation

- [Glossary](/docs/reference/glossary/): the studio words.
- [The desk's panels by name](/docs/reference/surfaces/desk/), [the keymap](/docs/reference/keymap/) and [the state vocabulary](/docs/reference/states/).
- [Playout](/docs/reference/playout/), [the chat](/docs/reference/chat/), [the public pages](/docs/reference/public-pages/) and [settings](/docs/reference/settings/).
- [Control plane routes](/docs/reference/control-api/) and the [API reference](/docs/reference/api/), generated from the code.
- [Packages and install files](/docs/reference/packages/) and [licences](/docs/reference/licence/).
- [What the lamp means](/docs/explanation/on-air/): ON AIR, OPEN, AUTO and PLAYOUT DOWN.
