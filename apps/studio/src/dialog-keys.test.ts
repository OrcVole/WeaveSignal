// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { describe, expect, it } from 'vitest';
import { dialogLets, escTaken, settingsEscape } from './lib/dialogKeys.ts';

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

describe('an Esc something has already taken', () => {
  it('is not the keymap’s as well: a dialog that closes on Esc does not also clear the Schedule tab’s selection', () => {
    expect(escTaken({ key: 'Escape', defaultPrevented: true })).toBe(true);
    // An Esc nobody took is the keymap's, and so is any other key a control prevented.
    expect(escTaken({ key: 'Escape', defaultPrevented: false })).toBe(false);
    expect(escTaken({ key: 'Enter', defaultPrevented: true })).toBe(false);
  });
});

describe('Esc on the Settings page', () => {
  const e = (key: string, mods: Partial<{ ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean }> = {}) => ({
    key,
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    ...mods,
  });

  it('leaves Settings for the tab it came from', () => {
    expect(settingsEscape(e('Escape'), false)).toBe('leave');
  });

  it('first leaves a text field (keymap rule 3), and the next Esc leaves Settings', () => {
    expect(settingsEscape(e('Escape'), true)).toBe('leave-field');
  });

  it('is only a bare Esc, and no other key', () => {
    expect(settingsEscape(e('Escape', { ctrlKey: true }), false)).toBeNull();
    expect(settingsEscape(e('Escape', { shiftKey: true }), false)).toBeNull();
    expect(settingsEscape(e('Enter'), false)).toBeNull();
    expect(settingsEscape(e('KeyA'), true)).toBeNull();
  });
});
