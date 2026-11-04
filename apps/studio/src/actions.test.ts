// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The desk's actions without a page: an arm remembers whose tile it was pressed on (End pass,
// ADR-0119) while the keyed two-press actions carry no identity, and ending a pass names the
// person in its confirmation even though the view that comes back no longer has them.
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from './lib/api.ts';
import { run } from './lib/actions.ts';
import { studio } from './lib/studio.svelte.ts';
import type { DeskPerson, View } from './lib/api.ts';

vi.mock('./lib/api.ts', async (orig) => ({ ...(await orig<typeof api>()), endPass: vi.fn(), desk: vi.fn() }));

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
  };
}

beforeEach(() => {
  studio.view = view(true);
  studio.armed = null;
  studio.note = null;
  vi.mocked(api.endPass).mockReset();
  vi.mocked(api.desk).mockReset();
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
    expect(studio.note?.message).toBe('Gregor is out of the room and their link no longer works. Send a new invite if they are to come back.');
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
