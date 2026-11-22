// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The handover window at the desk (1.2; handover-window.md): the rows under the clock's post
// line, the seat notice, and the rules' choices.
import { describe, expect, it } from 'vitest';
import type { HandoverWindow, Notice, PostView, Source } from './lib/api.ts';
import { rulesProblems, ruleFieldOf } from './lib/hosts.ts';
import { fromWords, headsUpNotice, names, nextSeat, readinessChip, windowChoices, windowRows } from './lib/window.ts';

// Friday 20 November 2026, 13:52 UTC: Steeped starts at 14:00, Kettle Drive at 16:00.
const NOW = Date.UTC(2026, 10, 20, 13, 52);
const AT_14 = Date.UTC(2026, 10, 20, 14, 0);
const AT_16 = Date.UTC(2026, 10, 20, 16, 0);

const win = (o: Partial<HandoverWindow> = {}): HandoverWindow => ({
  state: 'announced',
  show: 'steeped',
  show_name: 'Steeped',
  start_ms: AT_14,
  opens_ms: AT_14 - 300_000,
  host: 'Finlay Grant',
  from: 'push',
  heads_up_ms: null,
  ...o,
});
const post = (o: Partial<PostView> = {}): PostView => ({
  show: { slug: 'steeped', name: 'Steeped', start_ms: AT_14, started: false },
  source: 'K1',
  source_name: 'Finlay Grant',
  state: 'not_connected',
  holders: [],
  window: win(),
  ...o,
});
const person = (identity: string, o: Partial<Source> = {}): Source => ({
  identity,
  name: identity,
  role: 'dj',
  on_air: false,
  mute: false,
  cue: false,
  talk: false,
  talk_to: null,
  present: true,
  away_ms: null,
  linked: true,
  lufs: null,
  lufs_peak: null,
  slot: 5,
  caller: null,
  ...o,
});
const notice = (o: Partial<Notice> = {}): Notice => ({ id: 1, at_ms: AT_14 - 300_000, surface: 'desk', tone: 'good', kind: 'handover.heads_up', subject: null, title: 'Next: Finlay Grant, Steeped', message: 'Steeped starts in 5 min; Finlay Grant is not here yet.', ...o });

