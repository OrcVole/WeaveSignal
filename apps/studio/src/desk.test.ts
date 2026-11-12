// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Desk B and the screener rendered on the server from a view: every panel is drawn under its
// registered name (ADR-0070), sources sit by their slot's keys, OPEN replaces ON AIR while
// playout is down (ADR-0062), and the screener marks DESK BUSY and BANNED BEFORE (ADR-0069).
import { render } from 'svelte/server';
import { beforeEach, describe, expect, it } from 'vitest';
import { CARTS, DESK_B, SCREENER, panel } from '../../../packages/ui/panels.ts';
import type { Cart, Catalogue } from '../../../packages/ui/carts.ts';
import DeskB from './desk/DeskB.svelte';
import CartWall from './carts/CartWall.svelte';
import Screener from './screener/Screener.svelte';
import { studio } from './lib/studio.svelte.ts';
import { theme } from './lib/theme.svelte.ts';
import type { DeskPerson, View } from './lib/api.ts';

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
  lufs_peak: -19.8,
});

function view(playoutUp: boolean): View {
  const now = Date.UTC(2026, 9, 25, 13, 57, 12);
  return {
    station: 'Kettle FM',
    now_ms: now,
    show: { title: 'The Morning Kettle', rundown: [{ at: '14:00', title: 'News', detail: 'hard post', hard_post: true, key: 'F2' }] },
    desk: { reachable: true, error: null, playout: { connected: playoutUp, for_ms: 14_000 } },
    seats: { presenter: 'eilidh', screener: 'moira', presenter_name: 'Eilidh Fraser-MacLeod', screener_name: 'Moira Campbell' },
    sources: [
      { ...person('eilidh', 'Eilidh Fraser-MacLeod', 'presenter', true), slot: 1, caller: null },
      { ...person('gregor', 'Gregor Stewart', 'guest', true), slot: 2, caller: null },
      {
        ...person('dougal', 'Dougal Thomson', 'caller', false),
        slot: 4,
        caller: { line: 'Bin strike', note: 'Nervous; go gentle', name_on_air: 'Dougal', sent: true, taken: false },
      },
    ],
    callers: [
      {
        identity: 'dougal', name: 'Dougal Thomson', ordinal: 1, state: 'ready', since_ms: now - 60_000, arrived_ms: now - 300_000,
        line: 'Bin strike', note: 'Nervous', name_on_air: 'Dougal', declined: { at_ms: now - 5_000, reason: 'in a two-way' },
        banned_before: null, topic: '', consent: { at_ms: 0, version: '0123456789abcdef', by: 'caller' }, dropped_ms: null, slot: 4, on_air: false, cue: false, linked: true, present: true, lufs: -30,
      },
      {
        identity: 'tavish', name: 'Tavish', ordinal: 2, state: 'waiting', since_ms: now - 90_000, arrived_ms: now - 90_000,
        line: '', note: '', name_on_air: '', declined: null, banned_before: { date: '2026-09-12', reason: 'Abuse on air' },
        topic: '', consent: null, dropped_ms: null, slot: null, on_air: false, cue: false, linked: true, present: true, lufs: null,
      },
    ],
    desk_taking_callers: false,
    chat: { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false, influx: false, influx_since_ms: null },
    notices: [],
    carts: { version: 0, desk: true, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
    listener_delay: { ms: 15_000, measured: false },
  };
}

const NOW = Date.UTC(2026, 9, 25, 13, 57, 12);

function cart(slot: number, label: string, over: Partial<Cart> = {}): Cart {
  return {
    id: `c${slot}`, bank: 1, slot, label, detail: '', mode: 'one_shot', file: `${slot}.flac`, duration_ms: 6_000,
    gain_db: -7, loudness_lufs: -16, true_peak_dbtp: -1.5, ramp_ms: null, origin: '', licence: '', missing: false, ...over,
  };
}

/** Kettle FM's wall as the design page draws it: a bed ducked under Eilidh's open mic, the
 *  applause ending, a cart without its file, and empty slots. */
function wall(): { view: View; catalogue: Catalogue } {
  const v = view(true);
  v.carts = {
    version: 3, desk: true, desk_error: null, ducked: true, mic_open: true, missing: [], importing: null,
    sounding: [
      { cart: 'c4', pool: 'bed', player: 1, mode: 'bed', cue: false, play: 1, started_ms: NOW - 22_000, ends_ms: null, stopping: false, seq: 1 },
      { cart: 'c8', pool: 'shot', player: 1, mode: 'one_shot', cue: false, play: 2, started_ms: NOW - 4_000, ends_ms: NOW + 2_000, stopping: false, seq: 2 },
      { cart: 'c2', pool: 'shot', player: 2, mode: 'one_shot', cue: true, play: 3, started_ms: NOW - 1_000, ends_ms: NOW + 5_000, stopping: false, seq: 3 },
    ],
  };
  const catalogue: Catalogue = {
    version: 3,
    loudness_lufs: -23,
    banks: [{ number: 1, name: 'The Morning Kettle', colour_slot: 3 }, { number: 4, name: 'Station idents', colour_slot: 11 }],
    carts: [
      cart(1, 'SNK opener', { mode: 'sweeper' }),
      cart(2, 'Kettle FM ident, short'),
      cart(4, 'Back-anno bed', { mode: 'bed', duration_ms: 60_000 }),
      cart(8, 'Applause'),
      cart(19, 'Show close', { file: null }),
      cart(20, 'Intro: Ghost Train', { detail: 'Firth Lights', ramp_ms: 12_000 }),
    ],
  };
  return { view: v, catalogue };
}

beforeEach(() => {
  studio.note = null;
  studio.selected = null;
  studio.armed = null;
});

/** The tile drawn for a slot, from its panel name to the next tile or the end. */
function tile(body: string, slot: number): string {
  const at = body.indexOf(`data-panel="desk.tile.${slot}"`);
  expect(at, `desk.tile.${slot}`).toBeGreaterThan(-1);
  const next = body.indexOf('data-panel="desk.tile.', at + 1);
  return body.slice(at, next < 0 ? undefined : next);
}

describe('desk B', () => {
  it('draws every registered desk panel under its name, and only registered names', () => {
    studio.view = view(true);
    const { body } = render(DeskB);
    for (const p of DESK_B.filter((p) => p.name !== 'desk.tile.3' && p.name !== 'desk.tile.5')) {
      expect(body.includes(`data-panel="${p.name}"`), p.name).toBe(true);
    }
    for (const m of body.matchAll(/data-panel="([^"]+)"/g)) expect(() => panel(m[1])).not.toThrow();
  });

  it('offers the invite at the head of Not on air, with a DJ as the first role', () => {
    studio.view = view(true);
    const { body } = render(DeskB);
    const waiting = body.slice(body.indexOf('data-panel="desk.waiting"'), body.indexOf('data-panel="desk.audience"'));
    expect(waiting).toMatch(/<button[^>]*aria-haspopup="dialog"[^>]*>Invite someone…<\/button>/);
    const dialog = waiting.slice(waiting.indexOf('data-panel="desk.invite"'));
    expect(dialog).toContain('Invite someone to the room');
    expect(dialog.indexOf('<strong>DJ</strong>')).toBeLessThan(dialog.indexOf('<strong>Guest</strong>'));
    expect(dialog).toContain('Joins off air, under Not on air.');
    expect(dialog).toContain('On air the moment they join.');
  });

  it('puts the handover button on the DJ’s tile and the rundown’s ⇧⏎ row, worded as the next press', () => {
    const v = view(true);
    v.show.rundown.push({ at: '14:05', title: 'Handover', detail: 'Finlay on air beside Eilidh', hard_post: false, key: '⇧⏎' });
    // On the desk's clock at 13:57, so the 14:05 row is still to come whenever the test runs
    // (it failed every afternoon after 14:05 on the machine's own clock).
    studio.now = NOW;
    studio.skew = 0;
    studio.zone = 'Europe/London';
    // No DJ in the room: no button anywhere, and the row shows its key as any row does.
    studio.handoverTo = null;
    studio.view = v;
    let body = render(DeskB).body;
    expect(body).not.toContain('data-handover');
    expect(body).toContain('<kbd>⇧⏎</kbd>');
    // Finlay waits off air on strip 5: the first press brings him on.
    const finlay = { ...person('finlay', 'Finlay Grant', 'dj', false), slot: 5, caller: null };
    studio.view = { ...v, sources: [...v.sources, finlay] };
    body = render(DeskB).body;
    expect(tile(body, 5)).toMatch(/data-handover="on"[^>]*><span>Bring Finlay on air<\/span><kbd>⇧⏎<\/kbd>/);
    expect(tile(body, 5)).toContain('title="On air beside you: the overlap."');
    expect(tile(body, 1)).not.toContain('data-handover');
    const rundown = () => body.slice(body.indexOf('data-panel="desk.rundown"'), body.indexOf('data-panel="desk.playout"'));
    expect(rundown()).toMatch(/class="row[^"]*hand[^"]*"[\s\S]*data-handover="on"[^>]*>Bring Finlay on air <kbd>⇧⏎<\/kbd>/);
    // The four live buttons come before it in the tile: it never moves them.
    expect(tile(body, 5).indexOf('class="controls"')).toBeLessThan(tile(body, 5).indexOf('data-handover'));
    // The overlap: the second press hands over, and says who comes off.
    studio.handoverTo = 'finlay';
    studio.view = { ...v, sources: [...v.sources, { ...finlay, on_air: true }] };
    body = render(DeskB).body;
    expect(tile(body, 5)).toMatch(/data-handover="off"[^>]*><span>Hand over to Finlay<\/span><kbd>⇧⏎<\/kbd>/);
    expect(tile(body, 5)).toContain('title="Eilidh comes off air."');
    expect(rundown()).toMatch(/data-handover="off"[^>]*>Hand over to Finlay/);
    // Done: Finlay holds the air and the seat; nothing left to press.
    studio.handoverTo = null;
    studio.view = {
      ...v,
      seats: { ...v.seats, presenter: 'finlay' },
      sources: [...v.sources.map((s) => (s.identity === 'eilidh' ? { ...s, on_air: false } : s)), { ...finlay, on_air: true }],
    };
    body = render(DeskB).body;
    expect(body).not.toContain('data-handover');
  });

  it('puts open sources on the stage under ON AIR and keeps each on its slot keys', () => {
    studio.view = view(true);
    const { body } = render(DeskB);
    const tile4 = body.slice(body.indexOf('data-panel="desk.tile.4"'));
    expect(tile4).toContain('<kbd>4</kbd>');
    expect(tile4).toContain('<kbd>R</kbd>');
    expect(body).toMatch(/class="lamp onair lamp-on-air[ "]/);
    expect(body).toMatch(/fill-onair[^>]*>.*?On air/s);
    expect(body).toContain('Dougal is ready for you · strip 4');
  });

  it('offers End pass on the invited people only, and Drop on the caller', () => {
    studio.view = view(true);
    const { body } = render(DeskB);
    expect(tile(body, 1)).toContain('End pass');
    expect(tile(body, 2)).toContain('End pass');
    expect(tile(body, 2)).toContain('aria-label="End pass for Gregor Stewart"');
    expect(tile(body, 4)).toContain('Drop');
    expect(tile(body, 4)).not.toContain('End pass');
    expect(tile(body, 6)).not.toContain('End pass');
    expect(tile(body, 6)).not.toContain('Drop');
    expect(body).not.toContain('Press again');
  });

  it('arms End pass on the tile it was pressed on, and no other', () => {
    studio.view = view(true);
    studio.armed = { what: 'end-pass', at: NOW, identity: 'gregor' };
    const { body } = render(DeskB);
    expect(tile(body, 2)).toContain('class="btn quiet armed"');
    expect(tile(body, 2)).toContain('Press again');
    expect(tile(body, 2)).toContain('aria-label="Press again to end pass for Gregor Stewart"');
    expect(tile(body, 1)).toContain('End pass');
    expect(tile(body, 1)).not.toContain('Press again');
    expect(tile(body, 4)).not.toContain('Press again');
    // An armed Drop is the caller's and never lights End pass.
    studio.armed = { what: 'drop', at: NOW };
    const again = render(DeskB).body;
    expect(tile(again, 4)).toContain('Press again');
    expect(tile(again, 2)).toContain('End pass');
    expect(tile(again, 2)).not.toContain('Press again');
  });

  it('says PLAYOUT DOWN on the lamp and OPEN on the open sources while playout is down', () => {
    studio.view = view(false);
    const { body } = render(DeskB);
    expect(body).toMatch(/class="lamp serious lamp-playout-down[ "]/);
    expect(body).toContain('fill-open');
    expect(body).toContain('class="tile is-open"');
    expect(body).not.toContain('class="tile is-onair"');
    expect(body).toContain('Playout down · room alive');
  });
});

