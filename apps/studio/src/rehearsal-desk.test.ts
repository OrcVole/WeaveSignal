// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The rehearsal desk's screens (1.3.0, step R13): the rules in `lib/rehearsalDesk.ts`, where a
// key goes while the Desk tab shows the rehearsal desk, and the screens rendered on the server
// in each state the studio can be in. Nothing in the rehearsal studio reaches the air, so the
// rendered desk is checked for the air's marks (its classes and glyphs, never its words alone:
// "On air at 14:05" is a time, and "Open the rehearsal studio" a verb). Sample people are from
// the Tartan National Names list (ADR-0071); the shows are Kettle FM's.

import { render } from 'svelte/server';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Action } from '../../../packages/ui/keymap.ts';
import { CHIP } from '../../../packages/ui/states.ts';
import { PANELS } from '../../../packages/ui/panels.ts';
import type { HandoverWindow, Me, RehearsalPerson, RehearsalView, Source, View } from './lib/api.ts';
import { account } from './lib/account.svelte.ts';
import { INVITE_ROLES, REHEARSAL_ROLES, WHERE, joinsLine, ownIdentity, rehearsalJoinsLine, rehearsalRolesAt } from './lib/invite.ts';
import { rehearsal } from './lib/rehearsal.svelte.ts';
import {
  MOVE,
  REHEARSAL_CHIP,
  REHEARSAL_OFF_WHY,
  assignSlots,
  asSource,
  fromRehearsal,
  fromRehearsalNote,
  mayRun,
  moveRows,
  movedWords,
  openingLine,
  personChip,
  routeKey,
  settingsLine,
  stageSub,
  studioChoice,
  windowLine,
  type Moved,
} from './lib/rehearsalDesk.ts';
import { mayDrive, moveNow, onBeforeStudioSwitch, personAt, rdesk, runRehearsal, settleSlots, switchDeskStudio } from './lib/rehearsalDesk.svelte.ts';
import { resetStores } from './lib/reset.ts';
import { NO_SHOW } from './lib/steps.ts';
import { studio } from './lib/studio.svelte.ts';
import { windowRows } from './lib/window.ts';
import { setup } from './lib/wizard.svelte.ts';
import DeskB from './desk/DeskB.svelte';
import RehearsalDesk from './desk/RehearsalDesk.svelte';

const at = (h: number, m: number, s = 0) => new Date(2026, 10, 20, h, m, s).getTime();
const EILIDH_ID = '3f2c9a52-7a8e-4b3c-9d3e-1d2f4a6b8c10';
const EILIDH = ownIdentity(EILIDH_ID);

const person = (identity: string, name: string, over: Partial<RehearsalPerson> = {}): RehearsalPerson => ({ identity, name, role: 'guest', on: true, mute: false, cue: false, talk: false, talk_to: null, present: true, linked: true, lufs: -20, ...over });
const THREE = [person(EILIDH, 'Eilidh Fraser-MacLeod', { role: 'presenter' }), person('guest-gregor', 'Gregor Stewart', { mute: true }), person('guest-kirsty', 'Kirsty Wilson', { on: false })];
const WINDOW: HandoverWindow = { state: 'announced', show: 'steeped', show_name: 'Steeped', start_ms: at(14, 5), opens_ms: at(13, 50), host: 'Eilidh Fraser-MacLeod', from: 'room', rehearsing: true };
const OPEN: RehearsalView = {
  enabled: true,
  desk: { linked: true, error: null },
  session: { id: 's1', show: 'steeped', show_name: 'Steeped', opened_by: 'Eilidh', opened_at_ms: at(13, 30) },
  people: THREE,
  carts_sounding: [],
  max_people: 6,
  window: WINDOW,
};
const CLOSED: RehearsalView = { ...OPEN, session: null, people: [], window: null };
const staff = (role: Me['role'], id = EILIDH_ID): Me => ({ kind: 'staff', role, account: { id, username: 'eilidh', name: 'Eilidh Fraser-MacLeod', role, disabled_ms: null, created_ms: 1 } as NonNullable<Me['account']> });
const SHOWS = [
  { slug: 'steeped', name: 'Steeped', presenter_account: EILIDH_ID, step: 'guests' },
  { slug: 'the-morning-kettle', name: 'The Morning Kettle', presenter_account: 'someone-else', step: 'full' },
] as unknown as typeof setup.shows;

