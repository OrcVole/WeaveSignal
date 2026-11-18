// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The desk's 1.1 screens, third round: the counts' last seconds (14a), Leave by the presenter's
// own tile, Hand to automation beside End show, the join words, and the listener figures
// (issue 17). The desk is rendered on the server, as the other desk tests do.

import { render } from 'svelte/server';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from './lib/api.ts';
import type { AudienceView, DeskPerson, DeskSource, LinkLost, Me, PostView, Source, View } from './lib/api.ts';
import { run } from './lib/actions.ts';
import { AUTOMATION_PLAYS } from './lib/air.ts';
import { audienceWords } from './lib/audience.ts';
import { delayLine, delayParts, delayWords } from './lib/format.ts';
import { LEAVE, OWN_LINK, airHost, isMine, leaveWords, ownIdentity, ownLinkBeside } from './lib/invite.ts';
import { hurry, keptHeld, keptLeft, keptLine, type Kept } from './lib/post.ts';
import { NO_SHOW } from './lib/steps.ts';
import { account } from './lib/account.svelte.ts';
import { studio } from './lib/studio.svelte.ts';
import DeskB from './desk/DeskB.svelte';

vi.mock('./lib/api.ts', async (orig) => ({ ...(await orig<typeof api>()), endPass: vi.fn() }));

const at = (h: number, m: number, s = 0) => Date.UTC(2026, 10, 20, h, m, s);
const person = (identity: string, name: string, role: DeskPerson['role'], on_air: boolean): DeskPerson => ({ identity, name, role, on_air, mute: false, cue: false, talk: false, talk_to: null, present: true, away_ms: null, linked: true, lufs: -22.6, lufs_peak: -19.8 });
const srt: DeskSource = { way: 'srt', format: 'mpegts/aac 48 kHz stereo', key: 'MREQM2YS', show: 'steeped', connected_ms: 291_000, leveller_db: -7.4, level_db: -15.2 };
const finlay = (on_air: boolean): Source => ({ ...person('source-MREQM2YS', 'Finlay Grant', 'source', on_air), slot: 5, caller: null, source: srt });
const SHONA_ID = '3f2c9a52-7a8e-4b3c-9d3e-1d2f4a6b8c10';
const shona: Source = { ...person(ownIdentity(SHONA_ID), 'Shona Campbell', 'presenter', true), slot: 1, caller: null };
const duncan: Source = { ...person('presenter-0123456789ab', 'Duncan Boyd', 'presenter', true), slot: 2, caller: null };
const me = (id = SHONA_ID): Me => ({ kind: 'staff', role: 'producer', account: { id, username: 'shona', name: 'Shona Campbell', role: 'producer', disabled_ms: null, created_ms: 1 } });
const post = (over: Partial<PostView>): PostView => ({ show: null, source: null, source_name: null, state: null, standing_since_ms: null, on_air_at_ms: null, grace_ends_ms: null, held_by: null, late_ends_ms: null, holders: [], link_lost: null, ...over });

