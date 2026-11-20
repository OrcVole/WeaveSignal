// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The desk's actions without a page: an arm remembers whose tile it was pressed on (End pass,
// ADR-0119) while the keyed two-press actions carry no identity, and ending a pass names the
// person in its confirmation even though the view that comes back no longer has them.
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from './lib/api.ts';
import { NO_SHOW } from './lib/steps.ts';
import { run } from './lib/actions.ts';
import { studio } from './lib/studio.svelte.ts';
import type { DeskPerson, View } from './lib/api.ts';

vi.mock('./lib/api.ts', async (orig) => ({ ...(await orig<typeof api>()), endPass: vi.fn(), desk: vi.fn(), restartPlayout: vi.fn(), screener: vi.fn() }));

const person = (identity: string, name: string, role: DeskPerson['role']): DeskPerson => ({
  identity,
  name,
  role,
  on_air: true,
  mute: false,
  cue: false,
  talk: false,
  talk_to: null,
  present: true,
  away_ms: null,
  linked: true,
  lufs: -22.6,
  lufs_peak: -19.8,
});

function view(withGregor: boolean): View {
  return {
    station: 'Kettle FM',
    now_ms: 1_000,
    show: { title: 'The Morning Kettle', rundown: [] },
    desk: { reachable: true, error: null, playout: { connected: true, for_ms: 14_000 } },
    seats: { presenter: 'eilidh', screener: 'moira', presenter_name: 'Eilidh Fraser-MacLeod', screener_name: 'Moira Campbell' },
    sources: [
      { ...person('eilidh', 'Eilidh Fraser-MacLeod', 'presenter'), slot: 1, caller: null },
      ...(withGregor ? [{ ...person('gregor', 'Gregor Stewart', 'guest'), slot: 2, caller: null }] : []),
    ],
    callers: [],
    desk_taking_callers: false,
    chat: { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false, influx: false, influx_since_ms: null },
    notices: [],
    carts: { version: 0, desk: true, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
    listener_delay: { ms: 15_000, measured: false },
    step: NO_SHOW,
  };
}

beforeEach(() => {
  studio.view = view(true);
  studio.armed = null;
  studio.note = null;
  vi.mocked(api.endPass).mockReset();
  vi.mocked(api.desk).mockReset();
  vi.mocked(api.restartPlayout).mockReset();
});

describe('arming', () => {
  it('keeps the identity End pass was pressed for, and none for a keyed two-press action', async () => {
    await run({ type: 'arm', what: 'end-pass', identity: 'gregor' });
    expect(studio.armed).toMatchObject({ what: 'end-pass', identity: 'gregor' });
    // Pressed on another tile: that tile is armed instead.
    await run({ type: 'arm', what: 'end-pass', identity: 'eilidh' });
    expect(studio.armed).toMatchObject({ what: 'end-pass', identity: 'eilidh' });
    await run({ type: 'arm', what: 'drop' });
    expect(studio.armed?.what).toBe('drop');
    expect(studio.armed?.identity).toBeUndefined();
    await run({ type: 'disarm' });
    expect(studio.armed).toBeNull();
  });
});

describe('end pass', () => {
  it('sends the identity, disarms, takes the new view and names the person who has gone', async () => {
    vi.mocked(api.endPass).mockResolvedValue(view(false));
    studio.armed = { what: 'end-pass', at: 0, identity: 'gregor' };
    await run({ type: 'end-pass', identity: 'gregor' });
    expect(api.endPass).toHaveBeenCalledWith({ identity: 'gregor' });
    expect(studio.armed).toBeNull();
    expect(studio.view?.sources.map((s) => s.identity)).toEqual(['eilidh']);
    expect(studio.note?.tone).toBe('good');
    expect(studio.note?.title).toBe('Pass ended');
    expect(studio.note?.message).toBe('Gregor is out of the room and their link no longer works. Send a new invitation if they are to come back.');
  });

  it('says why when the control plane refuses, and keeps the view', async () => {
    vi.mocked(api.endPass).mockRejectedValue(new api.ApiError(502, 'the room did not answer'));
    await run({ type: 'end-pass', identity: 'gregor' });
    expect(studio.note).toMatchObject({ tone: 'warning', title: 'Not done', message: 'the room did not answer' });
    expect(studio.view?.sources).toHaveLength(2);
  });
});

describe('dump', () => {
  const withDelay = (delay_ms: number): View => {
    const v = view(true);
    v.desk.delay = { delay_ms, target_ms: 8_000, rebuilding: true, dumps: 0, dumped_at_ms: null };
    return v;
  };

  it('is sent while the delay rebuilds, when it holds anything (ADR-0166)', async () => {
    studio.view = withDelay(2_000);
    vi.mocked(api.desk).mockResolvedValue(withDelay(0));
    await run({ type: 'dump' });
    expect(api.desk).toHaveBeenCalledWith({ op: 'dump' });
  });

  it('says why when nothing is held yet, and sends nothing', async () => {
    studio.view = withDelay(0);
    await run({ type: 'dump' });
    expect(api.desk).not.toHaveBeenCalled();
    expect(studio.note).toMatchObject({ tone: 'warning', title: 'Nothing held' });
  });
});

describe('a held talk key', () => {
  it('lets go of the talk it opened, even if the presenter’s seat moved meanwhile (ADR-0231)', async () => {
    vi.mocked(api.desk).mockResolvedValue(view(true));
    studio.view = view(true);
    await run({ type: 'talk', slot: 2, held: true });
    // While G is held the handover takes Eilidh off and the seat goes to Finlay.
    const after = view(true);
    after.seats = { ...after.seats, presenter: 'finlay', presenter_name: 'Finlay Grant' };
    after.sources = [...after.sources, { ...person('finlay', 'Finlay Grant', 'dj'), slot: 5, caller: null }];
    studio.view = after;
    vi.mocked(api.desk).mockResolvedValue(after);
    await run({ type: 'talk', slot: 2, held: false });
    expect(vi.mocked(api.desk).mock.calls.map((c) => c[0])).toEqual([
      { op: 'talk_to', identity: 'eilidh', to: 'gregor', on: true },
      { op: 'talk_to', identity: 'eilidh', to: 'gregor', on: false },
    ]);
  });

  it('sends nothing on letting go of a key that opened nothing', async () => {
    await run({ type: 'talk', slot: 3, held: true });
    await run({ type: 'talk', slot: 3, held: false });
    expect(api.desk).not.toHaveBeenCalled();
  });
  // Issue 25: with nobody screening, T on the Screener tab is the presenter's talk to the
  // selected caller; the desk carries the caller's answer back to the presenter alone.
  it('talks as the screener on the Screener tab, else as the presenter, else not at all', async () => {
    const waiting = (v: View): View => ({
      ...v,
      callers: [{ identity: 'niall', name: 'Niall Anderson', state: 'waiting' } as unknown as View['callers'][number]],
    });
    vi.mocked(api.desk).mockImplementation(async () => studio.view!);
    vi.mocked(api.screener).mockImplementation(async () => studio.view!);
    studio.selected = 'niall';
    studio.view = waiting(view(false));
    await run({ type: 'screen-talk', held: true });
    await run({ type: 'screen-talk', held: false });
    const alone = waiting(view(false));
    alone.seats = { ...alone.seats, screener: null, screener_name: '' };
    studio.view = alone;
    await run({ type: 'screen-talk', held: true });
    await run({ type: 'screen-talk', held: false });
    expect(vi.mocked(api.desk).mock.calls.map((c) => c[0])).toEqual([
      { op: 'talk_to', identity: 'moira', to: 'niall', on: true },
      { op: 'talk_to', identity: 'moira', to: 'niall', on: false },
      { op: 'talk_to', identity: 'eilidh', to: 'niall', on: true },
      { op: 'talk_to', identity: 'eilidh', to: 'niall', on: false },
    ]);
    const nobody = waiting(view(false));
    nobody.seats = { ...nobody.seats, screener: null, presenter: null };
    studio.view = nobody;
    vi.mocked(api.desk).mockClear();
    await run({ type: 'screen-talk', held: true });
    expect(api.desk).not.toHaveBeenCalled();
    expect(studio.note).toMatchObject({ title: 'Nobody to talk to' });
    await run({ type: 'screen-talk', held: false });
    expect(api.desk).not.toHaveBeenCalled();
  });

  it('lets go of the Screener tab’s talk it opened, even if a screener joined or the selection moved meanwhile', async () => {
    const queue = (v: View): View => ({
      ...v,
      callers: ['niall', 'jessie'].map((identity) => ({ identity, name: identity, state: 'screening' }) as unknown as View['callers'][number]),
    });
    const alone = queue(view(false));
    alone.seats = { ...alone.seats, screener: null, screener_name: '' };
    vi.mocked(api.desk).mockImplementation(async () => studio.view!);
    studio.view = alone;
    studio.selected = 'niall';
    await run({ type: 'screen-talk', held: true });
    // While T is held Moira joins as the screener and the selection moves to Jessie.
    studio.view = queue(view(false));
    studio.selected = 'jessie';
    await run({ type: 'screen-talk', held: false });
    expect(vi.mocked(api.desk).mock.calls.map((c) => c[0])).toEqual([
      { op: 'talk_to', identity: 'eilidh', to: 'niall', on: true },
      { op: 'talk_to', identity: 'eilidh', to: 'niall', on: false },
    ]);
  });
});

// Issue 9 (ADR-0289): ⌃R twice restarts playout through the station; ⌃⇧R and ⌃⇧B are not keys; ⌃4
// opens the Screener's Room lane, because the chat is moderated there and the app has no Chat tab.
describe('restarting playout (⌃R twice)', () => {
  it('asks the station once, says so, and keeps no key armed', async () => {
    vi.mocked(api.restartPlayout).mockResolvedValue({ restarting: true, words: 'Playout is restarting. Listeners hear the fallback for a few seconds.' });
    studio.armed = { what: 'restart-playout', at: 1 };
    await run({ type: 'restart-playout' });
    expect(api.restartPlayout).toHaveBeenCalledTimes(1);
    expect(studio.armed).toBeNull();
    expect(studio.note).toMatchObject({ tone: 'good', title: 'Playout is restarting' });
  });

  it('says what the second press does when the first arms it', async () => {
    await run({ type: 'arm', what: 'restart-playout' });
    expect(studio.armed).toMatchObject({ what: 'restart-playout' });
    expect(studio.note).toMatchObject({ title: 'Restart playout?' });
    expect(studio.note?.message).toContain('⌃R again');
    expect(api.restartPlayout).not.toHaveBeenCalled();
  });

  it('gives the station’s own words when it refuses', async () => {
    vi.mocked(api.restartPlayout).mockRejectedValue(new api.ApiError(502, 'Playout did not answer. If it is stopped, the server’s supervisor is starting it.'));
    await run({ type: 'restart-playout' });
    expect(studio.note).toMatchObject({ tone: 'warning', title: 'Not done' });
    expect(studio.note?.message).toContain('supervisor');
  });
});

describe('⌃4', () => {
  /** A page with a Room panel in it: the lines the focus may go to, and the heading. */
  const page = (lines: boolean) => {
    const focused: string[] = [];
    const el = (name: string, attrs: Record<string, string> = {}) => ({
      focus: () => focused.push(name),
      hasAttribute: (n: string) => n in attrs,
      setAttribute: (n: string, v: string) => (attrs[n] = v),
    });
    const line = el('line', { tabindex: '0' });
    const head = el('heading');
    const room = { querySelector: (q: string) => (q.includes('room-line') ? (lines ? line : null) : q === 'h2' ? head : null) };
    vi.stubGlobal('document', {
      documentElement: { dataset: {} as Record<string, string> },
      querySelector: (q: string) => (q === '[data-panel="screener.room"]' ? room : null),
    });
    return focused;
  };

  it('opens the Screener tab and puts the focus in the Room lane, on its newest or selected line', async () => {
    const focused = page(true);
    studio.tab = 'desk';
    await run({ type: 'tab', tab: 'chat' });
    expect(studio.tab).toBe('screener');
    expect(focused).toEqual(['line']);
    vi.unstubAllGlobals();
  });

  it('falls back to the Room’s heading when the Room has no line yet', async () => {
    const focused = page(false);
    studio.tab = 'carts';
    await run({ type: 'tab', tab: 'chat' });
    expect(studio.tab).toBe('screener');
    expect(focused).toEqual(['heading']);
    vi.unstubAllGlobals();
  });

  it('never says the Chat tab is missing: there is no such note', async () => {
    studio.tab = 'desk';
    await run({ type: 'tab', tab: 'chat' });
    expect(studio.note).toBeNull();
  });
});
