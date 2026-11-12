// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// End show and the control for automation (ADR-0272, ADR-0183): what each one asks before it
// acts, what it says after, and that the desk draws the button and the one dialog.
import { render } from 'svelte/server';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { airPlan, endShowNote, modeButton, modeNote, modeRefusal, needsConfirm } from './lib/air.ts';
import type { EndShowResult, Source, View } from './lib/api.ts';
import { NO_SHOW } from './lib/steps.ts';
import DeskB from './desk/DeskB.svelte';
import { studio } from './lib/studio.svelte.ts';
import { run, pressMode, confirmAir } from './lib/actions.ts';

const src = (identity: string, name: string, over: Partial<Source> = {}): Source => ({
  identity,
  name,
  role: 'presenter',
  on_air: true,
  mute: false,
  cue: false,
  talk: false,
  talk_to: null,
  present: true,
  away_ms: null,
  linked: true,
  lufs: -22,
  lufs_peak: -20,
  slot: 1,
  caller: null,
  ...over,
});

function view(over: Partial<View> = {}, sources: Source[] = [src('eilidh', 'Eilidh Fraser-MacLeod'), src('gregor', 'Gregor Stewart', { role: 'guest', slot: 2 })]): View {
  return {
    station: 'Kettle FM',
    now_ms: Date.UTC(2026, 10, 11, 12),
    show: { title: 'The Morning Kettle', rundown: [] },
    desk: {
      reachable: true,
      error: null,
      playout: { connected: true, for_ms: 1000 },
      delay: { delay_ms: 8_000, target_ms: 8_000, rebuilding: false, dumps: 0, dumped_at_ms: null },
    },
    seats: { presenter: 'eilidh', screener: null, presenter_name: 'Eilidh', screener_name: '' },
    sources,
    callers: [],
    desk_taking_callers: true,
    chat: { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false, influx: false, influx_since_ms: null },
    notices: [],
    carts: { version: 0, desk: true, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
    listener_delay: { ms: 23_000, measured: false },
    step: NO_SHOW,
    mode: 'live',
    ...over,
  };
}

describe('End show asks first, and says what happens', () => {
  it('names who goes off air, that the delay plays out, and keeps ⏎ and Esc', () => {
    const p = airPlan('end', view());
    expect(p.title).toBe('End the show?');
    expect(p.lede).toBe('The Morning Kettle ends for listeners.');
    expect(p.lines.join(' ')).toContain('Eilidh and Gregor go off air now');
    expect(p.lines.join(' ')).toContain('callers stay in the queue');
    expect(p.lines.join(' ')).toContain('the last 8 s of the show, which the profanity delay still holds');
    expect(p.confirm).toBe('End show');
    expect(p.keep).toBe('Keep going');
  });

  it('is honest on a station with nothing to hand the air to, or already on automation', () => {
    expect(airPlan('end', view({ mode: null })).lines.join(' ')).toContain('no automation to hand the air to');
    expect(airPlan('end', view({ mode: 'auto' })).lines.join(' ')).toContain('Automation already holds the air');
    expect(airPlan('end', view({}, [])).lines[0]).toBe('Nobody is on air.');
  });

  it('says so when the desk is not answering', () => {
    const v = view();
    v.desk.reachable = false;
    expect(airPlan('end', v).lines[0]).toContain('desk is not answering');
  });

  it('always asks, even with nobody on air', () => {
    expect(needsConfirm('end', view({}, []))).toBe(true);
  });
});

describe('the control for automation', () => {
  it('reads the mode: hand over while live, take back while auto', () => {
    expect(modeButton(view())).toEqual({ ask: 'auto', label: 'Hand to automation' });
    expect(modeButton(view({ mode: 'auto' }))).toEqual({ ask: 'live', label: 'Take the air back' });
    expect(modeButton(null).ask).toBe('auto');
  });

  it('asks only when someone would be cut off or put on the air', () => {
    expect(needsConfirm('auto', view())).toBe(true);
    expect(needsConfirm('auto', view({}, []))).toBe(false);
    expect(needsConfirm('live', view({ mode: 'auto' }))).toBe(true);
    // A muted source is on air but not open: nothing to cut off.
    expect(needsConfirm('auto', view({}, [src('eilidh', 'Eilidh', { mute: true })]))).toBe(false);
    expect(airPlan('auto', view()).lede).toContain('Listeners stop hearing Eilidh and Gregor at once');
    expect(airPlan('live', view({ mode: 'auto' })).lede).toContain('reach listeners the moment you confirm');
  });

  it('is refused plainly on a station without a playout layer, not in silence', () => {
    expect(modeRefusal(view({ mode: null }))).toContain('no playout layer');
    expect(modeRefusal({ ...view(), mode: undefined })).toContain('no playout layer');
    expect(modeRefusal(view())).toBeNull();
    expect(modeRefusal(view({ mode: 'auto' }))).toBeNull();
    // A desk that is down is when to hand the air to automation, so it does not refuse.
    const down = view();
    down.desk.reachable = false;
    expect(modeRefusal(down)).toBeNull();
  });
});

describe('what is said after', () => {
  const result = (over: Partial<EndShowResult>): EndShowResult => ({ view: view(), off_air: 2, automation_in_ms: 9_000, ...over });
  it('says when automation takes the air', () => {
    expect(endShowNote(result({})).message).toBe('2 sources are off air. Automation takes the air in 9 s, when the last of the delay has played out.');
    expect(endShowNote(result({ off_air: 1 })).message).toContain('1 source is off air');
    expect(endShowNote(result({ off_air: 0, automation_in_ms: 0 })).message).toBe('Nobody was on air. Automation already holds the air.');
    expect(endShowNote(result({ automation_in_ms: null })).message).toContain('no automation to hand the air to');
  });
  it('says what each mode change did', () => {
    expect(modeNote('auto').title).toBe('Automation holds the air');
    expect(modeNote('live').title).toBe('You have the air');
  });
});

describe('the desk draws the button and the dialog', () => {
  beforeEach(() => {
    studio.airAsk = null;
  });

  it('draws the button under the lamp and the dialog under their panel names', () => {
    studio.view = view();
    const { body } = render(DeskB);
    expect(body).toContain('data-panel="desk.air"');
    expect(body).toContain('Hand to automation');
    expect(body).toContain('data-panel="desk.end-show"');
    expect(body.indexOf('desk.air')).toBeGreaterThan(body.indexOf('data-panel="desk.lamp"'));
  });

  it('reads Take the air back in auto mode', () => {
    studio.view = view({ mode: 'auto' });
    expect(render(DeskB).body).toContain('Take the air back');
  });

  it('disables the button with the reason where there is no playout layer', () => {
    studio.view = view({ mode: null });
    const { body } = render(DeskB);
    expect(body).toContain('aria-disabled="true"');
    expect(body).toContain('no playout layer');
  });
});

describe('the actions', () => {
  const calls: { url: string; body: unknown }[] = [];
  beforeEach(() => {
    calls.length = 0;
    studio.view = view();
    studio.airAsk = null;
    studio.note = null;
    studio.tab = 'screener';
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, init?: RequestInit) => {
        // The catalogue is fetched again when a view's cart version moves; not what is tested.
        if (!String(url).endsWith('/api/v1/carts')) calls.push({ url: String(url), body: JSON.parse(String(init?.body ?? '{}')) });
        const end = String(url).endsWith('/show/end');
        const data = end ? { view: view({ mode: 'live' }, []), off_air: 2, automation_in_ms: 9_000 } : view({ mode: 'auto' });
        return new Response(JSON.stringify(data), { status: 200 });
      }),
    );
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('⌃⇧E brings the desk up and opens the dialog, and sends nothing yet', async () => {
    await run({ type: 'end-show' });
    expect(studio.tab).toBe('desk');
    expect(studio.airAsk).toBe('end');
    expect(calls).toEqual([]);
  });

  it('confirming End show posts once and says what happens next', async () => {
    studio.airAsk = 'end';
    await confirmAir('end');
    expect(calls.map((c) => c.url.replace(/^.*\/api/, '/api'))).toEqual(['/api/v1/show/end']);
    expect(studio.airAsk).toBeNull();
    expect(studio.note?.title).toBe('The show is over');
    expect(studio.note?.message).toContain('Automation takes the air in 9 s');
  });

  it('hands over at once when nobody is open, and asks first when someone is', async () => {
    studio.view = view({}, []);
    await pressMode('auto');
    expect(calls.map((c) => [c.url.replace(/^.*\/api/, '/api'), c.body])).toEqual([['/api/v1/playout/mode', { mode: 'auto' }]]);
    expect(studio.note?.title).toBe('Automation holds the air');
    calls.length = 0;
    studio.view = view();
    await pressMode('auto');
    expect(calls).toEqual([]);
    expect(studio.airAsk).toBe('auto');
  });

  it('does nothing but say why on a station without automation', async () => {
    studio.view = view({ mode: null });
    await pressMode('auto');
    expect(calls).toEqual([]);
    expect(studio.airAsk).toBeNull();
    expect(studio.note?.title).toBe('No automation here');
  });
});