describe('auto mode (ADR-0183)', () => {
  const autoView = (playoutUp = true): View => ({ ...view(playoutUp), mode: 'auto' });

  it('draws the AUTO lamp, hollow and full size, and the station line', () => {
    studio.view = autoView();
    const { body } = render(DeskB);
    expect(body).toMatch(/class="lamp auto lamp-auto[ "]/);
    expect(body).toContain('data-state="auto"');
    expect(body).toContain('href="#g-auto"');
    expect(body).toContain('Automation holds the air');
    expect(body).not.toContain('lamp onair');
  });

  it('says OPEN on every open source, since the desk never airs', () => {
    studio.view = autoView();
    const { body } = render(DeskB);
    const stage = body.slice(body.indexOf('data-panel="desk.stage"'), body.indexOf('data-panel="desk.audience"'));
    expect(stage).toContain('fill-open');
    expect(stage).not.toContain('fill-onair');
    expect(body).toContain('class="tile is-open"');
    expect(stage).toContain('btn lit-open');
    expect(stage).not.toContain('lit-onair');
  });

  it('says ON AUTOMATION for the audience and ON AIR for the playout panel', () => {
    studio.view = autoView();
    const { body } = render(DeskB);
    expect(body).toContain('On automation');
    expect(body).toContain('hollow-auto');
    const playout = body.slice(body.indexOf('data-panel="desk.playout"'), body.indexOf('data-panel="desk.stage"'));
    expect(playout).toMatch(/fill-onair[^>]*>[^]*?On air/);
    expect(body).not.toContain('Hearing the desk');
  });

  it('keeps AUTO over PLAYOUT DOWN, and a silent desk a fault', () => {
    studio.view = autoView(false);
    expect(render(DeskB).body).toContain('data-state="auto"');
    studio.view = { ...autoView(), desk: { reachable: false, error: 'gone', playout: { connected: false, for_ms: null } } };
    const { body } = render(DeskB);
    expect(body).toContain('data-state="fault"');
    expect(body).not.toContain('On automation');
  });

  it('is the live desk again when the mode is live, null or absent', () => {
    for (const mode of ['live', null, undefined] as const) {
      studio.view = { ...view(true), mode };
      const { body } = render(DeskB);
      expect(body, String(mode)).toContain('data-state="on-air"');
      expect(body, String(mode)).not.toContain('On automation');
    }
  });

  it('puts AUTO in the screener status bar and OPEN on a caller who is on air', () => {
    const v = autoView();
    v.callers[0] = { ...v.callers[0], on_air: true };
    studio.view = v;
    const { body } = render(Screener);
    expect(body).toContain('hollow-auto');
    expect(body).toContain('>Open<');
    expect(body).not.toContain('>On air<');
  });
});

