// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The update screen's rule: nothing starts while a strip is open or a talk key is held.
import { describe, expect, it } from 'vitest';
import { dateWords, megabytes, progressWords, updateGate } from './update.ts';
import type { Source, View } from './api.ts';

function source(over: Partial<Source>): Source {
  return {
    identity: 'a', name: 'Moira Fraser', role: 'presenter', on_air: false, mute: false, cue: false, talk: false,
    talk_to: null, present: true, away_ms: null, linked: true, lufs: null, lufs_peak: null, slot: 1, caller: null,
    ...over,
  };
}

function view(sources: Source[], desk: { reachable?: boolean; playout?: boolean } = {}): View {
  return {
    sources,
    desk: {
      reachable: desk.reachable ?? true,
      error: null,
      playout: { connected: desk.playout ?? true, for_ms: null },
    },
  } as View;
}

describe('whether the app may be updated now', () => {
  it('refuses while a strip is open and the station is on air', () => {
    const g = updateGate(view([source({ on_air: true })]));
    expect(g.blocked).toMatch(/^The station is on air: 1 strip is open on the desk/);
  });

  it('counts every open strip', () => {
    const g = updateGate(view([source({ on_air: true }), source({ identity: 'b', on_air: true }), source({ identity: 'c' })]));
    expect(g.blocked).toContain('2 strips are open');
  });

  it('refuses while playout is down and the room is still open (the white OPEN lamp)', () => {
    const g = updateGate(view([source({ on_air: true })], { playout: false }));
    expect(g.blocked).toMatch(/^The room is open: 1 strip is open/);
  });

  it('allows a muted strip, a strip that is only on cue, and an empty desk', () => {
    expect(updateGate(view([source({ on_air: true, mute: true })])).blocked).toBeNull();
    expect(updateGate(view([source({ cue: true })])).blocked).toBeNull();
    expect(updateGate(view([])).blocked).toBeNull();
  });

  it('refuses while a talk key is held, even with no strip open', () => {
    expect(updateGate(view([]), ['a']).blocked).toMatch(/talk key is held/);
    expect(updateGate(null, ['a']).blocked).toMatch(/talk key is held/);
  });

  it('allows with a caution when the studio cannot see the desk', () => {
    const g = updateGate(null);
    expect(g.blocked).toBeNull();
    expect(g.caution).toMatch(/cannot tell whether a show is on air/);
  });

  it('allows when the desk is down: nothing is on air', () => {
    expect(updateGate(view([], { reachable: false })).blocked).toBeNull();
  });
});

describe('the words', () => {
  it('gives sizes in decimal megabytes and progress as a share', () => {
    expect(megabytes(83_886_080)).toBe('83.9 MB');
    expect(progressWords(40_000_000, 80_000_000)).toBe('40.0 MB of 80.0 MB (50 %)');
    expect(progressWords(1_500_000, null)).toBe('1.5 MB');
  });

  it('gives a release date, or nothing', () => {
    expect(dateWords(null)).toBe('');
    expect(dateWords(Date.UTC(2026, 10, 2, 12) / 1000, 'en-GB')).toBe('2 November 2026');
  });
});
