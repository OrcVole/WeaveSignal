---
title: "Run the loopback stack with Compose"
description: "Run the whole station on one machine with Podman or Docker Compose, every listener on loopback, for development and tests."
type: howto
status: draft
last_verified: 2026-11-04
sidebar:
  order: 32
---

> **By hand, on loopback.** The Compose bundle (`deploy/compose/compose.yaml`) runs the whole station on one machine with every listener on loopback: LiveKit, the desk, playout and the listener process (Liquidsoap 2.5 with `icecast.server`), the control plane, the chat and PostgreSQL.
> On its own it has no reverse proxy and no TURN, so nobody off the machine can join or listen.
> [`weavesignal.sh`](/docs/how-to/install-with-weavesignal-sh/) installs this bundle in one command, making the secrets and choosing free ports; this page is the same by hand. To put it on the internet, with a reverse proxy, a certificate and TURN over TLS, see [Publish a station from the Compose bundle](/docs/how-to/publish-with-compose/) (`public.yaml`).

## Before you start

- Podman 5 with `docker-compose` as its compose provider, or Docker with `docker compose`.
  Not `podman-compose`: it ignores the settings file each service is given (the top-level `configs`), so the services start without them.
  On Podman, `docker-compose` reaches Podman through your user's API socket: `systemctl --user enable --now podman.socket` if it cannot connect.
- The repository, checked out where it will stay: the bundle mounts playout's scripts (`playout/`) and the default loop (`assets/default-loop/`) from it.
- A server that is not overloaded ([Memory and CPU](/docs/reference/resources/#what-this-means-for-a-host)).

## The images

The bundle runs the published server image, `forgejo.weavesignal.top/weavesignal/weavesignal-server:1.0.0`, which holds the desk, the control plane, the chat and the public pages.
Liquidsoap 2.5.0-rc1, LiveKit 1.13.10 and PostgreSQL 18 come from their upstream images.
Compose pulls them all on the first `up`.

To run an image built from the repository instead, from its root:

```sh
podman build -t localhost/weavesignal-desk-spike:dev spike/desk-image
just desk-image
just server-image
```

The first two build the desk's base and build images; the third builds `localhost/weavesignal-server:m1`.
Set `SERVER_IMAGE=localhost/weavesignal-server:m1` in `.env` to use it.

## Fill in the settings

1. Copy the example:

   ```sh
   cd deploy/compose
   cp .env.example .env
   ```

   `.env` holds secrets and is ignored by git.
2. Set the station's identifier, `STATION_ID`: `weavesignal-control sample-config` prints a fresh one.
3. Set the secrets:

   | Setting | What |
   |---|---|
   | `LIVEKIT_API_SECRET` | LiveKit's API secret, at least 32 characters |
   | `LISTENERS_SOURCE_PASSWORD`, `POSTGRES_PASSWORD` | any strong passwords |
   | `CONTROL_ADMIN_TOKEN` | the station key: the studio app signs in with it first and makes the first staff account ([Add staff and sign in](/docs/how-to/add-staff/)); the chat also reads the studio's view with it |
   | `CHAT_EMAIL_KEY` | at least 32 characters, such as `openssl rand -hex 32`; the chat does not start without it ([The email key](/docs/reference/chat/#the-email-key)) |
   | `CONTROL_EMAIL_KEY` | the same for staff email addresses; needed when `MAIL_MODE` is on |
   | `CONTROL_TOKEN_KEY` | at least 32 characters; keys API tokens and webhook secrets ([Integrations](/docs/reference/integrations/#the-key)). Optional: unset, it is derived from the station key, and changing the station key then revokes every token |

   Keep the three keys with the database's backups: a different key finds none of the accounts, and revokes every token, made under the old one.
4. Set `CONSENT_CONTACT` to an email address or a link where callers ask for their call to be taken down. Call-in stays closed until it is set (ADR-0124).

Every port is a setting (ADR-0075): change `CONTROL_PORT`, `CHAT_PORT`, `LISTENERS_PORT`, `SRT_INGEST_PORT`, `PLAYOUT_FEED_PORT`, `DESK_FEED_PORT`, `LIVEKIT_HTTP_PORT` or `LIVEKIT_MEDIA_PORT` if a default is taken.
`.env.example` names the other settings: mail, listing, the MP3 stream, playout's processing, video, and `FEED_BUFFER_S`, the listener process's cushion of playout's feed (0.5 s; raise it to 1–2 s only on a busy host, as each second is a second more delay).

## Start it

```sh
podman compose up -d
```

Or `docker compose up -d`.
The services use the host's network so the loopback addresses stay exact.
PostgreSQL listens on a Unix socket only and takes no port (ADR-0092).
The desk is restarted if it stops, and playout connects to it again within about a second.

## Check it

```sh
curl http://127.0.0.1:7310/health
```

It answers with `"database":"ok"` once the control plane has its database.
A listener on the same machine can play `http://127.0.0.1:7312/programme.opus`.

To check that only LiveKit's media port listens off loopback, run the bundle's own test from the repository's root:

```sh
just test-bundle
```

It builds the server image, brings the bundle up on test ports with throwaway secrets, checks the control plane, a room token, the desk and playout, the listener process's feed cushion behind the real desk, the MP3 stream and the default loop, scans the ports, and takes the bundle down with its volumes.

## The studio app

The control plane in the bundle is given the desk's control socket, so the studio app, signed in to the station, drives the desk.
The bundle has no synthetic people; to try the studio app with a room already full of them, use the test stack in [Run a first show](/docs/tutorials/first-show/).

`just compose-check`, which validates the bundle, with its edges and its public route, against `.env.example`, and `just test-bundle` are both part of `just ci`.