describe('the tooltip on each state word (design system section 3)', () => {
  it('names a tooltip for a state word, as a sibling the chip describes, and leaves other words alone', () => {
    studio.view = view(true);
    const { body } = render(DeskB);
    const m = body.match(/aria-describedby="(st-tip-[^"]+)"[^>]*>(?:(?!<\/span>).)*?On air/s);
    expect(m, 'an ON AIR chip with a tooltip').not.toBeNull();
    expect(body).toContain(`id="${m![1]}" class="st-tip`);
    expect(body).toContain('role="tooltip"');
    expect(body).toContain('Listeners can hear this now; speak as if the whole audience is in the room, because it is.');
    // READY has no approved sentence: a plain chip, no tab stop.
    expect(body).toMatch(/<span class="state hollow hollow-good[^"]*"><svg[^>]*><use href="#g-good"><\/use><\/svg>Ready<\/span>/);
  });

  it('puts the lamp and every vocabulary chip behind a tab stop and keeps the panel name on the lamp', () => {
    studio.view = view(false);
    const { body } = render(DeskB);
    expect(body).toMatch(/<span class="lamp serious lamp-playout-down st-trigger[^"]*"[^>]*tabindex="0"[^>]*data-panel="desk.lamp"/);
  });
});

describe('the default loop notice (ADR-0130)', () => {
  it('shows a chip in the playout panel only while listeners hear the shipped loop', () => {
    studio.view = view(true);
    expect(render(DeskB).body).not.toContain('Default loop');
    studio.view = { ...view(true), default_fallback: false };
    expect(render(DeskB).body).not.toContain('Default loop');
    studio.view = { ...view(true), default_fallback: true };
    const { body } = render(DeskB);
    expect(body).toContain('data-testid="default-loop"');
    expect(body).toContain('Default loop');
    expect(body).toContain('no fallback files of its own');
  });
});

