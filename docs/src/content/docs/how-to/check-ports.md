---
title: "Check a station's ports"
description: "Find out whether every port the station listens on is free, and what to change when one is taken."
type: howto
status: draft
last_verified: 2026-11-04
sidebar:
  order: 30
---

Every port the station listens on is a setting (ADR-0075).
Check them before the station starts, with the control plane's settings file: while it runs, its own ports show as taken.
The Compose route's installer, `weavesignal.sh`, makes the same check itself and chooses free ports for you ([Ports](/reference/ports/#with-weavesignalsh)); this command is for a station whose `control.toml` you write yourself.

```sh
weavesignal-control --config control.toml check-ports
```

It lists all thirteen ports under `[ports]`, each as `free` or `TAKEN`, with its setting and what listens on it, and ends with `every port is free` when they all are:

```text
free    7310/tcp  control  (the control plane's HTTP API, behind the proxy)
free    7882/udp  livekit_media  (LiveKit's one WebRTC media port (ADR-0023))
```

Loopback ports are checked on the internal address (`listen.internal`, `127.0.0.1` by default), and the public ones (the proxy's, LiveKit's media, ICE over TCP and TURN) on every address.

For a port that is not free, the command adds a line naming what wanted it and the next suggested alternative that is free now:

```text
weavesignal-control: port 7310/tcp for `control` (the control plane's HTTP API, behind the proxy) is in use; the next free suggestion is 17310/tcp (set `ports.control = 17310`), and any free port may be used
```

1. Set the new port under `[ports]` in `control.toml`.
   Any free port may be used; the suggestions are only ones other software rarely claims.
2. Run the check again.
   The command exits with status 1 while any port is not free, and 0 when all are.

Ports below 1024 (the proxy's 443 and 80) need privileges.
Run as an ordinary user, the check lists them as `TAKEN` and its message says why: the port "needs privileges this process does not have", with the next suggestion (8443, 8080).

On Cloudron, Cloudron chooses and checks the public ports at install; see [Ports](/reference/ports/#on-cloudron).

Two settings that ask for the same port and protocol are refused when the settings are read, before any port is checked.
