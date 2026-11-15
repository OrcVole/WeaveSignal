// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A changed rule, told to every window (`rule.changed`; new-show.md section 3): which notices
// are news to a window, and which of them are for the change the window made itself.
import { afterEach, describe, expect, it, vi } from 'vitest';
import * as api from './lib/api.ts';
import type { Notice } from './lib/api.ts';
import { Heard, NEWS_MS, OWN_MS, forgetOwn, markOwn, takeOwn, unmarkOwn } from './lib/ruleNotice.ts';

const SHOWS = [
  { slug: 'folk-club', name: 'The Folk Club' },
  { slug: 'firth-sessions', name: 'Firth Sessions' },
];
const T = 1_794_000_000_000;
const notice = (id: number, title: string, at_ms = T, kind = 'rule.changed'): Notice => ({ id, at_ms, surface: 'desk', tone: 'good', kind, subject: null, title, message: '' });
const MOIRA = 'Moira changed when The Folk Club airs.';

afterEach(() => {
  forgetOwn();
  vi.unstubAllGlobals();
});

describe('which notices are news', () => {
  it('what the first view carries happened before the window looked', () => {
    const heard = new Heard();
    expect(heard.fresh([notice(1, MOIRA)], T)).toEqual([]);
    expect(heard.fresh([notice(1, MOIRA)], T + 1000)).toEqual([]);
  });

  it('a notice is heard once, however many views carry it', () => {
    const heard = new Heard();
    heard.fresh([], T);
    expect(heard.fresh([notice(1, MOIRA)], T).map((n) => n.id)).toEqual([1]);
    expect(heard.fresh([notice(1, MOIRA)], T + 1000)).toEqual([]);
    expect(heard.fresh([notice(1, MOIRA), notice(2, MOIRA, T + 1500)], T + 2000).map((n) => n.id)).toEqual([2]);
  });

  it('only a changed rule is this tab’s to say', () => {
    const heard = new Heard();
    heard.fresh([], T);
    expect(heard.fresh([notice(1, 'Eilidh is ready.', T, 'caller.ready')], T)).toEqual([]);
  });

  it('ids start again when the control plane does: the time tells the two apart', () => {
    const heard = new Heard();
    heard.fresh([notice(1, MOIRA)], T);
    expect(heard.fresh([notice(1, 'Shona changed when Firth Sessions airs.', T + 5000)], T + 5000)).toHaveLength(1);
  });

  it('an old notice first seen late is not news', () => {
    const heard = new Heard();
    heard.fresh([], T);
    expect(heard.fresh([notice(1, MOIRA, T)], T + NEWS_MS)).toEqual([]);
  });
});

describe('the change this window made itself', () => {
  it('is known by the show it saved, and only once', () => {
    markOwn('folk-club', T);
    expect(takeOwn('Shona changed when The Folk Club airs.', SHOWS, T + 800)).toBe(true);
    expect(takeOwn('Moira changed when The Folk Club airs.', SHOWS, T + 900)).toBe(false);
  });

  it('is not another show’s change', () => {
    markOwn('folk-club', T);
    expect(takeOwn('Moira changed when Firth Sessions airs.', SHOWS, T + 800)).toBe(false);
    expect(takeOwn('Shona changed when The Folk Club airs.', SHOWS, T + 900)).toBe(true);
  });

  it('two saves of one show (Skip, then Undo) are two notices', () => {
    markOwn('firth-sessions', T);
    markOwn('firth-sessions', T + 3000);
    expect(takeOwn('Shona changed when Firth Sessions airs.', SHOWS, T + 3500)).toBe(true);
    expect(takeOwn('Shona changed when Firth Sessions airs.', SHOWS, T + 3600)).toBe(true);
    expect(takeOwn('Shona changed when Firth Sessions airs.', SHOWS, T + 3700)).toBe(false);
  });

  it('a mark nobody takes lapses, so it cannot hide a later change by someone else', () => {
    markOwn('folk-club', T);
    expect(takeOwn(MOIRA, SHOWS, T + OWN_MS)).toBe(false);
  });

  it('a refused save leaves no mark', () => {
    markOwn('folk-club', T);
    unmarkOwn('folk-club');
    expect(takeOwn(MOIRA, SHOWS, T + 100)).toBe(false);
  });
});

describe('the rule’s routes mark their own saves', () => {
  const answer = (status: number, body: unknown) => vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })));

  it('a saved rule is marked: its notice is this window’s', async () => {
    answer(200, { rule: null, rule_words: null, moved: [] });
    await api.putShowRule('folk-club', null, 'leave');
    expect(takeOwn('Shona changed when The Folk Club airs.', SHOWS)).toBe(true);
  });

  it('a series’ rule marks its show', async () => {
    answer(200, { rule: null, rule_words: null, moved: [] });
    await api.putSeriesRule('firth-sessions', 2, null, 'move');
    expect(takeOwn('Shona changed when Firth Sessions airs.', SHOWS)).toBe(true);
  });

  it('a refused rule is not: the next notice for that show is someone else’s', async () => {
    answer(409, { error: 'Saturday Night Kettle has that time.', field: 'rule' });
    await expect(api.putShowRule('folk-club', null, 'leave')).rejects.toThrow('Saturday Night Kettle has that time.');
    expect(takeOwn(MOIRA, SHOWS)).toBe(false);
  });
});
