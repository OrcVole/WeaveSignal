// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What the public hears now (`desk.keyed`, keyed shows): the three states of a keyed show's
// window on the desk. Kept, not kept (the alarm), and, from 1.3.2, not confirmed, while playout
// does not answer the station and neither of the other two is playout's word.
import { render } from 'svelte/server';
import { beforeEach, describe, expect, it } from 'vitest';
import DeskB from './desk/DeskB.svelte';
import { account } from './lib/account.svelte.ts';
import type { View } from './lib/api.ts';
import { NOT_CONFIRMED, keyedStanding } from './lib/keys.ts';
import { keyed } from './lib/keys.svelte.ts';
import { studio } from './lib/studio.svelte.ts';

const NOW = Date.UTC(2026, 10, 27, 20, 12, 0);
const SHOW = '0193a000-0000-7000-8000-000000000007';

function view(k: NonNullable<View['keyed']> | null): View {
  return {
    station: 'Kettle FM',
    now_ms: NOW,
    show: { title: 'Friday Folk', rundown: [] },
    desk: { reachable: true, error: null, playout: { connected: true, for_ms: 14_000 }, delay: { delay_ms: 8000, target_ms: 8000, rebuilding: false, dumps: 0, dumped_at_ms: null } },
    seats: { presenter: 'isla', screener: 'shona', presenter_name: 'Isla Fraser', screener_name: 'Shona Campbell' },
    sources: [],
    callers: [],
    desk_taking_callers: false,
    chat: { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false, influx: false, influx_since_ms: null, held: 0, open_reports: 0 },
    notices: [],
    carts: { version: 0, desk: true, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
    listener_delay: { ms: 15_000, measured: false },
    mode: 'live',
    step: { step: 'full', show: { slug: 'friday-folk', name: 'Friday Folk' }, on_air: true, next: null, may_change: true },
    keyed: k,
  };
}

function panel(k: NonNullable<View['keyed']> | null): string {
  studio.view = view(k);
  const body = render(DeskB).body;
  const at = body.indexOf('data-panel="desk.keyed"');
  if (at < 0) return '';
  return body.slice(at, body.indexOf('class="q"', at));
}
const text = (html: string): string => html.replace(/<!--[\s\S]*?-->/g, '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();

beforeEach(() => {
  account.me = { kind: 'staff', role: 'producer', account: { id: 'a-shona', username: 'shona', name: 'Shona Campbell', role: 'producer', disabled_ms: null, created_ms: 0 }, preparing: false };
  account.loaded = true;
  studio.tab = 'desk';
  studio.settings = false;
  studio.now = NOW;
  studio.skew = 0;
  studio.zone = 'Europe/London';
  keyed.list = { shows: [{ id: SHOW, slug: 'friday-folk', name: 'Friday Folk', now: 'key', opens_at_ms: null, slate: 'announcement', slate_note: '' }], get_a_key_text: '', get_a_key_url: '' };
});

describe('where a keyed window stands', () => {
  it('is kept or not kept by playout’s own word, and not confirmed when playout gave none', () => {
    expect(keyedStanding({ kept: true, confirmed: true })).toBe('kept');
    expect(keyedStanding({ kept: false, confirmed: true })).toBe('not-kept');
    // Playout away: the station reports kept (the listener process's fallback, by design), and
    // that is not playout's word. Not confirmed wins over both.
    expect(keyedStanding({ kept: true, confirmed: false })).toBe('not-confirmed');
    expect(keyedStanding({ kept: false, confirmed: false })).toBe('not-confirmed');
  });

  it('reads a station that does not say as confirmed, as it was before', () => {
    expect(keyedStanding({ kept: true })).toBe('kept');
    expect(keyedStanding({ kept: false })).toBe('not-kept');
  });
});

describe('the desk’s What the public hears now', () => {
  it('is not drawn while no keyed show’s window is open', () => {
    expect(panel(null)).toBe('');
  });

  it('kept: the quiet chip, what the public hears, and no alarm', () => {
    const p = panel({ show: SHOW, kept: true, confirmed: true, public_hears: 'slate_file' });
    expect(p).toMatch(/data-kept="true" data-standing="kept"/);
    expect(p).toMatch(/class="state sm hollow hollow-standby[^"]*"/);
    expect(p).not.toContain('dashed');
    expect(text(p)).toContain('Friday Folk is for people with a key, and the station is keeping it from the public: they hear the station’s slate until it ends');
    expect(p).not.toMatch(/role="alert"|role="status"/);
    expect(text(p)).not.toContain('not confirmed');
  });

  it('not kept: the alarm, as playout said', () => {
    const p = panel({ show: SHOW, kept: false, confirmed: true, public_hears: null });
    expect(p).toMatch(/data-standing="not-kept"/);
    expect(p).toContain('hollow-warning');
    expect(p).toContain('role="alert"');
    expect(text(p)).toContain('but playout is not keeping it from the public: everyone hears the show. Restart playout, and see its log.');
  });

  it('not confirmed: said beside the heading and in a sentence, with what should be so and what to do; a status, not an alarm', () => {
    const p = panel({ show: SHOW, kept: true, confirmed: false, public_hears: 'listener_fallback' });
    expect(p).toMatch(/data-kept="true" data-standing="not-confirmed"/);
    // The dashed chip is NOT CONNECTED's borrowed shape: playout is not answering.
    expect(p).toMatch(/class="state sm hollow hollow-standby[^"]* dashed[^"]*"/);
    expect(p).not.toContain('hollow-warning');
    expect(text(p)).toContain('What the public hears now · not confirmed');
    expect(p).toContain('role="status"');
    expect(p).not.toContain('role="alert"');
    expect(text(p)).toContain(`Friday Folk ${NOT_CONFIRMED.body} ${NOT_CONFIRMED.act}`);
    // It claims neither of the other two.
    expect(text(p)).not.toContain('is keeping it from the public');
    expect(text(p)).not.toContain('everyone hears the show');
  });

  it('not confirmed wins over the alarm too: nothing playout did not say is said as fact', () => {
    const p = panel({ show: SHOW, kept: false, confirmed: false, public_hears: null });
    expect(p).toMatch(/data-standing="not-confirmed"/);
    expect(p).not.toContain('role="alert"');
  });

  it('a station that does not send confirmed draws the two states it always did', () => {
    expect(panel({ show: SHOW, kept: true, public_hears: 'fallback' })).toMatch(/data-standing="kept"/);
    expect(panel({ show: SHOW, kept: false, public_hears: null })).toMatch(/data-standing="not-kept"/);
  });

  it('says what should be so without saying it is', () => {
    expect(NOT_CONFIRMED.body).toContain('this is not confirmed');
    expect(NOT_CONFIRMED.body).toContain('should be playing the station’s own fallback, not the show');
    expect(NOT_CONFIRMED.body).not.toMatch(/is playing|they hear/);
    expect(NOT_CONFIRMED.act).toContain('restart playout');
  });
});