describe('the handover window’s rows', () => {
  it('says nothing while the window is quiet, off, or not sent', () => {
    expect(windowRows(null, NOW)).toEqual([]);
    expect(windowRows(post({ window: null }), NOW)).toEqual([]);
    expect(windowRows(post({ window: win({ state: 'quiet' }) }), NOW)).toEqual([]);
  });

  it('announces the next host, where they come from, and that they are not here yet', () => {
    const [r, ...more] = windowRows(post(), NOW);
    expect(more).toEqual([]);
    expect(r).toMatchObject({ is: 'next', label: 'Next', who: 'Finlay Grant', from: 'own studio', readiness: 'not_here', note: null, show: 'steeped' });
    expect(r.what).toMatch(/^Steeped · \d\d:00$/);
    expect(r.said).toMatch(/^Next: Finlay Grant, Steeped · \d\d:00, own studio, not here yet\.$/);
  });

  it('follows the host from connecting to ready, and says when they were told', () => {
    expect(windowRows(post({ window: win({ state: 'cueing' }) }), NOW)[0].readiness).toBe('connecting');
    const ready = windowRows(post({ window: win({ state: 'ready', from: 'room', heads_up_ms: AT_14 - 300_000 }) }), NOW)[0];
    expect(ready).toMatchObject({ readiness: 'ready', from: 'in the room' });
    expect(ready.note).toMatch(/^heads-up \d\d:55$/);
  });

  it('a host running over is named, with how far the next one has got', () => {
    const rows = windowRows(post({ show: { slug: 'steeped', name: 'Steeped', start_ms: AT_14, started: true }, state: 'standing_by', holders: ['Eilidh Fraser-MacLeod'], window: win({ state: 'overrun' }) }), AT_14 + 60_000);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ is: 'overrun', label: 'Over', who: 'Eilidh', note: 'Finlay is ready', readiness: null });
    expect(rows[0].what).toMatch(/^Steeped was due \d\d:00$/);
    expect(rows[0].said).toMatch(/^Eilidh is running over\. Steeped was due \d\d:00\. Finlay is ready\.$/);
    const two = windowRows(post({ show: { slug: 'steeped', name: 'Steeped', start_ms: AT_14, started: true }, holders: ['Eilidh Fraser-MacLeod', 'Gregor Stewart'], window: win({ state: 'overrun' }) }), AT_14 + 60_000)[0];
    expect(two.said).toMatch(/^Eilidh and Gregor are running over\./);
  });

  it('a held handover keeps the next host in sight, with who holds it', () => {
    const [r] = windowRows(post({ show: { slug: 'steeped', name: 'Steeped', start_ms: AT_14, started: true }, state: 'standing_by', held_by: 'moira', window: win({ state: 'held' }) }), AT_14 + 60_000);
    expect(r).toMatchObject({ is: 'next', readiness: 'ready', note: 'held by moira: a press only' });
  });

  it('shows both when the show under way still waits for a late host and the next window opens: the late one first', () => {
    // Steeped began at 14:00 and Finlay never came; Kettle Drive's window has opened.
    const p = post({
      show: { slug: 'steeped', name: 'Steeped', start_ms: AT_14, started: true },
      state: 'not_connected',
      window: win({ state: 'ready', show: 'kettle-drive', show_name: 'Kettle Drive', start_ms: AT_16, opens_ms: AT_16 - 300_000, host: 'Heather Douglas', from: 'room' }),
    });
    const rows = windowRows(p, AT_16 - 240_000);
    expect(rows.map((r) => r.is)).toEqual(['late', 'next']);
    expect(rows[0]).toMatchObject({ label: 'Late', who: 'Finlay Grant', readiness: 'not_here', show: 'steeped' });
    expect(rows[0].what).toMatch(/^Steeped · since \d\d:00$/);
    expect(rows[1]).toMatchObject({ label: 'Next', who: 'Heather Douglas', readiness: 'ready', from: 'in the room', show: 'kettle-drive' });
  });

  it('names a presenter who never came to the room, as the station says who is late (1.2.1)', () => {
    // Steeped began at 14:00 from the room and Heather never came to the desk: the post has no
    // source of hers to name and is already Kettle Drive's, whose window is open.
    const p = post({
      show: { slug: 'kettle-drive', name: 'Kettle Drive', start_ms: AT_16, started: false },
      state: 'standing_by',
      window: win({
        state: 'ready',
        show: 'kettle-drive',
        show_name: 'Kettle Drive',
        start_ms: AT_16,
        opens_ms: AT_16 - 300_000,
        late: { show: 'steeped', show_name: 'Steeped', start_ms: AT_14, host: 'Heather Douglas', from: 'room', state: 'not_connected' },
      }),
    });
    const rows = windowRows(p, AT_16 - 240_000);
    expect(rows.map((r) => r.is)).toEqual(['late', 'next']);
    expect(rows[0]).toMatchObject({ label: 'Late', who: 'Heather Douglas', from: 'in the room', readiness: 'not_here', show: 'steeped' });
    expect(rows[0].what).toMatch(/^Steeped · since \d\d:00$/);
    expect(rows[0].said).toMatch(/^Late: Heather Douglas, Steeped · since \d\d:00, in the room, not here yet\.$/);
    expect(rows[1]).toMatchObject({ label: 'Next', who: 'Finlay Grant', readiness: 'ready', show: 'kettle-drive' });
  });

  it('takes the station’s word that nobody is late, and how far a late host has got', () => {
    const base = { show: { slug: 'steeped', name: 'Steeped', start_ms: AT_14, started: true }, state: 'not_connected' as const };
    const next = { state: 'announced' as const, show: 'kettle-drive', show_name: 'Kettle Drive', start_ms: AT_16, opens_ms: AT_16 - 300_000, host: 'Heather Douglas' };
    // A 1.2.1 station that says null: no late row, whatever the post's source reads.
    expect(windowRows(post({ ...base, window: win({ ...next, late: null }) }), AT_16 - 240_000).map((r) => r.is)).toEqual(['next']);
    // Connecting, from their own studio.
    const [late] = windowRows(post({ ...base, window: win({ ...next, late: { show: 'steeped', show_name: 'Steeped', start_ms: AT_14, host: 'Finlay Grant', from: 'push', state: 'connecting' } }) }), AT_16 - 240_000);
    expect(late).toMatchObject({ is: 'late', who: 'Finlay Grant', from: 'own studio', readiness: 'connecting' });
  });

  it('the late row is left out once the show under way has been dealt with', () => {
    // Its host went on air and came off: the window moved on, and nobody is late.
    const p = post({
      show: { slug: 'steeped', name: 'Steeped', start_ms: AT_14, started: true },
      state: 'on_air',
      window: win({ state: 'announced', show: 'kettle-drive', show_name: 'Kettle Drive', start_ms: AT_16, opens_ms: AT_16 - 300_000, host: 'Heather Douglas' }),
    });
    expect(windowRows(p, AT_16 - 240_000).map((r) => r.is)).toEqual(['next']);
  });

  it('a readiness is a chip of the vocabulary with its own word', () => {
    expect(readinessChip('ready').word).toBe('Ready');
    expect(readinessChip('connecting').word).toBe('Connecting');
    expect(readinessChip('not_here').word).toBe('Not here yet');
    expect(readinessChip('not_here').chip.cls).toContain('dashed');
  });

  it('names and origins in plain words', () => {
    expect(names(['Eilidh Fraser-MacLeod'])).toBe('Eilidh');
    expect(names(['Eilidh Fraser-MacLeod', 'Gregor Stewart', 'Kirsty Wilson'])).toBe('Eilidh, Gregor and Kirsty');
    expect(fromWords('room')).toBe('in the room');
    expect(fromWords('push')).toBe('own studio');
    expect(fromWords('pull')).toBe('pulled by the station');
    expect(fromWords(null)).toBeNull();
  });
});

