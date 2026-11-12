---
title: "Memory and CPU"
description: "What a station uses, measured on a workstation, on a dedicated server and on a shared one, what one server gives an audience of 2,000, and what that means for the host you choose."
type: reference
status: draft
last_verified: 2026-11-06
sidebar:
  order: 7
---

Each figure is dated and says where it was measured.
They will change as the station does.

## What this means for a host

**Run a station on a server that is not overloaded.**
The desk and Liquidsoap hand audio on every 10–20 ms, and the listener process keeps only a short cushion of playout's feed (`FEED_BUFFER_S`, 0.5 s by default; [Playout](/docs/reference/playout/)).
When the host cannot give them CPU on time, the cushion runs dry, listeners hear the fallback for moments, and each refill leaves the stream further behind:

| Server | Cushion set | What the listener process held | When |
|---|---|---|---|
| A dedicated server, 4 vCPU, load 0.3–0.5, the Compose bundle on the public route | 0.5 s | 0.20–0.38 s, over 24 hours; no container restarted | 2026-11-03 to 11-04 |
| A heavily shared server, 12 cores at load 9–22, the Cloudron package | 3 s | median 8.5 s, at most 10.0 s (the buffer's cap), over 7 hours | 2026-11-03 |

- Give a station on air a server of its own, or one where its CPU is reserved. A busy shared server is fine for checking that a station works.
- Raising `FEED_BUFFER_S` to 1–2 s can cover a host that is only sometimes busy, but each second is a second more listener delay, and 3 s did not save the shared server above. The Cloudron package sets 1.5 s.
- Give monitors and recorders the `listener` role ([Roles](/docs/reference/roles/)): a monitor joined as a producer costs the desk a full mix and an encoder.

A caller's choppy return, first blamed on the shared server's load, was the desk's own fault and was as choppy on an idle server: the desk stamped each block it sent with the clock at the moment it was encoded, and receivers concealed a gap at nearly every packet.
Each block is now stamped from its place in the mix; the receiver's WebRTC statistics (`getStats`) read 29.8 % of the return concealed before the fix and 0.0 % after, on the local stack, and 32.3 % against 0.0 % over the internet.

## Memory

The Cloudron package's memory limit is **2 GiB** (1.5 GiB before 1.0.1, raised for video).
Measured with the package's memory gate (`deploy/cloudron/memory-gate.sh`) on a workstation, cgroup v2, read-only root, PostgreSQL outside the container as Cloudron's addon is, with the MP3 stream and playout's processing chain on (package 0.8.0, 2026-11-02):

| | Idle | Loaded, 300 s |
|---|---|---|
| The container, now / peak | 526 MiB / 527 MiB | peak 699 MiB, 45 % of the limit |
| Out-of-memory kills | 0 | 0 |
| The listener process (Liquidsoap) | | 334 MB |
| Playout (Liquidsoap) | | 198 MB |
| LiveKit | | 83 MB |
| The desk | | 62 MB |
| The control plane, the chat | | 38 MB, 37 MB |

The load was three tone publishers through LiveKit and the desk, fifty listeners for 300 s, the studio's event stream and the chat's load generator; the chat's share is understated, as its per-address limits closed most of the generator's sockets, all from one address.
The gate's rule (peak ÷ 0.7) gives 1 GiB; the limit stays higher for a Liquidsoap starting without its script cache, and for the chat with thousands of sockets (97 MiB resident for 4,000; [The live chat](/docs/reference/chat/#load)).

On the dedicated server above, after 24 hours, the Compose bundle's processes held: playout 248 MB, the listener process 160 MB, LiveKit 50 MB, PostgreSQL 47 MB, the desk 34 MB, the control plane 7 MB, the chat 2 MB.

With video on (`VIDEO=true`, package 1.0.1, 2026-11-06), the same gate with a 640 × 360 camera on each of the three publishers, which the desk decodes and encodes to AV1 with SVT-AV1 at one 480p20 rung:

| | Video on, two clean runs |
|---|---|
| The container at rest, now / peak since start | 893 MiB / 895 MiB; 881 MiB / 993 MiB (the desk kept to four CPUs) |
| The container loaded, peak | 1,221 MiB (80 % of 1.5 GiB); 1,256 MiB (82 %) |
| The desk, resident, at rest / mid-load | 373 MB / 578 MB; 375 MB / 497 MB |
| The desk's CPU, over the last 270 s of the load | 26 % of one core, 205 threads; 22 %, 220 threads |
| The whole container's CPU, the same window | 42 %; 37 % |
| Out-of-memory kills | 0; 0 |

The gate's rule (peak ÷ 0.7) gives 1.7 to 1.75 GiB with video on, so the limit is 2 GiB from 1.0.1 ([Install on Cloudron](/docs/how-to/install-on-cloudron/#video)).
The desk accounts for most of the difference: about 310 MB more at rest than with video off (62 MB loaded, in 0.8.0), and 120 to 200 MB more again while three cameras were live. The share belonging to the encoder, the picture history and the slate was not traced.
Keeping the desk to four CPUs (`taskset`) changed the desk's memory by nothing measurable.
The container's current and peak figures include page cache, which varies by about 100 MiB between runs.

## A large audience on one server

The expected audience is about 2,000 at once (ADR-0051).
What one server can give them, from the figures so far:

| What | Figure | Where and when |
|---|---|---|
| `icecast.server` with 2,000 listeners | 17 % of one core, 773 MB | bench 01, on loopback for 90 s, the open-file limit at 65,536 |
| `icecast.server` with 4,000 listeners | 26 % of one core, 1.63 GB | the same |
| 2,000 video viewers straight from the station | 1,972 Mbit/s | bench 02 ([Run edges](/docs/how-to/run-edges/)) |
| A 12-core server's port, in London | 3.4 Gbit/s to a server in London, 4.4 Gbit/s to one in Amsterdam | iperf3, 8 streams for 10 s, 2026-11-06 |
| A Cloudron station just after start | 383 MB, against a 6 GB limit | the same server, package 1.0.0, video off, 2026-11-06 |

What they mean:

- **Audio for 2,000 listeners fits one server.** At 96 kbit/s they take about 200 Mbit/s (about 250 with the MP3 stream at 128 kbit/s), a small part of the port measured, and the listener process needs well under a core. The Opus rate is a setting (ADR-0233), and listeners who choose the 320 kbit/s MP3 stream (ADR-0232) take 2.5 times as much each: 2,000 of them alone would need about 640 Mbit/s.
- **Video for 2,000 viewers does not, yet.** About 2 Gbit/s straight from the station is over half the port measured, and that port was measured for 10 s, not for a show's hour; a server's monthly traffic allowance is the other limit. Send viewers through edges or a CDN ([Run edges](/docs/how-to/run-edges/), [Put Bunny in front of the station](/docs/how-to/use-bunny/)).
- **Check the open-file limit.** Each listener is an open socket. Cloudron's container starts with a soft limit of 1,024, so package 1.0.0 takes fewer than 1,024 listeners; 1.0.1 raises it to 65,536 ([Install on Cloudron](/docs/how-to/install-on-cloudron/#open-files-and-large-audiences)).
- **Give memory to the listener process.** The bench's 773 MB for 2,000 listeners comes on top of what the station uses at rest, so on Cloudron raise the app's memory limit before a large audience (*inference*: the station has not yet been measured with 2,000 real listeners).

## CPU

The desk's cost grows with the number of people at the desk.
Measured on a workstation with a release build and the desk's load test (`just test-load`), 2026-10-25, the desk doing every mix in one pass per 20 ms block:

| People at the desk | One core's share |
|---|---|
| 4 | 9 % |
| 8 | 18 % |
| 13 | 24 % |

Carts add a little: in a later run, a bed and a one-shot sounding took 4 people from 9 to 10 %, 8 from 16 to 17 % and 13 from 26 to 29 %.

The returns are encoded by a pool of threads, one per core and at most four.
The desk asks for real-time priority for its mixing and encoding threads, but a container gets it only when given the right (`CAP_SYS_NICE` or an `rtprio` limit), which neither the Compose bundle nor the Cloudron package grants; without it the desk carries on at normal priority.

On 2026-10-25, before the one-pass mixer, the desk cost about 3.5 times as much CPU on a shared 12-CPU server, 50–86 % busy, as on the workstation, and listeners heard gaps; pausing the server's other apps helped most.
That server has not been measured with the one-pass mixer.
