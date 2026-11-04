---
title: "The chat service"
description: "weavesignal-chat: its settings, routes, socket, what it keeps and for how long, and the measured load."
type: reference
status: draft
last_verified: 2026-11-04
---

`weavesignal-chat` is the live chat, its own process beside the control plane (ADR-0031).
It listens on `ports.chat` (7311) on loopback; the station's proxy sends everything under `/api/v1/chat/` to it, websockets included, never through the CDN.

![The chat on the player page at phone width: three messages, one of them held with Waiting for a moderator, and the rules above the composer, with slow mode on.](../../../../assets/screens/reference-chat-player.png)

## Settings

`weavesignal-chat sample-config` prints `chat.toml`: the station's identifier and name (the control plane's), the listen address and port, the control plane's URL, and how sign-in codes are mailed (`off`, `command` for a sendmail-compatible program, or `outbox` for files).
Secrets come from the environment: `WEAVESIGNAL_DATABASE_URL` (the station's database) and `WEAVESIGNAL_CONTROL_ADMIN_TOKEN` (the studio's bearer).

The room's modes (slow, hold new accounts, trusted-only links, influx) live in the control plane; the chat follows its studio view.
The screener's switches reach the chat at `POST /api/v1/chat/modes`, and the chat passes them on to the control plane with the caller's own bearer, so the control plane's log names who switched a mode.
The station's own chat settings (terms, the accountable person, the contact point, the minimum age, comments only, guests, blocked words, forbidden names) are kept in the database: `GET` and `PUT /api/v1/chat/admin/settings`.
In comments-only mode a chatter cannot reply, and an @mention is plain text: the name stays in the message, but the player page does not highlight it or announce it to the person named (ADR-0147).

## The minimum age

Signing up, a chatter ticks "I am *N* or older"; without the tick the chat refuses the account, and a guest's name, and says why.
Only that declaration and when it was made are kept, never a date of birth.

*N* follows the station's jurisdiction, the retention preset chosen in Settings (ADR-0148): 13 under No retention, Custom and the UK, US and Canadian presets; 15 under the Australian ones; 16 under Ireland's, Germany's and the EU preset.
A change of jurisdiction applies from the next sign-up.
A station sets its own age, 13 to 21, with `min_age` in its chat settings; leaving it out (or `null`) follows the jurisdiction again, and so does 13.
A station in Quebec sets 14.

## Routes

| Route | Who | What |
|---|---|---|
| `GET /api/v1/chat/info` | anyone | the station's terms and contacts, the rules now, report reasons, retention in words |
| `GET /api/v1/chat/socket` | anyone | the WebSocket; the session goes in the first frame |
| `POST /api/v1/chat/signin/start`, `/signin/finish` | anyone | sign in by an emailed six-digit code |
| `POST /api/v1/chat/guest` | anyone | a guest name, where the station allows guests |
| `POST /api/v1/chat/notices` | anyone | a notice from someone without an account |
| `GET /api/v1/chat/me`, `POST /signout`, `/reports`, `/statements/{id}/review` | a chatter | themselves, reports, a review |
| `POST /api/v1/chat/staff/session` | the studio | the studio's own chat session |
| `POST /api/v1/chat/mod`; `GET /mod/log`, `/mod/accounts/{id}` | the studio, or a chatter the station made a moderator | moderation |
| `POST /api/v1/chat/modes` | the studio | a room mode, passed on to the control plane |
| `GET` and `PUT /api/v1/chat/admin/settings`; `GET /admin/accounts/{id}/export`, `POST …/erase`, `GET /admin/holds`, `/admin/risk-assessment` | an admin | the station's chat settings, export, erasure, evidence holds, a risk-assessment template |

*The studio* is a staff session (`rss_…`) or the station key; *an admin* is a staff session with the `admin` role, or the station key.
An API token (`rsa_…`) opens none of the chat's routes.
These routes are not in the control plane's [API reference](/docs/reference/api/).

The frames and shapes are in `packages/ui/chat.ts` and `crates/chat/src/wire.rs`.
Every message carries the server's time and the sender's playhead (ADR-0038).
A reconnecting page sends its epoch and offset and gets what it missed; a page that falls behind is closed with 4008 and recovers.

## What it keeps

Email addresses are kept only as a keyed hash (HMAC-SHA-256 under [the email key](#the-email-key)), never the address itself.
A sign-in code is kept as a keyed hash too, for ten minutes at most.
Chat messages and moderation records follow the station's Jurisdiction setting (ADR-0113); under No retention, the default, messages go when the scheduled episode they were written in ends (6 hours after they were written, where no episode is scheduled; an episode's length is its own, or its series' or show's slot), moderation records when their 14-day review window closes, and IP addresses are not stored.
A retention hold or an escalation keeps what it names until it is released (ADR-0116); an open report stays until it is decided.

## The email key

The chat needs one secret to start: `WEAVESIGNAL_CHAT_EMAIL_KEY`, at least 32 characters (`openssl rand -hex 32` makes one).
It keys the hash an address is kept under (ADR-0125).
Someone who copies the database cannot test it against a list of addresses without the key, which a plain SHA-256 would allow.
The key stays outside the database: in the environment of the chat process, never in `chat.toml`.

- **Where it comes from.** `weavesignal.sh` makes `CHAT_EMAIL_KEY` in the settings file on its first run and keeps it on later runs; the Compose bundle passes it to the chat as `WEAVESIGNAL_CHAT_EMAIL_KEY`; the Quadlets store it as the Podman secret `<project>-chat-email-key`. Run by hand, export it before `weavesignal-chat serve`.
- **Without it.** The chat exits with code 2 and says which variable to set. There is no fallback to an unkeyed hash.
- **Keep it with the backups.** An account is found by the hash of its owner's address. With a different key nobody's address finds their account: they would sign in as new chatters, and their old names would stay taken. The chat does not rotate the key.
- **Accounts from before the keyed hash.** A station that made chat accounts under the old plain hash keeps them. The chat cannot convert a hash without the address, so it re-keys each account at its owner's next sign-in, once they have entered a correct emailed code. Until then the chat's start-up log says how many accounts still hold the old hash. Pending sign-in codes were dropped when the chat was upgraded; a chatter asks for a new one.

## Load

Measured on 2026-10-26 on one machine (`just test-chat-load`): 4,000 sockets, 40 that never read and 200 that reconnect, at 7, 20 and 60 messages a second.

| Rate | p50 | p99 | Server CPU (one core) | Resident memory |
|---|---|---|---|---|
| 7 a second | 16 ms | 29 ms | 23 % | 95 MiB |
| 20 a second | 8 ms | 21 ms | 53 % | 97 MiB |
| 60 a second | 8 ms | 24 ms | 108 % | 97 MiB |

All 200 reconnects recovered.
In a longer run (1,000 sockets, 15,000 messages) all 20 sockets that never read were closed with 4008 while the others' p99 stayed under 60 ms.