/** Every mark of the air a rendered rehearsal desk must be free of. */
const AIR_MARKS = ['fill-onair', 'fill-open', 'is-onair', 'is-open', 'lit-onair', 'lit-open', '#g-onair', '#g-open', 'lamp onair', 'lamp-on-air', 'data-panel="desk.tile', 'data-panel="desk.lamp', 'data-panel="desk.stage', 'data-panel="desk.delay', 'data-panel="desk.air', 'End pass', 'Dump', 'End show'];

function put(view: RehearsalView | null, me: Me | null = staff('presenter')): void {
  rehearsal.view = view;
  rehearsal.absent = false;
  rehearsal.link = 'live';
  rehearsal.error = null;
  account.me = me;
  setup.shows = SHOWS;
  setup.loaded = true;
  studio.now = at(13, 55);
  studio.skew = 0;
  settleSlots();
}

afterEach(() => {
  resetStores();
  account.me = null;
  setup.shows = [];
  vi.unstubAllGlobals();
});

describe('the rehearsal desk’s words', () => {
  it('never borrows the air’s shapes or words', () => {
    for (const c of Object.values(REHEARSAL_CHIP)) {
      expect(c.cls).toMatch(/^hollow/);
      expect(c.cls).not.toMatch(/onair|open|critical/);
      expect(c.word).not.toMatch(/on air|open/i);
    }
    expect(personChip({ on: true, mute: false, cue: false })).toBe(REHEARSAL_CHIP.inMix);
    expect(personChip({ on: true, mute: true, cue: false })).toBe(CHIP.muted);
    expect(personChip({ on: false, mute: false, cue: true })).toBe(CHIP.cue);
    expect(personChip({ on: false, mute: false, cue: false })).toBe(REHEARSAL_CHIP.notInMix);
    expect(stageSub(OPEN)).toBe('1 of 3 in the mix · nothing here reaches listeners');
    expect(stageSub({ ...OPEN, people: [] })).toBe('Nobody is here yet · nothing here reaches listeners');
  });

  it('says when the rehearsed show is on air, and that moving is a press', () => {
    expect(windowLine(null, at(13, 55))).toBeNull();
    expect(windowLine(WINDOW, at(13, 55))).toEqual({ text: 'On air at 14:05: move by then', due: false });
    expect(windowLine(WINDOW, at(14, 6))).toEqual({ text: 'Steeped was due on air at 14:05: move now', due: true });
  });

  it('gives each person a strip that never changes while they are listed', () => {
    const first = assignSlots({}, THREE);
    // The presenter leads; then by identity, so two windows agree.
    expect(first).toEqual({ [EILIDH]: 1, 'guest-gregor': 2, 'guest-kirsty': 3 });
    // Gregor leaves the list: nobody is renumbered, and a newcomer takes the free strip.
    const later = assignSlots(first, [THREE[0], THREE[2], person('guest-finlay', 'Finlay Grant')]);
    expect(later).toEqual({ [EILIDH]: 1, 'guest-kirsty': 3, 'guest-finlay': 2 });
    // Beyond the sixth strip a person has a tile and no keys.
    const many = Array.from({ length: 8 }, (_, i) => person(`guest-${i}`, `Guest ${i}`));
    expect(Object.keys(assignSlots({}, many))).toHaveLength(6);
    expect(asSource(THREE[1], 2)).toMatchObject({ identity: 'guest-gregor', on_air: true, mute: true, slot: 2, caller: null });
  });

  it('works out who may drive as the station does, and offers the choice of studio honestly', () => {
    expect(mayRun(null, EILIDH_ID)).toBe(false);
    expect(mayRun({ kind: 'station', role: 'admin' }, null)).toBe(true);
    expect(mayRun(staff('producer', 'x'), EILIDH_ID)).toBe(true);
    expect(mayRun(staff('presenter'), EILIDH_ID)).toBe(true);
    expect(mayRun(staff('presenter', 'x'), EILIDH_ID)).toBe(false);
    expect(mayRun(staff('screener'), null)).toBe(false);

    expect(studioChoice('absent', null).shown).toBe(false);
    expect(studioChoice('unknown', null).shown).toBe(false);
    expect(studioChoice('off', { ...CLOSED, enabled: false })).toMatchObject({ shown: true, can: false, mark: 'off', why: REHEARSAL_OFF_WHY });
    expect(REHEARSAL_OFF_WHY).toContain('REHEARSAL=true');
    expect(studioChoice('desk-down', { ...CLOSED, desk: { linked: false, error: 'not answering' } })).toMatchObject({ can: true, mark: 'not answering' });
    expect(studioChoice('closed', CLOSED)).toMatchObject({ can: true, mark: '' });
    expect(studioChoice('open', OPEN)).toMatchObject({ can: true, mark: 'Steeped' });
    expect(studioChoice('open', OPEN).why).toMatch(/^Eilidh is rehearsing Steeped\./);
  });

  it('says in Settings whether the studio is on, and names the setting', () => {
    expect(settingsLine('unknown', null)).toBeNull();
    expect(settingsLine('absent', null)).toBeNull();
    expect(settingsLine('off', { ...CLOSED, enabled: false })).toMatchObject({ state: 'off' });
    expect(settingsLine('closed', CLOSED)?.rest).toMatch(/It takes 6 people at once and has no way to the air\..*REHEARSAL/);
    expect(settingsLine('desk-down', { ...CLOSED, desk: { linked: false, error: null } })?.rest).toContain('its desk is not answering');
  });

  it('lists who moves with their opening, and only the people present', () => {
    expect(MOVE.lede).toBe('Nobody goes on air until the handover. Their links keep working.');
    const rows = moveRows({ ...OPEN, people: [...THREE, person('guest-finlay', 'Finlay Grant', { present: false })] });
    expect(rows.map((r) => [r.name, r.role, r.kind, r.opening])).toEqual([
      ['Eilidh Fraser-MacLeod', 'Presenter', 'on', 'On at the handover'],
      ['Gregor Stewart', 'Guest', 'muted', 'On at the handover, muted'],
      ['Kirsty Wilson', 'Guest', 'off', 'Stands by until you put them on'],
    ]);
  });
});

