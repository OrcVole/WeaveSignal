// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What the studio shows at each beginner step (ADR-0271, ADR-0274), rendered on the server from
// a view: the frame's step control, tabs and Show everything; the desk panel by panel, as the
// design's tables list it (docs/reference/design/beginner-steps.md, section 1); the dashed
// "off at" tags while everything is shown; and how the window keeps its look in line when the
// station's step moves under it.
import { render } from 'svelte/server';
import { beforeEach, describe, expect, it } from 'vitest';
import { CALENDAR, DESK_B, PANELS, SANDBOX, STUDIO, panel } from '../../../packages/ui/panels.ts';
import App from './App.svelte';
import DeskB from './desk/DeskB.svelte';
import StepControl from './lib/StepControl.svelte';
import { readyCaller, run, tellOff } from './lib/actions.ts';
import { look, setShowAll, studio, takeView } from './lib/studio.svelte.ts';
import { chat } from './lib/chat.svelte.ts';
import { STEPS, STEP_WORD } from './lib/steps.ts';
import type { Caller, DeskPerson, ShowStep, Source, StepView, View } from './lib/api.ts';
import type { Msg } from '../../../packages/ui/chat.ts';

const NOW = Date.UTC(2026, 10, 11, 9, 57, 12);
const KETTLE = { slug: 'the-morning-kettle', name: 'The Morning Kettle' };

const person = (identity: string, name: string, role: DeskPerson['role'], on_air: boolean): DeskPerson => ({
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
  lufs: -22.6,
  lufs_peak: -19.4,
});
const src = (slot: number, identity: string, name: string, role: DeskPerson['role'], on_air = true): Source => ({ ...person(identity, name, role, on_air), slot, caller: null });
const EILIDH = src(1, 'eilidh', 'Eilidh Fraser-MacLeod', 'presenter');
const GREGOR = src(2, 'gregor', 'Gregor Stewart', 'guest');
const DOUGAL: Source = { ...src(4, 'dougal', 'Dougal Thomson', 'caller', false), cue: true, caller: { line: 'The bins in Portobello', note: '', name_on_air: '', sent: true, taken: true } };
const FINLAY = src(5, 'finlay', 'Finlay Grant', 'dj', false);

const caller = (identity: string, name: string, topic: string, waitedMs: number, over: Partial<Caller> = {}): Caller => ({
  identity,
  name,
  ordinal: 1,
  state: 'waiting',
  since_ms: NOW - waitedMs,
  arrived_ms: NOW - waitedMs,
  line: '',
  note: '',
  name_on_air: '',
  declined: null,
  banned_before: null,
  topic,
  consent: { at_ms: NOW - waitedMs, version: '2026-11-01', by: 'caller' },
  dropped_ms: null,
  slot: null,
  on_air: false,
  cue: false,
  linked: true,
  present: true,
  lufs: null,
  ...over,
});
const NIALL = caller('niall', 'Niall Anderson', 'The bus lanes on Leith Walk', 72_000);
const JESSIE = caller('jessie', 'Jessie Brown', 'No night bus from the Infirmary', 34_000);

const stepOf = (step: ShowStep, over: Partial<StepView> = {}): StepView => ({ step, show: KETTLE, on_air: true, next: null, may_change: true, ...over });

/** The people of the design's mock-ups at each step. */
const CAST: Record<ShowStep, { sources: Source[]; callers: Caller[] }> = {
  solo: { sources: [EILIDH], callers: [] },
  guests: { sources: [EILIDH, GREGOR], callers: [] },
  callers: { sources: [EILIDH, GREGOR, DOUGAL], callers: [caller('dougal', 'Dougal Thomson', 'The bins in Portobello', 300_000, { state: 'ready', slot: 4, cue: true }), NIALL, JESSIE] },
  full: { sources: [EILIDH, GREGOR, { ...DOUGAL, cue: false, on_air: true }, FINLAY], callers: [] },
};

