// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// `/join/` opened without an invitation (1.3.3): what the page says, with and without the
// station's name.
import { describe, expect, it } from 'vitest';
import { bareWords } from './lib/join-words.ts';
import { entryWords } from './lib/keyed/words.ts';

describe('the join page opened without an invitation', () => {
  it('says who the page is for, what to do with the link, who to ask, and where listeners go', () => {
    expect(bareWords('Kettle FM')).toEqual({
      title: 'Join Kettle FM',
      heading: 'Join with an invitation',
      who: 'This page is for people Kettle FM has invited to its studio: guests, remote DJs and presenters.',
      link: 'Have the link the station sent you? Open that link, all of it, on this device, and your invitation comes with it.',
      ask: 'No link, or only part of one? Ask whoever invited you to send it again.',
      listen: 'Here to listen, or to call the show?',
      player: 'Go to Kettle FM’s player',
    });
  });

  it('reads as whole sentences before the station’s name is known', () => {
    const w = bareWords('  ');
    expect(w.title).toBe('Join the studio');
    expect(w.who).toBe('This page is for people the station has invited to its studio: guests, remote DJs and presenters.');
    expect(w.player).toBe('Go to the player');
  });

  it('is not worded as a fault, and blames nobody', () => {
    const all = Object.values(bareWords('Kettle FM')).join(' ');
    expect(all).not.toMatch(/doesn't work|does not work|error|wrong|invalid|problem|sorry/i);
  });

  it('keeps the words /listen/ uses for the same advice', () => {
    // "Open that link, all of it, on this device" is said the same way on both pages.
    expect(bareWords('Kettle FM').link).toContain('Open that link, all of it, on this device');
    expect(entryWords('Kettle FM').link).toContain('Open that link, all of it, on this device');
  });
});
