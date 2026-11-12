// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The beginner steps (ADR-0271, ADR-0274, ADR-0275), the rules without a page (lib/steps.ts):
// the four steps and their words; what each step draws; which keys a step switches off and
// what the one note says; what the step dialog says will change, and its keys; the desk's
// queue at the Callers step; and the chat glance's line for a moderator. The words are the
// design's (docs/reference/design/beginner-steps.md, section 5).
import { describe, expect, it } from 'vitest';
import { KeyEngine, type Action, type KeyInput } from '../../../packages/ui/keymap.ts';
import type { Caller, DeskPerson, ShowStep, Source, StepView, View } from './lib/api.ts';
import {
  ARRIVES,
  NO_SHOW,
  STEPS,
  STEP_LONG,
  STEP_SHORT,
  STEP_WORD,
  asStep,
  atLeast,
  chatNeeds,
  chatNeedsWhere,
  deskCallers,
  deskCallersShown,
  gate,
  listNames,
  lookOf,
  menuKey,
  needsOwnLink,
  offHere,
  offNote,
  offTag,
  onlyWaiting,
  plain,
  rank,
  showTookAir,
  shown,
  stageOf,
  stationStep,
  stepChanges,
  stepHeading,
  stepLabel,
  stepLede,
  stepNoteShown,
  stepTitle,
  tabShown,
  type Part,
} from './lib/steps.ts';

const NOW = Date.UTC(2026, 10, 11, 9, 57, 12);

const person = (identity: string, name: string, role: DeskPerson['role'], on_air = true): DeskPerson => ({
  identity,
  name,
  role,
  on_air,
  mute: false,
  cue: false,
  talk: false,
  talk_to: null,
  present: true,
  away_ms: null,
  linked: true,
  lufs: -23,
  lufs_peak: -20,
});
const src = (slot: number, identity: string, name: string, role: DeskPerson['role'], on_air = true): Source => ({ ...person(identity, name, role, on_air), slot, caller: null });
const EILIDH = src(1, 'eilidh', 'Eilidh Fraser-MacLeod', 'presenter');
const GREGOR = src(2, 'gregor', 'Gregor Stewart', 'guest');
const DOUGAL = { ...src(4, 'dougal', 'Dougal Thomson', 'caller', false), caller: { line: 'The bins in Portobello', note: '', name_on_air: '', sent: true, taken: true } };
const FINLAY = src(5, 'finlay', 'Finlay Grant', 'dj', false);

const caller = (identity: string, name: string, arrived_ms: number, over: Partial<Caller> = {}): Caller => ({
  identity,
  name,
  ordinal: 1,
  state: 'waiting',
  since_ms: arrived_ms,
  arrived_ms,
  line: '',
  note: '',
  name_on_air: '',
  declined: null,
  banned_before: null,
  topic: '',
  consent: { at_ms: arrived_ms, version: '2026-11-01', by: 'caller' },
  dropped_ms: null,
  slot: null,
  on_air: false,
  cue: false,
  linked: true,
  present: true,
  lufs: null,
  ...over,
});
const NIALL = caller('niall', 'Niall Anderson', NOW - 72_000, { topic: 'The bus lanes on Leith Walk' });
const JESSIE = caller('jessie', 'Jessie Brown', NOW - 34_000, { topic: 'No night bus from the Infirmary' });
const CALUM = caller('calum', 'Calum Robertson', NOW - 20_000, { topic: 'Parking at Easter Road on match days' });
const FLORA = caller('flora', 'Flora Armstrong', NOW - 10_000, { topic: 'Just to say hello' });
const IONA = caller('iona', 'Iona MacIntosh', NOW - 5_000, { topic: 'The ferry fares, from Derry' });

const KETTLE = { slug: 'the-morning-kettle', name: 'The Morning Kettle' };
const at = (step: ShowStep, over: Partial<StepView> = {}): StepView => ({ step, show: KETTLE, on_air: true, next: null, may_change: true, ...over });

