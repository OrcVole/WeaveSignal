// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The desk's room at 1,440 × 900 and in one column (1.2.0): what the three faults of 1.1.1's run
// turned on, held here in the desk as rendered on the server and in the style sheet's own words.
// The sizes themselves are measured in a browser by tests/hosts/shoot.mjs (section 2u); these
// fail first, and without one.
//
// 1. End show lay over Dump: the head's title kept 12 rem and the lamp, the delay and End show
//    shared what was left, so the delay was drawn narrower than its content.
// 2. A note on an on-air tile was cut, and with three on air and two waiting the on-air tiles
//    lost their buttons: Not on air took its height first, and it grew when playout dropped.
// 3. In one column a toast was at the foot of a page 2,000 px long.

import { render } from 'svelte/server';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import DeskB from './desk/DeskB.svelte';
import { ONE_COLUMN_MAX, guessOneColumn } from './lib/columns.ts';
import { studio } from './lib/studio.svelte.ts';
import type { DeskPerson, Source, View } from './lib/api.ts';
import { NO_SHOW } from './lib/steps.ts';

// The app has no Node types, so the module is named in a way the type checker does not follow.
const fsModule = 'node:fs';
const { readFileSync } = (await import(/* @vite-ignore */ fsModule)) as { readFileSync(path: URL, encoding: 'utf8'): string };
const css = readFileSync(new URL('./styles/desk-b.css', import.meta.url), 'utf8');
const steps = readFileSync(new URL('./styles/steps.css', import.meta.url), 'utf8');
/** The declarations of the first rule whose selector is exactly this, as written. */
function rule(sheet: string, selector: string): string {
  const at = sheet.split('\n').find((l) => l.trim().startsWith(`${selector} {`));
  expect(at, selector).toBeDefined();
  return at!.trim().slice(selector.length + 2, at!.indexOf('}'));
}
/** A block of the sheet from the line that starts it to the line that closes it at the same depth. */
function block(sheet: string, opener: string): string {
  const from = sheet.indexOf(opener);
  expect(from, opener).toBeGreaterThan(-1);
  let depth = 0;
  for (let i = from; i < sheet.length; i++) {
    if (sheet[i] === '{') depth++;
    if (sheet[i] === '}' && --depth === 0) return sheet.slice(from, i + 1);
  }
  return sheet.slice(from);
}

