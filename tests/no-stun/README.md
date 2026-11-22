<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
# No outside STUN by default: `just test-no-stun`

ADR-0290 decided that a station contacts no outside STUN server unless its operator names one.
This test proves it from what leaves the station, rather than from the settings that are meant to
bring it about.

## How it works

- `run.sh` starts the Compose bundle (`deploy/compose/compose.yaml`) with `.env.example`'s settings,
  filling in only the secrets and the image, as `just test-bundle` does.
- `netns.yaml` puts every station service into one network namespace of its own, the `netns`
  service's, on the project's Podman network.
  The services still reach each other on 127.0.0.1, as on the host network.
  Everything that leaves the station crosses that namespace's one interface, `eth0`.
- That namespace belongs to rootless Podman's user namespace, so `tcpdump` can capture on `eth0`
  under `podman unshare nsenter`, with no root and no sudo.
  The capture starts before the station does.
- A person joins the studio room, publish-only (GStreamer's `livekitwebrtcsink` from the server
  image, inside the namespace).
  Its own STUN server is set to `127.0.0.1:9`, so it never asks one, and its traffic stays on
  loopback.
  The test waits until the desk hears them, so the desk's WebRTC connections are really made
  with its STUN setting.
- `analyse.py` reads the capture and fails on:
  - any STUN request on any UDP port (the magic cookie `0x2112A442`; Google's server is on 19302,
    not 3478);
  - any TCP connection to 3478, 5349 or 19302 to 19309;
  - any lookup of a name with `stun` in a label.
  It also lists every destination and every name looked up.
  A STUN request that carries USERNAME is an ICE connectivity check between two peers (the desk
  and LiveKit), not a question to a STUN server.
  One addressed to loopback cannot leave the machine.
  Both are listed, not judged.
  libnice binds to `eth0`, so its checks toward LiveKit's `127.0.0.1` candidate (the example's
  `node_ip`) appear there.

## The two phases

1. **The defaults.** The player's settings (`/api/v1/player`, `relay.stun_servers`) name no
   STUN server, and nothing above leaves the station.
2. **A named server.** `STUN_SERVERS=stun-local:3478`, with `stun-local.yaml`'s small responder
   (`stun-responder.py`) in a container of its own.
   The player names it, and the capture must hold the station's binding requests to it and to
   no other server.
   This is the proof that phase 1 is not quiet merely because the capture or the detector misses
   things.

Each phase ends with a canary: the test sends one binding request of its own from the
namespace to TEST-NET-1 (`192.0.2.1`).
The judge must find it, or the capture was not live.
The UDP sockets and their processes (`ss -uanp` in the namespace) are recorded, so a request's
source port names the process that sent it.

## What it does not cover

- The public route (`public.yaml`: Caddy, TURN over TLS).
- The phone line (`phone.yaml`).
- Cloudron's package.
- The Quadlets (generated from this bundle).
- Browsers: the join pages are covered only by what the station tells them.
- A real `NODE_IP`: LiveKit keeps the example's `127.0.0.1`.

LiveKit runs with `use_external_ip: false`, as the bundle sets it.
The ICE servers LiveKit hands its own clients in its signalling (Twilio's and Google's, which the
pages remove) are read from the loopback capture and reported, not judged.

## Running it

```sh
just test-no-stun                      # builds the server image first, as test-bundle does
WEAVESIGNAL_SERVER_IMAGE=localhost/weavesignal-server:mine tests/no-stun/run.sh
WATCH_S=60 tests/no-stun/run.sh        # watch each phase for longer once the person is heard
```

It needs `tcpdump` and `jq` on this machine.
It takes about two and a half minutes (150 s on 2026-11-21), and it is part of `just ci`, after
`test-bundle`, whose server image it uses.
The captures, the judge's reports (`*-report.json`), the sockets and the containers' logs stay in
`tests/no-stun/run/`.
