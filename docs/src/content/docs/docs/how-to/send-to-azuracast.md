---
title: "Send your programme to AzuraCast or another Icecast server"
description: "Make WeaveSignal an Icecast source on another station, such as a DJ's AzuraCast, so that station carries your programme while you are live, with the profanity delay and Dump already in it."
type: howto
status: draft
last_verified: 2026-11-06
sidebar:
  order: 37
---

A DJ may broadcast to a station of their own, run by AzuraCast or another Icecast server.
WeaveSignal can send its programme there as an Icecast source (ADR-0226).
The other station leaves its automation while your programme arrives, and goes back to it when you stop.
What it carries is what your listeners hear: the profanity delay is in it, and so is Dump.

The stream is MP3.
Icecast-type servers drop a source that sends a chained Ogg stream, and MP3 has no chain.

## What you need

- A station on AzuraCast, or any Icecast server, that takes a source with a user name and a password.
- The station's address, the port for sources, the mount, and the user name and password.
- A WeaveSignal station you run yourself, on the Compose bundle, the Quadlets or Cloudron.
  The other station's address must be reachable from where WeaveSignal runs.

## 1. Make a streamer account on AzuraCast

1. Sign in to AzuraCast and open the station.
2. Open **Streamers/DJ Accounts** and add an account.
   Give it a user name and a password, and save it.
3. On the same page, read **Connection Information**.
   It gives the **Server**, the **Port** and the **Mount Name**.
   The mount is likely `/`.

AzuraCast warns that a reverse proxy such as Cloudflare often blocks direct connections to broadcast ports.
If the connection fails from WeaveSignal, use the server's direct IP address as the host.

For another Icecast server, use a mount that takes a source with that user name and password.

## 2. Give WeaveSignal the details

The settings are the same on every route; only the place differs.

| Setting | Default | Is |
|---|---|---|
| `ICECAST_OUT` | `false` | `true` turns the feature on |
| `ICECAST_OUT_HOST` | empty | the **Server**, such as `radio.example.org`, without `https://` |
| `ICECAST_OUT_PORT` | `8005` | the **Port** for sources |
| `ICECAST_OUT_MOUNT` | `/` | the **Mount Name** |
| `ICECAST_OUT_USER` | `source` | the streamer account's user name |
| `ICECAST_OUT_PASSWORD` | empty | the streamer account's password: a secret |
| `ICECAST_OUT_TLS` | `false` | `true` connects over TLS |
| `ICECAST_OUT_BITRATE` | `128` | the MP3 bit rate in kbit/s |

Host, mount and user must hold no quote, backslash or space.

- **Compose bundle:** add the lines to the station's `.env` (the example file has the block), then run `weavesignal.sh` again or restart the bundle.
  Running `weavesignal.sh` again keeps these lines.
- **Quadlets:** put the same lines in the settings file you give `generate.sh`, and generate the units again.
  The password becomes a Podman secret, `weavesignal-icecast-out-password` for the default project, and is in no unit file.
  Create the secrets with `--create-secrets`.
- **Cloudron:** put the settings in `/app/data/weavesignal.env` and the password in `/app/data/secrets.env` (the line `ICECAST_OUT_PASSWORD=` is there after the first start), then restart the app.
  Type the password as it is after the `=`: it is not read as shell, so `&`, `$` or a space in it need no quoting.
  See [Install on Cloudron](/docs/how-to/install-on-cloudron/#the-stations-files).

The password is read by the listener process alone.
It is not in the settings files the services read, in the control plane's answers or in any log.

## 3. Choose when it sends

Sign in to the studio app as an admin and open **Settings**.
The panel **Send to another Icecast server** shows whether the connection holds, and has three choices:

| Choice | Sends |
|---|---|
| **While the desk is on air** (the default) | while the station is in live mode and the desk is on air, and no other time |
| **Always** | what listeners hear, automation and fallback included, until you turn it off. Use it to test the connection |
| **Never** | nothing |

With the default, the connection opens when you go on air and closes when the desk has been quiet long enough for automation to take over (the dead-air time, ten seconds by default).
AzuraCast then returns to its own automation.
Putting the station on automation by choice closes the connection too.

The same choice is the admin route `PUT /api/v1/icecast-out` with `{"mode": "auto"}`, `"on"` or `"off"`; `GET /api/v1/icecast-out` reads the state.
The choice is kept across restarts.

## When it does not connect

The panel, and `GET /api/v1/icecast-out`, give the last error in words.

| It says | Means |
|---|---|
| the server refused the user name or password | the account or password is wrong, or the account is not allowed on that mount |
| nothing is listening at that address and port | wrong host or port, or the server is down |
| the server did not answer in time | a firewall or proxy in the way; try the server's direct address |
| the connection to the server was lost | the server went away while sending; WeaveSignal tries again |
| the address or password is not set | `ICECAST_OUT_HOST` or `ICECAST_OUT_PASSWORD` is empty |

WeaveSignal tries again by itself after 5 seconds, then 10, then 20, then every 30 seconds, and starts from 5 again once a connection holds.
A wrong password is therefore one refused login every 30 seconds, not a flood.

A failure on the other server never reaches your own listeners or playout.
The outgoing stream has its own clock and buffer, so a server that stalls holds up only itself.

Bytes sent are counted from the stream's constant bit rate while connected, not read from the connection.

## What has been tried

The outgoing source is tested on the test machine against Icecast 2.5 standing in for AzuraCast: one mount that takes a source only with a user name and a password (`just test-icecast-out`).
It has not yet been run against an AzuraCast itself (2026-11-06), so try it with **Always** before the first show.

## Delay

Your own listeners and the other station are not in step.
The other station adds its own buffering, usually several seconds, on top of WeaveSignal's.
Do not use one to cue the other.