describe('after a move, at the live desk', () => {
  const moved: Moved = { moved: [EILIDH, 'guest-gregor', 'guest-kirsty'], opening: { airing_start_ms: at(14, 5), standby_until_ms: at(14, 10), host: EILIDH, on: [EILIDH, 'guest-gregor'], mute: ['guest-gregor'] }, airing_start_ms: at(14, 5), show: 'steeped', show_name: 'Steeped' };
  const src = (identity: string, name: string, on_air = false, present = true) => ({ identity, name, on_air, present });

  it('marks only the people moved who still stand by, until the standing-by time', () => {
    expect(fromRehearsal(moved, src(EILIDH, 'Eilidh'), at(14, 0))).toBe(true);
    expect(fromRehearsal(moved, src(EILIDH, 'Eilidh', true), at(14, 0))).toBe(false);
    expect(fromRehearsal(moved, src(EILIDH, 'Eilidh', false, false), at(14, 0))).toBe(false);
    expect(fromRehearsal(moved, src('guest-other', 'Rory'), at(14, 0))).toBe(false);
    expect(fromRehearsal(moved, src(EILIDH, 'Eilidh'), at(14, 10))).toBe(false);
    expect(fromRehearsal(null, src(EILIDH, 'Eilidh'), at(14, 0))).toBe(false);
    expect(fromRehearsalNote(moved, EILIDH)).toBe('From rehearsal · on at the handover');
    expect(fromRehearsalNote(moved, 'guest-gregor')).toBe('From rehearsal · on at the handover, muted');
    expect(fromRehearsalNote(moved, 'guest-kirsty')).toBe('From rehearsal · stands by until you put them on');
  });

  it('names who the opening state puts on, and stops once it has', () => {
    const all = [src(EILIDH, 'Eilidh Fraser-MacLeod'), src('guest-gregor', 'Gregor Stewart'), src('guest-kirsty', 'Kirsty Wilson')];
    expect(openingLine(moved, all, at(14, 0))).toBe('The handover puts Eilidh and Gregor on air · from rehearsal');
    expect(openingLine(moved, all.map((s) => ({ ...s, on_air: s.identity !== 'guest-kirsty' })), at(14, 6))).toBe('From rehearsal: Kirsty stands by until you put them on');
    expect(openingLine(moved, all.map((s) => ({ ...s, on_air: true })), at(14, 6))).toBeNull();
    expect(openingLine(moved, all, at(14, 11))).toBeNull();
    expect(movedWords(moved)).toEqual({ title: 'Steeped is in the live studio', message: '3 people are standing by under Not on air. Nobody is on air until the handover at 14:05.' });
  });

  it('draws them under Not on air with the note, the opening under the window, and rehearsing in the window’s row', () => {
    const p = (identity: string, name: string, role: Source['role'], slot: number): Source => ({ identity, name, role, on_air: false, mute: false, cue: false, talk: false, talk_to: null, present: true, away_ms: null, linked: true, lufs: -20, lufs_peak: -18, slot, caller: null });
    const now = at(14, 0);
    const v: View = {
      station: 'Kettle FM',
      now_ms: now,
      show: { title: 'The Morning Kettle', rundown: [] },
      desk: { reachable: true, error: null, playout: { connected: true, for_ms: 14_000 } },
      seats: { presenter: null, screener: null, presenter_name: '', screener_name: '' },
      sources: [p(EILIDH, 'Eilidh Fraser-MacLeod', 'presenter', 1), p('guest-gregor', 'Gregor Stewart', 'guest', 2), p('guest-kirsty', 'Kirsty Wilson', 'guest', 3)],
      callers: [],
      desk_taking_callers: false,
      chat: { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false, influx: false, influx_since_ms: null },
      notices: [],
      carts: { version: 0, desk: true, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
      listener_delay: { ms: 15_000, measured: false },
      mode: 'auto',
      step: { ...NO_SHOW, step: 'full' },
    } as View;
    studio.view = v;
    studio.skew = now - studio.now;
    rdesk.moved = moved;
    const body = render(DeskB).body;
    const waiting = body.slice(body.indexOf('data-panel="desk.waiting"'), body.indexOf('data-panel="desk.audience"'));
    expect(waiting.match(/data-testid="from-rehearsal"/g)).toHaveLength(3);
    expect(waiting).toContain('From rehearsal · on at the handover, muted');
    expect(body).toMatch(/data-testid="opening-line"[^>]*>.*The handover puts Eilidh and Gregor on air · from rehearsal/);
    // Nobody moved is drawn on air: every one of them stands by.
    expect(body.slice(body.indexOf('data-panel="desk.stage"'), body.indexOf('data-panel="desk.waiting"'))).not.toContain('data-panel="desk.tile.');
    // Without the window's memory of the move, they are anyone standing by: nothing is claimed.
    rdesk.moved = null;
    expect(render(DeskB).body).not.toContain('from-rehearsal');

    const rows = windowRows({ show: { slug: 'steeped', name: 'Steeped', start_ms: at(14, 5), started: false }, source: null, source_name: null, state: null, holders: [], window: WINDOW }, now);
    expect(rows.at(-1)).toMatchObject({ is: 'next', note: 'rehearsing' });
    expect(windowRows({ show: { slug: 'steeped', name: 'Steeped', start_ms: at(14, 5), started: false }, source: null, source_name: null, state: null, holders: [], window: { ...WINDOW, rehearsing: false } }, now).at(-1)?.note ?? null).toBeNull();
  });
});

describe('where a key goes while the Desk tab shows the rehearsal desk', () => {
  const EVERY: Action[] = [
    { type: 'tab', tab: 'carts' }, { type: 'arm', what: 'dump' }, { type: 'arm', what: 'drop' }, { type: 'arm', what: 'end-pass', identity: 'x' }, { type: 'arm', what: 'restart-playout' }, { type: 'arm', what: 'fade-all' },
    { type: 'disarm' }, { type: 'dump' }, { type: 'drop' }, { type: 'end-pass', identity: 'x' }, { type: 'disconnect', identity: 'x', key: 'k' }, { type: 'influx' }, { type: 'restart-playout' }, { type: 'end-show' },
    { type: 'talk-seat', held: true }, { type: 'cart', n: 1 }, { type: 'bank', n: 2 }, { type: 'picture' }, { type: 'escape' }, { type: 'leave-field' },
    { type: 'on', slot: 1 }, { type: 'cue', slot: 1 }, { type: 'mute', slot: 1 }, { type: 'talk', slot: 1, held: true }, { type: 'handover' }, { type: 'take-caller' },
    { type: 'cart-key', slot: 1 }, { type: 'cart-cue', slot: 1 }, { type: 'cart-stop-last' }, { type: 'fade-all' }, { type: 'cart-move', dx: 1, dy: 0 }, { type: 'cart-enter' },
    { type: 'queue-move', delta: 1 }, { type: 'queue-jump', n: 1 }, { type: 'screen-talk', held: true }, { type: 'send' }, { type: 'back' }, { type: 'focus-name' }, { type: 'release-place' },
    { type: 'stations' }, { type: 'week', delta: 1 }, { type: 'today' }, { type: 'view', view: 'day' },
  ] as Action[];
  const TO_REHEARSAL = ['on', 'cue', 'mute', 'talk', 'cart'];
  const WINDOWS_OWN = ['tab', 'disarm', 'bank', 'escape', 'leave-field', 'influx', 'cart-move', 'queue-move', 'queue-jump', 'stations', 'week', 'today', 'view'];

  it('sends the strips’ keys and F1 to F6 to the rehearsal, keeps the window’s own, and swallows every key of the air', () => {
    for (const a of EVERY) {
      const route = routeKey(a, 'desk', 'rehearsal');
      const want = TO_REHEARSAL.includes(a.type) ? 'rehearsal' : WINDOWS_OWN.includes(a.type) ? 'live' : 'swallow';
      expect(route, `${a.type}${'what' in a ? ` ${a.what}` : ''}`).toBe(want);
    }
    // Dump, the handover, End show, Restart playout, the picture, the callers: nothing is sent.
    for (const t of ['dump', 'handover', 'end-show', 'restart-playout', 'picture', 'take-caller', 'talk-seat', 'drop', 'end-pass'] as const) {
      expect(routeKey(EVERY.find((a) => a.type === t)!, 'desk', 'rehearsal'), t).toBe('swallow');
    }
  });

  it('leaves every key as it was with the live desk shown, and on every other tab', () => {
    for (const a of EVERY) {
      expect(routeKey(a, 'desk', 'live')).toBe('live');
      for (const tab of ['carts', 'screener', 'schedule'] as const) expect(routeKey(a, tab, 'rehearsal')).toBe('live');
    }
  });
});

describe('the presses', () => {
  const sent: { url: string; body: unknown }[] = [];
  let answer: { status: number; body: unknown } = { status: 200, body: OPEN };
  beforeEach(() => {
    sent.length = 0;
    answer = { status: 200, body: OPEN };
    vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
      sent.push({ url, body: init?.body ? JSON.parse(String(init.body)) : undefined });
      return new Response(JSON.stringify(answer.body), { status: answer.status, headers: { 'content-type': 'application/json' } });
    });
  });

  it('go to the rehearsal’s own route, by the strip’s person, and never to the live desk', async () => {
    put(OPEN);
    expect(mayDrive()).toBe(true);
    expect(personAt(3)?.name).toBe('Kirsty Wilson');
    await runRehearsal({ type: 'on', slot: 3 });
    await runRehearsal({ type: 'mute', slot: 2 });
    await runRehearsal({ type: 'cue', slot: 1 });
    // Talk is this window's own voice in the rehearsal room, to someone else there.
    await runRehearsal({ type: 'talk', slot: 2, held: true });
    expect(rdesk.talking).toEqual(['guest-gregor']);
    await runRehearsal({ type: 'talk', slot: 2, held: false });
    expect(rdesk.talking).toEqual([]);
    expect(sent.map((s) => s.body)).toEqual([
      { op: 'on_air', identity: 'guest-kirsty', on: true },
      { op: 'mute', identity: 'guest-gregor', on: false },
      { op: 'cue', identity: EILIDH, on: true },
      { op: 'talk_to', identity: EILIDH, to: 'guest-gregor', on: true },
      { op: 'talk_to', identity: EILIDH, to: 'guest-gregor', on: false },
    ]);
    expect(sent.every((s) => s.url === '/api/v1/rehearsal/commands')).toBe(true);
    // An empty strip, and Talk to oneself, send nothing.
    sent.length = 0;
    await runRehearsal({ type: 'on', slot: 5 });
    await runRehearsal({ type: 'talk', slot: 1, held: true });
    expect(sent).toEqual([]);
  });

  it('F1 to F6 fire the rehearsed show’s carts into the rehearsal, and stop one that is sounding', async () => {
    put(OPEN);
    const cart = { id: 'c0ffee00-0000-4000-8000-000000000001', label: 'Steeped ident', bank: 1, slot: 1, mode: 'once', duration_ms: 4_000, file: 'ident.flac', missing: false, show: 'steeped' };
    rdesk.catalogue = { version: 3, show: 'steeped', banks: [{ id: 'b1', number: 1, name: 'Steeped', colour_slot: 4, show: 'steeped' }], carts: [cart], loudness_lufs: -16 } as unknown as typeof rdesk.catalogue;
    studio.bank = 1;
    await runRehearsal({ type: 'cart', n: 1 });
    expect(sent.at(-1)).toEqual({ url: '/api/v1/rehearsal/commands', body: { op: 'cart_fire', cart: cart.id } });
    // Sounding: the same key fades it.
    rehearsal.view = { ...OPEN, carts_sounding: [{ cart: cart.id, label: 'Steeped ident', mode: 'once', cue: false, elapsed_ms: 500, remaining_ms: 3_500, loop: false, stopping: false }] } as unknown as RehearsalView;
    await runRehearsal({ type: 'cart', n: 1 });
    expect(sent.at(-1)).toEqual({ url: '/api/v1/rehearsal/commands', body: { op: 'cart_stop', cart: cart.id } });
    // An empty slot says so and sends nothing; nothing ever went to the live carts' route.
    const before = sent.length;
    await runRehearsal({ type: 'cart', n: 2 });
    expect(sent).toHaveLength(before);
    expect(studio.note?.title).toBe('No cart here yet');
    expect(sent.every((x) => x.url === '/api/v1/rehearsal/commands')).toBe(true);
  });

  it('send nothing for another show’s rehearsal, and a 403 makes the screen read-only', async () => {
    put({ ...OPEN, session: { ...OPEN.session!, show: 'the-morning-kettle', show_name: 'The Morning Kettle', opened_by: 'Kirsty' } });
    expect(mayDrive()).toBe(false);
    await runRehearsal({ type: 'on', slot: 1 });
    expect(sent).toEqual([]);
    expect(studio.note?.title).toBe('This is Kirsty’s rehearsal');

    // The station has the last word: a producer here, refused there.
    put(OPEN, staff('producer', 'x'));
    expect(mayDrive()).toBe(true);
    answer = { status: 403, body: { error: 'Only the presenter of Steeped, a producer or an admin runs its rehearsal.' } };
    await runRehearsal({ type: 'on', slot: 3 });
    expect(rdesk.refused).toBe('Only the presenter of Steeped, a producer or an admin runs its rehearsal.');
    expect(mayDrive()).toBe(false);
  });

  it('moves, remembers what the station answered, and shows the live desk', async () => {
    put(OPEN);
    studio.deskStudio = 'rehearsal';
    answer = { status: 200, body: { moved: [EILIDH, 'guest-gregor', 'guest-kirsty'], opening: { airing_start_ms: at(14, 5), standby_until_ms: at(14, 10), host: EILIDH, on: [EILIDH, 'guest-gregor'], mute: ['guest-gregor'] }, airing_start_ms: at(14, 5) } };
    expect(await moveNow()).toEqual({ ok: true });
    expect(sent.at(-1)?.url).toBe('/api/v1/rehearsal/move');
    expect(rdesk.moved).toMatchObject({ show: 'steeped', show_name: 'Steeped', moved: [EILIDH, 'guest-gregor', 'guest-kirsty'] });
    expect(studio.deskStudio).toBe('live');
    expect(rehearsal.view?.session).toBeNull();
    expect(studio.note?.title).toBe('Steeped is in the live studio');

    // A refusal is the station's sentence, and nothing changes.
    put(OPEN);
    studio.deskStudio = 'rehearsal';
    answer = { status: 409, body: { error: 'Steeped has no airing in the next twelve hours: it is next on air on Saturday at 14:05.' } };
    expect(await moveNow()).toEqual({ ok: false, why: 'Steeped has no airing in the next twelve hours: it is next on air on Saturday at 14:05.' });
    expect(studio.deskStudio).toBe('rehearsal');
    expect(rehearsal.view?.session).not.toBeNull();
  });

  it('lets every held key go before the Desk tab shows the other studio', async () => {
    put(OPEN);
    studio.deskStudio = 'rehearsal';
    await runRehearsal({ type: 'talk', slot: 2, held: true });
    const order: string[] = [];
    onBeforeStudioSwitch(async () => void order.push('frame'));
    await switchDeskStudio('live');
    onBeforeStudioSwitch(null);
    expect(order).toEqual(['frame']);
    expect(sent.at(-1)?.body).toEqual({ op: 'talk_to', identity: EILIDH, to: 'guest-gregor', on: false });
    expect(rdesk.talking).toEqual([]);
    expect(studio.deskStudio).toBe('live');
  });
});

