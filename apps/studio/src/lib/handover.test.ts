// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { describe, expect, it } from 'vitest';
import type { Role, Source } from './api.ts';
import { handoverButton, handoverStep } from './handover.ts';

const src = (identity: string, role: Role, slot: number, on_air: boolean, present = true): Source => ({
  identity,
  name: identity,
  role,
  on_air,
  mute: false,
  cue: false,
  talk: false,
  talk_to: null,
  present,
  away_ms: null,
  linked: true,
  lufs: null,
  lufs_peak: null,
  slot,
  caller: null,
});

describe('the one-key handover', () => {
  it('puts the waiting DJ on air, then takes the presenter off, then is done', () => {
    let room = [src('eilidh', 'presenter', 1, true), src('kirsty', 'guest', 3, true), src('finlay', 'dj', 5, false)];
    const first = handoverStep(room, 'eilidh');
    expect(first).toMatchObject({ kind: 'on', dj: { identity: 'finlay' } });

    room = [src('eilidh', 'presenter', 1, true), src('kirsty', 'guest', 3, true), src('finlay', 'dj', 5, true)];
    const second = handoverStep(room, 'eilidh', 'finlay');
    expect(second.kind).toBe('off');
    // The guest stays on with the next DJ.
    if (second.kind === 'off') expect(second.off.map((s) => s.identity)).toEqual(['eilidh']);

    // The seat followed the air to Finlay.
    room = [src('eilidh', 'presenter', 1, false), src('kirsty', 'guest', 3, true), src('finlay', 'dj', 5, true)];
    expect(handoverStep(room, 'finlay', 'finlay')).toMatchObject({ kind: 'done', dj: { identity: 'finlay' } });
    // And with Eilidh gone, from a fresh app that never pressed it.
    expect(handoverStep([src('finlay', 'dj', 5, true)], 'finlay')).toMatchObject({ kind: 'done' });
  });

  it('works when a DJ put on air by hand (5) comes before the key', () => {
    const room = [src('eilidh', 'presenter', 1, true), src('finlay', 'dj', 5, true)];
    expect(handoverStep(room, 'eilidh')).toMatchObject({ kind: 'off', dj: { identity: 'finlay' } });
  });

  it('hands from one DJ to another even when the seat lands on the incoming one', () => {
    // Eilidh presents as a DJ in slot 5; Finlay joins in slot 2.
    let room = [src('finlay', 'dj', 2, false), src('eilidh', 'dj', 5, true)];
    expect(handoverStep(room, 'eilidh')).toMatchObject({ kind: 'on', dj: { identity: 'finlay' } });
    // In the overlap the seat goes to the first DJ on air by slot: Finlay. The pending DJ wins.
    room = [src('finlay', 'dj', 2, true), src('eilidh', 'dj', 5, true)];
    const step = handoverStep(room, 'finlay', 'finlay');
    expect(step.kind).toBe('off');
    if (step.kind === 'off') expect(step.off.map((s) => s.identity)).toEqual(['eilidh']);
  });

  it('has nothing to do with no DJ in the room, or one who has left', () => {
    expect(handoverStep([src('eilidh', 'presenter', 1, true)], 'eilidh')).toEqual({ kind: 'none' });
    const gone = [src('eilidh', 'presenter', 1, true), src('finlay', 'dj', 5, false, false)];
    expect(handoverStep(gone, 'eilidh')).toEqual({ kind: 'none' });
  });

  it('takes off a second presenter too, but never a caller', () => {
    const room = [
      src('eilidh', 'presenter', 1, true),
      src('rory', 'presenter', 2, true),
      src('dougal', 'caller', 4, true),
      src('finlay', 'dj', 5, true),
    ];
    const step = handoverStep(room, 'eilidh', 'finlay');
    if (step.kind !== 'off') throw new Error(step.kind);
    expect(step.off.map((s) => s.identity)).toEqual(['eilidh', 'rory']);
  });

  it('labels the button as the next press: bring on, then hand over, then nothing', () => {
    const named = (s: Source, name: string): Source => ({ ...s, name });
    const eilidh = (on: boolean) => named(src('eilidh', 'presenter', 1, on), 'Eilidh Fraser-MacLeod');
    const finlay = (on: boolean) => named(src('finlay', 'dj', 5, on), 'Finlay Grant');
    expect(handoverButton(handoverStep([eilidh(true)], 'eilidh'))).toBeNull();
    expect(handoverButton(handoverStep([eilidh(true), finlay(false)], 'eilidh'))).toEqual({
      kind: 'on',
      dj: 'finlay',
      label: 'Bring Finlay on air',
      then: 'On air beside you: the overlap.',
    });
    expect(handoverButton(handoverStep([eilidh(true), finlay(true)], 'eilidh', 'finlay'))).toEqual({
      kind: 'off',
      dj: 'finlay',
      label: 'Hand over to Finlay',
      then: 'Eilidh comes off air.',
    });
    const rory = named(src('rory', 'presenter', 2, true), 'Rory Bruce');
    expect(handoverButton(handoverStep([eilidh(true), rory, finlay(true)], 'eilidh', 'finlay'))?.then).toBe('Eilidh and Rory come off air.');
    expect(handoverButton(handoverStep([eilidh(false), finlay(true)], 'finlay'))).toBeNull();
  });
});