function view(step: ShowStep, over: Partial<View> = {}, sv: Partial<StepView> = {}): View {
  return {
    station: 'Kettle FM',
    now_ms: NOW,
    show: { title: 'The Morning Kettle', rundown: [] },
    desk: {
      reachable: true,
      error: null,
      playout: { connected: true, for_ms: 14_000 },
      delay: { delay_ms: 8000, target_ms: 8000, rebuilding: false, dumps: 0, dumped_at_ms: null },
    },
    seats: { presenter: 'eilidh', screener: null, presenter_name: 'Eilidh Fraser-MacLeod', screener_name: '' },
    sources: CAST[step].sources,
    callers: CAST[step].callers,
    desk_taking_callers: true,
    chat: { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false, influx: false, influx_since_ms: null, held: 0, open_reports: 0 },
    notices: [],
    carts: { version: 0, desk: true, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
    listener_delay: { ms: 15_000, measured: false },
    step: stepOf(step, sv),
    ...over,
  };
}

/** The panel names a rendering carries. */
const panels = (body: string): string[] => [...new Set([...body.matchAll(/data-panel="([^"]+)"/g)].map((m) => m[1]))];
/** One panel's markup, from its name to the next panel's. */
function part(body: string, name: string): string {
  const at = body.indexOf(`data-panel="${name}"`);
  expect(at, name).toBeGreaterThan(-1);
  const next = body.indexOf('data-panel="', at + 1);
  return body.slice(at, next < 0 ? undefined : next);
}
const desk = (step: ShowStep, over: Partial<View> = {}, sv: Partial<StepView> = {}): string => {
  studio.view = view(step, over, sv);
  return render(DeskB).body;
};

beforeEach(() => {
  studio.showAll = false;
  studio.stepMenu = false;
  studio.offNote = null;
  studio.offTold = [];
  studio.note = null;
  studio.armed = null;
  studio.tab = 'desk';
  studio.settings = false;
  studio.now = NOW;
  studio.skew = 0;
  chat.link = 'off';
  chat.why = '';
  chat.room = { lines: [], hidden: {}, featuredAt: {}, pinned: null };
});

describe('the registry', () => {
  it('names the seven new panels, each with one line, and words invitations as a noun', () => {
    for (const name of ['studio.step', 'studio.step.menu', 'studio.step.banner', 'desk.step-note', 'desk.callers', 'calendar.show-step', 'sandbox.steps']) {
      expect(PANELS[name], name).toBeTruthy();
    }
    expect(STUDIO.map((p) => p.name)).toEqual(['studio.frame', 'studio.step', 'studio.step.menu', 'studio.step.banner', 'studio.prepare', 'studio.prepare.menu', 'studio.prepare.banner']);
    expect(DESK_B.map((p) => p.name)).toContain('desk.callers');
    expect(CALENDAR.map((p) => p.name)).toContain('calendar.show-step');
    expect(SANDBOX.map((p) => p.name)).toContain('sandbox.steps');
    expect(PANELS['desk.chat']).toBe('In the right panel. The chat to read: the room’s last messages, and one line when something waits for a moderator.');
    expect(PANELS['desk.invite']).toMatch(/^The invitation dialog/);
    expect(PANELS['desk.waiting']).toMatch(/an invitation/);
    // "invite" stays only as the verb, in "Invite someone…".
    for (const [name, what] of Object.entries(PANELS)) expect(what.replace(/Invite someone…/g, ''), name).not.toMatch(/\binvites?\b/i);
  });
});

describe('the frame at each step', () => {
  const tabsOf = (body: string) => {
    const nav = body.slice(body.indexOf('<nav class="tabs'), body.indexOf('</nav>'));
    return nav
      .split('<button')
      .slice(1)
      .map((b) => [b.match(/>([A-Za-z]+)</)?.[1] ?? '', b.match(/<kbd>([^<]*)<\/kbd>/)?.[1] ?? '']);
  };

  it('draws Desk, Carts and Schedule below Full, each keeping its number, and every tab at Full', () => {
    for (const s of ['solo', 'guests', 'callers'] as const) {
      studio.view = view(s);
      expect(tabsOf(render(App).body), s).toEqual([['Desk', '⌃1'], ['Carts', '⌃3'], ['Schedule', '⌃5'], ['Settings', '']]);
    }
    studio.view = view('full');
    expect(tabsOf(render(App).body)).toEqual([['Desk', '⌃1'], ['Screener', '⌃2'], ['Carts', '⌃3'], ['Chat', '⌃4'], ['Schedule', '⌃5'], ['Settings', '']]);
  });

  it('puts the step control beside the station’s name, before the tabs, as pips and a word', () => {
    studio.view = view('solo');
    const body = render(App).body;
    expect(body.indexOf('class="brand"')).toBeLessThan(body.indexOf('data-panel="studio.step"'));
    expect(body.indexOf('data-panel="studio.step"')).toBeLessThan(body.indexOf('<nav class="tabs'));
    const ctl = part(body, 'studio.step');
    expect(ctl).toContain('aria-label="Step: Solo, 1 of 4. Change the step"');
    expect(ctl).toContain('title="The step of The Morning Kettle"');
    expect(ctl).toContain('aria-haspopup="dialog"');
    expect(ctl).toMatch(/<span class="word">Solo<\/span>/);
    // Four pips of one size, filled up to the step.
    expect([...ctl.matchAll(/<rect[^>]*>/g)]).toHaveLength(4);
    expect([...ctl.matchAll(/<rect[^>]*class="[^"]*\bon\b[^"]*"/g)]).toHaveLength(1);
    // Not a state chip: no live or status colour.
    expect(ctl).not.toMatch(/class="state|fill-onair|hollow-/);
    for (const s of STEPS) {
      studio.view = view(s);
      const c = part(render(App).body, 'studio.step');
      expect(c, s).toContain(`<span class="word">${STEP_WORD[s]}</span>`);
      expect([...c.matchAll(/<rect[^>]*class="[^"]*\bon\b[^"]*"/g)], s).toHaveLength(STEPS.indexOf(s) + 1);
    }
  });

  it('offers Show everything below Full and not at Full, where nothing is hidden', () => {
    for (const s of ['solo', 'guests', 'callers'] as const) {
      studio.view = view(s);
      expect(part(render(App).body, 'studio.step'), s).toMatch(/class="showall"[^>]*title="See the whole studio for now\. Nothing is switched on\."[^>]*>Show everything</);
    }
    studio.view = view('full');
    expect(render(App).body).not.toContain('Show everything');
  });

  it('draws the button’s edge dashed between shows, and says which show the step is kept from', () => {
    studio.view = view('solo', {}, { on_air: false });
    const ctl = part(render(App).body, 'studio.step');
    expect(ctl).toMatch(/class="stepbtn[^"]*\baway\b/);
    expect(ctl).toContain('No show is on air: the step of The Morning Kettle, the show last on air');
    studio.view = view('solo');
    expect(part(render(App).body, 'studio.step')).not.toMatch(/class="stepbtn[^"]*\baway\b/);
  });

  it('says Connected while the link is good, and the full words whenever it is not', () => {
    studio.view = view('solo');
    studio.link = 'live';
    let link = render(App).body.match(/<span class="link[^"]*"[\s\S]*?<\/span>/)![0];
    expect(link).toMatch(/<b[^>]*>Connected<\/b>/);
    expect(link).not.toMatch(/Control plane <b/);
    expect(link).toContain('title="Control plane connected"');
    expect(link).toContain('data-link="live"');
    studio.link = 'lost';
    studio.linkWhy = 'the control plane closed the stream';
    link = render(App).body.match(/<span class="link[^"]*"[\s\S]*?<\/span>/)![0];
    expect(link).toMatch(/Control plane <b[^>]*>not connected<\/b>/);
    expect(link).toContain('title="the control plane closed the stream"');
    studio.link = 'connecting';
    expect(render(App).body).toMatch(/Control plane <b[^>]*>connecting<\/b>/);
  });

  it('lists the four steps in the dialog, the one in force marked now', () => {
    studio.view = view('guests');
    const menu = part(render(StepControl).body, 'studio.step.menu');
    expect(menu).toContain('Step for The Morning Kettle');
    expect(menu).toContain('On air now. A step shows the parts of the studio this show uses');
    expect([...menu.matchAll(/data-step="(\w+)"/g)].map((m) => m[1])).toEqual(['solo', 'guests', 'callers', 'full']);
    const rows = menu.split('<label').slice(1);
    expect(rows.map((r) => r.includes('class="nowtag"'))).toEqual([false, true, false, false]);
    expect(menu).not.toMatch(/<input[^>]*disabled/);
    expect(menu).toContain('Show everything');
    expect(menu).toMatch(/Close\s*<kbd>Esc<\/kbd>/);
    // The move is named only once another step is picked: nothing to confirm yet.
    expect(menu).not.toContain('Move to');
  });

  it('shows the list to read, with who can change it, to someone who may not', () => {
    studio.view = view('solo', {}, { may_change: false });
    const menu = part(render(StepControl).body, 'studio.step.menu');
    expect(menu).toContain('a producer or an admin can change the step. You can look at the whole studio without changing anything.');
    expect([...menu.matchAll(/<input[^>]*type="radio"[^>]*>/g)].every((m) => m[0].includes('disabled'))).toBe(true);
    expect(menu).toContain('Show everything');
  });

  it('says the studio shows everything when no show has been on air', () => {
    studio.view = view('full', {}, { show: null, on_air: false, may_change: false });
    const menu = part(render(StepControl).body, 'studio.step.menu');
    expect(menu).toContain('Steps belong to a show, and no show has been on air yet, so the studio shows everything and there is no step to change.');
    // Each locked choice, and one line under them, says what unlocks it (the operator's 1.0.2 run).
    expect(menu).toContain('Locked until a show has been on air: make a show with an episode on the Schedule tab');
    expect(menu).toMatch(/<label title="Locked until a show has been on air/);
    expect(menu).not.toContain('Show everything</button>');
  });
});

describe('Show everything (ADR-0274): a look, not a switch', () => {
  it('draws the whole studio in this window, with the row that says so and the way back', () => {
    studio.view = view('solo');
    setShowAll(true);
    expect(look()).toBe('full');
    const body = render(App).body;
    const banner = part(body, 'studio.step.banner');
    expect(banner).toContain('Showing everything for now.');
    expect(banner).toContain('The Morning Kettle still runs at Solo, so the parts marked');
    expect(banner).toContain('<span class="offtag">off at Solo</span>');
    expect(banner).toContain('To switch one on, move the show up a step.');
    expect(banner).toMatch(/<button[^>]*>Back to Solo<\/button>/);
    // The tabs return, and the frame's button gives its place to them.
    expect(body).toMatch(/>Screener<!--[^>]*-->(<!--[^>]*-->)*\s*<kbd>⌃2<\/kbd>/);
    expect(part(body, 'studio.step')).not.toContain('class="showall"');
    // The station's step is untouched: nothing was sent, and the view still says Solo.
    expect(studio.view.step.step).toBe('solo');
    setShowAll(false);
    expect(look()).toBe('solo');
    expect(render(App).body).not.toContain('data-panel="studio.step.banner"');
  });

  it('tags each part the step keeps off, and disables its controls with the reason', () => {
    studio.view = view('solo');
    setShowAll(true);
    const body = render(DeskB).body;
    const delay = part(body, 'desk.delay');
    expect(delay).toMatch(/aria-disabled="true"[^>]*title="Dump is off at Solo\. It comes on at Callers\."/);
    expect(delay).toContain('<span class="offtag">off at Solo</span>');
    expect(part(body, 'desk.waiting')).toContain('<span class="offtag">invitations off at Solo</span>');
    expect(part(body, 'desk.waiting')).not.toContain('Invite someone…');
    const picture = part(body, 'desk.picture');
    expect(picture).toContain('<span class="offtag">off at Solo</span>');
    expect(picture).toContain('the slate holds the picture');
    const status = part(body, 'desk.status');
    expect(status).toMatch(/aria-disabled="true"[^>]*title="Talk to the screener is off at Solo\. It comes with Full\."/);
    expect(status).toContain('<span class="offtag">off at Solo</span>');
    // The step's own panels give way to the whole studio's.
    expect(body).not.toContain('data-panel="desk.step-note"');
  });

  it('tags only what the step lacks: at Callers, Dump and invitations are on and the picture is off', () => {
    studio.view = view('callers');
    setShowAll(true);
    const body = render(DeskB).body;
    expect(part(body, 'desk.delay')).not.toContain('offtag');
    expect(part(body, 'desk.waiting')).toContain('Invite someone…');
    expect(part(body, 'desk.picture')).toContain('<span class="offtag">off at Callers</span>');
    // The queue is the Screener tab's while everything is shown.
    expect(body).not.toContain('data-panel="desk.callers"');
  });

  it('means nothing at Full, lasts until the next show takes the air, and is this window’s alone', () => {
    studio.view = view('full');
    setShowAll(true);
    expect(studio.showAll).toBe(false);
    studio.view = view('solo');
    setShowAll(true);
    // The same show carries on, and moves a step: the look stays.
    takeView(view('guests'));
    expect(studio.showAll).toBe(true);
    // Another show takes the air: back to its step.
    takeView(view('solo', {}, { show: { slug: 'steeped', name: 'Steeped' } }));
    expect(studio.showAll).toBe(false);
    // The show moved up to Full while everything was shown: nothing is hidden any more.
    setShowAll(true);
    takeView(view('full', {}, { show: { slug: 'steeped', name: 'Steeped' } }));
    expect(studio.showAll).toBe(false);
  });

  it('goes back to the Desk when the look loses the tab this window is on', () => {
    studio.view = view('solo');
    setShowAll(true);
    studio.tab = 'screener';
    setShowAll(false);
    expect(studio.tab).toBe('desk');
    // The step moved down under a window on the Screener tab.
    studio.view = view('full');
    studio.tab = 'screener';
    takeView(view('callers'));
    expect(studio.tab).toBe('desk');
    // Carts and Schedule are at every step: they stay.
    studio.tab = 'carts';
    takeView(view('solo'));
    expect(studio.tab).toBe('carts');
  });
});

describe('the desk at each step (design section 1)', () => {
  it('draws the panels the step has, and no others', () => {
    const always = ['desk.clock', 'desk.rundown', 'desk.playout', 'desk.stage', 'desk.lamp', 'desk.air', 'desk.end-show', 'desk.tile.1', 'desk.audience', 'desk.carts', 'desk.chat', 'desk.status'];
    const want: Record<ShowStep, string[]> = {
      solo: [...always, 'desk.step-note'],
      guests: [...always, 'desk.tile.2', 'desk.waiting', 'desk.invite', 'desk.tile.6'],
      callers: [...always, 'desk.tile.2', 'desk.waiting', 'desk.invite', 'desk.tile.6', 'desk.delay', 'desk.tile.4', 'desk.callers'],
      full: [...always, 'desk.tile.2', 'desk.waiting', 'desk.invite', 'desk.tile.6', 'desk.delay', 'desk.tile.4', 'desk.tile.5', 'desk.picture'],
    };
    for (const s of STEPS) {
      const got = panels(desk(s));
      expect([...got].sort(), s).toEqual([...want[s]].sort());
      for (const name of got) expect(() => panel(name), name).not.toThrow();
    }
  });

  it('keeps End show and the air control under the lamp at every step, Solo included', () => {
    for (const s of STEPS) {
      const stage = desk(s);
      expect(stage, s).toMatch(/End show\s*<kbd>⌃⇧E<\/kbd>/);
      expect(stage, s).toContain('data-panel="desk.air"');
      expect(stage.includes('Screener <kbd>⌃2</kbd>') || /Screener\s*<kbd>⌃2<\/kbd>/.test(stage), s).toBe(s === 'full');
    }
  });

  it('keeps the Solo tile whole, with Talk disabled because there is nobody to talk to', () => {
    const tile = part(desk('solo'), 'desk.tile.1');
    for (const key of ['1', 'Q', 'A', 'Z']) expect(tile).toContain(`<kbd>${key}</kbd>`);
    expect(tile).toMatch(/aria-disabled="true"[^>]*title="Talk needs someone to talk to\. It comes with Guests\."/);
    // From Guests on, Talk on the DJ's strip is as it was.
    expect(part(desk('guests'), 'desk.tile.1')).not.toContain('Talk needs someone to talk to');
  });

  it('explains the quiet desk once, in the guest’s empty seat, and offers the way up', () => {
    const note = part(desk('solo'), 'desk.step-note');
    expect(note).toContain('Just you on this show');
    expect(note).toContain('The Morning Kettle runs at Solo. Guests, callers and the screener’s seat are switched');
    expect(note).toMatch(/nobody can arrive unannounced\./);
    expect(note).toMatch(/<button[^>]*aria-haspopup="dialog"[^>]*>Move up a step…<\/button>/);
    for (const s of ['guests', 'callers', 'full'] as const) expect(desk(s), s).not.toContain('Just you on this show');
  });

  it('holds the DJ’s tile on the stage at Solo, on air or off, with no Not on air row', () => {
    const body = desk('solo', { sources: [{ ...EILIDH, on_air: false }] });
    expect(body).toContain('data-panel="desk.tile.1"');
    expect(body).not.toContain('data-panel="desk.waiting"');
    expect(body).not.toContain('Nobody is open');
    // From Guests the move rule holds: off air, the DJ is under Not on air.
    const guests = desk('guests', { sources: [{ ...EILIDH, on_air: false }] });
    expect(guests.indexOf('data-panel="desk.waiting"')).toBeLessThan(guests.indexOf('data-panel="desk.tile.1"'));
  });

  it('never hides a person: a guest left from a higher step keeps a tile at Solo, with End pass', () => {
    const body = desk('solo', { sources: [EILIDH, { ...GREGOR, on_air: false }] });
    const tile = part(body, 'desk.tile.2');
    expect(tile).toContain('Gregor Stewart');
    expect(tile).toContain('End pass');
    expect(body).not.toContain('data-panel="desk.waiting"');
    // And a caller left on strip 4 keeps Drop.
    expect(part(desk('guests', { sources: [EILIDH, DOUGAL] }), 'desk.tile.4')).toContain('Drop');
  });

  it('offers Join with my microphone… on an empty stage, with no name to type and no role to choose', () => {
    const body = desk('solo', { sources: [] });
    const stage = body.slice(body.indexOf('data-panel="desk.stage"'), body.indexOf('data-panel="desk.audience"'));
    expect(stage).toContain('You are not in the room yet');
    expect(stage).toContain('Your voice reaches the desk from a browser tab on this computer. Open your microphone link, press Join the studio, and you are on air.');
    expect(stage).toMatch(/<button[^>]*class="btn primary[^"]*"[^>]*>Join with my microphone…<\/button>/);
    expect(stage).not.toContain('Nobody is open');
    expect(body).not.toContain('data-panel="desk.step-note"');
    // The link's dialog is the invitation dialog; its first page (a name, a role) is not this button's.
    // Above Solo (the operator, 2026-11-12): offered to whoever may change the step, not to a
    // screener's window, and never while a DJ who took over is on air.
    expect(desk('guests', { sources: [] })).toContain('Join with my microphone…');
    expect(desk('full', { sources: [] }, { may_change: false })).not.toContain('Join with my microphone…');
    expect(desk('full', { sources: [{ ...EILIDH, identity: 'finlay', role: 'dj' }] })).not.toContain('Join with my microphone…');
  });

  it('says why the stage is empty and what to do, not "Nobody is open" (the operator\'s 1.0.2 run)', () => {
    const empty = desk('full', { sources: [] }, { may_change: false });
    expect(empty).toContain('Nobody is in the room yet');
    expect(empty).toContain('Invite someone…');
    expect(empty).not.toContain('Nobody is open');
    const waiting = desk('full', { sources: [{ ...EILIDH, on_air: false }] });
    expect(waiting).toContain('Nobody is on air');
    expect(waiting).toContain('Eilidh Fraser-MacLeod has joined and waits');
  });

  it('offers a guest and a presenter from Guests, and the next show’s DJ only at Full', () => {
    const roles = (body: string) => [...part(body, 'desk.invite').matchAll(/<strong>([^<]+)<\/strong>/g)].map((m) => m[1]);
    expect(roles(desk('guests'))).toEqual(['Guest', 'Presenter']);
    expect(roles(desk('callers'))).toEqual(['Guest', 'Presenter']);
    expect(roles(desk('full'))).toEqual(['DJ', 'Guest', 'Presenter']);
    expect(part(desk('guests'), 'desk.invite')).toContain('An invitation for the next show’s DJ comes with Full.');
    expect(part(desk('full'), 'desk.invite')).not.toContain('comes with Full');
    expect(part(desk('guests'), 'desk.invite')).toContain('Create invitation');
    expect(desk('guests')).toMatch(/>Invite someone…<\/button>/);
  });

  it('brings the delay and Dump at Callers, and keeps them out of sight below', () => {
    expect(desk('solo')).not.toContain('Dump');
    expect(desk('guests')).not.toContain('Dump');
    const delay = part(desk('callers'), 'desk.delay');
    expect(delay).toMatch(/class="btn big[^"]*danger-hollow/);
    expect(delay).toMatch(/Dump\s*<kbd>Del<\/kbd><kbd>Del<\/kbd>/);
    expect(delay).not.toContain('offtag');
  });

  it('puts the queue on the desk at Callers: who waits, what about, how long, and one button', () => {
    const q = part(desk('callers'), 'desk.callers');
    expect(q).toMatch(/<h2>Callers<\/h2>/);
    expect(q).toContain('2 waiting · strip 4 taken');
    expect(q.indexOf('Niall Anderson')).toBeLessThan(q.indexOf('Jessie Brown'));
    expect(q).toContain('The bus lanes on Leith Walk');
    expect(q).toContain('1:12');
    expect(q).toContain('0:34');
    // Strip 4 holds one caller: the button waits, and says why.
    expect(q).toMatch(/aria-disabled="true"[^>]*title="Strip 4 holds one caller\. Put Dougal on air or drop the call, then take the next\."/);
    expect(q).toMatch(/Take Niall to strip 4\s*<kbd>P<\/kbd>/);
    // The strip free: the button is live.
    const free = part(desk('callers', { sources: [EILIDH], callers: [NIALL, JESSIE] }), 'desk.callers');
    expect(free).toContain('2 waiting · lines open');
    expect(free).not.toContain('aria-disabled');
    expect(free).toContain('title="They join strip 4 off air. Cue to listen, hold Talk to speak to them, then On."');
    // Nobody waiting.
    const empty = part(desk('callers', { sources: [EILIDH], callers: [] }), 'desk.callers');
    expect(empty).toContain('<b>Nobody is waiting.</b> Listeners call from the player page; each one appears here with their name and what they want to talk about.');
    expect(empty).toContain('lines open');
    // More than three waiting: three rows, then a count.
    const many = [NIALL, JESSIE, caller('calum', 'Calum Robertson', 'Parking at Easter Road on match days', 20_000), caller('flora', 'Flora Armstrong', 'Just to say hello', 10_000), caller('iona', 'Iona MacIntosh', 'The ferry fares, from Derry', 5_000)];
    const long = part(desk('callers', { sources: [EILIDH], callers: many }), 'desk.callers');
    expect([...long.matchAll(/<li>/g)]).toHaveLength(3);
    expect(long).toContain('and 2 more waiting');
  });

  it('says the DJ took the call at Callers, where no screener sent it', () => {
    expect(part(desk('callers'), 'desk.tile.4')).toContain('Caller · you took the call');
    expect(part(desk('full'), 'desk.tile.4')).toContain('Caller · sent by the screener');
    // And the toast a screener's send raises is not the DJ's own press: none at Callers.
    studio.view = view('callers', { sources: [EILIDH, { ...DOUGAL, cue: false, caller: { ...DOUGAL.caller!, taken: false } }] });
    expect(readyCaller()).toBeNull();
    // While everything is shown at Callers the call is still the DJ's own: the same line, no toast.
    setShowAll(true);
    expect(readyCaller()).toBeNull();
    expect(part(render(DeskB).body, 'desk.tile.4')).toContain('Caller · you took the call');
    setShowAll(false);
    studio.view = view('full', { sources: [EILIDH, { ...DOUGAL, cue: false, caller: { ...DOUGAL.caller!, taken: false } }] });
    expect(readyCaller()?.identity).toBe('dougal');
  });

  it('brings the picture, the handover and the status line’s talk to the screener at Full only', () => {
    const v = view('full');
    v.show.rundown.push({ at: '10:03', title: 'Handover', detail: 'Finlay on air beside Eilidh', hard_post: false, key: '⇧⏎' });
    studio.view = v;
    const full = render(DeskB).body;
    expect(part(full, 'desk.tile.5')).toMatch(/data-handover="on"/);
    expect(part(full, 'desk.status')).toContain('Talk to the screener');
    expect(part(full, 'desk.picture')).not.toContain('offtag');
    // A DJ left in the room below Full keeps a tile, and no handover button: the handover is off.
    const callers = desk('callers', { sources: [EILIDH, FINLAY] });
    expect(part(callers, 'desk.tile.5')).not.toContain('data-handover');
    expect(part(callers, 'desk.status')).not.toContain('Talk to the screener');
    expect(part(callers, 'desk.status')).toMatch(/Talk <b>A<\/b>–<b>H<\/b> held, not latched/);
    // At Solo the status line has no talk rule at all: there is nobody to talk to.
    expect(part(desk('solo'), 'desk.status')).not.toContain('held, not latched');
    expect(part(desk('solo'), 'desk.status')).toContain('Room <b>LiveKit</b>');
  });
});

describe('desk.chat, to read, at every step', () => {
  const msg = (id: number, name: string, body: string, over: Partial<Msg> = {}): Msg => ({
    id,
    kind: 'chat',
    body,
    author: { id: `a-${id}`, name, standing: 'member' as Msg['author']['standing'], guest: false },
    reply_to: null,
    server_ms: NOW - 1000 * (10 - id),
    playhead_ms: null,
    state: 'visible',
    pinned: false,
    featured: false,
    reactions: [],
    ...over,
  });

  it('shows the room’s last messages with its head saying it is to read', () => {
    chat.link = 'live';
    chat.room = { lines: [msg(1, 'wee_maisie', 'Morning Eilidh! Kettle’s on here too'), msg(2, 'Blair_K', 'Leith Walk at eight was a car park again')], hidden: {}, featuredAt: {}, pinned: null };
    for (const s of STEPS) {
      const c = part(desk(s), 'desk.chat');
      expect(c, s).toMatch(/<h2>Chat<\/h2>[\s\S]*<span class="small">to read<\/span>/);
      expect(c, s).toContain('<b>wee_maisie</b> Morning Eilidh! Kettle’s on here too');
      expect(c, s).toContain('<b>Blair_K</b> Leith Walk at eight was a car park again');
      expect(c, s).not.toContain('The chat is on the Screener tab');
    }
  });

  it('leaves out a hidden or held message, and keeps to the last few', () => {
    chat.link = 'live';
    const lines = Array.from({ length: 12 }, (_, i) => msg(i + 1, `chatter${i + 1}`, `line ${i + 1}`));
    lines[10] = msg(11, 'chatter11', 'held back', { state: 'held' });
    chat.room = { lines, hidden: { 12: { by: 'you', why: 'spam' } as never }, featuredAt: {}, pinned: null };
    const c = part(desk('solo'), 'desk.chat');
    expect(c).not.toContain('held back');
    expect(c).not.toContain('line 12');
    expect(c).toContain('line 10');
    expect(c).not.toContain('line 1<');
    expect([...c.matchAll(/class="msg"/g)].length).toBeLessThanOrEqual(8);
  });

  it('says in one line when a moderator is needed, and where to go from this step', () => {
    const body = desk('solo', { chat: { ...view('solo').chat, held: 2, open_reports: 1 } });
    const solo = part(body, 'desk.chat');
    expect(solo).toMatch(/<div class="needs" role="status"><b>1 report, 2 held messages<\/b> need a moderator: Show everything, then the Screener tab\.<\/div>/);
    expect(body).toMatch(/class="panel chat[^"]*has-needs[^"]*" data-panel="desk\.chat"/);
    const full = part(desk('full', { chat: { ...view('full').chat, held: 0, open_reports: 2 } }), 'desk.chat');
    expect(full).toContain('<b>2 reports</b> need a moderator, on the Screener tab.');
    // Nothing waiting, or not counted: no line.
    expect(part(desk('solo'), 'desk.chat')).not.toContain('class="needs"');
    expect(part(desk('solo', { chat: { ...view('solo').chat, held: null, open_reports: null } }), 'desk.chat')).not.toContain('class="needs"');
  });

  it('says what the link is doing while there is nothing to read', () => {
    chat.link = 'connecting';
    expect(part(desk('solo'), 'desk.chat')).toContain('Connecting to the chat…');
    chat.link = 'none';
    expect(part(desk('solo'), 'desk.chat')).toContain('This station has no chat.');
    chat.link = 'lost';
    chat.why = 'the chat service is restarting';
    expect(part(desk('solo'), 'desk.chat')).toContain('The chat is not connected: the chat service is restarting. What shows may be out of date.');
    chat.link = 'live';
    expect(part(desk('solo'), 'desk.chat')).toContain('Nobody has written yet.');
  });
});

describe('a hidden part’s key', () => {
  it('says so once a session, in the toast slot, without a tone colour, with Step… and Dismiss', async () => {
    studio.view = view('solo');
    tellOff('dump');
    expect(studio.offNote).toMatchObject({ title: 'Dump is off at Solo', message: 'This show runs at Solo, so there is nothing for Dump to do. It comes on at Callers.' });
    const body = render(DeskB).body;
    const toast = part(body, 'desk.toast');
    expect(body).toMatch(/class="toast plain" data-panel="desk\.toast"/);
    expect(toast).toContain('role="status"');
    expect(toast).toContain('Dump is off at Solo');
    expect(toast).toMatch(/<button[^>]*>Step…[\s\S]*?<\/button>/);
    expect(toast).toMatch(/Dismiss[\s\S]{0,40}<kbd>Esc<\/kbd>/);
    // Esc dismisses it, and the second press of the same key says nothing more.
    await run({ type: 'escape' });
    expect(studio.offNote).toBeNull();
    tellOff('dump');
    expect(studio.offNote).toBeNull();
    // Another part's first press still speaks.
    tellOff('handover');
    expect(studio.offNote?.title).toBe('The handover is off at Solo');
  });

  it('is not counted as the first press away from the desk, where there is no slot to say it in', () => {
    studio.view = view('solo');
    studio.tab = 'carts';
    tellOff('dump');
    expect(studio.offNote).toBeNull();
    studio.tab = 'desk';
    tellOff('dump');
    expect(studio.offNote?.title).toBe('Dump is off at Solo');
  });
});