describe('the rehearsal desk, rendered', () => {
  const stage = (body: string) => body.slice(body.indexOf('data-panel="rehearsal.stage"'), body.indexOf('data-panel="rehearsal.carts"'));
  const clean = (body: string) => {
    for (const m of AIR_MARKS) expect(body, m).not.toContain(m);
    // The lamp: REHEARSAL, in its own shape, whatever the state.
    expect(body).toMatch(/class="lamp rehearsal[^"]*"[^>]*data-panel="rehearsal\.lamp"|data-panel="rehearsal\.lamp"[^>]*class="lamp rehearsal/);
    expect(body).toContain('#g-loop');
    expect(body).toContain('No way to the air from here');
  };

  it('three people: tiles that say In the mix or Not in the mix, on the desk’s keys', () => {
    put(OPEN);
    const body = render(RehearsalDesk).body;
    clean(body);
    expect(stage(body)).toContain('Rehearsing');
    expect(body).toContain('On air at 14:05: move by then');
    const mix = body.slice(body.indexOf('data-panel="rehearsal.stage"'), body.indexOf('data-panel="rehearsal.waiting"'));
    expect(mix).toContain('data-panel="rehearsal.tile.1"');
    expect(mix).toContain('data-panel="rehearsal.tile.2"');
    expect(mix).not.toContain('data-panel="rehearsal.tile.3"');
    expect(mix).toContain('In the mix');
    expect(mix).toContain('Muted');
    const wait = body.slice(body.indexOf('data-panel="rehearsal.waiting"'), body.indexOf('data-panel="rehearsal.carts"'));
    expect(wait).toContain('data-panel="rehearsal.tile.3"');
    expect(wait).toContain('Not in the mix');
    expect(wait).toContain('Kirsty Wilson');
    // Mix, Cue, Talk, Mute on the live desk's keys for the strip.
    expect(wait).toMatch(/Mix<\/span>(<!--[^>]*-->)*<kbd>3<\/kbd>/);
    expect(wait).toMatch(/Cue<\/span>(<!--[^>]*-->)*<kbd>E<\/kbd>/);
    expect(body).toContain('Move to the live studio…');
    expect(body).toContain('End rehearsal');
    expect(body).toContain('Invite someone…');
    // The move dialog lists who moves with their opening, and the plan's own sentence.
    const dialog = body.slice(body.indexOf('data-panel="rehearsal.move"'));
    expect(dialog).toContain('Move Steeped to the live studio?');
    expect(dialog).toContain(MOVE.lede);
    expect(dialog).toContain('On at the handover, muted');
    expect(dialog).toMatch(/Stay here <kbd>Esc<\/kbd>/);
    expect(dialog).toMatch(/Move <kbd>⏎<\/kbd>/);
  });

  it('no session: Open the rehearsal studio for…, with the shows this account may run', () => {
    put(CLOSED);
    const body = render(RehearsalDesk).body;
    clean(body);
    expect(stage(body)).toContain('Open the rehearsal studio for…');
    expect(stage(body)).toMatch(/<option value="steeped"[^>]*>Steeped<\/option>/);
    expect(stage(body)).not.toContain('The Morning Kettle');
    expect(stage(body)).toContain('Open for Steeped');
    expect(body).not.toContain('data-panel="rehearsal.waiting"');
    expect(body).not.toContain('Move to the live studio…');
    // A producer may open it for any show; an account with no show is told so.
    put(CLOSED, staff('producer', 'x'));
    expect(stage(render(RehearsalDesk).body)).toContain('The Morning Kettle');
    put(CLOSED, staff('presenter', 'nobody'));
    expect(stage(render(RehearsalDesk).body)).toContain('No show is tied to your account');
  });

  it('another show’s session: shown, and not driven', () => {
    put({ ...OPEN, session: { ...OPEN.session!, show: 'the-morning-kettle', show_name: 'The Morning Kettle', opened_by: 'Kirsty' } });
    const body = render(RehearsalDesk).body;
    clean(body);
    expect(body).toContain('data-testid="rehearsal-readonly"');
    expect(body).toContain('Kirsty is rehearsing The Morning Kettle. You can watch');
    expect(body).not.toContain('Move to the live studio…');
    expect(body).not.toContain('End rehearsal');
    expect(body).not.toContain('Invite someone…');
    // Every control of every tile is off.
    const tiles = stage(body);
    expect(tiles.match(/<button[^>]*class="btn[^"]*"[^>]*>/g)?.filter((b) => !/disabled/.test(b) && !/to-live/.test(b))).toEqual([]);
  });

  it('the desk not answering, off at this station, and not read yet', () => {
    put({ ...OPEN, desk: { linked: false, error: 'the rehearsal desk is not answering' } });
    let body = render(RehearsalDesk).body;
    clean(body);
    expect(body).toContain('The rehearsal desk is starting again');
    expect(body).toContain('the live studio is not affected');
    expect(body).toMatch(/Rehearsal desk <b>not answering<\/b>/);

    put({ ...CLOSED, desk: { linked: false, error: null } });
    body = render(RehearsalDesk).body;
    clean(body);
    expect(body).toContain('The rehearsal desk is not answering yet.');

    put({ ...CLOSED, enabled: false });
    body = render(RehearsalDesk).body;
    clean(body);
    expect(body).toContain('The rehearsal studio is off at this station');
    expect(body).toContain('REHEARSAL=true');
    expect(body).not.toContain('Open the rehearsal studio for…');

    put(null);
    body = render(RehearsalDesk).body;
    clean(body);
    expect(body).toContain('Reading the rehearsal studio…');
  });
});

