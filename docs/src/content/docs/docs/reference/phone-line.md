---
title: "The phone line"
description: "The telephone line in 1.2 at a glance: its settings on each install route, its ports, its API routes, what a call goes through before the queue, what the station keeps of a number, and the shipped voices with their licence."
type: reference
status: draft
last_verified: 2026-11-20
sidebar:
  order: 31
---

From 1.2 listeners can ring the studio from an ordinary phone, through the station's own SIP trunk and LiveKit's SIP service (v1.25.0, Apache-2.0, run beside the room as LiveKit is).
It runs on Compose and the Quadlets (`phone.yaml`), not yet on Cloudron, and is off until a trunk is set.
It has been tried against a local SIP client, not yet through a provider's trunk.
How to set it up and use it: [Take phone calls](/docs/how-to/take-phone-calls/); whether to play the words and whose contact to give: [Decide whose contact to give callers](/docs/how-to/decide-whose-contact-to-give/).

## Settings

The control plane's `[phone]` section; on Compose and the Quadlets each is a `PHONE_*` line in the settings file `weavesignal.sh` writes.

| `[phone]` | Settings file | Default | Meaning |
|---|---|---|---|
| `enabled` | `PHONE` | `false` | take calls; refused until a trunk is set (a number, the provider's addresses or a digest user, and the public address) |
| `numbers` | `PHONE_NUMBERS` | empty | the station's numbers as the provider sends them, TOML strings |
| `allowed_addresses` | `PHONE_ALLOWED_ADDRESSES` | empty | the provider's addresses or ranges |
| `auth_username` | `PHONE_AUTH_USERNAME` | empty | the provider's digest user; its password is `WEAVESIGNAL_PHONE_TRUNK_PASSWORD` (`PHONE_TRUNK_PASSWORD`), a secret, never in a settings file |
| `public_ip` | `PHONE_PUBLIC_IP` | empty | the station's public IP address; no outside server is asked for it (ADR-0290) |
| `country_code` | `PHONE_COUNTRY_CODE` | empty | the calling code without `+`, so a typed number is found in every form |
| `lines` | `PHONE_LINES` | 8 | calls at once (4, 8 or 16 in the control plane; 1 to 16 in `weavesignal.sh`) |
| `sip_port`, `sip_tls_port` | `PHONE_SIP_PORT` | 5060, 0 (off) | SIP from the provider |
| `rtp_port` | `PHONE_RTP_PORT`, `PHONE_RTP_LAST` | 7340 to 7355 | the calls' sound, two ports a line, starting on an even port |
| `consent_words` | `PHONE_CONSENT_WORDS` | `true` | play the words and wait for a key before the queue |
| `default_voice` | `PHONE_DEFAULT_VOICE` | `alba` | the shipped recording until the station records its own: `alba`, `edinburgh` or `own` |
| `calls_per_number_per_hour` | — | 6 | calls one number may place in an hour; withheld callers share one count |
| — | `PHONE_REDIS_PORT`, `PHONE_REDIS_PASSWORD` | 7321, made by the script | Valkey on loopback, which LiveKit and its SIP service share while the line is on; nothing kept on disk |

The words' switch and the voice set in the studio (Settings › Phone line) are kept in the database and win over the settings file from the next call.
The ports, their alternatives and who may reach them: [Ports, the phone line](/docs/reference/ports/#the-phone-line).

## A call, from ringing to the queue

1. The provider sends the call to the station's SIP port; LiveKit's SIP service answers it into a booth room of its own, as a browser caller's.
2. With the words on, the desk plays them into the caller's return (the station's own recording, else the shipped voice) and holds the caller's voice out of every mix. **Any key** carries on; without one the words play twice, and after 90 seconds the call ends.
3. The caller waits in `screener.queue` like a caller from the player page, shown as `Phone ···0123` or `Withheld`. Their consent record says they pressed a key, or *not asked* when the words are off.
4. From there the screener and the desk treat them as any caller; dropping or releasing them ends the call.

The line takes calls in only: nothing can be dialled through it, and it cannot reach emergency services.

## What the station keeps of a number

A ban key (a keyed hash only this station can make) and the last four digits, never the number.
A ban keeps the ban key, the last four digits, the date and the reason; a withheld number cannot be banned by number, and caller ID can be faked.
A takedown search takes a typed number, finds its calls without keeping it, and erases them as any caller's consent is erased.
LiveKit's SIP service writes a number in its own log when a call fails, so it logs errors only, to one file of at most 512 KB that never goes to the system journal.

## Routes

All under `/api/v1/phone`; [The API](/docs/reference/api/) says which need an admin:

| Route | What |
|---|---|
| `GET /api/v1/phone` | the line's state, what is in force (`consent_words`, `default_voice`) and the help the studio shows |
| `GET`, `POST /api/v1/phone/bans`; `DELETE /api/v1/phone/bans/{ban_key}` | number bans: list, ban a number, lift a ban |
| `PUT /api/v1/phone/consent` | whether the words play, and which shipped voice |
| `GET /api/v1/phone/consent/default/{alba,edinburgh}.opus` | hear a shipped voice |
| `POST /api/v1/phone/takedown` | find a number's calls |
| `PUT`, `DELETE /api/v1/phone/words` | keep or remove the station's own recording of the words |

## The shipped voices

Both say *"You're through to the studio, and your call may go on air and be recorded. Press any key to carry on."*, about 7 seconds.

| `default_voice` | Voice | Made from |
|---|---|---|
| `alba` (the default) | a Scottish woman's | Piper's voice trained on the Alba speech corpus (University of Edinburgh, 2019) |
| `edinburgh` | a man's from Edinburgh | speaker p285 of the CSTR VCTK Corpus 0.92 (University of Edinburgh, 2019) |
| `own` | none | only the station's own recording; until there is one the line takes no calls while the words are on |

Both were made with Piper (piper-tts 1.8.0, which is GPL-3.0 and not shipped) and are released under CC BY 4.0, as their sources are; the Alba corpus also forbids any use derogatory to its voice talent.
The full attribution is in [Take phone calls](/docs/how-to/take-phone-calls/#the-recordings-that-come-with-weavesignal) and `NOTICES.md`.