function view(now: number, sources: Source[], over: Partial<View> = {}): View {
  return {
    station: 'Kettle FM',
    now_ms: now,
    show: { title: 'Steeped', rundown: [] },
    desk: { reachable: true, error: null, playout: { connected: true, for_ms: 14_000 } },
    seats: { presenter: null, screener: null, presenter_name: '', screener_name: '' },
    sources,
    callers: [],
    desk_taking_callers: false,
    chat: { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false, influx: false, influx_since_ms: null },
    notices: [],
    carts: { version: 0, desk: true, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
    listener_delay: { ms: 15_000, measured: false },
    mode: 'live',
    step: { ...NO_SHOW, step: 'full' },
    ...over,
  };
}
function show(v: View): void {
  studio.view = v;
  studio.skew = v.now_ms - studio.now;
}
function tile(body: string, slot: number): string {
  const from = body.indexOf(`data-panel="desk.tile.${slot}"`);
  expect(from, `desk.tile.${slot}`).toBeGreaterThan(-1);
  const next = body.indexOf('data-panel="desk.tile.', from + 1);
  return body.slice(from, next < 0 ? undefined : next);
}
function panel(body: string, name: string, close: string): string {
  const from = body.indexOf(`data-panel="${name}"`);
  expect(from, name).toBeGreaterThan(-1);
  return body.slice(from, body.indexOf(close, from));
}
const text = (html: string) => html.replace(/<!--.*?-->/gs, '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();

beforeEach(() => {
  studio.view = null;
  studio.tab = 'desk';
  studio.armed = null;
  studio.note = null;
  studio.offNote = null;
  studio.toldSources = ['source-MREQM2YS'];
  studio.lostDismissed = [];
  studio.graceDismissed = [];
  studio.letGo = [];
  studio.sourceSlots = {};
  studio.airAsk = null;
  studio.showAll = true;
  account.me = me();
  vi.mocked(api.endPass).mockReset();
});

describe('a count in its last seconds (14a)', () => {
  it('is plain above 30 s, amber from 0:30 to 0:11, red from 0:10, by the figure the desk shows', () => {
    expect(hurry(45_000)).toBeNull();
    expect(hurry(31_000)).toBeNull();
    expect(hurry(30_999)).toEqual({ level: 'amber', word: 'Last 30 seconds' }); // shows 0:30
    expect(hurry(11_000)).toEqual({ level: 'amber', word: 'Last 30 seconds' });
    expect(hurry(10_999)).toEqual({ level: 'red', word: 'Last 10 seconds' }); // shows 0:10
    expect(hurry(1)).toEqual({ level: 'red', word: 'Last 10 seconds' });
  });
  it('is never there once the count has run out', () => {
    expect(hurry(0)).toBeNull();
    expect(hurry(-3_000)).toBeNull();
  });
  it('counts down a kept place in its last 30 s, and says nothing of it before', () => {
    const kept: Kept = { key: 'K', identity: 'source-K', name: 'Finlay Grant', slot: 5, lost_ms: at(14, 41), until_ms: at(14, 46), covered: true, back: false };
    expect(keptLine(kept, at(14, 41, 7))).toBe('0:07 · place kept until 14:46');
    expect(keptLine(kept, at(14, 45, 40))).toBe('4:40 · place kept until 14:46 · 0:20 left');
    expect(keptLine(kept, at(14, 45, 55))).toBe('4:55 · place kept until 14:46 · 0:05 left');
    // The tile draws the line in two parts, so the mark can stand before the figure it marks.
    expect([keptHeld(kept, at(14, 45, 55)), keptLeft(kept, at(14, 45, 55))]).toEqual(['4:55 · place kept until 14:46', '0:05']);
    expect(keptLeft(kept, at(14, 41, 7))).toBeNull();
    expect(keptLeft({ ...kept, back: true }, at(14, 45, 55))).toBeNull();
  });

  /** The count's mark directly before a figure: the clock glyph in the level's colour, then the
   *  figure, with nothing between them but the renderer's own comments. */
  const mark = (level: 'amber' | 'red', figure: string) =>
    new RegExp(`<svg class="hurry ${level === 'red' ? 'g-critical' : 'g-warning'}" data-hurry-glyph="${level}" aria-hidden="true"><use href="#g-delay"></use></svg>(<!--[^>]*-->)*(<b[^>]*>)?${figure}`);

  it('turns the tile’s late count, the toast and the kept place amber and red, each with the count’s mark and words, and never FAULT’s triangle', () => {
    const late = (s: number) =>
      view(at(14, 10, 5), [finlay(false)], {
        mode: 'auto',
        post: post({ show: { slug: 'steeped', name: 'Steeped', start_ms: at(14, 0), started: true }, source: 'MREQM2YS', source_name: 'Finlay Grant', state: 'standing_by', standing_since_ms: at(14, 10, 5), late_ends_ms: at(14, 10, 5 + s), on_air_at_ms: at(14, 10, 5 + s) }),
      });
    show(late(40));
    expect(render(DeskB).body).not.toContain('data-hurry');
    show(late(25));
    let body = render(DeskB).body;
    expect(tile(body, 5)).toMatch(/class="count" data-testid="late-count" data-hurry="amber"/);
    // One place for the mark: directly before the figure it marks, on the figure's line.
    expect(tile(body, 5)).toMatch(mark('amber', '0:25'));
    expect(panel(body, 'desk.toast', '</div></div>')).toContain('data-hurry="amber"');
    expect(text(panel(body, 'desk.toast', '</div></div>'))).toContain('Last 30 seconds.');
    show(late(6));
    body = render(DeskB).body;
    expect(tile(body, 5)).toMatch(/data-testid="late-count" data-hurry="red"/);
    expect(tile(body, 5)).toMatch(mark('red', '0:06'));
    expect(text(tile(body, 5))).toContain('Last 10 seconds.');
    // The same shape at both levels: the triangle is FAULT's alone, and a count that runs out is no fault.
    expect(tile(body, 5)).not.toContain('#g-fault');
    // The toast keeps its own glyph, the clock, and takes no second mark.
    const toast = panel(body, 'desk.toast', '</div></div>');
    expect(toast).toMatch(/data-hurry="red".*<div class="body"><svg aria-hidden="true"><use href="#g-delay">/s);
    expect(toast).not.toContain('#g-fault');
    expect(toast).not.toContain('data-hurry-glyph');
  });

  it('turns the run-up to a post and a hard post amber and red as well, with the words for a screen reader', () => {
    const local = (h: number, m: number, s: number) => new Date(2026, 10, 20, h, m, s).getTime();
    // The run-up to the next show's post: a host standing by for Steeped at 14:00.
    const upTo = (s: number) =>
      view(local(13, 59, 60 - s), [finlay(false)], { post: post({ show: { slug: 'steeped', name: 'Steeped', start_ms: local(14, 0, 0), started: false }, source: 'MREQM2YS', source_name: 'Finlay Grant', state: 'standing_by', standing_since_ms: local(13, 50, 0), holders: ['Shona Campbell'] }) });
    show(upTo(40));
    expect(panel(render(DeskB).body, 'desk.clock', '</section>')).not.toContain('data-hurry');
    show(upTo(25));
    let clock = panel(render(DeskB).body, 'desk.clock', '</section>');
    expect(clock).toMatch(/data-post="post".*data-hurry="amber"/s);
    expect(clock).toMatch(mark('amber', '−0:25'));
    expect(text(clock)).toContain('the post 14:00:00 −0:25 Last 30 seconds.');
    show(upTo(5));
    clock = panel(render(DeskB).body, 'desk.clock', '</section>');
    expect(clock).toMatch(mark('red', '−0:05'));
    expect(clock).not.toContain('#g-fault');
    expect(text(clock)).toContain('−0:05 Last 10 seconds.');
    // A hard post in the rundown, with no schedule line to say instead.
    const hard = (s: number) => view(local(16, 1, 60 - s), [shona], { show: { title: 'Steeped', rundown: [{ at: '16:02', title: 'News', detail: '', hard_post: true, key: null }] } });
    show(hard(25));
    clock = panel(render(DeskB).body, 'desk.clock', '</section>');
    expect(clock).toMatch(/data-hurry="amber"/);
    expect(text(clock)).toContain('News · hard post 16:02:00 −0:25 Last 30 seconds.');
    show(hard(5));
    clock = panel(render(DeskB).body, 'desk.clock', '</section>');
    expect(clock).toMatch(mark('red', '−0:05'));
    expect(clock).not.toContain('#g-fault');
    expect(text(clock)).toContain('−0:05 Last 10 seconds.');
  });

  it('shows a lost link’s place running out', () => {
    const lost: LinkLost = { key_id: 'MREQM2YS', name: 'Finlay Grant', show: 'steeped', lost_ms: at(14, 41), covers_ms: at(14, 41, 4), place_kept_until_ms: at(14, 46), covered: true };
    show(view(at(14, 45, 40), [], { mode: 'auto', post: post({ show: { slug: 'steeped', name: 'Steeped', start_ms: at(14, 0), started: true }, source: 'MREQM2YS', source_name: 'Finlay Grant', state: 'link_lost', link_lost: lost }) }));
    const t = tile(render(DeskB).body, 5);
    expect(t).toMatch(/data-testid="source-kept" data-hurry="amber"/);
    expect(text(t)).toContain('4:40 · place kept until 14:46 · 0:20 left');
    // The mark is before the figure it marks, in the line, as on every count.
    expect(t).toMatch(mark('amber', '0:20'));
    show(view(at(14, 45, 55), [], { mode: 'auto', post: post({ show: { slug: 'steeped', name: 'Steeped', start_ms: at(14, 0), started: true }, source: 'MREQM2YS', source_name: 'Finlay Grant', state: 'link_lost', link_lost: lost }) }));
    const red = tile(render(DeskB).body, 5).split('data-testid="source-kept"')[1];
    expect(red).toMatch(mark('red', '0:05'));
    expect(red.split('</span></span>')[0]).not.toContain('#g-fault');
  });
});

describe('Leave, by the presenter’s own tile', () => {
  it('knows its own tile by the microphone link’s identity, and no one else’s', () => {
    expect(ownIdentity(SHONA_ID)).toBe('presenter-me-3f2c9a527a8e4b3c9d3e1d2f4a6b8c10');
    expect(isMine(shona.identity, me())).toBe(true);
    expect(isMine(duncan.identity, me())).toBe(false);
    expect(isMine(shona.identity, { kind: 'station', role: 'admin' })).toBe(false);
    expect(isMine(shona.identity, null)).toBe(false);
  });

  it('says You’re in the room and offers Leave on your own tile, and End pass on another presenter’s', () => {
    show(view(at(14, 10, 5), [shona, duncan]));
    const body = render(DeskB).body;
    const mine = tile(body, 1);
    expect(mine).toContain('data-testid="leave"');
    expect(text(mine)).toContain('Leave');
    expect(text(mine)).not.toContain('End pass');
    expect(text(mine)).toContain('You’re in the room · linked');
    expect(mine).toContain(`title="${LEAVE.why}"`);
    const other = tile(body, 2);
    expect(other).not.toContain('data-testid="leave"');
    expect(text(other)).toContain('End pass');
    expect(text(other)).toContain('Linked · in the room');
  });

  it('is two presses, then ends your own pass and says where you are', async () => {
    show(view(at(14, 10, 5), [shona]));
    vi.mocked(api.endPass).mockResolvedValue(view(at(14, 10, 6), []));
    await run({ type: 'arm', what: 'end-pass', identity: shona.identity });
    expect(vi.mocked(api.endPass)).not.toHaveBeenCalled();
    expect(render(DeskB).body).toContain('Press again');
    await run({ type: 'end-pass', identity: shona.identity });
    expect(vi.mocked(api.endPass)).toHaveBeenCalledWith({ identity: shona.identity });
    expect(studio.note?.title).toBe(LEAVE.doneTitle);
    // The air is said from the view the station answered with: nobody else on air, mode live.
    expect(studio.note?.message).toBe('You are off air and your microphone link is closed. Nobody is on air.');
  });

  it('says where the air is from the view, and offers the way back only where that button is drawn', () => {
    const closed = 'You are off air and your microphone link is closed.';
    const alone = view(at(14, 10, 6), [], { step: { ...NO_SHOW, step: 'solo', may_change: true } });
    expect(leaveWords(alone, true, 'solo').message).toBe(`${closed} Nobody is on air. Join with my microphone… makes the link again.`);
    expect(leaveWords({ ...alone, mode: 'auto' }, true, 'solo').message).toBe(`${closed} Automation holds the air. Join with my microphone… makes the link again.`);
    // No playout layer: no automation, and the listener process's fallback covers.
    expect(leaveWords({ ...alone, mode: null }, true, 'solo').message).toContain('Listeners hear the fallback.');
    // Another presenter in the room: they keep the air, and the button is not drawn.
    const with2 = view(at(14, 10, 6), [duncan], { step: { ...NO_SHOW, step: 'full', may_change: true } });
    expect(leaveWords(with2, true, 'full').message).toBe(`${closed} Duncan keeps the air.`);
    // A window on the station key has no link to make again.
    expect(leaveWords(alone, false, 'solo').message).not.toContain('Join with my microphone');
  });
});

describe('Hand to automation, under the lamp', () => {
  it('stays under the lamp it changes, with its panel name, and there is one dialog', () => {
    show(view(at(14, 10, 5), [shona]));
    const body = render(DeskB).body;
    const head = body.slice(body.indexOf('<header class="stage-head"'), body.indexOf('</header>'));
    expect(head.indexOf('data-panel="desk.air"')).toBeGreaterThan(head.indexOf('data-panel="desk.lamp"'));
    expect(head.indexOf('data-panel="desk.air"')).toBeLessThan(head.indexOf('<div class="acts"'));
    expect(body.match(/data-panel="desk.end-show"/g)).toHaveLength(1);
  });
  it('and End show wears the same outlined button, with its key cap: the proper way to end is not the quieter-looking', () => {
    show(view(at(14, 10, 5), [shona]));
    const body = render(DeskB).body;
    expect(body).toMatch(/<button type="button" class="btn quiet outline mode[^"]*" data-panel="desk.air"/);
    expect(body).toMatch(/<button type="button" class="btn quiet outline" data-testid="end-show"[^>]*>End show <kbd>⌃⇧E<\/kbd><\/button>/);
    // Screener only changes the tab: it stays a quiet button.
    expect(body).toMatch(/<button type="button" class="btn quiet">Screener <kbd>⌃2<\/kbd><\/button>/);
  });
  it('says in its tooltip what it does, and then what automation is; a tooltip of the studio’s own, not the browser’s', () => {
    const tip = (body: string) => body.slice(body.indexOf('<span id="air-tip"'), body.indexOf('<dialog data-panel="desk.end-show"'));
    const button = (body: string) => body.match(/<button[^>]*data-panel="desk.air"[^>]*>/)?.[0] ?? '';
    show(view(at(14, 10, 5), [shona]));
    let body = render(DeskB).body;
    // The button names the tooltip, so the keyboard's focus and a screen reader get it too.
    expect(button(body)).toContain('aria-describedby="air-tip"');
    expect(button(body)).not.toContain('title=');
    expect(tip(body)).toContain('role="tooltip"');
    expect(tip(body)).toContain('<b>Automation takes the air now. Whoever is open stays in the room.</b>');
    expect(tip(body)).toContain(`<span>${AUTOMATION_PLAYS}</span>`);
    show(view(at(14, 10, 5), [shona], { mode: 'auto' }));
    body = render(DeskB).body;
    expect(tip(body)).toContain('<b>The air comes back to the desk. Whoever is open reaches listeners.</b>');
    expect(tip(body)).toContain(`<span>${AUTOMATION_PLAYS}</span>`);
  });
  it('keeps saying why instead where it cannot act: no playout layer, or a window that is preparing', () => {
    show(view(at(14, 10, 5), [shona], { mode: null }));
    const body = render(DeskB).body;
    expect(body).not.toContain('id="air-tip"');
    expect(body).toMatch(/<button[^>]*data-panel="desk.air"[^>]*aria-describedby="air-why"/);
  });
});

describe('joining while a host in their own studio has the air (the look’s review, finding 2)', () => {
  const full = { step: { ...NO_SHOW, step: 'full' as const, may_change: true } };
  const lost: LinkLost = { key_id: 'MREQM2YS', name: 'Finlay Grant', show: 'steeped', lost_ms: at(14, 41), covers_ms: at(14, 41, 4), place_kept_until_ms: at(14, 46), covered: true };
  const stage = (body: string) => body.slice(body.indexOf('<div class="tiles"'), body.indexOf('data-panel="desk.waiting"'));
  const row = (body: string) => body.slice(body.indexOf('data-panel="desk.waiting"'), body.indexOf('data-panel="desk.tile.6"'));

  it('names who holds the air: a source on air, or one whose place is kept; nobody otherwise', () => {
    expect(airHost([finlay(true)], null)).toEqual({ name: 'Finlay', lost: false });
    expect(airHost([finlay(false)], null)).toBeNull();
    expect(airHost([shona], null)).toBeNull();
    expect(airHost([], { name: 'Finlay Grant' })).toEqual({ name: 'Finlay', lost: true });
    // Two hosts on air at once, in an overlap: neither is named.
    expect(airHost([finlay(true), { ...finlay(true), identity: 'source-OTHER', name: 'Heather Douglas' }], null)).toEqual({ name: '', lost: false });
  });
  it('says what joining is for, and that it puts you on air', () => {
    expect(ownLinkBeside({ name: 'Finlay', lost: false })).toBe('Join to talk to Finlay or to hear a cue. Joining puts you on air.');
    expect(ownLinkBeside({ name: '', lost: false })).toBe('Join to talk to the hosts or to hear a cue. Joining puts you on air.');
    // A lost link, the desk still holding the air (the first seconds, or a station without the
    // playout layer): nobody to talk to, and whoever joins holds the air, so a returning host stands by.
    expect(ownLinkBeside({ name: 'Finlay', lost: true }, 'live')).toBe('Joining puts you on air. If Finlay reconnects, they then stand by.');
    expect(ownLinkBeside({ name: 'Finlay', lost: true }, null)).toBe('Joining puts you on air. If Finlay reconnects, they then stand by.');
  });
  it('does not say on air while automation holds the air: whoever joins is open to the room, and not heard', () => {
    // Automation covers the lost link. The station brings a returning host back by itself and
    // takes the air back with them (`plan`, crates/model/src/post.rs), so whoever joined is on air then.
    expect(ownLinkBeside({ name: 'Finlay', lost: true }, 'auto')).toBe('You join open to the room, and are on air when Finlay is back.');
    // The air was handed to automation while the host is open.
    expect(ownLinkBeside({ name: 'Finlay', lost: false }, 'auto')).toBe('Join to talk to Finlay or to hear a cue. You join open to the room.');
    for (const lost of [true, false]) expect(ownLinkBeside({ name: 'Finlay', lost }, 'auto')).not.toMatch(/puts you on air/);
  });
  it('does not headline the offer on the stage: one quiet line in the Not on air row, beside Invite someone…', () => {
    show(view(at(14, 10, 5), [finlay(true)], full));
    const body = render(DeskB).body;
    expect(stage(body)).not.toContain('You are not in the room yet');
    expect(stage(body)).not.toContain('data-testid="own-link"');
    expect(body).not.toMatch(/class="btn primary[^"]*"[^>]*>Join with my microphone…/);
    expect(text(row(body))).toContain('Not on air Join to talk to Finlay or to hear a cue. Joining puts you on air. Join with my microphone…');
    expect(row(body).indexOf('>Invite someone…</button>')).toBeGreaterThan(row(body).indexOf('>Join with my microphone…</button>'));
    // The quiet button of the row, named by the line beside it; the row's own note steps aside.
    expect(row(body)).toMatch(/<button type="button" class="btn quiet invite[^"]*" aria-haspopup="dialog" aria-describedby="own-beside"[^>]*>Join with my microphone…<\/button>/);
    expect(row(body)).not.toContain('class="rule"');
    expect(body.match(/data-testid="own-link"/g)).toHaveLength(1);
  });
  it('keeps the quiet line while the host’s place is kept, with the words for a lost link, by who holds the air', () => {
    const kept = (mode: View['mode'], covered: boolean) =>
      view(at(14, 45, 0), [], { ...full, mode, post: post({ show: { slug: 'steeped', name: 'Steeped', start_ms: at(14, 0), started: true }, source: 'MREQM2YS', source_name: 'Finlay Grant', state: 'link_lost', link_lost: { ...lost, covered } }) });
    // Automation covers.
    show(kept('auto', true));
    let body = render(DeskB).body;
    expect(stage(body)).not.toContain('You are not in the room yet');
    expect(text(row(body))).toContain('You join open to the room, and are on air when Finlay is back. Join with my microphone…');
    // The first seconds, before automation covers: the desk still holds the air.
    show(kept('live', false));
    body = render(DeskB).body;
    expect(stage(body)).not.toContain('You are not in the room yet');
    expect(text(row(body))).toContain('Joining puts you on air. If Finlay reconnects, they then stand by. Join with my microphone…');
  });
  it('takes the guest’s seat at Solo, which has no Not on air row: a sentence and a quiet button, no headline', () => {
    studio.showAll = false;
    show(view(at(14, 10, 5), [finlay(true)], { step: { ...NO_SHOW, step: 'solo', may_change: true } }));
    const body = render(DeskB).body;
    expect(body).not.toContain('data-panel="desk.waiting"');
    expect(body).not.toContain('You are not in the room yet');
    expect(body).toMatch(/<div class="empty seat own" data-testid="own-link"><span id="own-beside">Join to talk to Finlay or to hear a cue\. Joining puts you on air\.<\/span>/);
    expect(body).not.toMatch(/class="btn primary[^"]*"[^>]*>Join with my microphone…/);
    expect(body).toMatch(/class="btn quiet invite[^"]*"[^>]*aria-describedby="own-beside"[^>]*>Join with my microphone…/);
    expect(body).not.toContain('Just you on this show');
  });
  it('is unchanged when no source holds the air: the headline and its filled button', () => {
    // Nobody in the room, and a host standing by who is not on air.
    for (const sources of [[], [finlay(false)]]) {
      show(view(at(14, 10, 5), sources, full));
      const body = render(DeskB).body;
      expect(text(stage(body))).toContain('You are not in the room yet');
      expect(stage(body)).toMatch(/data-testid="own-link".*class="btn primary[^"]*"[^>]*>Join with my microphone…/s);
      expect(row(body)).toContain('class="rule"');
      expect(row(body)).not.toContain('Join with my microphone');
    }
    // A presenter in the room: no offer at all, as before.
    show(view(at(14, 10, 5), [shona, finlay(true)], full));
    expect(render(DeskB).body).not.toContain('data-testid="own-link"');
  });
});

describe('the join words', () => {
  it('say that a browser tab carries the voice and must stay open, in the stage, the dialog and after Open in browser', () => {
    show(view(at(14, 10, 5), [], { step: { ...NO_SHOW, step: 'solo', may_change: true } }));
    expect(text(render(DeskB).body)).toContain('Keep that tab open: it carries your voice.');
    expect(OWN_LINK.lede).toContain('Keep that tab open: it carries your voice.');
    expect(OWN_LINK.opened).toContain('Keep that tab open: it carries your voice.');
    expect(OWN_LINK.stage).toContain('Keep that tab open: it carries your voice.');
    // Under automation joining opens a person to the room; neither line promises the air.
    expect(OWN_LINK.stageAuto).not.toContain('you are on air');
    expect(OWN_LINK.ledeAuto).not.toContain('You are on air');
    expect(OWN_LINK.stageAuto).toContain('Take the air back');
    expect(OWN_LINK.ledeAuto).toContain('Keep that tab open: it carries your voice.');
  });
});

describe('the listener figures (issue 17)', () => {
  const aud = (o: Partial<AudienceView>): AudienceView => ({ listeners: 138, by_mount: [{ mount: '/programme.opus', listeners: 138 }], error: null, edges: false, scope: 'origin', ...o });
  const withAud = (a: AudienceView | undefined) => ({ audience: a }) as unknown as View;

  it('shows the count by mount, and says nothing of edges when there are none', () => {
    const w = audienceWords(withAud(aud({})));
    expect(w).toEqual({ count: 138, detail: 'programme.opus 138', edges: '' });
  });
  it('shows why there is no count, or that the first answer is awaited', () => {
    expect(audienceWords(withAud(aud({ listeners: null, by_mount: [], error: 'the listener process did not answer' }))).detail).toBe('Not counted: the listener process did not answer.');
    expect(audienceWords(withAud(aud({ listeners: null, by_mount: [], error: null }))).detail).toBe('Waiting for the first answer from the listener process.');
  });
  it('says, with edges or a CDN, that the figure is this station’s own stream only', () => {
    const w = audienceWords(withAud(aud({ edges: true })));
    expect(w.edges).toBe('Edges or a CDN are set up. This counts this station’s own stream only: each edge counts as one, and the listeners on an edge are not added.');
  });

  it('states the delay once on the clock’s line: the figure while it is an estimate, with no parts in brackets, and the plain figure once measured', () => {
    expect(delayLine({ ms: 21_000, measured: false, basis: 'estimate', desk_ms: 8_000, process_ms: null, rest_ms: 13_000, read_ms: 8_000, set_ms: 13_000 })).toBe('about 21 s behind');
    expect(delayLine({ ms: 23_000, measured: false, basis: 'estimate', desk_ms: 8_000, process_ms: 500, rest_ms: 14_500, read_ms: 8_500, set_ms: 14_500 })).toBe('about 23 s behind');
    expect(delayLine({ ms: 21_200, measured: true, basis: 'measured', desk_ms: 8_000, process_ms: 500, rest_ms: 0, read_ms: 21_200, set_ms: 0 })).toBe('21 s behind');
    expect(delayWords({ ms: 21_200, measured: true, basis: 'measured' })).toBe('21 s behind');
  });
  it('names the parts in one place, the line under it, in a presenter’s words: never read or set', () => {
    const counted = { ms: 23_000, measured: false, basis: 'estimate' as const, desk_ms: 8_000, process_ms: 500, rest_ms: 14_500, read_ms: 8_500, set_ms: 14_500 };
    expect(delayParts(counted)).toBe('8.0 s profanity delay, 0.5 s buffer, about 14.5 s in listeners’ players (estimated)');
    // The listener process does not answer: its buffer is not named, and the rest stands for it.
    expect(delayParts({ ...counted, ms: 21_000, process_ms: null, rest_ms: 13_000, read_ms: 8_000, set_ms: 13_000 })).toBe('8.0 s profanity delay, about 13 s in listeners’ players (estimated)');
    // Measured end to end: the same parts, and nothing is called estimated.
    expect(delayParts({ ms: 21_200, measured: true, basis: 'measured', desk_ms: 8_000, process_ms: 500, rest_ms: 0 })).toBe('8.0 s profanity delay, 0.5 s buffer, 12.7 s in listeners’ players');
    for (const d of [counted, { ...counted, process_ms: null }, { ...counted, measured: true, basis: 'measured' as const }]) expect(`${delayLine(d)} ${delayParts(d)}`).not.toMatch(/\b(read|set)\b/i);
  });
  it('keeps the old words for a station before 1.1, which sends no parts: nothing under the line would say it is an estimate', () => {
    expect(delayLine({ ms: 15_000, measured: false })).toBe('about 15 s behind (an estimate)');
    expect(delayLine(null)).toBe('about 15 s behind (an estimate)');
    // 1.0.2 to 1.0.5 send the parts without a basis: the line under the figure labels it.
    expect(delayLine({ ms: 23_000, measured: false, desk_ms: 8_000, process_ms: 500, rest_ms: 14_500 })).toBe('about 23 s behind');
  });

  it('states the delay once in each panel of the desk: the clock’s line and its parts, the stage’s subline, and DELAY says what it holds', () => {
    const delay = { delay_ms: 8_000, target_ms: 8_000, rebuilding: false, dumps: 0, dumped_at_ms: null };
    const v = view(at(14, 10, 5), [shona], { listener_delay: { ms: 23_000, measured: false, basis: 'estimate', desk_ms: 8_000, process_ms: 500, rest_ms: 14_500, read_ms: 8_500, set_ms: 14_500 } });
    show({ ...v, desk: { ...v.desk, delay } });
    let body = render(DeskB).body;
    const clock = panel(body, 'desk.clock', '</section>');
    expect(text(clock)).toMatch(/Listeners hear \d\d:\d\d:\d\d · about 23 s behind 8\.0 s profanity delay, 0\.5 s buffer, about 14\.5 s in listeners’ players \(estimated\)$/);
    // Once: the figure is not said twice, the parts are in no tooltip, and no line says read or set.
    expect(text(clock).match(/behind/g)).toHaveLength(1);
    expect(text(clock).match(/profanity delay/g)).toHaveLength(1);
    expect(clock).not.toMatch(/title="[^"]*profanity delay/);
    expect(text(clock)).not.toMatch(/\b(read|set)\b/i);
    const head = body.slice(body.indexOf('<header class="stage-head"'), body.indexOf('</header>'));
    expect(text(head)).toContain('1 on air · listeners about 23 s behind (an estimate)');
    expect(text(head).match(/behind/g)).toHaveLength(1);
    // Under DELAY: what the delay holds, not where listeners are.
    expect(text(panel(body, 'desk.delay', '</button>'))).toContain('Delay 8.0 s full · 8.0 s held');
    // Just dumped, and building back: what it holds again.
    show({ ...v, desk: { ...v.desk, delay: { ...delay, delay_ms: 3_200, rebuilding: true, dumps: 1, dumped_at_ms: null } } });
    body = render(DeskB).body;
    expect(text(panel(body, 'desk.delay', '</button>'))).toContain('rebuilding · 3.2 s held');
    expect(text(body.slice(body.indexOf('<header class="stage-head"'), body.indexOf('</header>'))).match(/behind/g)).toHaveLength(1);
  });
});
