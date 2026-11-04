---
title: "Memory and CPU"
description: "What a station uses, measured on a workstation, on a dedicated server and on a shared one, and what that means for the host you choose."
type: reference
status: draft
last_verified: 2026-11-04
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
| A dedicated server, 4 vCPU, load 0.3–0.5, the Compose bundle on the public route | 0.5 s | 0.20–0.36 s, over 13 hours unattended; no container restarted | 2026-11-03 to 11-04 |
| A heavily shared server, 12 cores at load 9–22, the Cloudron package | 3 s | median 8.5 s, at most 10.0 s (the buffer's cap), over 7 hours | 2026-11-03 |

- Give a station on air a server of its own, or one where its CPU is reserved. A busy shared server is fine for checking that a station works.
- Raising `FEED_BUFFER_S` to 1–2 s can cover a host that is only sometimes busy, but each second is a second more listener delay, and 3 s did not save the shared server above. The Cloudron package sets 1.5 s.
- Give monitors and recorders the `listener` role ([Roles](/docs/reference/roles/)): a monitor joined as a producer costs the desk a full mix and an encoder.

A caller's choppy return, first blamed on the shared server's load, was the desk's own fault and was as choppy on an idle server: the desk stamped each block it sent with the clock at the moment it was encoded, and receivers concealed a gap at nearly every packet.
Each block is now stamped from its place in the mix; the receiver's WebRTC statistics (`getStats`) read 29.8 % of the return concealed before the fix and 0.0 % after, on the local stack, and 32.3 % against 0.0 % over the internet.

## Memory

The Cloudron package's memory limit is **1.5 GiB**.
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

On the dedicated server above, after 13 hours unattended, the Compose bundle's processes held: playout 249 MB, the listener process 159 MB, LiveKit 51 MB, PostgreSQL 46 MB, the desk 34 MB, the control plane 7 MB, the chat 2 MB.

Large audiences do not reach the station: 2,000 listeners reach it through edges or a CDN (ADR-0051).

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
