// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A changed rule, told to every window (`rule.changed`; new-show.md section 3): which notices
// are news to a window, and which of them are for the change the window made itself.
import { afterEach, describe, expect, it, vi } from 'vitest';
import * as api from './lib/api.ts';
import type { Notice } from './lib/api.ts';
import { Heard, NEWS_MS, OWN_MS, forgetOwn, isAbout, markOwn, takeOwn, unmarkOwn } from './lib/ruleNotice.ts';

const SHOWS = [
  { slug: 'folk-club', name: 'The Folk Club' },
  { slug: 'firth-sessions', name: 'Firth Sessions' },
];
const T = 1_794_000_000_000;
const notice = (id: number, title: string, at_ms = T, kind = 'rule.changed'): Notice => ({ id, at_ms, surface: 'desk', tone: 'good', kind, subject: null, title, message: '' });
const MOIRA = 'Moira changed when The Folk Club airs.';

afterEach(() => {
  forgetOwn();
  api.noteStationVersion('1.0.4');
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

describe('since 1.0.5 the notice names its show and its save', () => {
  const said = (title: string, subject: string | null, change: string | null) => ({ title, subject, change });

  it('a save’s token makes the notice this window’s exactly, and only once', () => {
    markOwn('folk-club', T, 'w1');
    expect(takeOwn(said(MOIRA, 'folk-club', 'other-window'), SHOWS, T + 500)).toBe(false);
    expect(takeOwn(said(MOIRA, 'folk-club', 'w1'), SHOWS, T + 600)).toBe(true);
    expect(takeOwn(said(MOIRA, 'folk-club', 'w1'), SHOWS, T + 700)).toBe(false);
  });

  it('two windows of one account saving one show are told apart', () => {
    markOwn('firth-sessions', T, 'mine');
    expect(takeOwn(said('Shona changed when Firth Sessions airs.', 'firth-sessions', 'theirs'), SHOWS, T + 100)).toBe(false);
    expect(takeOwn(said('Shona changed when Firth Sessions airs.', 'firth-sessions', 'mine'), SHOWS, T + 200)).toBe(true);
  });

  it('two shows of one name are told apart by their path', () => {
    const twins = [...SHOWS, { slug: 'folk-club-2', name: 'The Folk Club' }];
    markOwn('folk-club-2', T);
    expect(takeOwn(said(MOIRA, 'folk-club', null), twins, T + 100)).toBe(false);
    expect(takeOwn(said(MOIRA, 'folk-club-2', null), twins, T + 200)).toBe(true);
  });

  it('a save sent with a token is not taken by another window’s save sent without one', () => {
    markOwn('folk-club', T, 'mine');
    expect(takeOwn(said(MOIRA, 'folk-club', null), SHOWS, T + 100)).toBe(false);
    expect(takeOwn({ title: MOIRA }, SHOWS, T + 150)).toBe(false);
    expect(takeOwn(said(MOIRA, 'folk-club', 'mine'), SHOWS, T + 200)).toBe(true);
  });

  it('a refused save takes back its own mark, not another', () => {
    markOwn('folk-club', T, 'a');
    markOwn('folk-club', T + 10, 'b');
    unmarkOwn('folk-club', 'a');
    expect(takeOwn(said(MOIRA, 'folk-club', 'a'), SHOWS, T + 100)).toBe(false);
    expect(takeOwn(said(MOIRA, 'folk-club', 'b'), SHOWS, T + 100)).toBe(true);
  });

  it('a notice is about a show by its path, or by its sentence from an older station', () => {
    expect(isAbout(said(MOIRA, 'folk-club', null), 'folk-club', 'The Folk Club')).toBe(true);
    expect(isAbout(said(MOIRA, 'folk-club-2', null), 'folk-club', 'The Folk Club')).toBe(false);
    expect(isAbout({ title: MOIRA }, 'folk-club', 'The Folk Club')).toBe(true);
    expect(isAbout({ title: MOIRA }, 'firth-sessions', 'Firth Sessions')).toBe(false);
  });

  it('a 1.0.5 station is sent the token; an older one is not, as it refuses a field it does not know', async () => {
    const sent: unknown[] = [];
    vi.stubGlobal('fetch', vi.fn(async (_u: unknown, init?: RequestInit) => {
      sent.push(JSON.parse(String(init?.body ?? '{}')));
      return new Response(JSON.stringify({ rule: null, rule_words: null, moved: [] }), { status: 200, headers: { 'content-type': 'application/json' } });
    }));
    api.noteStationVersion('1.0.4-test.2');
    await api.putShowRule('folk-club', null, 'leave');
    expect(sent[0]).not.toHaveProperty('change');
    api.noteStationVersion('1.0.5');
    await api.putShowRule('folk-club', null, 'leave');
    const change = (sent[1] as { change?: string }).change;
    expect(change).toMatch(/^[A-Za-z0-9_-]{1,64}$/);
    expect(takeOwn(said(MOIRA, 'folk-club', change!), SHOWS)).toBe(true);
    expect(api.speaksChange('1.0.5-test.1')).toBe(true);
    expect(api.speaksChange('1.0.4')).toBe(false);
  });
});
