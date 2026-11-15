// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Delete show (ADR-0279): who is offered it, the words of its confirmation, and the request.
import { render } from 'svelte/server';
import { afterEach, describe, expect, it, vi } from 'vitest';
import ShowEditor from './schedule/ShowEditor.svelte';
import { account } from './lib/account.svelte.ts';
import { mayDeleteShow } from './lib/account.ts';
import { sched } from './lib/schedule.svelte.ts';
import { deleteShow } from './lib/schedule.ts';
import { countsOf, deletedWords, goesWith } from './schedule/showDelete.ts';
import type { Me } from './lib/api.ts';
import { CALENDAR } from '../../../packages/ui/panels.ts';

const SHOW = { id: 'i', slug: 'folk-club', name: 'The Folk Club', description: '', colour_slot: 2, layout: 'poster' as const, slot: null, hosts: [], tags: [], explicit: false, feed_guid: null, cards: null, presenter: '', contact: '', links: [], commercial_music: false, presenter_account: null };
const me = (role: Me['role'], kind: Me['kind'] = 'staff'): Me => ({ kind, role });

const editor = (onDeleted?: () => void) => render(ShowEditor, { props: { slug: SHOW.slug, show: SHOW as never, onSaved: () => {}, onCard: () => {}, onDeleted } }).body;

afterEach(() => {
  sched.rules = null;
  account.me = null;
  account.loaded = false;
  vi.unstubAllGlobals();
});

describe('who is offered Delete show…', () => {
  it('an admin, a producer and the station key; not a presenter or a screener', () => {
    expect(mayDeleteShow(me('admin'))).toBe(true);
    expect(mayDeleteShow(me('producer'))).toBe(true);
    expect(mayDeleteShow(me('admin', 'station'))).toBe(true);
    expect(mayDeleteShow(me('presenter'))).toBe(false);
    expect(mayDeleteShow(me('screener'))).toBe(false);
    expect(mayDeleteShow(null)).toBe(false);
  });

  it('is a secondary action at the foot of the show editor, named for the panel', () => {
    sched.rules = true;
    account.loaded = true;
    account.me = me('producer');
    const body = editor(() => {});
    const foot = body.slice(body.indexOf('class="foot"'));
    expect(foot).toContain('>Delete show…</button>');
    expect(foot).toMatch(/class="btn danger-hollow del"[^>]*>Delete show…/);
    expect(foot.indexOf('Delete show…')).toBeLessThan(foot.indexOf('Save show'));
  });

  it('is not shown to a presenter, before the station has said who this is, on a station older than 1.0.4, or without the tab to tell', () => {
    sched.rules = true;
    account.loaded = true;
    account.me = me('presenter');
    expect(editor(() => {})).not.toContain('Delete show');
    account.me = me('admin');
    account.loaded = false;
    expect(editor(() => {})).not.toContain('Delete show');
    account.loaded = true;
    sched.rules = false;
    expect(editor(() => {})).not.toContain('Delete show');
    sched.rules = true;
    expect(editor()).not.toContain('Delete show');
  });
});

describe('the confirmation says what goes with the show', () => {
  it('counts what the studio has read', () => {
    expect(goesWith({ series: 2, episodes: 7 })).toBe('Its 2 series and 7 drafted episodes go with it, and its cart banks and cards.');
    expect(goesWith({ series: 1, episodes: 1 })).toBe('Its 1 series and 1 drafted episode go with it, and its cart banks and cards.');
    expect(goesWith({ series: 1, episodes: 0 })).toBe('Its 1 series goes with it, and its cart banks and cards.');
    expect(goesWith({ series: 3, episodes: 0 })).toBe('Its 3 series go with it, and its cart banks and cards.');
    expect(goesWith({ series: 0, episodes: 0 })).toBe('Its cart banks and cards go with it.');
  });

  it('says the plain list when it could not count', () => {
    expect(goesWith({ series: null, episodes: null })).toBe('Its series, drafted episodes, cart banks and cards go with it.');
  });

  it('counts series and the drafted episodes in them, not one that has aired', () => {
    const draft = { episode: { aired_at: null } };
    const aired = { episode: { aired_at: 1_700_000_000 } };
    expect(countsOf([{ episodes: [draft, draft] }, { episodes: [draft, aired] }])).toEqual({ series: 2, episodes: 3 });
    expect(countsOf([])).toEqual({ series: 0, episodes: 0 });
  });

  it('tells the tab in one plain line', () => {
    expect(deletedWords('The Folk Club')).toBe('The Folk Club is deleted.');
  });

  it('has its panel in the registry', () => {
    expect(CALENDAR.some((p) => p.name === 'calendar.show-delete')).toBe(true);
  });
});

describe('the request', () => {
  it('is a DELETE on the show, and a refusal carries the station’s sentence', async () => {
    const calls: { url: string; method: string }[] = [];
    vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
      calls.push({ url: String(url), method: init?.method ?? 'GET' });
      return new Response(JSON.stringify({ error: 'The Folk Club has aired 2 episodes, so it stays as the station’s record: only a show that has never aired can be deleted.' }), { status: 409, headers: { 'content-type': 'application/json' } });
    });
    await expect(deleteShow('folk-club')).rejects.toMatchObject({ status: 409, message: expect.stringContaining('has aired 2 episodes') });
    expect(calls).toHaveLength(1);
    expect(calls[0]!.method).toBe('DELETE');
    expect(calls[0]!.url).toMatch(/\/api\/v1\/shows\/folk-club$/);
  });
});
