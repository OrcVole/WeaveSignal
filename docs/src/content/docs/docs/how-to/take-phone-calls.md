---
title: "Take phone calls"
description: "Let listeners ring the studio from an ordinary phone through a SIP trunk: the settings, the consent words and when you need them, recording your own words, number bans, takedowns, and what the line cannot do."
type: howto
status: draft
last_verified: 2026-11-20
sidebar:
  order: 11
---

:::caution[New in 1.2, not yet tried with a real trunk]
The phone line is off by default, and `weavesignal.sh` refuses `PHONE=true` until a trunk is set.
It runs on Compose and the Quadlets (`phone.yaml`), not yet on Cloudron.
The control plane's and the desk's sides are built and tested on their own, and LiveKit's SIP service has answered a test call, but no call has yet come through a provider's trunk to a running station.
:::

A phone caller rings the station's number, hears a few words, presses a key, and then waits in `screener.queue` like a caller from the player page.
WeaveSignal runs LiveKit's SIP service beside the room for this, and needs a SIP trunk from a provider: a number that sends calls to the station's address (an origination URI).
The station never calls out, so nothing can be dialled through it.

## What the station keeps of a number

Never the number.
For each call the station keeps a ban key (a keyed hash that only this station can make) and the last four digits, which the screener sees as `Phone ···4471`.
A caller who withholds their number shows as `Withheld`.
One exception is outside WeaveSignal's own records: LiveKit's SIP service writes the caller's number in its log when a call fails.

### The SIP service's log

WeaveSignal keeps that log as brief as the container engine allows:

- The service logs **errors only**: at its usual level it would log every caller's number on every call.
- On **Compose**, its log is one file of at most 512 KB, overwritten from the start when it fills, and deleted with the container (`weavesignal.sh --uninstall` removes it). It never goes to the system journal, which is where Podman sends logs unless told otherwise, and which keeps them as long as the machine is set to.
- On the **Quadlets**, the same: the unit sets `LogDriver=json-file` and `LogOpt=max-size=512k`.
- A copy you take yourself (`podman logs` into a file, a support bundle) keeps whatever numbers are in it: delete it when you are done.

Read it with `podman logs` and the SIP service's container name (`podman ps` lists it).

## Turn the line on

The settings are in the `[phone]` section, and every one is off or empty to begin with.
On Compose and the Quadlets they are the `PHONE_*` settings in the settings file (`.env.example` lists them): `PHONE=true` adds Valkey and LiveKit's SIP service, and `weavesignal.sh` then checks the line's ports, chooses free ones and makes Valkey's password ([Ports](/docs/reference/ports/#the-phone-line)).

| Setting | What it is |
|---|---|
| `enabled` | Take phone calls. Refused until a trunk is set: `numbers`, `allowed_addresses` or `auth_username`, and `public_ip`. |
| `numbers` | The station's number as the provider sends it. |
| `allowed_addresses` | The provider's addresses, as IPs or ranges (`203.0.113.0/24`). |
| `auth_username` | The provider's digest user, if it offers digest auth. The password is the environment variable `WEAVESIGNAL_PHONE_TRUNK_PASSWORD`, never the settings file. |
| `public_ip` | The station's public IP address. WeaveSignal does not ask an outside server for it. |
| `country_code` | Your calling code without `+` (`44`), so a number typed for a ban or a takedown is found in every form. |
| `lines` | Calls at once: 4, 8 (the default) or 16. |
| `sip_port`, `sip_tls_port` | SIP, 5060 by default; TLS is off (0). A port other than 5060 sees fewer scanners. |
| `rtp_port` | The first of the media ports: an even port, with two ports for each line (7340 to 7355 for 8 lines). |
| `consent_words` | Play the consent words first (below). On by default. |
| `default_voice` | Which recording that comes with WeaveSignal plays until you record your own: `alba` (the default), `edinburgh` or `own` (below). |
| `calls_per_number_per_hour` | Calls one number may place in an hour (6). Withheld callers share one count. |

Open the SIP port and the media ports to the provider's addresses, and no wider than you need ([Check the ports](/docs/how-to/check-ports/)).

## The consent words

Before a phone caller joins the queue, the station plays them a few words: who they have reached, that the call may go on air and be recorded, and to press a key to carry on.
Any key carries on.
Without a key, the words play twice, and after 90 seconds the call ends.

The words are a switch: `consent_words` (`PHONE_CONSENT_WORDS` on Compose and the Quadlets), on by default.
The studio shows this help beside it (`consent_words_help` in `GET /api/v1/phone`):

