// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { describe, expect, it } from 'vitest';
import { dialogLets } from './lib/dialogKeys.ts';

const k = (code: string, mods: Partial<{ ctrl: boolean; meta: boolean; shift: boolean; alt: boolean }> = {}) => ({
  code,
  ctrl: false,
  meta: false,
  shift: false,
  alt: false,
  ...mods,
});

describe('a key under an open dialog', () => {
  it('lets only ⌃T, the held talkback, through', () => {
    expect(dialogLets(k('KeyT', { ctrl: true }))).toBe(true);
    expect(dialogLets(k('KeyT', { meta: true }))).toBe(true);
  });

  it('keeps tab chords, Drop, End show and restart away from the desk', () => {
    for (const code of ['Digit1', 'Digit2', 'KeyD', 'KeyR', 'KeyE', 'KeyB']) expect(dialogLets(k(code, { ctrl: true }))).toBe(false);
    expect(dialogLets(k('KeyE', { ctrl: true, shift: true }))).toBe(false);
    expect(dialogLets(k('KeyT', { ctrl: true, shift: true }))).toBe(false);
    expect(dialogLets(k('KeyT'))).toBe(false);
  });
});