describe('the invitation’s words, and the registry', () => {
  it('says of every link that its person joins off air, and of a rehearsal link that it is not the air', () => {
    for (const r of INVITE_ROLES) expect(r.what).toMatch(/off air|standing by/);
    expect(joinsLine({ role: 'presenter', name: 'Eilidh Fraser-MacLeod', identity: 'p' })).toContain('They arrive standing by, off air');
    expect(rehearsalJoinsLine({ name: 'Gregor Stewart', identity: 'g' }, 'Steeped')).toBe('Gregor opens it in a browser and presses Join. They arrive in the rehearsal studio for Steeped: only the people there hear them, and nothing reaches listeners.');
    expect(WHERE.rehearsal('Steeped').name).toBe('Rehearsal studio · Steeped');
    // A rehearsal link is for the show's own people, at the show's step, and speaks of no air.
    expect(rehearsalRolesAt('solo')).toEqual([]);
    expect(rehearsalRolesAt('guests').map((r) => r.role)).toEqual(['guest', 'presenter']);
    expect(rehearsalRolesAt('full').map((r) => r.role)).toEqual(['guest', 'presenter']);
    for (const r of REHEARSAL_ROLES) expect(r.what).not.toMatch(/on air|Not on air|listeners/i);
    expect(WHERE.solo('Steeped')).toContain('only its presenter’s own link');
  });

  it('has every new panel in the registry, none speaking of the air as theirs', () => {
    for (const n of ['desk.studio', 'rehearsal.stage', 'rehearsal.lamp', 'rehearsal.tile.1', 'rehearsal.tile.6', 'rehearsal.waiting', 'rehearsal.clock', 'rehearsal.session', 'rehearsal.carts', 'rehearsal.toast', 'rehearsal.status', 'rehearsal.move', 'settings.rehearsal']) {
      expect(PANELS[n], n).toBeTruthy();
    }
    expect(PANELS['desk.invite']).not.toMatch(/presenter on air/);
  });
});