> Before a phone caller joins the queue, play them a few words: who they have reached, that the call may go on air and be recorded, and to press a key to carry on. Your own recording plays if you have made one, otherwise a short one that comes with WeaveSignal. Leave this on if you broadcast on FM, AM or DAB (a broadcaster's code usually asks you to tell callers the call is recorded for broadcast), if you record or keep calls (the archive, a podcast, clips), or if your callers may not know they can be heard on air. An internet-only station may turn it off if it never records or keeps calls and the number is only ever given out on air as a live phone-in, so that calling it is itself agreeing to be on air. With it off, callers go straight into the queue and the station's records say no words were played. This is not legal advice: the rules differ from country to country, so if you are unsure, leave it on.

**With the words off, callers press no key**: they go straight into the queue as soon as they are through, and their consent record says *not asked*, never that they agreed.
[Decide whose contact to give callers](/docs/how-to/decide-whose-contact-to-give/) walks through whether your station needs the words, and whose contact they should give, step by step.

### The recordings that come with WeaveSignal

Until you record your own, the station plays a short recording that comes with WeaveSignal, about 7 seconds long:

> You're through to the studio, and your call may go on air and be recorded. Press any key to carry on.

There are two voices, chosen with `default_voice` (`PHONE_DEFAULT_VOICE` on Compose and the Quadlets, and in `weavesignal.env` on Cloudron):

- `alba`, the default: a Scottish woman's voice.
- `edinburgh`: a man's voice from Edinburgh.
- `own`: neither. Only your own recording plays, and until you have made one the line takes no calls while the words are on (`GET /api/v1/phone` says `no_recording`).

Your own recording, once made, always plays instead of either voice.

### Change them from the studio

An admin can change both from the studio, under Settings, Phone line, while the line is on: the switch **Play the consent words**, and the choice of voice, with **Play** beside each voice to hear it first.
What you change there is kept in the station's database and wins over `consent_words` and `default_voice` in the settings file, from the next call (`PUT /api/v1/phone/consent`; the voices are at `GET /api/v1/phone/consent/default/alba.opus` and `edinburgh.opus`).
`GET /api/v1/phone` says what is in force: `consent_words` and `default_voice`.
Choosing a voice while the station has its own recording chooses what plays if that recording is removed; the station's own still plays.

The studio shows this help beside the choice (`default_voice_help` in `GET /api/v1/phone`):

> Which recording of the phone words plays when the station has not recorded its own: alba, a Scottish woman's voice (the default), or edinburgh, a man's voice from Edinburgh; both say "You're through to the studio, and your call may go on air and be recorded. Press any key to carry on." Choose own to play only the station's own recording: until there is one, the line takes no calls while the words are on. The station's own recording, once made, always plays instead of either.

**Where the voices come from.** Both are synthetic: they were made with Piper (piper-tts 1.8.0) and its voices from rhasspy/piper-voices (MIT).
The Alba voice was trained on the Alba speech corpus (C. Valentini-Botinhao and J. Yamagishi, University of Edinburgh, 2019, <https://doi.org/10.7488/ds/2506>); the Edinburgh voice is speaker p285 of the CSTR VCTK Corpus 0.92 (J. Yamagishi, C. Veaux and K. MacDonald, University of Edinburgh, 2019, <https://doi.org/10.7488/ds/2645>).
Both voices were finetuned from a base trained on NVIDIA's Hi-Fi Multi-Speaker English TTS Dataset (reader 92, Cori Samuel, from LibriVox), <https://www.openslr.org/109/>.
All three are CC BY 4.0, and the recordings are released under CC BY 4.0 too ([NOTICES.md](https://forgejo.weavesignal.top/WeaveSignal/weavesignal/src/branch/main/NOTICES.md)).
**The Alba corpus adds one condition:** "The Creative Commons licence does not affect the moral rights of the voice talent with respect to the recordings. Any use derogatory to the voice talent is prohibited." Do not cut, change or use the Alba recording in a way that would be derogatory to the person whose voice it was made from.

Piper itself, which is GPL-3.0, is not part of WeaveSignal: only the two recordings it made are shipped. That the GPL does not reach a program's output is the maintainers' reading (the usual reading of the GPL's FAQ), not yet confirmed by anyone else.

### Your own words

To name your station, how long calls are kept and how to ask for a takedown, record the words the studio shows you under Settings, Phone line.
They are the player page's consent words with your station's own takedown contact (not a show's, so the recording does not go out of date at every change of show), then "Press any key to carry on."
When any of them changes (the station's name, the Jurisdiction setting, the contact), the line closes until you record them again.

## End a call

A caller still hearing the words is not in `screener.queue` yet, so nobody can take or drop them until they press a key (or, with the words off, until they are through).
**Drop** hangs up a phone caller, waiting or on air, as it ends a caller's call from the player page, and keeps no place for them.
A caller who rings again is a new call and goes through the words and to the back of the queue.

## Ban a number

In `screener.queue`, ban a phone caller as you would any caller ([Screen callers](/docs/how-to/screen-callers/)).
The ban keeps the ban key, the last four digits, the date and the reason.
An admin can also ban a number a caller gives, in every form it may arrive in.

A withheld caller cannot be banned by number: every withheld caller looks the same to the station, so the button says *Cannot ban a withheld number*.
Caller ID can be faked, so a ban by number keeps out most repeat callers, not all.

## Take a call down

When a caller asks for their call to be taken down, an admin types the number they give into the takedown search.
It finds their calls without keeping the number, and each is erased as any caller's consent is ([Set retention](/docs/how-to/set-retention/)).
A withheld call cannot be found by number: find it by its date, time and show.

## Emergencies

The line takes calls in only and cannot reach emergency services.
If a caller is in danger, tell them to hang up and dial the emergency number.
Your provider keeps its own records of each call.