const NOW = Date.UTC(2026, 10, 20, 13, 52, 0);
const person = (identity: string, name: string, role: DeskPerson['role'], on_air: boolean): DeskPerson => ({ identity, name, role, on_air, mute: false, cue: false, talk: false, talk_to: null, present: true, away_ms: null, linked: true, lufs: -22.6, lufs_peak: -19.8 });
const SOURCES: Source[] = [
  { ...person('eilidh', 'Eilidh Fraser-MacLeod', 'presenter', true), slot: 1, caller: null },
  { ...person('gregor', 'Gregor Stewart', 'guest', true), slot: 2, caller: null },
  { ...person('kirsty', 'Kirsty Wilson', 'guest', true), slot: 3, caller: null },
  { ...person('dougal', 'Dougal Thomson', 'caller', true), slot: 4, caller: { line: 'Please put me through, the bins', note: 'Nervous: go gentle', name_on_air: 'Dougal', sent: true, taken: true } },
  { ...person('finlay', 'Finlay Grant', 'dj', false), slot: 5, caller: null },
];
function view(playoutUp: boolean): View {
  return {
    station: 'Kettle FM',
    now_ms: NOW,
    show: { title: 'The Morning Kettle', rundown: [] },
    desk: { reachable: true, error: null, playout: { connected: playoutUp, for_ms: 3_000 }, delay: { delay_ms: 6000, target_ms: 8000, rebuilding: true, dumps: 0, dumped_at_ms: null } },
    seats: { presenter: 'eilidh', screener: null, presenter_name: 'Eilidh Fraser-MacLeod', screener_name: '' },
    sources: SOURCES,
    callers: [],
    desk_taking_callers: false,
    chat: { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false, influx: false, influx_since_ms: null },
    notices: [],
    carts: { version: 0, desk: true, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
    listener_delay: { ms: 21_000, measured: false },
    mode: 'live',
    step: { ...NO_SHOW, step: 'full' },
  };
}
function tile(body: string, slot: number): string {
  const from = body.indexOf(`data-panel="desk.tile.${slot}"`);
  expect(from, `desk.tile.${slot}`).toBeGreaterThan(-1);
  const next = body.indexOf('data-panel="desk.tile.', from + 1);
  const end = body.indexOf('</section>', from);
  return body.slice(from, next < 0 ? end : Math.min(next, end));
}
const text = (html: string) => html.replace(/<!--.*?-->/gs, '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();
const count = (html: string, what: string) => html.split(what).length - 1;

beforeEach(() => {
  studio.view = null;
  studio.tab = 'desk';
  studio.armed = null;
  studio.note = null;
  studio.offNote = null;
  studio.showAll = false;
});
afterEach(() => vi.unstubAllGlobals());

describe('the stage head: Dump and End show are never the part that gives way', () => {
  it('gives the title what is left, with no width of its own to keep', () => {
    expect(rule(css, '.stage-head')).toContain('grid-template-columns: auto minmax(0, 1fr) auto auto;');
    expect(css).not.toContain('minmax(12rem, 1fr)');
  });

  it('keeps the lamp, the delay with Dump, and End show as wide as their words', () => {
    expect(rule(css, '.stage-head > .air, .stage-head > .delay, .stage-head > .acts')).toContain('min-width: max-content;');
  });

  it('draws those three as the head’s own children, in the order lamp, title, delay, End show', () => {
    studio.view = view(false);
    const { body } = render(DeskB);
    const head = body.slice(body.indexOf('class="stage-head"'), body.indexOf('class="stage-body"'));
    const at = ['data-panel="desk.lamp"', 'class="title"', 'data-panel="desk.delay"', 'data-testid="end-show"'].map((s) => head.indexOf(s));
    expect(at.every((n) => n > -1)).toBe(true);
    expect([...at].sort((a, b) => a - b)).toEqual(at);
    expect(head).toMatch(/<div class="air[ "]/);
    expect(head).toContain('<div class="delay" data-panel="desk.delay">');
    expect(head).toContain('<div class="acts">');
  });

  it('puts the tag that says Dump is off under Dump, not in a column of its own, below the large layout', () => {
    const small = block(css, '@container studio (max-width: 2199px)');
    expect(small).toContain('.delay:has(.offtag) { grid-template-columns: auto auto; }');
    expect(small).toContain('.delay .offtag { grid-column: 2;');
    // the fourth column is the large layout's alone
    expect(steps).toContain('@container studio (min-width: 2200px) { .desk .delay:has(.offtag) { grid-template-columns: auto auto auto auto; } }');
    expect(count(steps, 'grid-template-columns: auto auto auto auto')).toBe(1);
  });

  it('gives the delay a row of its own where the stage is narrower than the head’s parts', () => {
    const narrow = block(css, '@container studio (max-width: 920px)');
    expect(narrow).toContain('.stage-head > .delay { grid-column: 1 / -1; grid-row: 2;');
  });

  it('is one height whatever the delay says: three lines under the chip, at the title’s line height', () => {
    expect(rule(css, '.delay .lbl .small')).toContain('line-height: 1.25;');
    expect(rule(css, '.stage-head .title .sub')).toContain('-webkit-line-clamp: 3;');
  });
});

describe('the stage’s height: the tiles on air keep theirs, and Not on air gives way', () => {
  it('draws the tiles and Not on air in one box under the head, the tiles first', () => {
    studio.view = view(true);
    const { body } = render(DeskB);
    const stage = body.slice(body.indexOf('data-panel="desk.stage"'), body.indexOf('class="col right'));
    const [head, box, tiles, waiting] = ['class="stage-head"', 'class="stage-body"', 'class="tiles"', 'data-panel="desk.waiting"'].map((s) => stage.indexOf(s));
    expect(head).toBeGreaterThan(-1);
    expect(box).toBeGreaterThan(head);
    expect(tiles).toBeGreaterThan(box);
    expect(waiting).toBeGreaterThan(tiles);
    expect(count(stage, 'class="stage-body"')).toBe(1);
    // four on the stage, the DJ and playout under Not on air
    expect(stage.slice(tiles, waiting)).toContain('data-panel="desk.tile.4"');
    expect(stage.slice(waiting)).toContain('data-panel="desk.tile.5"');
    expect(stage.slice(waiting)).toContain('data-panel="desk.tile.6"');
  });

  it('sizes the on-air rows by what they need, and Not on air between a floor and what it needs', () => {
    expect(rule(css, '.stage')).toContain('grid-template-rows: auto minmax(0, 1fr);');
    const box = rule(css, '.stage-body');
    expect(box).toContain('grid-template-rows: minmax(min-content, 1fr);');
    expect(box).toContain('grid-auto-rows: minmax(var(--waiting-floor), auto);');
    expect(box).toContain('overflow: hidden auto;');
    expect(css).toContain('& { --waiting-floor: 6rem; }');
    const tiles = rule(css, '.tiles');
    expect(tiles).toContain('grid-auto-rows: minmax(min-content, auto);');
    // the rows that shrank to nothing
    expect(css).not.toMatch(/\.tiles \{[^}]*minmax\(0, 1fr\)/);
  });

  it('lets Not on air scroll inside itself, its rows whole and its name staying', () => {
    const waiting = rule(css, '.waiting');
    expect(waiting).toContain('grid-auto-rows: max-content;');
    expect(waiting).toContain('overflow: hidden auto;');
    expect(rule(css, '.waiting .wh')).toContain('position: sticky;');
    // a control the keys reach comes out from under the name that stays
    expect(css).toContain('.waiting { scroll-padding-top: 2.75rem; }');
    expect(css).toContain('.waiting::-webkit-scrollbar');
  });

  it('gives every row its content’s height in one column, and scrolls nothing inside the page', () => {
    const one = block(css, '@container (max-width: 1400px)');
    expect(one).toContain('.cols, .col, .col.left, .col.right, .stage, .stage-body, .tiles { grid-template-rows: none; grid-auto-rows: max-content; }');
    expect(one).toMatch(/\.stage-body, \.waiting \{ overflow: visible; \}|, \.stage-body, \.waiting \{ overflow: visible; \}/);
    expect(one).toContain('.waiting .wh { position: static;');
  });
});

describe('the Playout tile is as tall down as up', () => {
  /** The tile's lines row: from the tag that opens it to the one that opens the notes under it. */
  const lines = (html: string) => html.slice(html.lastIndexOf('<', html.indexOf('class="lines')), html.lastIndexOf('<', html.indexOf('class="rest"')));

  it('says what playout does while it is up', () => {
    studio.view = view(true);
    const t = tile(render(DeskB).body, 6);
    expect(text(lines(t))).toBe('Takes the desk into the programme SRT connected 0:03');
    expect(t).not.toContain('class="extra"');
  });

  it('says what its loss means in the same two places, and adds no note under them', () => {
    studio.view = view(false);
    const t = tile(render(DeskB).body, 6);
    expect(text(lines(t))).toBe('Listeners hear nothing of the room SRT link lost 0:03 ago · room alive');
    expect(t).toContain('data-testid="playout-down-line"');
    expect(t).not.toContain('Takes the desk into the programme');
    expect(t).not.toContain('class="extra"');
    // the same number of lines in the row, so the row is the same height
    studio.view = view(true);
    const up = tile(render(DeskB).body, 6);
    expect(count(lines(t), '<svg')).toBe(count(lines(up), '<svg'));
  });

  it('leaves how to restart it to the Playout panel, the status bar and the toast', () => {
    studio.view = view(false);
    const { body } = render(DeskB);
    const panel = body.slice(body.indexOf('data-panel="desk.playout"'), body.indexOf('data-panel="desk.stage"'));
    expect(text(panel)).toContain('Listeners hear nothing of the room. Restart playout to bring them back.');
    expect(text(panel)).toContain('Restart playout ⌃R ⌃R');
    const foot = body.slice(body.indexOf('data-panel="desk.status"'));
    expect(text(foot)).toContain('Restart ⌃R ⌃R');
    const toast = body.slice(body.indexOf('data-panel="desk.toast"'), body.indexOf('class="chat-cell"'));
    expect(text(toast)).toContain('Restart playout ⌃R ⌃R');
  });

  it('keeps a caller’s note on the on-air tile it belongs to', () => {
    studio.view = view(false);
    expect(text(tile(render(DeskB).body, 4))).toContain('Please put me through, the bins · Nervous: go gentle');
  });
});

describe('one column: a toast has a row of its own above the status bar', () => {
  it('guesses one column from the window and the zoom, as the style sheet folds it', () => {
    expect(guessOneColumn(1440, 1)).toBe(false);
    expect(guessOneColumn(1440, 1.2)).toBe(true); // 1,200 px of studio
    expect(guessOneColumn(1401, 1)).toBe(false);
    expect(guessOneColumn(1400, 1)).toBe(true);
    expect(guessOneColumn(2560, 1.35 * 1.2)).toBe(false); // 2,560 × 1,440 at 120 %: 1,580 px
    expect(guessOneColumn(1280, 1)).toBe(true);
    // on the server, and before a window has a size
    expect(guessOneColumn(undefined, 1)).toBe(false);
    expect(guessOneColumn(0, 1)).toBe(false);
  });

  it('folds at the style sheet’s own width, which says which layout it drew', () => {
    expect(css).toContain(`@container (max-width: ${ONE_COLUMN_MAX}px)`);
    const named = block(css, `@container studio (max-width: ${ONE_COLUMN_MAX}px)`);
    expect(named).toContain('& { --desk-columns: 1; }');
    expect(css).toContain('& { --desk-columns: 3; }');
    expect(css.indexOf('& { --desk-columns: 3; }')).toBeLessThan(css.indexOf('& { --desk-columns: 1; }'));
  });

  it('in three columns draws the toast in the slot, above the chat glance, once', () => {
    studio.view = view(false);
    const { body } = render(DeskB);
    expect(count(body, 'data-panel="desk.toast"')).toBe(1);
    expect(body).not.toContain('class="toast-row"');
    const slot = body.slice(body.indexOf('<div class="slot">'), body.indexOf('data-panel="desk.status"'));
    expect(slot.indexOf('data-panel="desk.toast"')).toBeGreaterThan(-1);
    expect(slot.indexOf('data-panel="desk.toast"')).toBeLessThan(slot.indexOf('data-panel="desk.chat"'));
  });

  it('in one column draws it once, in its own row after the columns and before the status bar', () => {
    vi.stubGlobal('innerWidth', 1200);
    studio.view = view(false);
    const { body } = render(DeskB);
    expect(count(body, 'data-panel="desk.toast"')).toBe(1);
    const [slot, chat, row, toast, foot] = ['<div class="slot">', 'data-panel="desk.chat"', 'class="toast-row"', 'data-panel="desk.toast"', 'data-panel="desk.status"'].map((s) => body.indexOf(s));
    expect(slot).toBeGreaterThan(-1);
    expect(chat).toBeGreaterThan(slot);
    expect(row).toBeGreaterThan(chat);
    expect(toast).toBeGreaterThan(row);
    expect(foot).toBeGreaterThan(toast);
    // the same toast: an alert, with its words and its keyed answer
    const t = body.slice(toast, foot);
    expect(body.slice(row, foot)).toContain('role="alert"');
    expect(text(t)).toContain('Playout down · room alive');
    expect(text(t)).toContain('Restart playout ⌃R ⌃R');
  });

  it('draws no row when there is no toast', () => {
    vi.stubGlobal('innerWidth', 1200);
    studio.view = view(true);
    const { body } = render(DeskB);
    expect(body).not.toContain('class="toast-row"');
    expect(body).not.toContain('data-panel="desk.toast"');
  });

  it('lays the row’s toast across the window, its answer beside its words', () => {
    expect(rule(css, '.toast-row .toast .body')).toContain('grid-template-columns: auto minmax(0, 1fr) auto;');
    expect(rule(css, '.toast-row .toast .acts')).toContain('grid-column: 3;');
  });
});
