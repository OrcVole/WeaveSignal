// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Prepare mode (ADR-0276): who starts preparing, what the frame, its band and its dialog say,
// that a preparing window's keys and buttons change nothing on air, and that the station's own
// sentence is shown when it refuses a window that thought it was live.
import { render } from 'svelte/server';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { KeyEngine, type Action, type KeyInput, type Tab } from '../../../packages/ui/keymap.ts';
import { PANELS } from '../../../packages/ui/panels.ts';
import App from './App.svelte';
import DeskB from './desk/DeskB.svelte';
import CartWall from './carts/CartWall.svelte';
import Screener from './screener/Screener.svelte';
import PrepareControl from './lib/PrepareControl.svelte';
import * as api from './lib/api.ts';
import { account } from './lib/account.svelte.ts';
import { run, confirmAir, pressMode } from './lib/actions.ts';
import { prepare, preparing, setPreparing, settleStart, switchToLive } from './lib/prepare.svelte.ts';
import {
  OFF_WHY,
  PREPARING_REFUSAL,
  canPrepare,
  cartShowFor,
  changesAir,
  liveAsks,
  onAirFor,
  prepareBand,
  prepareButtons,
  prepareChanges,
  prepareLabel,
  startsPreparing,
} from './lib/prepare.ts';
import { studio } from './lib/studio.svelte.ts';
import { bankAt, cartAt, stationBankUnder, tabOf, type Bank, type Cart, type Catalogue } from '../../../packages/ui/carts.ts';
import { setup } from './lib/wizard.svelte.ts';
import { chat } from './lib/chat.svelte.ts';
import type { Show } from './lib/schedule.ts';
import type { Caller, DeskPerson, Me, Source, StepView, View } from './lib/api.ts';

vi.mock('./lib/api.ts', async (orig) => ({
  ...(await orig<typeof api>()),
  desk: vi.fn(),
  carts: vi.fn(),
  screener: vi.fn(),
  endPass: vi.fn(),
  endShow: vi.fn(),
  setMode: vi.fn(),
  setPreparing: vi.fn(),
  whoAmI: vi.fn(),
}));
const ON_AIR_CALLS = [api.desk, api.carts, api.screener, api.endPass, api.endShow, api.setMode] as const;
const sent = () => ON_AIR_CALLS.reduce((n, f) => n + vi.mocked(f).mock.calls.length, 0);

const NOW = Date.UTC(2026, 10, 11, 9, 57, 12);
const KETTLE = { slug: 'the-morning-kettle', name: 'The Morning Kettle' };
const STEEPED = { slug: 'steeped', name: 'Steeped' };

/** A member of staff; `'unsaid'` is a control plane too old to report `preparing`. */
const staff = (id: string, name: string, role: Me['role'], preparing: boolean | 'unsaid' = false): Me => ({
  kind: 'staff',
  role,
  account: { id, username: id, name, role, disabled_ms: null, created_ms: 0 },
  ...(preparing === 'unsaid' ? {} : { preparing }),
});
/** The next DJ: Finlay presents Steeped, which follows The Morning Kettle. */
const FINLAY = staff('acc-finlay', 'Finlay Grant', 'presenter');
const EILIDH_ME = staff('acc-eilidh', 'Eilidh Fraser-MacLeod', 'presenter');
const MOIRA = staff('acc-moira', 'Moira Campbell', 'producer');
const KEY: Me = { kind: 'station', role: 'admin', preparing: false };

const show = (ref: { slug: string; name: string }, presenter_account: string | null): Show => ({
  id: ref.slug,
  ...ref,
  description: '',
  colour_slot: 1,
  layout: 'poster',
  slot: null,
  hosts: [],
  tags: [],
  explicit: false,
  feed_guid: null,
  cards: null,
  presenter: '',
  contact: '',
  links: [],
  commercial_music: false,
  presenter_account,
  step: 'full',
});
const SHOWS = [show(KETTLE, 'acc-eilidh'), show(STEEPED, 'acc-finlay')];

const stepOf = (on: { slug: string; name: string } | null, on_air = true): StepView => ({ step: 'full', show: on, on_air: !!on && on_air, next: null, may_change: false });

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
const NIALL: Caller = {
  identity: 'niall',
  name: 'Niall Anderson',
  ordinal: 1,
  state: 'waiting',
  since_ms: NOW - 72_000,
  arrived_ms: NOW - 72_000,
  line: '',
  note: '',
  name_on_air: '',
  declined: null,
  banned_before: null,
  topic: 'The bus lanes on Leith Walk',
  consent: { at_ms: NOW - 72_000, version: '2026-11-01', by: 'caller' },
  dropped_ms: null,
  slot: null,
  on_air: false,
  cue: false,
  linked: true,
  present: true,
  lufs: null,
};

