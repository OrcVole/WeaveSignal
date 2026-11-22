// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { describe, expect, it } from 'vitest';
import { gpuWords } from './lib/gpu.ts';

describe('a graphics card for video, in Settings', () => {
  it('says nothing where there is no card to speak of', () => {
    expect(gpuWords(null)).toBeNull();
    expect(gpuWords(undefined)).toBeNull();
    // Cloudron, and a server without a card.
    expect(gpuWords({ found: false, in_use: false, video: true })).toBeNull();
    // A GPU=nvidia left in the settings with no card found is not a card.
    expect(gpuWords({ found: false, in_use: true, video: true })).toBeNull();
  });

  it('offers a card that is there and not used, as two steps on the server', () => {
    const w = gpuWords({ found: true, in_use: false, video: true });
    expect(w).toMatchObject({ state: 'offered', tag: 'Not in use' });
    expect(w?.lead).toBe('This server has an NVIDIA graphics card, and the station is not using it.');
    expect(w?.rest).toMatch(/^Video is encoded on the processor \(SVT-AV1\)\. The desk can encode it on the card instead \(NVENC\)/);
    expect(w?.steps).toHaveLength(2);
    expect(w?.steps?.[0]).toMatchObject({ code: 'GPU=nvidia' });
    expect(w?.steps?.[1].text).toMatch(/^Run weavesignal\.sh again/);
    // Nothing is switched unasked (ADR-0292), and it says so.
    expect(w?.after).toMatch(/an update never moves the station to the card/);
  });

  it('never says the card can encode: the install knows a card is there, not its model', () => {
    for (const g of [
      { found: true, in_use: false, video: true },
      { found: true, in_use: false, video: false },
      { found: true, in_use: true, video: true },
      { found: true, in_use: true, video: false },
    ]) {
      const w = gpuWords(g)!;
      expect(`${w.lead} ${w.rest}`, JSON.stringify(g)).toMatch(/A card with an AV1 encoder \(NVIDIA’s RTX 40 series or later\) takes it; with an older card the desk stays on the processor/);
      expect(w.lead, JSON.stringify(g)).not.toMatch(/can encode|is encoded on/);
    }
  });

  it('with video off, the offer names both settings', () => {
    const w = gpuWords({ found: true, in_use: false, video: false });
    expect(w?.state).toBe('offered');
    expect(w?.rest).toMatch(/^Video is off/);
    expect(w?.steps?.[0].code).toBe('VIDEO=true and GPU=nvidia');
  });

  it('says when the desk has the card, and the way back; nothing is offered', () => {
    const on = gpuWords({ found: true, in_use: true, video: true });
    expect(on).toMatchObject({ state: 'in_use', tag: 'With the desk', steps: null });
    expect(on?.lead).toBe('The desk has been given this server’s NVIDIA graphics card.');
    expect(on?.after).toMatch(/GPU=none/);
    const ready = gpuWords({ found: true, in_use: true, video: false });
    expect(ready).toMatchObject({ state: 'ready', tag: 'With the desk · video off', steps: null });
    expect(ready?.rest).toMatch(/^Video is off/);
  });
});
