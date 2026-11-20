---
title: "Presenter kit"
description: "The presenter kit: its station half in 1.2 (Settings › Presenter kit: one clock, the station on the studio network, presets, VBAN in and out, on Compose and the Quadlets), and the panels of the presenter's own computer, which are not built."
type: reference
status: draft
last_verified: 2026-11-20
sidebar:
  order: 6
---

The presenter kit (ADR-0042) is for a presenter at home or a studio PC beside the station: one clock, presets, a patchbay, and VBAN in and out, in place of a virtual mixer on their machine.
ADR-0207 moved it out of release one; **1.2 builds its station half** (Phase 17g), on Compose and the Quadlets only.
The half on the presenter's own computer is [planned](/docs/reference/release-one-status/#not-in-release-one) and not built.
How to use what is built: [Connect a studio PC over VBAN](/docs/how-to/connect-a-studio-pc-over-vban/).

## The station half (1.2): `settings.kit`

**Settings › Presenter kit** in the studio app, for an admin.
VBAN has no password and no encryption, so it is off until the installation sets `KIT_VBAN=true` (never on Cloudron), and the panel says why while it is off.

| Part | What it does |
|---|---|
| **One clock** | each host's own studio at the desk, with its way in, its clock against the desk's in parts per million, the gaps it could not follow (**Placed afresh**) and the packets lost. The desk resamples every source once, at a rate steered to keep it a fixed cushion ahead of the mix |
| **The station on the studio network** | the station's own address on the local network (`bind`; a public address is refused unless allowed) and VBAN's port (6980) |
| **Presets** | the routing in one press: *Studio PC on the network* (the live programme before the delay), *Transmitter feed* (after the delay, 24-bit), *Local guest* (the first input's monitor mix back to it), *Custom* |
| **VBAN in** | up to four: a sender's address, its stream name (matched exactly), a show key, and the pair of channels to take. Each stands by at the desk as the way `vban`, as any host's own studio does; packets from anyone else are dropped and counted |
| **VBAN out** | up to four: one address and port each, never broadcast or multicast, 48 kHz stereo, 16-bit by default (`s24`, `f32`); the source is the programme before or after the delay, or a person's mix-minus (`mix_minus:<identity>`) |

The kit's settings are kept in the station's database (migration 0045) and reached through the API: `GET` and `PUT /api/v1/kit`, `GET /api/v1/kit/clock` and `POST /api/v1/kit/preset` ([The API](/docs/reference/api/#routes-kit)).
The [bridge](/docs/how-to/send-through-the-bridge/), also from 1.2, carries a host's software to the station from the studio app on their own computer.

## The presenter's computer (not built)

Each heading below is the name its panel will carry on screen (ADR-0070), from the design system's registry.

### `kit.head`

Whose machine this is, the one clock, and the links to and from the desk.

### `kit.presets`

The four presets (`⌥1` to `⌥4`) that fill the patchbay.

### `kit.sources`

What this machine contributes: the microphone, the bed, the returns, the screener's talkback, VBAN in.

### `kit.chain`

The microphone's processing chain, in a fixed order, each stage switchable.

### `kit.patchbay`

Sources against destinations: routed, off, never (it would loop or the desk owns it), not in this preset.

### `kit.driver`

WeaveSignal as a microphone in other apps, with the workaround per platform (D43).

### `kit.outputs`

What leaves this machine: headphones, the stream to the desk, the safety recorder, VBAN out.

### `kit.status`

The clock, the buffer, the platform's capture method, CPU and chain latency.