/** The Morning Kettle on air at Full: Eilidh presenting, Gregor a guest, Finlay standing by. */
function view(step: StepView = stepOf(KETTLE), over: Partial<View> = {}): View {
  return {
    station: 'Kettle FM',
    now_ms: NOW,
    show: { title: 'The Morning Kettle', rundown: [] },
    desk: { reachable: true, error: null, playout: { connected: true, for_ms: 14_000 }, delay: { delay_ms: 8000, target_ms: 8000, rebuilding: false, dumps: 0, dumped_at_ms: null } },
    seats: { presenter: 'eilidh', screener: 'shona', presenter_name: 'Eilidh Fraser-MacLeod', screener_name: 'Shona Murray' },
    sources: [src(1, 'eilidh', 'Eilidh Fraser-MacLeod', 'presenter'), src(2, 'gregor', 'Gregor Stewart', 'guest'), src(5, 'finlay', 'Finlay Grant', 'dj', false)],
    callers: [NIALL],
    desk_taking_callers: true,
    chat: { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false, influx: false, influx_since_ms: null, held: 0, open_reports: 0 },
    notices: [],
    carts: { version: 0, desk: true, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
    listener_delay: { ms: 15_000, measured: false },
    mode: 'live',
    step,
    ...over,
  };
}

/** One panel's markup, from its name to the next panel's. */
function part(body: string, name: string): string {
  const at = body.indexOf(`data-panel="${name}"`);
  expect(at, name).toBeGreaterThan(-1);
  const next = body.indexOf('data-panel="', at + 1);
  return body.slice(at, next < 0 ? undefined : next);
}
/** Every button of a surface: its opening tag and its words. The dialogs (the invitation, the
 *  air's) are left out: they are closed, and a dialog's own buttons are its own. */
const buttons = (body: string): { tag: string; text: string }[] =>
  [...body.replace(/<dialog[\s\S]*?<\/dialog>/g, '').matchAll(/<button\b([^>]*)>([\s\S]*?)<\/button>/g)].map((m) => ({
    tag: m[1],
    text: m[2].replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim(),
  }));
/** The band's words: its button, then what it says. */
function bandText(body: string): string {
  const band = part(body, 'studio.prepare.banner');
  return text(`<i ${band.slice(0, band.indexOf('</div>'))}`);
}
const off = (b: { tag: string }): boolean => /\sdisabled(=""|\s|$)/.test(b.tag);
const text = (html: string): string => html.replace(/<!--[\s\S]*?-->/g, '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();

function as(me: Me | null, shows: Show[] = SHOWS): void {
  account.me = me;
  account.loaded = true;
  setup.shows = shows;
  setup.loaded = true;
  setup.error = '';
}

beforeEach(() => {
  for (const f of [...ON_AIR_CALLS, api.setPreparing, api.whoAmI]) vi.mocked(f).mockReset();
  as(FINLAY);
  studio.view = view();
  studio.tab = 'desk';
  studio.settings = false;
  studio.showAll = false;
  studio.note = null;
  studio.offNote = null;
  studio.armed = null;
  studio.airAsk = null;
  studio.handoverTo = null;
  studio.selected = null;
  studio.zone = '';
  studio.now = NOW;
  studio.skew = 0;
  prepare.menu = false;
  prepare.pick = null;
  prepare.busy = false;
  prepare.problem = '';
  prepare.settled = false;
  prepare.nudgedAt = 0;
  chat.link = 'off';
});
afterEach(() => vi.unstubAllGlobals());

describe('who starts preparing', () => {
  it('is the presenter of a show that is not on air now', () => {
    // Someone else's show is on air; no show is on air; the show last on air was another's.
    expect(startsPreparing(FINLAY, SHOWS, stepOf(KETTLE))).toBe(true);
    expect(startsPreparing(FINLAY, SHOWS, stepOf(null))).toBe(true);
    expect(startsPreparing(FINLAY, SHOWS, stepOf(STEEPED, false))).toBe(true);
    expect(startsPreparing(FINLAY, SHOWS, null)).toBe(true);
  });

  it('is nobody else: the presenter whose show is on air, staff with no show, the station key, an older station', () => {
    expect(startsPreparing(EILIDH_ME, SHOWS, stepOf(KETTLE))).toBe(false);
    expect(startsPreparing(MOIRA, SHOWS, stepOf(KETTLE))).toBe(false);
    expect(startsPreparing(KEY, SHOWS, stepOf(KETTLE))).toBe(false);
    expect(startsPreparing(null, SHOWS, stepOf(KETTLE))).toBe(false);
    // A control plane older than Prepare mode says nothing of it: there is no route to ask.
    expect(startsPreparing(staff('acc-finlay', 'Finlay Grant', 'presenter', 'unsaid'), SHOWS, stepOf(KETTLE))).toBe(false);
  });

  it('looks at every show the account presents, and exempts no role', () => {
    const two = [...SHOWS, show({ slug: 'late-steeped', name: 'Late Steeped' }, 'acc-finlay')];
    expect(startsPreparing(FINLAY, two, stepOf({ slug: 'late-steeped', name: 'Late Steeped' }))).toBe(false);
    expect(startsPreparing(FINLAY, two, stepOf(KETTLE))).toBe(true);
    // An admin preparing their own show prepares like anyone else (ADR-0276).
    const admin = staff('acc-finlay', 'Finlay Grant', 'admin');
    expect(startsPreparing(admin, SHOWS, stepOf(KETTLE))).toBe(true);
  });

  it('says whose show is on air, and asks before Live only while it is someone else’s', () => {
    expect(onAirFor(FINLAY, SHOWS, stepOf(KETTLE))).toEqual({ whose: 'other', name: 'The Morning Kettle' });
    expect(onAirFor(FINLAY, SHOWS, stepOf(STEEPED))).toEqual({ whose: 'mine', name: 'Steeped' });
    expect(onAirFor(FINLAY, SHOWS, stepOf(STEEPED, false))).toEqual({ whose: 'none' });
    expect(onAirFor(MOIRA, SHOWS, stepOf(KETTLE))).toEqual({ whose: 'other', name: 'The Morning Kettle' });
    expect(liveAsks({ whose: 'other', name: 'The Morning Kettle' })).toBe(true);
    expect(liveAsks({ whose: 'mine', name: 'Steeped' })).toBe(false);
    expect(liveAsks({ whose: 'none' })).toBe(false);
  });

  it('settles the start once the station has said who this is, which shows there are and what is on air', async () => {
    vi.mocked(api.setPreparing).mockResolvedValue({ ...FINLAY, preparing: true });
    // Not before the shows and the first view are in.
    setup.loaded = false;
    settleStart();
    studio.view = null;
    setup.loaded = true;
    settleStart();
    expect(api.setPreparing).not.toHaveBeenCalled();
    expect(prepare.settled).toBe(false);
    studio.view = view();
    settleStart();
    expect(prepare.settled).toBe(true);
    await vi.waitFor(() => expect(api.setPreparing).toHaveBeenCalledExactlyOnceWith(true));
    await vi.waitFor(() => expect(preparing()).toBe(true));
    // Once: a later view, or the wizard's new show, changes nothing.
    settleStart();
    expect(api.setPreparing).toHaveBeenCalledTimes(1);
  });

  it('says so, in the station’s words, when the start’s switch is refused, and tries again at the next start', async () => {
    const kept = new Map<string, string>();
    vi.stubGlobal('sessionStorage', { getItem: (k: string) => kept.get(k) ?? null, setItem: (k: string, v: string) => void kept.set(k, v), removeItem: (k: string) => void kept.delete(k) });
    vi.mocked(api.setPreparing).mockRejectedValue(new api.FieldError(503, 'The station is restarting.', null));
    settleStart();
    await vi.waitFor(() => expect(studio.note?.title).toBe('This window could not start preparing'));
    expect(studio.note?.message).toBe('The station is restarting. It is live: its keys and buttons change what is on air.');
    expect(preparing()).toBe(false);
    expect(kept.size).toBe(0);
  });

  it('never switches a window to Live by itself, and stays as it is when the shows cannot be read', () => {
    // The sign-in was left preparing, and its show is on air now: the band says so; it does not switch.
    as({ ...FINLAY, preparing: true });
    studio.view = view(stepOf(STEEPED));
    settleStart();
    expect(prepare.settled).toBe(true);
    expect(api.setPreparing).not.toHaveBeenCalled();
    // The shows could not be read: who presents what is not known, so the window stays live.
    prepare.settled = false;
    as(FINLAY);
    setup.error = 'The database is not answering.';
    studio.view = view();
    settleStart();
    expect(prepare.settled).toBe(true);
    expect(api.setPreparing).not.toHaveBeenCalled();
    // Everyone else: live, as before.
    for (const me of [EILIDH_ME, MOIRA, KEY, null]) {
      prepare.settled = false;
      as(me);
      settleStart();
      expect(prepare.settled).toBe(true);
    }
    expect(api.setPreparing).not.toHaveBeenCalled();
  });

  it('does not apply the default again after a reload of a window that has settled it', async () => {
    const kept = new Map<string, string>();
    vi.stubGlobal('sessionStorage', { getItem: (k: string) => kept.get(k) ?? null, setItem: (k: string, v: string) => void kept.set(k, v), removeItem: (k: string) => void kept.delete(k) });
    // The next DJ switched to Live by hand; the window reloads; the station still says live.
    vi.mocked(api.setPreparing).mockResolvedValue({ ...FINLAY, preparing: false });
    as({ ...FINLAY, preparing: true });
    prepare.settled = true;
    expect(await setPreparing(false)).toBe(true);
    expect([...kept.values()]).toEqual(['acc-finlay']); // the account's id, and nothing secret
    vi.mocked(api.setPreparing).mockClear();
    prepare.settled = false;
    settleStart();
    expect(prepare.settled).toBe(true);
    expect(api.setPreparing).not.toHaveBeenCalled();
    // A new sign-in forgets it, so the default applies to it.
    api.settlePrepare(null);
    prepare.settled = false;
    vi.mocked(api.setPreparing).mockResolvedValue({ ...FINLAY, preparing: true });
    settleStart();
    await vi.waitFor(() => expect(api.setPreparing).toHaveBeenCalledExactlyOnceWith(true));
  });
});

describe('the frame', () => {
  it('names the three panels in the registry', () => {
    for (const name of ['studio.prepare', 'studio.prepare.menu', 'studio.prepare.banner']) expect(PANELS[name], name).toBeTruthy();
  });

  it('shows a live window as Live, after who is signed in, with the way to change it and no band', () => {
    const body = render(App).body;
    expect(body.indexOf('data-panel="studio.account"')).toBeLessThan(body.indexOf('data-panel="studio.prepare"'));
    const ctl = part(body, 'studio.prepare');
    expect(ctl).toContain('aria-label="This window: Live. Change it"');
    expect(ctl).toContain('title="This window is live: it drives the desk"');
    expect(ctl).toContain('aria-haspopup="dialog"');
    expect(ctl).toMatch(/<span class="word">Live<\/span>/);
    // Not a state chip, and never the red of ON AIR.
    expect(ctl).not.toMatch(/class="state|fill-onair|lit-onair|hollow-/);
    expect(body).not.toContain('data-panel="studio.prepare.banner"');
    expect(body).not.toMatch(/class="shell[^"]*with-prepare/);
  });

  it('shows a preparing window as Preparing, with a band that says so and holds the way to Live', () => {
    as({ ...FINLAY, preparing: true });
    const body = render(App).body;
    const ctl = part(body, 'studio.prepare');
    expect(ctl).toContain('aria-label="This window: Preparing. Change it"');
    expect(ctl).toContain('title="This window is preparing: it shows the live desk and cannot change what is on air"');
    expect(ctl).toMatch(/<span class="word">Preparing<\/span>/);
    expect(bandText(body)).toBe(
      'Switch to Live… This window is preparing. The Morning Kettle is on air: follow it here. Nothing in this window changes what is on air; the Schedule tab and Settings work as usual.',
    );
    // The band is a row of its own, between the frame and the surface.
    expect(body).toMatch(/class="shell[^"]*with-prepare[^"]*" style="grid-template-rows: auto auto minmax\(0, 1fr\);"/);
    expect(body.indexOf('data-panel="studio.prepare.banner"')).toBeLessThan(body.indexOf('data-panel="desk.clock"'));
  });

  it('words the band by what is on air: someone else’s show, your own, or none', () => {
    expect(prepareBand({ whose: 'other', name: 'The Morning Kettle' })).toMatchObject({ lead: 'This window is preparing.', button: 'Switch to Live…', primary: false });
    expect(prepareBand({ whose: 'mine', name: 'Steeped' })).toEqual({
      lead: 'Steeped is on air, and this window is still preparing.',
      rest: 'Switch it to Live to drive the desk.',
      button: 'Switch to Live',
      primary: true,
    });
    expect(prepareBand({ whose: 'none' })).toEqual({
      lead: 'This window is preparing.',
      rest: 'It shows the live desk. Nothing in it changes what is on air; the Schedule tab and Settings work as usual.',
      button: 'Switch to Live',
      primary: false,
    });
    as({ ...FINLAY, preparing: true });
    studio.view = view(stepOf(STEEPED));
    expect(part(render(App).body, 'studio.prepare.banner')).toMatch(/class="btn primary"[^>]*>Switch to Live</);
  });

  it('draws the band on the tabs that drive the air, and not over the Schedule tab or Settings', () => {
    as({ ...FINLAY, preparing: true });
    for (const tab of ['desk', 'screener', 'carts'] as const) {
      studio.tab = tab;
      expect(render(App).body, tab).toContain('data-panel="studio.prepare.banner"');
    }
    studio.tab = 'schedule';
    expect(render(App).body).not.toContain('data-panel="studio.prepare.banner"');
    studio.tab = 'desk';
    studio.settings = true;
    expect(render(App).body).not.toContain('data-panel="studio.prepare.banner"');
    // The frame still says Preparing there.
    expect(part(render(App).body, 'studio.prepare')).toMatch(/<span class="word">Preparing<\/span>/);
  });

  it('draws no switch for the station key, or on a station older than Prepare mode', () => {
    for (const me of [KEY, null, staff('acc-finlay', 'Finlay Grant', 'presenter', 'unsaid')]) {
      as(me);
      expect(canPrepare(me)).toBe(false);
      const body = render(App).body;
      expect(body).not.toContain('data-panel="studio.prepare"');
      expect(body).not.toContain('data-panel="studio.prepare.banner"');
    }
  });

  it('lists the two choices in the dialog, the one in force marked now, in plain words', () => {
    const menu = part(render(PrepareControl).body, 'studio.prepare.menu');
    expect(menu).toContain('This window</h2>');
    expect(menu).toContain('A studio window is Live or Preparing. Only a live window can change what is on air');
    expect([...menu.matchAll(/<input[^>]*data-window="(\w+)"/g)].map((m) => m[1])).toEqual(['live', 'preparing']);
    const rows = menu.split('<label').slice(1);
    expect(rows.map((r) => r.includes('class="nowtag"'))).toEqual([true, false]);
    expect(text(`<i ${rows[0]}`)).toContain('Live Drives the desk: its keys and buttons change what is on air now');
    expect(text(`<i ${rows[1]}`)).toContain('Preparing Shows the live desk without driving it, while you set up your own show');
    expect(prepareLabel('live')).toBe('This window: Live. Change it');
  });

  it('says what a switch changes before it is made, and asks once before taking the desk from another show', () => {
    const toPreparing = prepareChanges('preparing', { whose: 'other', name: 'The Morning Kettle' });
    expect(toPreparing.heading).toBe('Switching to Preparing:');
    expect(toPreparing.ask).toBe(false);
    expect(toPreparing.lines.join(' ')).toContain('no key or button in this window changes what is on air');
    const ask = prepareChanges('live', { whose: 'other', name: 'The Morning Kettle' });
    expect(ask.ask).toBe(true);
    expect(ask.lines[0]).toBe('The Morning Kettle is on air. This window’s keys and buttons will act on it at once: they put people on air, mute them, fire carts and end the show.');
    expect(prepareChanges('live', { whose: 'mine', name: 'Steeped' })).toMatchObject({ ask: false, lines: ['Steeped is on air. This window’s keys and buttons drive the desk.'] });
    expect(prepareChanges('live', { whose: 'none' }).ask).toBe(false);
    // The way out keeps what is; the primary names the move.
    expect(prepareButtons('preparing')).toEqual({ keep: 'Keep preparing', move: 'Switch to Live' });
    expect(prepareButtons('live')).toEqual({ keep: 'Stay live', move: 'Switch to Preparing' });
    // Never "invite" as a noun.
    expect(JSON.stringify([toPreparing, ask])).not.toMatch(/\binvites?\b/);
  });

  it('opens the ask from the band while someone else’s show is on air, and switches at once otherwise', async () => {
    as({ ...FINLAY, preparing: true });
    switchToLive();
    expect(prepare.menu).toBe(true);
    expect(prepare.pick).toBe('live');
    expect(api.setPreparing).not.toHaveBeenCalled();
    // Finlay's own show is on air: nobody to take the desk from.
    prepare.menu = false;
    studio.view = view(stepOf(STEEPED));
    vi.mocked(api.setPreparing).mockResolvedValue({ ...FINLAY, preparing: false });
    switchToLive();
    expect(prepare.menu).toBe(false);
    await vi.waitFor(() => expect(api.setPreparing).toHaveBeenCalledExactlyOnceWith(false));
    await vi.waitFor(() => expect(preparing()).toBe(false));
  });

  it('keeps the window as it was, with the station’s sentence, when the station refuses the switch', async () => {
    vi.mocked(api.setPreparing).mockRejectedValue(new api.FieldError(422, 'The station key is always live.', null));
    expect(await setPreparing(true)).toBe(false);
    expect(preparing()).toBe(false);
    expect(prepare.problem).toBe('The station key is always live.');
  });

  it('lets held keys go and disarms before it prepares', async () => {
    const order: string[] = [];
    const { onBeforePreparing } = await import('./lib/prepare.svelte.ts');
    onBeforePreparing(async () => void order.push('let go'));
    vi.mocked(api.setPreparing).mockImplementation(async () => (order.push('put'), { ...FINLAY, preparing: true }));
    studio.armed = { what: 'dump', at: NOW };
    studio.handoverTo = 'finlay';
    studio.airAsk = 'end';
    expect(await setPreparing(true)).toBe(true);
    expect(order).toEqual(['let go', 'put']);
    expect([studio.armed, studio.handoverTo, studio.airAsk]).toEqual([null, null, null]);
    onBeforePreparing(null);
  });
});

describe('a preparing window', () => {
  const press = (code: string, mods: Partial<KeyInput> = {}): KeyInput => ({ code, ctrl: false, shift: false, alt: false, meta: false, repeat: false, ...mods });
  /** What the keymap makes of a key on a tab: the action the frame would run. */
  function actionsOf(keys: KeyInput[], tab: Tab): Action[] {
    const engine = new KeyEngine();
    const out: Action[] = [];
    for (const k of keys) {
      // Twice, so a two-press key both arms and fires.
      for (let i = 0; i < 2; i++) {
        const o = engine.down(k, { tab, inTextField: false }, NOW + i);
        if (o.action) out.push(o.action);
        const up = engine.up({ code: k.code });
        if (up.action) out.push(up.action);
      }
    }
    return out;
  }
  const DESK_KEYS = [
    ...['Digit1', 'Digit2', 'Digit5', 'KeyQ', 'KeyW', 'KeyA', 'KeyS', 'KeyZ', 'KeyX', 'KeyP'].map((c) => press(c)),
    press('Enter', { shift: true }), // the handover
    ...['F1', 'F6', 'F7', 'Delete'].map((c) => press(c)), // carts, the picture, Dump
    press('KeyD', { ctrl: true }), // drop
    press('KeyE', { ctrl: true, shift: true }), // end the show
    press('KeyT', { ctrl: true }), // talk to the screener
    press('KeyR', { ctrl: true }), // restart playout
  ];

  it('runs none of the desk’s keys: nothing is sent, nothing arms, and the band answers', async () => {
    as({ ...FINLAY, preparing: true });
    const actions = actionsOf(DESK_KEYS, 'desk');
    expect(actions.length).toBeGreaterThanOrEqual(DESK_KEYS.length);
    for (const a of actions) {
      expect(changesAir(a), JSON.stringify(a)).toBe(true);
      prepare.nudgedAt = 0;
      await run(a);
      expect(prepare.nudgedAt, JSON.stringify(a)).toBeGreaterThan(0);
      expect(studio.armed, JSON.stringify(a)).toBeNull();
      expect(studio.airAsk, JSON.stringify(a)).toBeNull();
    }
    expect(sent()).toBe(0);
    expect(studio.note).toBeNull();
    // The same keys in a live window do reach the station.
    as(FINLAY);
    vi.mocked(api.desk).mockResolvedValue(view());
    await run({ type: 'mute', slot: 1 });
    expect(api.desk).toHaveBeenCalledWith({ op: 'mute', identity: 'eilidh', on: true });
  });

  it('runs none of the carts’ or the screener’s on-air keys either', async () => {
    as({ ...FINLAY, preparing: true });
    studio.selected = 'niall';
    const carts = actionsOf([press('Digit1'), press('KeyQ', { shift: true }), press('Space'), press('Space', { shift: true }), press('Enter')], 'carts');
    const screener = actionsOf([press('KeyT'), press('KeyP'), press('KeyW'), press('KeyC', { shift: true }), press('KeyD', { ctrl: true })], 'screener');
    for (const a of [...carts, ...screener]) {
      expect(changesAir(a), JSON.stringify(a)).toBe(true);
      await run(a);
    }
    await pressMode('auto');
    await confirmAir('end');
    expect(sent()).toBe(0);
    expect(studio.armed).toBeNull();
  });

  it('keeps what is the window’s own: the tabs, the bank, moving in a queue, the chat, and Esc', async () => {
    as({ ...FINLAY, preparing: true });
    const free: Action[] = [
      { type: 'tab', tab: 'schedule' },
      { type: 'bank', n: 2 },
      { type: 'queue-move', delta: 1 },
      { type: 'cart-move', dx: 1, dy: 0 },
      { type: 'mode', mode: 'slow' },
      { type: 'arm', what: 'influx' },
      { type: 'arm', what: 'ban' },
      { type: 'mod-hide' },
      { type: 'week', delta: 1 },
      { type: 'escape' },
    ];
    for (const a of free) expect(changesAir(a), JSON.stringify(a)).toBe(false);
    for (const what of ['dump', 'drop', 'fade-all', 'end-pass'] as const) expect(changesAir({ type: 'arm', what })).toBe(true);
    vi.stubGlobal('document', { documentElement: { dataset: {} }, activeElement: null, querySelector: () => null });
    await run({ type: 'tab', tab: 'schedule' });
    expect(studio.tab).toBe('schedule');
    // Esc dismisses this window's note, and does not decline the caller who is ready for the desk.
    studio.tab = 'desk';
    const ready: Source = { ...src(4, 'niall', 'Niall Anderson', 'caller', false), caller: { line: '', note: '', name_on_air: '', sent: true, taken: false } };
    studio.view = view(stepOf(KETTLE), { sources: [...view().sources, ready] });
    studio.note = { id: 1, tone: 'good', title: 'A note', message: 'of this window', until: NOW + 6000 };
    await run({ type: 'escape' });
    expect(studio.note).toBeNull();
    await run({ type: 'escape' });
    expect(sent()).toBe(0);
  });

  it('draws the live desk with every on-air control off, and says why', () => {
    // With the picture on and the handover's row in the rundown, so their controls are drawn too.
    const at = new Date(NOW + 60_000).toISOString().slice(11, 16);
    studio.zone = 'UTC';
    studio.view = view(stepOf(KETTLE), {
      show: { title: 'The Morning Kettle', rundown: [{ at, title: 'Steeped with Finlay Grant', detail: 'Next show', hard_post: false, key: '⇧⏎' }] },
      desk: { ...view().desk, picture: { mode: { mode: 'auto' }, shot: 'person', identity: 'eilidh', cameras: ['eilidh'], hls: true, room: true, still: true } },
    });
    as({ ...FINLAY, preparing: true });
    const live = (() => {
      as(FINLAY);
      return buttons(render(DeskB).body);
    })();
    as({ ...FINLAY, preparing: true });
    const body = render(DeskB).body;
    const all = buttons(body);
    // Nothing is taken away: the same buttons, in the same order.
    expect(all.map((b) => b.text)).toEqual(live.map((b) => b.text));
    const stays = (b: { text: string }) => /^(Invite someone…|Screener)/.test(b.text);
    for (const b of all) {
      expect(off(b), b.text).toBe(!stays(b));
      if (!stays(b)) expect(b.tag, b.text).toContain(`title="${OFF_WHY}"`);
    }
    for (const word of ['On 1', 'Cue Q', 'Talk A', 'Mute Z', 'End pass', 'Dump Del Del', 'End show ⌃⇧E', 'Hand to automation', 'Slate F7', 'Remove', 'Bring Finlay on air ⇧⏎']) {
      expect(all.some((b) => b.text.startsWith(word)), word).toBe(true);
    }
    // The handover is drawn twice, on the DJ's tile and in the rundown's row: both are off.
    expect(all.filter((b) => b.tag.includes('data-handover')).map(off)).toEqual([true, true]);
    // The slate's file chooser too.
    expect(body).toMatch(/<input[^>]*type="file"[^>]*\sdisabled/);
    // The states stay true: the show can be followed from here.
    expect(body).toContain('lit-onair');
    expect(text(part(body, 'desk.tile.1'))).toContain('On air');
    // A live window has none of them off for this reason.
    expect(live.some((b) => b.tag.includes(OFF_WHY))).toBe(false);
  });

  it('does not offer the presenter’s own microphone link, which joins on air', () => {
    const solo = view({ ...stepOf(KETTLE), step: 'solo' }, { sources: [], callers: [] });
    studio.view = solo;
    expect(render(DeskB).body).toContain('data-testid="own-link"');
    as({ ...FINLAY, preparing: true });
    expect(render(DeskB).body).not.toContain('data-testid="own-link"');
  });

  it('offers none of a toast’s on-air answers', () => {
    const ready: Source = { ...src(4, 'niall', 'Niall Anderson', 'caller', false), caller: { line: 'The bus lanes', note: '', name_on_air: '', sent: true, taken: false } };
    studio.view = view(stepOf(KETTLE), { sources: [...view().sources, ready] });
    expect(part(render(DeskB).body, 'desk.toast')).toContain('Cue Niall');
    as({ ...FINLAY, preparing: true });
    const toast = part(render(DeskB).body, 'desk.toast');
    expect(toast).toContain('Niall is ready for you');
    expect(toast).not.toContain('<button');
  });

  it('turns the Carts tab’s pads and Fade all off, and leaves the banks', () => {
    as({ ...FINLAY, preparing: true });
    const all = buttons(render(CartWall).body);
    const pads = all.filter((b) => b.tag.includes('data-cart-slot'));
    expect(pads).toHaveLength(24);
    expect(pads.every(off)).toBe(true);
    expect(off(all.find((b) => b.text.startsWith('Fade all'))!)).toBe(true);
    const banks = all.filter((b) => b.tag.includes('data-panel="carts.bank.'));
    expect(banks).toHaveLength(5);
    expect(banks.some(off)).toBe(false);
  });

  it('turns the Screener tab’s caller controls off, makes its notes read-only, and leaves the chat’s', () => {
    as({ ...FINLAY, preparing: true });
    studio.selected = 'niall';
    const body = render(Screener).body;
    const all = buttons(body);
    for (const word of ['Talk to Eilidh', 'Talk · hold', 'Send to desk', 'To queue', 'Drop']) {
      const b = all.find((x) => x.text.startsWith(word));
      expect(b, word).toBeTruthy();
      expect(off(b!), word).toBe(true);
    }
    expect([...body.matchAll(/<(?:input|textarea)[^>]*id="f-(?:line|note|name)"[^>]*>/g)].map((m) => /\sreadonly/.test(m[0]))).toEqual([true, true, true]);
    // The chat's modes are the chat's own, and stay.
    for (const word of ['Slow', 'Hold new accounts', 'Influx']) expect(off(all.find((x) => x.text.startsWith(word))!), word).toBe(false);
  });
});

describe('when the station refuses anyway', () => {
  it('shows the station’s sentence, and follows the station to Preparing', async () => {
    // The window thinks it is live; the same sign-in was switched to Preparing elsewhere.
    vi.mocked(api.desk).mockRejectedValue(new api.ApiError(409, PREPARING_REFUSAL));
    vi.mocked(api.whoAmI).mockResolvedValue({ ...FINLAY, preparing: true });
    expect(preparing()).toBe(false);
    await run({ type: 'on', slot: 2 });
    expect(api.desk).toHaveBeenCalledWith({ op: 'on_air', identity: 'gregor', on: false });
    expect(studio.note).toMatchObject({ tone: 'warning', title: 'Not done', message: 'This window is preparing a show, so it cannot change what is on air. Switch it to Live first.' });
    // The sentence is in the desk's toast slot, as given.
    const toast = part(render(DeskB).body, 'desk.toast');
    expect(toast).toContain('This window is preparing a show, so it cannot change what is on air. Switch it to Live first.');
    // And the frame follows the station.
    await vi.waitFor(() => expect(preparing()).toBe(true));
    const body = render(App).body;
    expect(part(body, 'studio.prepare')).toMatch(/<span class="word">Preparing<\/span>/);
    expect(body).toContain('data-panel="studio.prepare.banner"');
  });

  it('shows the same sentence when End show or the station’s mode is refused', async () => {
    vi.mocked(api.endShow).mockRejectedValue(new api.ApiError(409, PREPARING_REFUSAL));
    vi.mocked(api.whoAmI).mockResolvedValue({ ...FINLAY, preparing: true });
    await confirmAir('end');
    expect(studio.note?.message).toBe(PREPARING_REFUSAL);
    await vi.waitFor(() => expect(preparing()).toBe(true));
  });

  it('asks who it is again only for a 409, and shows any other refusal as before', async () => {
    vi.mocked(api.desk).mockRejectedValue(new api.ApiError(503, 'The desk is not answering.'));
    await run({ type: 'mute', slot: 1 });
    expect(studio.note?.message).toBe('The desk is not answering.');
    expect(api.whoAmI).not.toHaveBeenCalled();
  });
});

// ---- a cart wall per show (1.0.2) ----

const bankOf = (number: number, name: string, show: string | null): Bank => ({ number, name, colour_slot: number, show });
const cartOf = (bank: number, slot: number, label: string, show: string | null): Cart => ({
  id: `${show ?? 'station'}-${bank}-${slot}`,
  bank,
  slot,
  label,
  detail: '',
  mode: 'one_shot',
  file: 'x.flac',
  duration_ms: 1000,
  gain_db: 0,
  loudness_lufs: null,
  true_peak_dbtp: null,
  ramp_ms: null,
  origin: '',
  licence: '',
  missing: false,
  show,
});
/** Steeped's window: the station's banks 1 and 2, and Steeped's own bank 2 and 3. */
const STEEPED_CAT: Catalogue = {
  version: 3,
  show: 'steeped',
  banks: [bankOf(1, 'Reactions', null), bankOf(2, 'Idents', null), bankOf(2, 'Dog woofs', 'steeped'), bankOf(3, 'Beds', 'steeped')],
  carts: [cartOf(1, 1, 'Applause', null), cartOf(2, 1, 'Top of the hour', null), cartOf(2, 1, 'Woof', 'steeped'), cartOf(3, 2, 'Rain bed', 'steeped')],
  loudness_lufs: -16,
};

describe('a cart wall per show', () => {
  it('works with the show on air in a live window, and the show it prepares in a preparing one', () => {
    expect(cartShowFor(false, FINLAY, SHOWS, stepOf(KETTLE))).toBe('the-morning-kettle');
    // Between shows, the show the step comes from.
    expect(cartShowFor(false, FINLAY, SHOWS, stepOf(KETTLE, false))).toBe('the-morning-kettle');
    expect(cartShowFor(false, FINLAY, SHOWS, stepOf(null))).toBe(null);
    // Preparing: Finlay's own show, whatever is on air.
    expect(cartShowFor(true, FINLAY, SHOWS, stepOf(KETTLE))).toBe('steeped');
    // Of two shows, the one that comes on next.
    const two = [...SHOWS, show({ slug: 'late-steep', name: 'The Late Steep' }, 'acc-finlay')];
    const next = { ...stepOf(KETTLE), next: { slug: 'late-steep', name: 'The Late Steep', start_ms: NOW + 3_600_000, step: 'solo' as const } };
    expect(cartShowFor(true, FINLAY, two, next)).toBe('late-steep');
    expect(cartShowFor(true, FINLAY, two, stepOf(KETTLE))).toBe('steeped');
    // No show of their own: the station's banks alone.
    expect(cartShowFor(true, MOIRA, SHOWS, stepOf(KETTLE))).toBe(null);
  });

  it('puts the show’s own bank on its tab, else the station’s, and finds each cart’s tab', () => {
    expect(bankAt(STEEPED_CAT, 1)?.name).toBe('Reactions');
    expect(bankAt(STEEPED_CAT, 2)?.name).toBe('Dog woofs');
    expect(bankAt(STEEPED_CAT, 3)?.name).toBe('Beds');
    expect(bankAt(STEEPED_CAT, 4)).toBe(null);
    expect(stationBankUnder(STEEPED_CAT, 2)?.name).toBe('Idents');
    expect(stationBankUnder(STEEPED_CAT, 1)).toBe(null);
    expect(cartAt(STEEPED_CAT, 2, 1)?.label).toBe('Woof');
    expect(cartAt(STEEPED_CAT, 1, 1)?.label).toBe('Applause');
    // The station's bank 2 is not on a tab in Steeped's window.
    expect(tabOf(STEEPED_CAT, STEEPED_CAT.carts[1])).toBe(null);
    expect(tabOf(STEEPED_CAT, STEEPED_CAT.carts[2])).toBe(2);
    // A catalogue from a station older than 1.0.2 has no owners: every bank is the station's.
    const old: Catalogue = { version: 1, banks: [{ number: 1, name: 'Reactions', colour_slot: 1 }], carts: [{ ...cartOf(1, 1, 'Applause', null), show: undefined }], loudness_lufs: -16 };
    expect(cartAt(old, 1, 1)?.label).toBe('Applause');
  });

  it('labels each tab of the strip as the station’s or the show’s, and keeps the bank keys', () => {
    as({ ...FINLAY, preparing: true });
    studio.catalogue = STEEPED_CAT;
    studio.cartShow = 'steeped';
    studio.bank = 2;
    const body = render(CartWall).body;
    const tab = (n: number) => text(part(body, `carts.bank.${n}`).slice(0, part(body, `carts.bank.${n}`).indexOf('</button>')));
    expect(tab(1)).toContain('Reactions station 1/24');
    expect(tab(2)).toContain('Dog woofs show 1/24');
    expect(tab(3)).toContain('Beds show 1/24');
    expect(tab(4)).toContain('Bank 4 not made');
    expect(part(body, 'carts.bank.2')).toContain('Steeped’s own bank: in its windows it takes the place of the station’s bank 2, Idents');
    expect(part(body, 'carts.bank.1')).toContain('The station’s bank, shared by every show');
    expect(body).toContain('aria-label="Cart banks: the station’s, and Steeped’s own"');
    // The wall shows tab 2's bank: Steeped's Woof, not the station's Top of the hour.
    const grid = body.slice(body.indexOf('class="wall"'), body.indexOf('data-panel="carts.sounding"'));
    expect(grid).toContain('Woof');
    expect(grid).not.toContain('Top of the hour');
    studio.catalogue = null;
    studio.cartShow = null;
    studio.bank = 1;
  });

  it('names a sounding cart that this window’s banks do not hold', () => {
    as({ ...FINLAY, preparing: true });
    studio.catalogue = STEEPED_CAT;
    studio.cartShow = 'steeped';
    const sounding = { cart: 'the-morning-kettle-1-1', label: 'Miaow', pool: 'shot' as const, player: 1, mode: 'one_shot' as const, cue: false, play: 1, started_ms: NOW, ends_ms: NOW + 1000, stopping: false, seq: 1 };
    studio.view = view(stepOf(KETTLE), { carts: { ...view().carts, sounding: [sounding] } });
    expect(text(part(render(CartWall).body, 'carts.sounding'))).toContain('Miaow');
    studio.catalogue = null;
    studio.cartShow = null;
  });
});
