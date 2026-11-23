// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Open the player (1.3.1): which address the buttons use, and what they say when there is none.
import { describe, expect, it } from 'vitest';
import { PLAYER, playerLink } from './lib/player.ts';

describe('the player address', () => {
  it('is the public address with one slash when the station has one', () => {
    const l = playerLink({ publicUrl: 'https://radio.example.org/', asked: true, own: 'https://studio.example.org' });
    expect(l).toEqual({ state: 'ready', url: 'https://radio.example.org/', why: '', own: false });
  });

  it('prefers the public address even before the asking is done (a known address needs no wait)', () => {
    expect(playerLink({ publicUrl: 'https://radio.example.org', asked: false, own: '' }).url).toBe('https://radio.example.org/');
  });

  it('waits, with the reason, until the station has been asked', () => {
    const l = playerLink({ publicUrl: undefined, asked: false, own: 'http://127.0.0.1:7310' });
    expect(l.state).toBe('asking');
    expect(l.url).toBeNull();
    expect(l.why).toBe(PLAYER.asking);
  });

  it('falls back to the window’s own address on a home station with no public address, and says so', () => {
    for (const publicUrl of [null, undefined, '']) {
      const l = playerLink({ publicUrl, asked: true, own: 'http://192.168.1.20:7310' });
      expect(l).toEqual({ state: 'ready', url: 'http://192.168.1.20:7310/', why: '', own: true });
    }
  });

  it('writes no link, and says why, when there is no address of any kind', () => {
    const l = playerLink({ publicUrl: null, asked: true, own: '' });
    expect(l).toEqual({ state: 'none', url: null, why: PLAYER.none, own: false });
    expect(playerLink({ publicUrl: null, asked: true, own: 'not an address' }).state).toBe('none');
  });

  it('ignores a public address that is not a web address', () => {
    expect(playerLink({ publicUrl: 'wss://station.example', asked: true, own: 'http://127.0.0.1:7310' }).own).toBe(true);
  });
});
