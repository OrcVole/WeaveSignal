---
title: "Presenter kit"
description: "The presenter kit's panels by name: presets, sources, the chain, the patchbay, outputs. Planned for release two."
type: reference
status: draft
last_verified: 2026-11-04
sidebar:
  order: 6
---

**[Planned](/docs/reference/release-one-status/#not-in-release-one) for release two** (ADR-0042, moved out of release one by ADR-0207): the presenter kit, for a presenter at home: one clock, radio presets, a patchbay, VBAN in and out.
WeaveSignal 1.0.0 has no presenter kit tab; presenters join from a browser and the studio app drives the desk ([Get ready to present](/docs/how-to/get-ready/)).
Each heading below is the name its panel will carry on screen (ADR-0070), from the design system's registry.

## `kit.head`

Whose machine this is, the one clock, and the links to and from the desk.

## `kit.presets`

The four presets (⌥1 to ⌥4) that fill the patchbay.

## `kit.sources`

What this machine contributes: the microphone, the bed, the returns, the screener's talkback, VBAN in.

## `kit.chain`

The microphone's processing chain, in a fixed order, each stage switchable.

## `kit.patchbay`

Sources against destinations: routed, off, never (it would loop or the desk owns it), not in this preset.

## `kit.driver`

WeaveSignal as a microphone in other apps, with the workaround per platform (D43).

## `kit.outputs`

What leaves this machine: headphones, the stream to the desk, the safety recorder, VBAN out.

## `kit.status`

The clock, the buffer, the platform's capture method, CPU and chain latency.
