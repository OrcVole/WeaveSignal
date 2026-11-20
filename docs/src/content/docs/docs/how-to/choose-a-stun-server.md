---
title: "Choose a STUN server"
description: "Decide whether your station asks an outside STUN server for anyone's public address (from 1.2 it asks none unless you name one), and if so, choose one in the install wizard or set STUN_SERVERS on Cloudron, Compose or the Quadlets."
type: howto
status: draft
last_verified: 2026-11-20
sidebar:
  order: 12
---

A STUN server tells a program the public address it is seen from.
From 1.2 a WeaveSignal station asks **none** unless you name one (ADR-0290): not the desk, not LiveKit, not the join pages in guests' and callers' browsers.
Most stations should leave it that way.

## Decide whether you need one

You almost certainly do not:

- the station finds its own public address from `NODE_IP` or its name's A record ([LiveKit's public address](/docs/reference/packages/#livekits-public-address));
- a guest or caller behind a home router reaches that address directly, or through the station's own TURN relay.

Name one only if guests on some network cannot join and the station's own address and TURN are already right.
Whoever runs the server sees the station's address, and the address of each guest and caller whose browser asks it, and when they ask: nothing else.
None of the listed servers is ours, and nobody has promised one to your station.

## With the install wizard

1. Open the [install wizard](/docs/install-wizard/) and answer its questions.
2. Under **4. What it does**, **Outside STUN server** offers:
   - **None (recommended)**: the station contacts no outside STUN server;
   - **Nextcloud's** (`stun.nextcloud.com:443`) or **FreeSWITCH's** (`stun.freeswitch.org:3478`), run by those free-software projects, with who runs each said under the choice;
   - **Another server…**: type up to four as `host:port`, with commas between them, without `stun:` in front.
3. The settings file the wizard gives you carries the choice as `STUN_SERVERS`; with **None** it has no such line.

The question appears only for a station reachable from the internet: a station for one machine has nobody outside to reach it.

## In the settings file

Set `STUN_SERVERS` in the station's settings:

| Route | Where | Then |
|---|---|---|
| Cloudron | `/app/data/weavesignal.env`, in double quotes: `STUN_SERVERS="stun.nextcloud.com:443"` | restart the app |
| Compose | the settings file beside `compose.yaml`: `STUN_SERVERS=stun.nextcloud.com:443` | run `weavesignal.sh` again |
| Quadlets | the settings file you generate from | run `weavesignal.sh --no-start` and `generate.sh` again, then restart the target |

Leave it empty for none.
A server that is not `host:port` (a DNS name or an IPv4 address, a colon and a port from 1 to 65535), or more than four, stops the start on Cloudron and stops `weavesignal.sh`, naming it.

## Check it

- LiveKit tells browsers to use only the servers you named, the join pages drop any other, and the desk uses the first.
- The servers that answered when the list was made, and why Google's and Twilio's are left out, are in [STUN servers](/docs/reference/packages/#stun-servers).
- To run your own instead, the `coturn` package is free software and answers STUN on UDP and TCP 3478.
