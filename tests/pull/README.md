<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
# Pulled sources: `just test-pull`

A show can come from a stream the station pulls itself (1.2, issue #20; the how-to *Pull a
partner's stream into a show*).
This test runs it end to end, in containers on loopback.

## What runs

- PostgreSQL.
- The control plane, the ingest and the real desk, all from the server image.
  The control plane has `WEAVESIGNAL_PULL_ALLOW_PRIVATE=1`, as the documentation says, so a pull
  may open this machine's own addresses.
- Two partners standing in for other stations:
  - Icecast 2.5 from the relay image, with a mount fed by Liquidsoap (MP3, 440 Hz);
  - an SRT listener: Liquidsoap's `output.srt` (libsrt) in listener mode, sending AAC in MPEG-TS
    (1 kHz); `SRT_SENDER=gstreamer` uses GStreamer's `srtsink` (libsrt) instead.
    The ingest calls it with its own SRT implementation (`srt-tokio`), so this is also a check
    that the two work together.

Each partner is a show key with `hint: pull`, for two shows one after the other.
The first starts about 40 s after the test does; the second when the first ends.
The rules are short: a grace of 4 s, a place kept for 40 s.

## What it checks

1. Both pulls are opened before their shows' times and stand by (`STANDBY`, `pull: true`, way
   `pull`), each heard at the desk with a level and not on air.
2. At the first show's post the Icecast pull goes on air (automation held the air).
3. The Icecast partner's source drops: LINK LOST, then the station opens the stream again on its
   own and the pull goes back on air in its kept place.
4. At the second show's post, after the grace, the SRT pull takes the air.
   The first pull, its door closed, is closed: a pull is never a private test.
5. The SRT partner drops and comes back: LINK LOST, reopened, on air again.

## Found on its first runs (2026-11-21)

**An SRT pull from a libsrt listener gets nothing** with 1.2.0's ingest.
The partner sends the usual 1316-byte messages (seven MPEG-TS packets), as ffmpeg, OBS, GStreamer
and Liquidsoap all do.
Liquidsoap logs "Message too large to send (exceeds send buffer size)" at every connection, and
GStreamer's `srtsink` shut each connection within about 150 ms.
With `SRT_PAYLOAD=1128` (six packets) every check passes, so the rest of the path works.
The likely cause is inferred from the code, not proven.
The ingest's caller (`srt-tokio` 0.4.4) gives its payload size, 1316, as the handshake's largest
packet, where libsrt expects the MSS (1500).
libsrt would then keep 1316 − 44 = 1272 bytes for each message.
It does not yet explain why ffmpeg's 1316-byte messages into the ingest's own SRT listener work
(`just test-sources`).
ffmpeg and OBS as pulled listeners were not tried; they use the same library, libsrt.
The test keeps 1316 as its default, so it fails until the ingest is fixed.

## Running it

```sh
just test-pull                         # builds the server and relay images first
WEAVESIGNAL_SERVER_IMAGE=localhost/weavesignal-server:mine RELAY_IMAGE=localhost/weavesignal-relay:mine tests/pull/run.sh
SRT_PAYLOAD=1128 tests/pull/run.sh     # the partner sends six MPEG-TS packets a message, not seven
```

It takes about four minutes.
Ports are in 21xxx on loopback, each a setting (`CONTROL_PORT`, `PG_PORT`, `DESK_SOURCES_PORT`,
`DESK_SRT_PORT`, `PARTNER_ICECAST_PORT`, `PARTNER_SRT_PORT`).
The containers' logs stay in `tests/pull/run/`.

Not covered here:

- HLS pulls and HTTPS (the ingest's unit tests have both);
- an address refused without `WEAVESIGNAL_PULL_ALLOW_PRIVATE` (the control plane's tests have it);
- Disconnect and Let back in.