function view(step: StepView, sources: Source[] = [EILIDH], callers: Caller[] = []): View {
  return {
    station: 'Kettle FM',
    now_ms: NOW,
    show: { title: 'The Morning Kettle', rundown: [] },
    desk: { reachable: true, error: null, playout: { connected: true, for_ms: 14_000 } },
    seats: { presenter: 'eilidh', screener: null, presenter_name: 'Eilidh Fraser-MacLeod', screener_name: '' },
    sources,
    callers,
    desk_taking_callers: true,
    chat: { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false, influx: false, influx_since_ms: null },
    notices: [],
    carts: { version: 1, desk: true, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
    listener_delay: { ms: 15_000, measured: false },
    step,
  };
}

describe('the four steps', () => {
  it('are Solo, Guests, Callers and Full, in that order, each with its lines', () => {
    expect([...STEPS]).toEqual(['solo', 'guests', 'callers', 'full']);
    expect(STEPS.map((s) => STEP_WORD[s])).toEqual(['Solo', 'Guests', 'Callers', 'Full']);
    expect(STEP_SHORT).toEqual({
      solo: 'Just you',
      guests: 'You and guests you invite by link',
      callers: 'Guests, and listeners who call in',
      full: 'A screener on the phones, the picture, the next DJ',
    });
    expect(STEP_LONG.solo).toBe('Just you: your microphone, the music and carts, the listener count, the chat to read');
    expect(STEP_LONG.callers).toBe('Adds listeners who call in; you answer them at the desk, and Dump is on');
    expect(rank('solo')).toBe(0);
    expect(atLeast('callers', 'guests')).toBe(true);
    expect(atLeast('guests', 'callers')).toBe(false);
  });

  it('read a view with no step, or an unknown word, as Full: the studio as it was', () => {
    expect(stationStep(null)).toBe('full');
    expect(stationStep({})).toBe('full');
    expect(stationStep({ step: NO_SHOW })).toBe('full');
    expect(asStep('telephony')).toBe('full');
    expect(stationStep({ step: at('guests') })).toBe('guests');
  });

  it('name the frame’s button and say whose step it is', () => {
    expect(stepLabel('solo')).toBe('Step: Solo, 1 of 4. Change the step');
    expect(stepLabel('full')).toBe('Step: Full, 4 of 4. Change the step');
    expect(stepTitle(at('solo'))).toBe('The step of The Morning Kettle');
    expect(stepTitle(at('solo', { on_air: false }))).toBe('No show is on air: the step of The Morning Kettle, the show last on air');
    expect(stepTitle(NO_SHOW)).toMatch(/No show has been on air yet/);
  });
});

describe('what this window draws', () => {
  it('is the station’s step, or everything while Show everything is on', () => {
    expect(lookOf('solo', false)).toBe('solo');
    expect(lookOf('solo', true)).toBe('full');
    expect(lookOf('full', true)).toBe('full');
  });

  it('keeps Desk, Carts and Schedule at every step, and brings the Screener and Chat tabs at Full', () => {
    for (const s of STEPS) {
      expect(['desk', 'carts', 'schedule'].every((t) => tabShown(t as 'desk', s)), s).toBe(true);
      expect(tabShown('screener', s), s).toBe(s === 'full');
      expect(tabShown('chat', s), s).toBe(s === 'full');
    }
  });

  it('draws each part from the step that brings it (the design’s tables)', () => {
    const drawn = (look: ShowStep) => (Object.keys(ARRIVES) as Part[]).filter((p) => shown(p, look)).sort();
    expect(drawn('solo')).toEqual([]);
    expect(drawn('guests')).toEqual(['invite', 'talk-rule', 'waiting']);
    expect(drawn('callers')).toEqual(['delay', 'dump', 'invite', 'talk-rule', 'waiting']);
    expect(drawn('full')).toEqual((Object.keys(ARRIVES) as Part[]).sort());
    // The two panels that belong to one step only.
    expect(STEPS.filter(stepNoteShown)).toEqual(['solo']);
    expect(STEPS.filter(deskCallersShown)).toEqual(['callers']);
  });

  it('marks a part off when it is drawn only because everything is shown', () => {
    // At Solo with everything shown: Dump, the invitation, the picture and the handover are drawn, and off.
    for (const p of ['dump', 'invite', 'picture', 'handover', 'talk-screener'] as Part[]) expect(offHere(p, 'solo', 'full'), p).toBe(true);
    // At Callers with everything shown, Dump and the invitation are on; the Full parts are off.
    expect(offHere('dump', 'callers', 'full')).toBe(false);
    expect(offHere('invite', 'callers', 'full')).toBe(false);
    expect(offHere('picture', 'callers', 'full')).toBe(true);
    // Nothing is off at Full, and nothing is tagged where it is not drawn.
    expect((Object.keys(ARRIVES) as Part[]).some((p) => offHere(p, 'full', 'full'))).toBe(false);
    expect(offHere('dump', 'solo', 'solo')).toBe(false);
    expect(offTag('solo')).toBe('off at Solo');
    expect(offTag('guests')).toBe('off at Guests');
  });

  it('keeps every tile on the stage at Solo, on air or off, and moves them from Guests on', () => {
    const off = { ...EILIDH, on_air: false };
    expect(stageOf([off], 'solo')).toEqual({ stage: [off], waiting: [] });
    expect(stageOf([off, GREGOR], 'guests')).toEqual({ stage: [GREGOR], waiting: [off] });
    // A guest left from a higher step keeps a tile at Solo, though no Not on air row is drawn.
    const left = { ...GREGOR, on_air: false };
    expect(stageOf([EILIDH, left], 'solo').stage).toEqual([EILIDH, left]);
  });

  it('offers the DJ their own microphone link at every step until someone holds the presenter\'s seat', () => {
    expect(needsOwnLink([], 'solo', false)).toBe(true);
    expect(needsOwnLink([{ ...GREGOR, on_air: false }], 'solo', false)).toBe(true);
    expect(needsOwnLink([EILIDH], 'solo', true)).toBe(false);
    // Above Solo: to those who may change the step, not to a screener's window.
    expect(needsOwnLink([], 'guests', true)).toBe(true);
    expect(needsOwnLink([], 'full', true)).toBe(true);
    expect(needsOwnLink([], 'full', false)).toBe(false);
    // After a handover the DJ on air holds the seat; a DJ waiting off air does not.
    expect(needsOwnLink([{ ...GREGOR, role: 'dj', on_air: true }], 'full', true)).toBe(false);
    expect(needsOwnLink([{ ...GREGOR, role: 'dj', on_air: false }], 'full', true)).toBe(true);
    // A window on the station key has no account to make the link for.
    expect(needsOwnLink([], 'solo', true, false)).toBe(false);
  });
});

describe('the keys of hidden parts (design section 4)', () => {
  const key = (code: string, mods: Partial<KeyInput> = {}): KeyInput => ({ code, ctrl: false, shift: false, alt: false, meta: false, repeat: false, ...mods });
  const told = (a: Action, step: ShowStep, v: View | null = null, look: ShowStep = step) => {
    const g = gate(a, step, look, v);
    return g.allow ? null : g.told;
  };

  it('gives the Desk tab’s P to taking a caller, and leaves the Screener tab’s P as send (ADR-0275)', () => {
    const k = new KeyEngine();
    expect(k.down(key('KeyP'), { tab: 'desk', inTextField: false }, 0).action).toEqual({ type: 'take-caller' });
    expect(k.down(key('KeyP'), { tab: 'screener', inTextField: false }, 0).action).toEqual({ type: 'send' });
    // A text field owns the letter, and ⇧P stays free on the desk.
    expect(k.down(key('KeyP'), { tab: 'desk', inTextField: true }, 0).handled).toBe(false);
    expect(k.down(key('KeyP', { shift: true }), { tab: 'desk', inTextField: false }, 0).handled).toBe(false);
  });

  it('switches each key on at the step its part arrives', () => {
    const table: [Action, (ShowStep | 'none')[]][] = [
      // [the action, the steps at which it goes ahead]
      [{ type: 'tab', tab: 'desk' }, ['solo', 'guests', 'callers', 'full']],
      [{ type: 'tab', tab: 'carts' }, ['solo', 'guests', 'callers', 'full']],
      [{ type: 'tab', tab: 'schedule' }, ['solo', 'guests', 'callers', 'full']],
      [{ type: 'tab', tab: 'screener' }, ['full']],
      [{ type: 'tab', tab: 'chat' }, ['full']],
      [{ type: 'dump' }, ['callers', 'full']],
      [{ type: 'arm', what: 'dump' }, ['callers', 'full']],
      [{ type: 'drop' }, ['callers', 'full']],
      [{ type: 'arm', what: 'drop' }, ['callers', 'full']],
      [{ type: 'take-caller' }, ['callers']],
      [{ type: 'talk-seat', held: true }, ['full']],
      [{ type: 'handover' }, ['full']],
      [{ type: 'picture' }, ['full']],
      // As today at every step: the DJ's strip, the carts, End show, Esc, restart, a strip's talk.
      [{ type: 'on', slot: 1 }, ['solo', 'guests', 'callers', 'full']],
      [{ type: 'cue', slot: 1 }, ['solo', 'guests', 'callers', 'full']],
      [{ type: 'mute', slot: 1 }, ['solo', 'guests', 'callers', 'full']],
      [{ type: 'talk', slot: 2, held: true }, ['solo', 'guests', 'callers', 'full']],
      [{ type: 'cart', n: 2 }, ['solo', 'guests', 'callers', 'full']],
      [{ type: 'end-show' }, ['solo', 'guests', 'callers', 'full']],
      [{ type: 'escape' }, ['solo', 'guests', 'callers', 'full']],
      [{ type: 'restart-playout' }, ['solo', 'guests', 'callers', 'full']],
      [{ type: 'arm', what: 'end-pass', identity: 'gregor' }, ['solo', 'guests', 'callers', 'full']],
    ];
    for (const [a, on] of table) {
      expect(STEPS.filter((s) => gate(a, s, s, null).allow), JSON.stringify(a)).toEqual(on);
    }
  });

  it('lets a held talk key go at any step, so a talk held across a move down still closes', () => {
    expect(gate({ type: 'talk-seat', held: false }, 'solo', 'solo', null).allow).toBe(true);
  });

  it('keeps Drop working on a caller still in the room from an earlier step, until they have left', () => {
    const v = view(at('solo'), [EILIDH, DOUGAL]);
    expect(told({ type: 'arm', what: 'drop' }, 'solo', v)).toBeNull();
    expect(told({ type: 'drop' }, 'solo', v)).toBeNull();
    expect(told({ type: 'drop' }, 'solo', view(at('solo'), [EILIDH]))).toBe('drop');
  });

  it('reaches the Screener and Chat tabs again while everything is shown, and keeps what is off, off', () => {
    expect(told({ type: 'tab', tab: 'screener' }, 'solo', null, 'full')).toBeNull();
    expect(told({ type: 'tab', tab: 'chat' }, 'guests', null, 'full')).toBeNull();
    expect(told({ type: 'dump' }, 'solo', null, 'full')).toBe('dump');
    expect(told({ type: 'handover' }, 'callers', null, 'full')).toBe('handover');
    expect(told({ type: 'picture' }, 'solo', null, 'full')).toBe('picture');
    // The queue is on the Screener tab while everything is shown: the Desk tab's P does nothing.
    expect(told({ type: 'take-caller' }, 'callers', null, 'full')).toBe('take-caller');
  });

  it('says so once, in the design’s words, without inventing a refusal for a key that is simply elsewhere', () => {
    expect(offNote('dump', 'solo')).toEqual({ title: 'Dump is off at Solo', message: 'This show runs at Solo, so there is nothing for Dump to do. It comes on at Callers.' });
    expect(offNote('dump', 'guests')?.title).toBe('Dump is off at Guests');
    expect(offNote('screener-tab', 'guests')?.message).toMatch(/The Screener tab comes with Full\.$/);
    expect(offNote('handover', 'callers')?.message).toMatch(/It comes on at Full\.$/);
    expect(offNote('picture', 'solo')?.title).toBe('The picture is held on the slate at Solo');
    expect(offNote('take-caller', 'guests')?.message).toMatch(/Call-in comes with Callers\.$/);
    // P on the Desk tab at Full (or while everything is shown at Callers): nothing, and no note.
    expect(offNote('take-caller', 'full')).toBeNull();
    expect(offNote('take-caller', 'callers')).toBeNull();
    for (const p of ['dump', 'drop', 'screener-tab', 'chat-tab', 'talk-screener', 'handover', 'picture'] as const) {
      const n = offNote(p, 'solo');
      expect(n?.title, p).toMatch(/Solo$/);
      expect(n?.message, p).toMatch(/^This show runs at Solo/);
    }
  });
});

describe('the step dialog', () => {
  it('titles itself for the show on air, and from the show last on air', () => {
    expect(stepHeading(at('solo'))).toBe('Step for The Morning Kettle');
    expect(stepHeading(at('solo', { on_air: false }))).toBe('Step from The Morning Kettle');
  });

  it('says whose step it is and who may change it', () => {
    expect(plain(stepLede(at('solo'), ''))).toBe('On air now. A step shows the parts of the studio this show uses and switches the rest off, so nothing can wait unseen.');
    const next = { slug: 'steeped', name: 'Steeped', start_ms: Date.UTC(2026, 10, 11, 10, 5), step: 'full' as const };
    expect(plain(stepLede(at('solo', { on_air: false, next }), '', 'Europe/London'))).toBe(
      'No show is on air. The station keeps the step of the show last on air until Steeped starts at 10:05, which runs at Full. A change here changes The Morning Kettle.',
    );
    expect(plain(stepLede(at('solo', { on_air: false }), ''))).toBe('No show is on air. The station keeps the step of the show last on air. A change here changes The Morning Kettle.');
    expect(plain(stepLede(NO_SHOW, ''))).toBe('Steps belong to a show, and no show has been on air yet, so the studio shows everything and there is no step to change. Make a show and give it an episode on the Schedule tab: a new show starts at Solo, and its step applies while it is on air.');
    expect(plain(stepLede(at('solo', { may_change: false }), 'Eilidh Fraser-MacLeod'))).toBe(
      'On air now, at Solo. Eilidh Fraser-MacLeod, a producer or an admin can change the step. You can look at the whole studio without changing anything.',
    );
    // The presenter's name is bold when the window knows it, and a plain phrase when it does not.
    expect(stepLede(at('solo', { may_change: false }), 'Eilidh Fraser-MacLeod')).toContainEqual({ b: 'Eilidh Fraser-MacLeod' });
    expect(plain(stepLede(at('guests', { may_change: false }), ''))).toMatch(/^On air now, at Guests\. The show’s presenter, a producer or an admin/);
  });

  it('says what a move up brings, and what stays off', () => {
    const c = stepChanges('solo', 'guests', view(at('solo')), 'The Morning Kettle', true);
    expect(c.heading).toBe('Moving to Guests, at once:');
    expect(c.lines.map(plain)).toEqual([
      'Invite someone… appears on the desk, and a guest can join by link.',
      'The Not on air row and talkback appear.',
      'Call-in stays closed. The screener’s seat, the picture and the handover stay off.',
    ]);
    expect(c.people).toBe(false);
    expect(c.last).toBe('Saved to The Morning Kettle. Each show keeps its own step.');
    expect(stepChanges('guests', 'callers', null, 'The Morning Kettle', true).lines.map(plain)).toEqual([
      'The call-in page opens: listeners can call from now.',
      'Callers wait in a new panel on the desk, and Dump comes on.',
      'The screener’s seat, the picture and the handover stay off.',
    ]);
    expect(stepChanges('callers', 'full', null, 'The Morning Kettle', true).lines.map(plain)).toEqual([
      'The Screener tab opens, and its seat can be taken.',
      'The picture can be switched, and the next DJ can be invited.',
    ]);
    // Two rungs at once say both, and nothing stays off at Full.
    expect(stepChanges('solo', 'full', null, 'The Morning Kettle', true).lines).toHaveLength(6);
    // A show that is not on air changes nothing at once.
    expect(stepChanges('solo', 'guests', null, 'Steeped', false).heading).toBe('Moving to Guests:');
  });

  it('says what a move down does to the people present: the waiting are told, the seated stay', () => {
    const v = view(at('callers'), [EILIDH, GREGOR, DOUGAL], [NIALL, JESSIE, caller('dougal', 'Dougal Thomson', NOW - 300_000, { state: 'ready', slot: 4 })]);
    const c = stepChanges('callers', 'solo', v, 'The Morning Kettle', true);
    expect(c.heading).toBe('Moving to Solo, at once:');
    expect(c.people).toBe(true);
    const lines = c.lines.map(plain);
    expect(lines).toContain('The call-in page says the lines are closed. Niall and Jessie are waiting: they are told the lines have closed.');
    expect(lines).toContain('Dougal stays on strip 4 until you drop the call, and Gregor stays until you end their pass. Nobody new can join.');
    expect(lines.at(-1)).toBe('Dump goes off.');
    // The names are bold, as the design draws them.
    expect(c.lines.flat()).toContainEqual({ b: 'Niall and Jessie' });
    expect(c.lines.flat()).toContainEqual({ b: 'Dougal' });
    // One caller waiting reads in the singular; nobody waiting names nobody.
    expect(stepChanges('callers', 'guests', view(at('callers'), [EILIDH], [NIALL]), 'The Morning Kettle', true).lines.map(plain)).toContain(
      'The call-in page says the lines are closed. Niall is waiting: they are told the lines have closed.',
    );
    const quiet = stepChanges('callers', 'guests', view(at('callers')), 'The Morning Kettle', true);
    expect(quiet.people).toBe(false);
    expect(quiet.lines.map(plain)).toEqual(['The call-in page says the lines are closed.', 'Dump goes off.']);
    // Down from Full: the next DJ stays until their pass is ended.
    const full = stepChanges('full', 'callers', view(at('full'), [EILIDH, FINLAY]), 'The Morning Kettle', true);
    expect(full.lines.map(plain)).toEqual([
      'The Screener tab closes, and its seat cannot be taken. The picture is held on the slate, and the handover goes off.',
      'Finlay stays until you end their pass. Nobody new can join.',
    ]);
    // Off air the step closes no door yet, so nobody in the room is named.
    expect(stepChanges('callers', 'solo', v, 'The Morning Kettle', false).people).toBe(false);
  });

  it('counts only the callers who are only waiting as the ones a falling step lets go', () => {
    const v = view(at('callers'), [EILIDH], [NIALL, caller('dougal', 'Dougal Thomson', NOW, { slot: 4 }), caller('flora', 'Flora Armstrong', NOW, { cue: true }), caller('iona', 'Iona MacIntosh', NOW, { on_air: true })]);
    expect(onlyWaiting(v).map((c) => c.identity)).toEqual(['niall']);
    expect(listNames(['Niall'])).toBe('Niall');
    expect(listNames(['Niall', 'Jessie', 'Calum'])).toBe('Niall, Jessie and Calum');
  });

  it('owns the keyboard: ↑ ↓ move between steps, ⏎ confirms a move, Esc keeps the step', () => {
    expect(menuKey('ArrowDown', 'solo', 'solo', false)).toEqual({ type: 'pick', step: 'guests' });
    expect(menuKey('ArrowUp', 'callers', 'solo', false)).toEqual({ type: 'pick', step: 'guests' });
    // The list has ends: it does not wrap.
    expect(menuKey('ArrowUp', 'solo', 'solo', false)).toEqual({ type: 'pick', step: 'solo' });
    expect(menuKey('ArrowDown', 'full', 'solo', false)).toEqual({ type: 'pick', step: 'full' });
    // ⏎ confirms only a move: on the step in force there is nothing to confirm.
    expect(menuKey('Enter', 'guests', 'solo', false)).toEqual({ type: 'confirm' });
    expect(menuKey('Enter', 'solo', 'solo', false)).toBeNull();
    expect(menuKey('Escape', 'guests', 'solo', false)).toEqual({ type: 'close' });
    // Other keys stay the browser's (Tab between the buttons, Space on one).
    expect(menuKey('Tab', 'solo', 'solo', false)).toBeNull();
    expect(menuKey(' ', 'solo', 'solo', false)).toBeNull();
    // Someone who may not change the step can only close it.
    expect(menuKey('ArrowDown', 'solo', 'solo', true)).toBeNull();
    expect(menuKey('Enter', 'guests', 'solo', true)).toBeNull();
    expect(menuKey('Escape', 'solo', 'solo', true)).toEqual({ type: 'close' });
  });
});

describe('Show everything', () => {
  it('ends when another show takes the air, and not when the same show carries on', () => {
    const steeped = { slug: 'steeped', name: 'Steeped' };
    expect(showTookAir(at('solo'), at('full', { show: steeped }))).toBe(true);
    // Between shows, then the next one starts.
    expect(showTookAir(at('solo', { on_air: false }), at('full', { show: steeped }))).toBe(true);
    expect(showTookAir(at('solo'), at('guests'))).toBe(false);
    expect(showTookAir(at('solo'), at('solo', { on_air: false }))).toBe(false);
    expect(showTookAir(null, at('solo'))).toBe(false);
  });
});

describe('desk.callers', () => {
  it('lists who waits, the longest wait first, and takes the one who has waited longest', () => {
    const q = deskCallers(view(at('callers'), [EILIDH], [JESSIE, NIALL]));
    expect(q.head).toBe('2 waiting · lines open');
    expect(q.rows.map((r) => [r.n, r.name, r.topic])).toEqual([
      [1, 'Niall Anderson', 'The bus lanes on Leith Walk'],
      [2, 'Jessie Brown', 'No night bus from the Infirmary'],
    ]);
    expect(q.next).toEqual({ identity: 'niall', name: 'Niall' });
    expect(q.holds).toBeNull();
    expect(q.title).toBe('They join strip 4 off air. Cue to listen, hold Talk to speak to them, then On.');
    expect(q.more).toBe(0);
  });

  it('holds one caller on the strip: the next waits until that call is on air or dropped', () => {
    const q = deskCallers(view(at('callers'), [EILIDH, DOUGAL], [caller('dougal', 'Dougal Thomson', NOW - 300_000, { state: 'ready', slot: 4 }), NIALL, JESSIE]));
    expect(q.head).toBe('2 waiting · strip 4 taken');
    expect(q.holds).toEqual({ name: 'Dougal', slot: 4 });
    expect(q.title).toBe('Strip 4 holds one caller. Put Dougal on air or drop the call, then take the next.');
    // The one on the strip is not in the rows; the next is still named, for the button's words.
    expect(q.rows.map((r) => r.identity)).toEqual(['niall', 'jessie']);
    expect(q.next?.name).toBe('Niall');
  });

  it('draws three rows at most, then a count, so the chat below keeps room', () => {
    const q = deskCallers(view(at('callers'), [EILIDH], [NIALL, JESSIE, CALUM, FLORA, IONA]));
    expect(q.rows).toHaveLength(3);
    expect(q.more).toBe(2);
    expect(q.head).toBe('5 waiting · lines open');
  });

  it('says the lines are open when nobody waits, and skips a caller who cannot go to the desk', () => {
    expect(deskCallers(view(at('callers')))).toMatchObject({ head: 'lines open', rows: [], next: null });
    const dropped = caller('iona', 'Iona MacIntosh', NOW - 90_000, { dropped_ms: NOW - 5_000 });
    const noConsent = caller('flora', 'Flora Armstrong', NOW - 80_000, { consent: null });
    const q = deskCallers(view(at('callers'), [EILIDH], [dropped, noConsent, NIALL]));
    expect(q.rows[0]).toMatchObject({ identity: 'iona', dropped: true });
    expect(q.next?.identity).toBe('niall');
    expect(deskCallers(view(at('callers'), [EILIDH], [dropped])).next).toBeNull();
  });
});

describe('desk.chat', () => {
  it('says what waits for a moderator, and where to go', () => {
    expect(chatNeeds({ held: 2, open_reports: 1 })).toBe('1 report, 2 held messages');
    expect(chatNeeds({ held: 1, open_reports: 0 })).toBe('1 held message');
    expect(chatNeeds({ held: 0, open_reports: 3 })).toBe('3 reports');
    // Nothing waiting, not counted, or a station too old to say: no line.
    expect(chatNeeds({ held: 0, open_reports: 0 })).toBe('');
    expect(chatNeeds({ held: null, open_reports: null })).toBe('');
    expect(chatNeeds({})).toBe('');
    expect(chatNeeds(null)).toBe('');
    // Below Full the Screener tab is behind Show everything; at Full, and while everything is shown, it is a tab away.
    expect(chatNeedsWhere('solo', { held: 2, open_reports: 1 })).toBe('need a moderator: Show everything, then the Screener tab.');
    expect(chatNeedsWhere('full', { held: 2, open_reports: 1 })).toBe('need a moderator, on the Screener tab.');
    expect(chatNeedsWhere('callers', { held: 1, open_reports: 0 })).toBe('needs a moderator: Show everything, then the Screener tab.');
  });
});