describe('the screener', () => {
  it('draws its panels, the queue in order, DESK BUSY and BANNED BEFORE', () => {
    studio.view = view(true);
    const { body } = render(Screener);
    for (const name of ['screener.status', 'screener.queue', 'screener.screening', 'screener.needs', 'screener.studio', 'screener.room']) {
      expect(body.includes(`data-panel="${name}"`), name).toBe(true);
    }
    expect(SCREENER.length).toBe(9);
    expect(body.indexOf('Dougal Thomson')).toBeLessThan(body.indexOf('Tavish'));
    expect(body).toContain('Desk busy');
    expect(body).toContain('Banned before');
    expect(body).toContain('not taking callers');
    expect(body).toContain('Influx');
  });
});

describe('the cart wall', () => {
  beforeEach(() => {
    const w = wall();
    studio.view = w.view;
    studio.catalogue = w.catalogue;
    studio.bank = 1;
    studio.now = NOW;
    studio.skew = 0;
  });

  it('draws its panels under their registered names, and the grid under the left hand', () => {
    const { body } = render(CartWall);
    for (const p of CARTS) expect(body.includes(`data-panel="${p.name}"`), p.name).toBe(true);
    for (const m of body.matchAll(/data-panel="([^"]+)"/g)) expect(() => panel(m[1])).not.toThrow();
    const keys = [...body.matchAll(/data-cart-slot="(\d+)"[^>]*>(?:<!--[^>]*-->)*<span class="top"><kbd>([^<]+)<\/kbd>/g)].map((m) => m[2]);
    expect(keys).toEqual('123456QWERTYASDFGHZXCVBN'.split(''));
  });

  it('shows each state in words: playing, ENDING, a ducked bed, FILE MISSING, empty, a ramp', () => {
    const { body } = render(CartWall);
    const pad = (slot: number) => {
      const at = body.lastIndexOf('<button', body.indexOf(`data-cart-slot="${slot}"`));
      return body.slice(at, body.indexOf('</button>', at));
    };
    expect(pad(4)).toContain('loop · playing');
    expect(pad(4)).toContain('ducked −12 dB · Eilidh’s mic is open');
    expect(pad(8)).toContain('class="pad ending');
    expect(pad(8)).toContain('−0:02');
    expect(pad(8)).toContain('aria-label="W, Applause, one-shot, 0:06, playing, 0:02 left"');
    expect(pad(19)).toContain('File missing');
    expect(pad(19)).toContain('The key does nothing.');
    expect(pad(19)).toContain('aria-disabled="true"');
    expect(pad(3)).toContain('No cart here yet.');
    expect(pad(20)).toContain('ramp 12 s to the vocal');
    expect(pad(20)).toContain('<small>Firth Lights</small>');
    expect(body).toContain('Sounding: Back-anno bed, Applause');
    expect(body).toContain('--bank-colour:var(--cart-3)');
  });

  it('says FAULT when the desk, which plays the carts, does not answer', () => {
    studio.view!.carts.desk = false;
    studio.view!.carts.desk_error = 'the desk is not answering';
    const { body } = render(CartWall);
    expect(body).toContain('Fault');
    expect(body).toContain('Carts cannot sound until the desk answers.');
  });

  it('draws a cued cart in violet with CUE, in your headphones (ADR-0117)', () => {
    const { body } = render(CartWall);
    const at = body.indexOf('data-cart-slot="2"');
    const pad = body.slice(body.lastIndexOf('<button', at), body.indexOf('data-cart-slot="3"'));
    expect(pad).toContain('class="pad cued"');
    expect(pad).toContain('fill-cue');
    expect(pad).toContain('in your headphones');
    expect(pad).toContain('aria-label="2, Kettle FM ident, short, one-shot, 0:06, cued, in your headphones"');
    expect(body).toContain('cue · headphones');
  });

  it('puts the same top row on the desk’s glance, F1 to F6', () => {
    const { body } = render(DeskB);
    const glance = body.slice(body.indexOf('data-panel="desk.carts"'), body.indexOf('data-panel="desk.chat"'));
    expect(glance).toContain('SNK opener');
    expect(glance).toContain('aria-label="F3, empty slot"');
    expect(glance).toContain('aria-label="F4, Back-anno bed, looping"');
    expect(glance).toContain('The Morning Kettle');
  });
});

describe('the theme', () => {
  it('starts dark', () => {
    expect(theme.current).toBe('dark');
  });
});