describe('the heads-up and the seat notice', () => {
  it('the heads-up is the newest one since the window opened', () => {
    const old = notice({ id: 1, at_ms: AT_14 - 3_600_000 });
    expect(headsUpNotice({ notices: [old], post: post() })).toBeNull();
    expect(headsUpNotice({ notices: [old, notice({ id: 2 })], post: post() })?.id).toBe(2);
  });

  it('a host in their own studio is found by their show, and told on their own tile until the post', () => {
    const tile = person('source-k1', { role: 'source', source: { way: 'icecast', format: 'mp3', key: 'K1', show: 'steeped', connected_ms: 9000, leveller_db: 0 } });
    const v = { notices: [], sources: [person('eilidh', { role: 'presenter', on_air: true }), tile], post: post({ state: 'standing_by', window: win({ state: 'ready', heads_up_ms: AT_14 - 300_000 }) }) };
    const seat = nextSeat(v, NOW);
    expect(seat?.identity).toBe('source-k1');
    expect(seat?.words).toMatch(/^On next · Steeped at \d\d:00 · told \d\d:55$/);
    expect(nextSeat(v, AT_14 + 1)).toBeNull();
    expect(nextSeat({ ...v, sources: [{ ...tile, on_air: true }] }, NOW)).toBeNull();
  });

  it('a presenter in the room is found by the heads-up’s subject, or by the post’s own source', () => {
    const p = post({ source: 'heather', source_name: 'Heather Douglas', window: win({ state: 'ready', from: 'room', host: 'Heather Douglas' }) });
    expect(nextSeat({ notices: [], sources: [person('heather')], post: p }, NOW)?.identity).toBe('heather');
    const elsewhere = post({ window: win({ state: 'ready', show: 'kettle-drive', show_name: 'Kettle Drive', start_ms: AT_16, opens_ms: NOW - 1000, host: 'Heather Douglas', from: 'room' }) });
    expect(nextSeat({ notices: [], sources: [person('heather')], post: elsewhere }, NOW)).toBeNull();
    expect(nextSeat({ notices: [notice({ at_ms: NOW, subject: 'heather' })], sources: [person('heather')], post: elsewhere }, NOW)?.identity).toBe('heather');
  });

  it('nobody is told on a tile while the window is quiet or the host is running over', () => {
    const v = { notices: [], sources: [person('heather')], post: post({ source: 'heather', window: win({ state: 'quiet' }) }) };
    expect(nextSeat(v, NOW)).toBeNull();
    expect(nextSeat({ ...v, post: post({ source: 'heather', window: win({ state: 'overrun' }) }) }, NOW)).toBeNull();
  });
});

describe('the rules’ window', () => {
  const rules = { door_before_min: 30, grace_s: 120, late_count_s: 20, test_outside: true, test_max_min: 15, steady_s: 5, silence_s: 4, place_kept_s: 300, fade_ms: 2000, port_icecast: 8005, port_srt: 9710, port_whip: 9712 };

  it('offers Off and 5, 10, 15 and 30 minutes', () => {
    expect(windowChoices(300).map((w) => w.s)).toEqual([0, 300, 600, 900, 1800]);
    expect(windowChoices(undefined).map((w) => w.s)).toEqual([0, 300, 600, 900, 1800]);
  });

  it('shows a stored length that is none of the five as it is, in its place', () => {
    const c = windowChoices(420);
    expect(c.map((w) => w.s)).toEqual([0, 300, 420, 600, 900, 1800]);
    expect(c[2].label).toBe('7 min before (set through the API)');
    expect(windowChoices(90).find((w) => w.s === 90)?.label).toBe('1 min 30 s before (set through the API)');
  });

  it('checks 0 to 1,800 where the station has the setting, and nothing where it has not', () => {
    expect(rulesProblems(rules).window_s).toBeUndefined();
    expect(rulesProblems({ ...rules, window_s: 0 }).window_s).toBeUndefined();
    expect(rulesProblems({ ...rules, window_s: 1800 }).window_s).toBeUndefined();
    expect(rulesProblems({ ...rules, window_s: 1801 }).window_s).toBeTruthy();
    expect(ruleFieldOf('window_s')).toBe('window_s');
  });

  it('a port the installation turned off does not stop the rules being saved', () => {
    expect(rulesProblems({ ...rules, port_icecast: 0, port_srt: 0 })).toEqual({});
  });
});
