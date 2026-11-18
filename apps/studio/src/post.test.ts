// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The post's rule on the desk (ADR-0280; hosts-in-their-own-studios.md sections 3.4 to 3.6, 4.2
// and 8) without a station: the rules as pure functions (the clock's post line, the rundown's
// row, the kept place, the toasts, where End show sends the air), the desk rendered on the
// server in each state, what Hold, Go on air now, Not yet and Esc send, and the driver's
// stand-in station held against the station's own API description. Sample people are the
// designs' (ADR-0071), on the design's own day.
import { render } from 'svelte/server';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import openapi from '../../../docs/openapi/weavesignal-v1.json';
import * as api from './lib/api.ts';
import type { DeskPerson, DeskSource, LinkLost, Me, PostView, Source, View } from './lib/api.ts';
import { CHIP } from '../../../packages/ui/states.ts';
import { deskToastNow, dismissGrace, dismissLost, goOnAir, holdPost, notYet, offAirKept, run } from './lib/actions.ts';
import { airPlan, endShowNote } from './lib/air.ts';
import { hm, hms } from './lib/format.ts';
import { handoverButton, handoverStep } from './lib/handover.ts';
import {
  backOnAir,
  countPct,
  countWords,
  coverWord,
  deskToastKind,
  endsTo,
  endsToWords,
  freeSlot,
  goNow,
  graceToast,
  handNext,
  heldBy,
  keptLine,
  keptNote,
  keptPlace,
  keptSub,
  lateCount,
  lateToast,
  letGoWords,
  lostToast,
  mayHold,
  nextCard,
  nextSource,
  ownStudiosLost,
  postLine,
  postRow,
  whose,
  withKept,
  withoutNext,
} from './lib/post.ts';
import { tileLink } from './lib/hosts.ts';
import { account } from './lib/account.svelte.ts';
import { NO_SHOW, stageOf } from './lib/steps.ts';
import { studio, takeView } from './lib/studio.svelte.ts';
import DeskB from './desk/DeskB.svelte';

vi.mock('./lib/api.ts', async (orig) => ({ ...(await orig<typeof api>()), desk: vi.fn(), disconnectSource: vi.fn(), holdPost: vi.fn(), releasePost: vi.fn(), sourceGo: vi.fn(), sourceNotYet: vi.fn(), sourceOffAir: vi.fn() }));

/** A time on Friday 20 November 2026, in UTC (London is on GMT then). */
const at = (h: number, m: number, s = 0) => Date.UTC(2026, 10, 20, h, m, s);

const person = (identity: string, name: string, role: DeskPerson['role'], on_air: boolean): DeskPerson => ({ identity, name, role, on_air, mute: false, cue: false, talk: false, talk_to: null, present: true, away_ms: null, linked: true, lufs: -22.6, lufs_peak: -19.8 });
const srt: DeskSource = { way: 'srt', format: 'mpegts/aac 48 kHz stereo', key: 'MREQM2YS', show: 'steeped', connected_ms: 291_000, leveller_db: -7.4, level_db: -15.2 };
const icecast: DeskSource = { ...srt, way: 'icecast', format: 'mp3 44.1 kHz stereo', key: 'PN6E32MR', show: 'kettle-drive', connected_ms: 203_000, leveller_db: 2.4 };
const duncan: Source = { ...person('duncan', 'Duncan Boyd', 'presenter', true), slot: 1, caller: null };
const finlay = (on_air: boolean, over: Partial<DeskSource> = {}): Source => ({ ...person('source-MREQM2YS', 'Finlay Grant', 'source', on_air), slot: 5, caller: null, source: { ...srt, ...over } });
const heather = (on_air: boolean): Source => ({ ...person('source-PN6E32MR', 'Heather Douglas', 'source', on_air), slot: 1, caller: null, source: icecast });

const staff = (role: Me['role'], username = 'shona'): Me => ({ kind: 'staff', role, account: { id: `a-${username}`, username, name: 'Shona Campbell', role, disabled_ms: null, created_ms: 1 } as Me['account'] });

function view(now: number, sources: Source[], post: PostView | null | undefined, over: Partial<View> = {}): View {
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
    ...(post === undefined ? {} : { post }),
    ...over,
  };
}

const steeped = (started: boolean) => ({ slug: 'steeped', name: 'Steeped', start_ms: at(14, 0), started });
const kettle = (started: boolean) => ({ slug: 'kettle-drive', name: 'Kettle Drive', start_ms: at(16, 0), started });
const post = (over: Partial<PostView>): PostView => ({ show: null, source: null, source_name: null, state: null, standing_since_ms: null, on_air_at_ms: null, grace_ends_ms: null, held_by: null, late_ends_ms: null, holders: [], link_lost: null, ...over });

// ---- the design's four moments ----

/** 13:52: The Long Lunch from the room; Finlay standing by for Steeped at 14:00. */
const BEFORE = at(13, 52);
const before = () => view(BEFORE, [duncan, finlay(false)], post({ show: steeped(false), source: 'MREQM2YS', source_name: 'Finlay Grant', state: 'standing_by', standing_since_ms: at(13, 47, 14), holders: ['Duncan Boyd'] }), { show: { title: 'The Long Lunch', rundown: [] } });
/** 16:01:18: Finlay running over, Heather standing by since 15:58; the grace ends at 16:02:00. */
const GRACE = at(16, 1, 18);
const gracePost = (over: Partial<PostView> = {}) => post({ show: kettle(true), source: 'PN6E32MR', source_name: 'Heather Douglas', state: 'standing_by', standing_since_ms: at(15, 58), grace_ends_ms: at(16, 2), holders: ['Finlay Grant'], ...over });
const grace = (over: Partial<PostView> = {}) => view(GRACE, [finlay(true), heather(false)], gracePost(over), { show: { title: 'Kettle Drive', rundown: [] } });
/** 14:10:05: automation holds Steeped's air; Finlay connected late and is counted on at 14:10:25. */
const LATE = at(14, 10, 5);
const latePost = (over: Partial<PostView> = {}) => post({ show: steeped(true), source: 'MREQM2YS', source_name: 'Finlay Grant', state: 'standing_by', standing_since_ms: LATE, late_ends_ms: at(14, 10, 25), on_air_at_ms: at(14, 10, 25), ...over });
const late = (over: Partial<PostView> = {}) => view(LATE, [finlay(false, { connected_ms: 5_000 })], latePost(over), { mode: 'auto' });
/** 14:41:07: Finlay's link has been gone seven seconds; automation covers; his place is kept. */
const LOST = at(14, 41, 7);
const lostLink = (over: Partial<LinkLost> = {}): LinkLost => ({ key_id: 'MREQM2YS', name: 'Finlay Grant', show: 'steeped', lost_ms: at(14, 41), covers_ms: at(14, 41, 4), place_kept_until_ms: at(14, 46), covered: true, ...over });
const lostPost = (over: Partial<LinkLost> = {}) => post({ show: steeped(true), source: 'MREQM2YS', source_name: 'Finlay Grant', state: 'link_lost', link_lost: lostLink(over) });
const lost = (over: Partial<LinkLost> = {}, sources: Source[] = []) => view(LOST, sources, lostPost(over), { mode: 'auto' });

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
/** One panel's markup, to its closing tag. */
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
  studio.toldSources = ['source-MREQM2YS', 'source-PN6E32MR'];
  studio.lostDismissed = [];
  studio.graceDismissed = [];
  studio.letGo = [];
  studio.sourceSlots = {};
  studio.holdAsked = null;
  studio.handoverTo = null;
  studio.showAll = false;
  account.me = staff('producer');
  for (const f of [api.desk, api.disconnectSource, api.holdPost, api.releasePost, api.sourceGo, api.sourceNotYet, api.sourceOffAir]) vi.mocked(f).mockReset();
});

// ---- the rules ----

describe('the clock’s post line', () => {
  it('names the show before its post, within the half day a clock can say', () => {
    expect(postLine(before().post, BEFORE)).toEqual({ kind: 'post', name: 'Steeped', at_ms: at(14, 0) });
    // The station names the next post up to a week ahead; a clock's time has no date.
    expect(postLine(post({ show: { ...steeped(false), start_ms: BEFORE + 13 * 3600_000 }, state: 'not_connected' }), BEFORE)).toBeNull();
    // A show that comes from the room has no host to bring on: the clock is as it was.
    expect(postLine(post({ show: steeped(false), holders: ['Duncan Boyd'] }), BEFORE)).toBeNull();
    expect(postLine(post({ show: steeped(false), source: 'MREQM2YS', source_name: 'Finlay Grant', state: 'not_connected' }), BEFORE)).toMatchObject({ kind: 'post' });
    expect(postLine(null, BEFORE)).toBeNull();
    expect(postLine(undefined, BEFORE)).toBeNull();
    expect(postLine(post({}), BEFORE)).toBeNull();
  });

  it('counts the grace: who takes the air, by the schedule, and when', () => {
    const line = postLine(gracePost(), GRACE);
    expect(line).toEqual({ kind: 'count', why: 'grace', name: 'Kettle Drive', at_ms: at(16, 2), from_ms: at(16, 0), key: null });
    if (line?.kind !== 'count') throw new Error('not a count');
    expect(countWords(line)).toEqual({ name: 'Kettle Drive takes the air', by: `by the schedule ${hms(at(16, 2))}` });
    // 78 of 120 seconds have run.
    expect(countPct(line, GRACE)).toBeCloseTo(65, 5);
    expect(countPct(line, at(16, 3))).toBe(100);
    // A host who stood by after the post: the grace started then.
    expect(postLine(gracePost({ standing_since_ms: at(16, 0, 30), grace_ends_ms: at(16, 2, 30) }), GRACE)).toMatchObject({ from_ms: at(16, 0, 30) });
  });

  it('says a held handover, by whom, before any count', () => {
    expect(postLine(gracePost({ grace_ends_ms: null, held_by: 'shona' }), GRACE)).toEqual({ kind: 'held', by: 'shona' });
    expect(heldBy('shona', staff('producer'))).toBe('you');
    expect(heldBy('shona', staff('admin', 'morag'))).toBe('shona');
    expect(heldBy('station key', staff('admin', 'morag'))).toBe('the station key');
    expect(heldBy('shona', { kind: 'station', role: 'admin' })).toBe('shona');
  });

  it('counts a late host on air, with the key its two buttons act on', () => {
    expect(postLine(latePost(), LATE)).toEqual({ kind: 'count', why: 'late', name: 'Steeped', at_ms: at(14, 10, 25), from_ms: LATE, key: 'MREQM2YS' });
  });

  it('says the DJ on air keeps the air while the next host has not connected', () => {
    const waiting = gracePost({ state: 'not_connected', standing_since_ms: null, grace_ends_ms: null });
    expect(postLine(waiting, GRACE)).toEqual({ kind: 'words', text: 'Heather has not connected. You keep the air.' });
    expect(postLine({ ...waiting, state: 'connecting' }, GRACE)).toEqual({ kind: 'words', text: 'Heather is connecting. You keep the air.' });
    // With automation on the air there is nobody to tell: the rundown's row says NOT CONNECTED.
    expect(postLine({ ...waiting, holders: [] }, GRACE)).toBeNull();
    // A show that comes from the room has no host to wait for.
    expect(postLine(post({ show: kettle(true), holders: ['Duncan Boyd'] }), GRACE)).toBeNull();
  });

  it('says a press only when the host stands by and nothing counts', () => {
    // The grace is Off, or the count was stopped.
    expect(postLine(gracePost({ grace_ends_ms: null }), GRACE)).toEqual({ kind: 'words', text: 'Heather is standing by · a press only' });
    expect(postLine(latePost({ late_ends_ms: null, on_air_at_ms: null, standing_since_ms: null }), LATE)).toEqual({ kind: 'words', text: 'Finlay is standing by · a press only' });
  });
});

describe('the rundown’s row for the show whose post it is', () => {
  it('says where the show comes from and how its source stands', () => {
    expect(postRow(before(), BEFORE)).toEqual({ at_ms: at(14, 0), title: 'Steeped', detail: 'Finlay Grant · own studio', chip: CHIP.standby, now: false, identity: 'source-MREQM2YS' });
    expect(postRow(grace(), GRACE)).toMatchObject({ title: 'Kettle Drive', detail: 'Heather Douglas · own studio', chip: CHIP.standby, now: true, identity: 'source-PN6E32MR' });
    expect(postRow(lost(), LOST)).toMatchObject({ chip: CHIP.linkLost, now: true, identity: null });
  });

  it('says NOT CONNECTED only once the post has come: the view does not carry the door', () => {
    const due = (started: boolean) => view(GRACE, [finlay(true)], gracePost({ show: kettle(started), state: 'not_connected', standing_since_ms: null, grace_ends_ms: null }));
    expect(postRow(due(true), GRACE)).toMatchObject({ chip: CHIP.notConnected, identity: null });
    expect(postRow(due(false), at(15, 40))).toMatchObject({ chip: null });
    expect(CHIP.notConnected.cls).toContain('dashed');
  });

  it('has no row for a show from the room, for a post too far ahead, or on a station older than 1.1', () => {
    expect(postRow(view(GRACE, [duncan], post({ show: kettle(false), holders: ['Duncan Boyd'] })), GRACE)).toBeNull();
    expect(postRow(view(BEFORE, [], post({ show: { ...steeped(false), start_ms: BEFORE + 13 * 3600_000 }, source: 'MREQM2YS', source_name: 'Finlay Grant', state: 'not_connected' })), BEFORE)).toBeNull();
    expect(postRow(view(BEFORE, [duncan], undefined), BEFORE)).toBeNull();
    expect(postRow(null, BEFORE)).toBeNull();
  });

  it('names a presenter in the room as in the room', () => {
    const v = view(GRACE, [duncan, { ...person('acct-heather', 'Heather Douglas', 'dj', false), slot: 5, caller: null }], gracePost({ source: 'acct-heather' }));
    expect(postRow(v, GRACE)).toMatchObject({ detail: 'Heather Douglas · in the room', identity: 'acct-heather' });
  });
});

describe('a lost link: the place kept', () => {
  it('keeps the tile on the strip this window last saw the source on', () => {
    expect(keptPlace(lost(), { MREQM2YS: 3 })).toEqual({ key: 'MREQM2YS', identity: 'source-MREQM2YS', name: 'Finlay Grant', slot: 3, lost_ms: at(14, 41), until_ms: at(14, 46), covered: true, back: false });
    // A window opened during the loss: where a source goes (5, then 1, 2, 3).
    expect(keptPlace(lost(), {})?.slot).toBe(5);
    expect(freeSlot([{ slot: 5 }, { slot: 1 }])).toBe(2);
    // The strip it was on has been taken since: the next free one.
    expect(keptPlace(lost({}, [{ ...duncan, slot: 3 }]), { MREQM2YS: 3 })?.slot).toBe(5);
    expect(keptPlace(before(), {})).toBeNull();
    expect(keptPlace(null, {})).toBeNull();
  });

  it('says how long the link has been lost and until when the place is kept', () => {
    const k = keptPlace(lost(), {})!;
    expect(keptLine(k, LOST)).toBe(`0:07 · place kept until ${hm(at(14, 46))}`);
    expect(keptNote(k, 'auto', false)).toBe('Listeners hear automation until the stream is steady.');
    expect(keptSub(k)).toBe(`Finlay’s place is kept until ${hm(at(14, 46))}`);
    // In the first seconds, and with someone else on air, nothing is covered: the line alone.
    const fresh = keptPlace(lost({ covered: false }), {})!;
    expect(keptNote(fresh, 'live', true)).toBeNull();
    expect(keptSub(fresh)).toBeNull();
    // A station without the playout layer has no automation.
    expect(keptNote(k, null, false)).toBe('Listeners hear the fallback until the stream is steady.');
    expect(coverWord(undefined)).toBe('the fallback');
  });

  it('stays the same tile when its software reconnects, and goes when it is on air again', () => {
    const back = keptPlace(lost({}, [finlay(false, { connected_ms: 1_000 })]), {})!;
    expect(back).toMatchObject({ back: true, slot: 5, identity: 'source-MREQM2YS' });
    expect(keptLine(back, LOST)).toBe('Back on air once steady');
    expect(keptNote(back, 'auto', false)).toBeNull();
    // The station puts a returning source back only when nobody else presents.
    expect(keptLine(back, LOST, true)).toBe('Reconnected · standing by');
    expect(keptNote(back, 'live', true)).toBe('Not put back by itself while someone else has the air.');
    expect(keptSub(back)).toBe(`Finlay’s place is kept until ${hm(at(14, 46))}`);
    expect(keptPlace(lost({}, [finlay(false)]), {})).not.toBeNull();
    expect(keptPlace(lost({}, [finlay(true)]), {})).toBeNull();
  });

  it('holds its place on the stage, in slot order', () => {
    const guest: Source = { ...person('eilidh', 'Eilidh Fraser', 'guest', true), slot: 2, caller: null };
    const late6: Source = { ...person('x', 'X', 'guest', true), slot: 6, caller: null };
    const k = keptPlace(lost({}, [duncan, guest, late6]), { MREQM2YS: 5 })!;
    const placed = withKept(stageOf([duncan, guest, late6], 'full'), k);
    expect(placed.stage.map((s) => s.slot)).toEqual([1, 2, 5, 6]);
    expect(placed.stage[2]).toMatchObject({ identity: 'source-MREQM2YS', role: 'source', present: false, lufs: null, source: null });
    // Reconnected and off air, it does not drop to Not on air for the seconds before it is back.
    const again = finlay(false);
    const kept = keptPlace(lost({}, [duncan, again]), {})!;
    const moved = withKept(stageOf([duncan, again], 'full'), kept);
    expect(moved.stage.map((s) => s.identity)).toEqual(['duncan', 'source-MREQM2YS']);
    expect(moved.waiting).toEqual([]);
    expect(withKept({ stage: [duncan], waiting: [] }, null).stage).toEqual([duncan]);
  });

  it('says where the air is after Off air: automation took it, already held it, or someone else keeps it', () => {
    // The station's own word that automation took the air with the press.
    expect(letGoWords('Finlay Grant', true, { ...lost({ covered: false }), mode: 'live' })).toEqual({ title: 'Finlay’s place is let go', message: 'Automation has taken the air. If Finlay reconnects, they stand by.' });
    // Automation had covered already.
    expect(letGoWords('Finlay Grant', false, lost()).message).toBe('Automation holds the air. If Finlay reconnects, they stand by.');
    // Someone else presents: nothing changes for listeners.
    const others = lost({ covered: false }, [duncan]);
    expect(letGoWords('Finlay Grant', false, { mode: 'live', post: { ...others.post!, holders: ['Duncan Boyd'] } }).message).toBe('Duncan keeps the air. If Finlay reconnects, they stand by.');
    expect(letGoWords('Finlay Grant', false, { mode: 'live', post: { ...others.post!, holders: ['Duncan Boyd', 'Eilidh Fraser'] } }).message).toContain('Duncan and Eilidh keep the air.');
    // A station without the playout layer has no automation to say.
    expect(letGoWords('Finlay Grant', false, { mode: null, post: others.post! }).message).toContain('Listeners hear the fallback.');
    expect(whose('Angus')).toBe('Angus’');
  });

  it('counts it in the foot', () => {
    expect(ownStudiosLost('', keptPlace(lost(), {}))).toBe('1 link lost');
    expect(ownStudiosLost('1 standing by', keptPlace(lost(), {}))).toBe('1 standing by · 1 link lost');
    expect(ownStudiosLost('1 standing by', keptPlace(lost({}, [finlay(false)]), {}))).toBe('1 standing by');
    expect(ownStudiosLost('1 on air', null)).toBe('1 on air');
  });
});

describe('the toasts', () => {
  it('says a lost link once automation covers, and until when the place is kept', () => {
    expect(lostToast(lost(), [])).toEqual({ id: `MREQM2YS@${at(14, 41)}`, key: 'MREQM2YS', title: 'Finlay’s link is lost', message: `Automation covers. Their place is kept until ${hm(at(14, 46))}.` });
    // A station without the playout layer never says `covered`: the fallback covers once the
    // silence is over with nobody else on air, by the view's own clock.
    const bare = (now: number, holders: string[] = []): View => ({ ...lost({ covered: false }), mode: null, now_ms: now, post: { ...lostPost({ covered: false }), holders } });
    expect(lostToast(bare(LOST), [])?.message).toBe(`The fallback covers. Their place is kept until ${hm(at(14, 46))}.`);
    expect(lostToast(bare(at(14, 41, 2)), [])).toBeNull();
    expect(lostToast(bare(LOST, ['Duncan Boyd']), [])).toBeNull();
    expect(keptPlace(bare(LOST), {})?.covered).toBe(true);
    expect(backOnAir(bare(LOST), { ...view(at(14, 41, 36), [finlay(true)], null), mode: null })?.message).toBe('Their link held steady, and the fallback has handed the air back.');
    // With someone else on air, or in the first seconds, only the tile changes.
    expect(lostToast(lost({ covered: false }), [])).toBeNull();
    // Dismissed for this loss; a second drop is said again.
    expect(lostToast(lost(), [`MREQM2YS@${at(14, 41)}`])).toBeNull();
    expect(lostToast(lost({ lost_ms: at(14, 43) }), [`MREQM2YS@${at(14, 41)}`])).not.toBeNull();
    // Reconnected: the tile says it is coming back.
    expect(lostToast(lost({}, [finlay(false)]), [])).toBeNull();
  });

  it('counts a late host on air, in the station’s seconds', () => {
    expect(lateToast(late(), LATE)).toEqual({ key: 'MREQM2YS', left_ms: 20_000, title: 'Finlay goes on air in 0:20', lead: 'Finlay goes on air', tick: 'in 0:20', said: `at ${hms(at(14, 10, 25))}`, message: 'They connected late. Automation holds their air.' });
    expect(lateToast(late(), at(14, 10, 22))?.title).toBe('Finlay goes on air in 0:03');
    expect(lateToast(late({ late_ends_ms: null, on_air_at_ms: null }), LATE)).toBeNull();
    expect(lateToast(grace(), GRACE)).toBeNull();
  });

  it('counts, with Not yet, even when the press is not offered: automation holds the air with a presenter open', async () => {
    // The station counts whenever automation holds the air; the press waits on nobody presenting.
    show({ ...late({ holders: ['Duncan Boyd'] }), sources: [{ ...duncan, slot: 1 }, finlay(false, { connected_ms: 5_000 })] });
    expect(goNow(studio.view)).toBeNull();
    expect(lateCount(studio.view)).toMatchObject({ key: 'MREQM2YS', at_ms: at(14, 10, 25) });
    const { body } = render(DeskB);
    expect(text(tile(body, 5))).toContain('On air in 0:20 · by the schedule Last 30 seconds. Not yet');
    expect(body).not.toContain('data-handover="go"');
    const toast = text(panel(body, 'desk.toast', '</div></div>'));
    expect(toast).toContain('Not yet Esc');
    expect(toast).not.toContain('On air ⇧⏎');
    vi.mocked(api.sourceNotYet).mockResolvedValue({ waiting: true });
    await run({ type: 'escape' });
    expect(api.sourceNotYet).toHaveBeenCalledWith('MREQM2YS');
  });

  it('says a source is back on air after a lost link, from what the window saw', () => {
    const covered = lost();
    const backNow = view(at(14, 41, 36), [finlay(true)], post({ show: kettle(false) }));
    expect(backOnAir(covered, backNow)).toEqual({ title: 'Finlay is back on air', message: 'Their link held steady, and automation has handed the air back.' });
    expect(backOnAir(lost({ covered: false }, [duncan]), view(at(14, 41, 36), [duncan, finlay(true)], null))).toEqual({ title: 'Finlay is back on air', message: 'Their link held steady.' });
    // Still lost, reconnected and off air, or let go: nothing to say.
    expect(backOnAir(covered, lost())).toBeNull();
    expect(backOnAir(covered, lost({}, [finlay(false)]))).toBeNull();
    expect(backOnAir(covered, view(at(14, 46, 1), [], null))).toBeNull();
    expect(backOnAir(before(), before())).toBeNull();
    expect(backOnAir(null, covered)).toBeNull();
  });

  it('takes the view and says it once', () => {
    show(lost());
    takeView(view(at(14, 41, 36), [finlay(true)], null));
    expect(studio.note).toMatchObject({ tone: 'good', title: 'Finlay is back on air' });
    expect(studio.sourceSlots).toEqual({ MREQM2YS: 5 });
    studio.note = null;
    takeView(view(at(14, 41, 37), [finlay(true)], null));
    expect(studio.note).toBeNull();
  });

  it('offers a grace: who goes on air, in how long, by the schedule, or by a press now', () => {
    expect(graceToast(grace(), GRACE, [], true)).toEqual({
      id: `PN6E32MR@${at(16, 0)}`,
      left_ms: 42_000,
      title: 'Heather goes on air in 0:42',
      lead: 'Heather goes on air',
      tick: 'in 0:42',
      said: `at ${hms(at(16, 2))}`,
      message: `By the schedule, at ${hms(at(16, 2))}. Or bring them on now.`,
    });
    // Where the handover's key is not offered (below Full, a preparing window), the other way is End show.
    expect(graceToast(grace(), GRACE)?.message).toBe(`By the schedule, at ${hms(at(16, 2))}. Or end the show, and the air passes now.`);
    expect(graceToast(grace(), at(16, 1, 57))?.title).toBe('Heather goes on air in 0:03');
    // Not now puts the offer away for this handover; held and let run again, it is not offered twice.
    expect(graceToast(grace(), GRACE, [`PN6E32MR@${at(16, 0)}`])).toBeNull();
    expect(graceToast(grace({ grace_ends_ms: at(16, 3) }), GRACE, [`PN6E32MR@${at(16, 0)}`])).toBeNull();
    // Tomorrow's handover is another offer.
    expect(graceToast(grace(), GRACE, [`PN6E32MR@${at(16, 0) - 86_400_000}`])).not.toBeNull();
    // No grace runs: held, the next host not standing by, before the post, a late count.
    expect(graceToast(grace({ grace_ends_ms: null, held_by: 'shona' }), GRACE)).toBeNull();
    expect(graceToast(grace({ state: 'not_connected', grace_ends_ms: null }), GRACE)).toBeNull();
    expect(graceToast(before(), BEFORE)).toBeNull();
    expect(graceToast(late(), LATE)).toBeNull();
    expect(graceToast(null, GRACE)).toBeNull();
  });

  it('puts the most urgent first: playout down, a lost link, a caller, a degraded link, a grace, a late host, an arrival', () => {
    const all = { playoutDown: true, lost: true, caller: true, degraded: true, grace: true, late: true, arrived: true };
    const order = ['playout', 'lost', 'caller', 'degraded', 'grace', 'late', 'arrived'] as const;
    const flags = ['playoutDown', 'lost', 'caller', 'degraded', 'grace', 'late', 'arrived'] as const;
    let f = { ...all };
    for (let i = 0; i < order.length; i++) {
      expect(deskToastKind(f)).toBe(order[i]);
      f = { ...f, [flags[i]]: false };
    }
    expect(deskToastKind(f)).toBeNull();
  });
});

describe('who may hold a handover', () => {
  it('an admin, a producer or the station key; nobody else', () => {
    expect(mayHold(staff('admin'))).toBe(true);
    expect(mayHold(staff('producer'))).toBe(true);
    expect(mayHold({ kind: 'station', role: 'admin' })).toBe(true);
    expect(mayHold(staff('presenter'))).toBe(false);
    expect(mayHold(staff('screener'))).toBe(false);
    expect(mayHold(null)).toBe(false);
  });
});

describe('the handover’s key and a host’s own studio', () => {
  it('is one press for a source standing by after its post with nobody presenting', () => {
    const go = goNow(late());
    expect(go).toMatchObject({ key: 'MREQM2YS', counting: true, at_ms: at(14, 10, 25), source: { identity: 'source-MREQM2YS' } });
    const step = handoverStep(late().sources, null, null, go);
    expect(step).toMatchObject({ kind: 'go', key: 'MREQM2YS', counting: true });
    expect(handoverButton(step)).toEqual({ kind: 'go', dj: 'source-MREQM2YS', label: 'Go on air now', then: 'Finlay takes the air from automation at once.' });
    // Told Not yet, the count is gone and the press is the handover's own.
    const waits = goNow(late({ late_ends_ms: null, on_air_at_ms: null, standing_since_ms: null }));
    expect(waits).toMatchObject({ counting: false, at_ms: null });
    expect(handoverButton(handoverStep(late().sources, null, null, waits))?.label).toBe('Bring Finlay on air');
  });

  it('comes before any DJ waiting', () => {
    const dj: Source = { ...person('rory', 'Rory Bruce', 'dj', false), slot: 2, caller: null };
    expect(handoverStep([...late().sources, dj], null, null, goNow(late()))).toMatchObject({ kind: 'go' });
    expect(handoverStep([...late().sources, dj], null)).toMatchObject({ kind: 'on', dj: { identity: 'rory' } });
  });

  it('with someone presenting, picks the next show’s source standing by: two presses, as for a DJ', () => {
    const next = handNext(grace());
    expect(next).toMatchObject({ identity: 'source-PN6E32MR' });
    const first = handoverStep(grace().sources, null, null, null, next);
    expect(first).toMatchObject({ kind: 'on', dj: { identity: 'source-PN6E32MR' } });
    expect(handoverButton(first)).toEqual({ kind: 'on', dj: 'source-PN6E32MR', label: 'Bring Heather on air', then: 'On air beside you: the overlap.' });
    // After the first press both sound; the second takes off everyone presenting but the one brought on.
    const both = [finlay(true), heather(true)];
    const second = handoverStep(both, null, 'source-PN6E32MR', null, null);
    expect(second).toMatchObject({ kind: 'off', dj: { identity: 'source-PN6E32MR' }, off: [{ identity: 'source-MREQM2YS' }] });
    expect(handoverButton(second)).toEqual({ kind: 'off', dj: 'source-PN6E32MR', label: 'Hand over to Heather', then: 'Finlay comes off air.' });
    expect(handoverStep([finlay(false), heather(true)], null, 'source-PN6E32MR')).toMatchObject({ kind: 'done' });
    // It comes before any DJ waiting, and a DJ handed over to takes a source off with the presenters.
    const dj: Source = { ...person('rory', 'Rory Bruce', 'dj', false), slot: 2, caller: null };
    expect(handoverStep([...grace().sources, dj], null, null, null, next)).toMatchObject({ kind: 'on', dj: { identity: 'source-PN6E32MR' } });
    expect(handoverStep([finlay(true), { ...dj, on_air: true }], null, 'rory')).toMatchObject({ kind: 'off', dj: { identity: 'rory' }, off: [{ identity: 'source-MREQM2YS' }] });
  });

  it('the next source is the post’s own, standing by behind someone: before the post too, never from automation or a kept place', () => {
    expect(handNext(before())).toMatchObject({ identity: 'source-MREQM2YS' });
    expect(handNext(grace({ grace_ends_ms: null, held_by: 'shona' }))).toMatchObject({ identity: 'source-PN6E32MR' });
    // Automation holds the air: that is the one press (`goNow`), not the handover's first.
    expect(handNext(late())).toBeNull();
    expect(handNext(grace({ state: 'connecting' }))).toBeNull();
    expect(handNext(view(GRACE, [finlay(true), heather(true)], gracePost({ state: 'on_air' })))).toBeNull();
    const back = lost({}, [finlay(false)]);
    expect(handNext({ ...back, post: { ...back.post!, state: 'standing_by', holders: ['Duncan Boyd'] } })).toBeNull();
    expect(handNext(null)).toBeNull();
  });

  it('is not offered before the post, in a grace, while it is not steady, or once it is on air', () => {
    expect(goNow(before())).toBeNull();
    // Someone presents: the air passes at the grace's end or by the handover, not by this press.
    expect(goNow(grace())).toBeNull();
    expect(goNow(late({ state: 'connecting' }))).toBeNull();
    expect(goNow(view(LATE, [finlay(true)], latePost()))).toBeNull();
    expect(goNow(view(LATE, [], latePost()))).toBeNull();
    expect(goNow(view(LATE, [finlay(false)], undefined))).toBeNull();
  });
});

describe('End show says where the air goes', () => {
  it('to the next host while the schedule still has them standing by to bring on', () => {
    expect(endsTo(grace())).toEqual({ name: 'Heather', show: 'Kettle Drive' });
    expect(endsToWords(grace())).toBe('Heather is standing by: Kettle Drive starts as you end.');
    // Before its show starts too (the station passes the air to the next show's source).
    expect(endsToWords(before())).toBe('Finlay is standing by: Steeped starts as you end.');
  });

  it('to automation otherwise: not connected, told Not yet, or no host at all', () => {
    expect(endsTo(grace({ state: 'not_connected', standing_since_ms: null, grace_ends_ms: null }))).toBeNull();
    // Standing by, and the schedule will not bring them on by itself: End show does not either.
    expect(endsTo(grace({ standing_since_ms: null, grace_ends_ms: null }))).toBeNull();
    expect(endsToWords(grace({ standing_since_ms: null, grace_ends_ms: null }))).toBe('Automation takes the air.');
    expect(endsToWords(view(BEFORE, [duncan], undefined))).toBe('Automation takes the air.');
    expect(endsToWords(late({ state: 'connecting', standing_since_ms: null }))).toBe('Automation already holds the air.');
    // A station without the playout layer has nothing to hand the air to: the dialog says so.
    expect(endsToWords({ ...before(), post: null, mode: null })).toBeNull();
  });

  it('the dialog names who takes over, and that nothing goes to automation', () => {
    const plan = airPlan('end', grace());
    // The show's name on the desk is already the next show's during a grace (ADR-0214).
    expect(plan.lede).toBe('Heather is standing by: Kettle Drive starts as you end.');
    expect(plan.lines).toEqual(['Finlay goes off air now. They stay standing by, and callers stay in the queue.', 'End show passes the air to Heather, at once. Nothing goes to automation.']);
    expect(plan.confirm).toBe('End show');
    const alone = airPlan('end', grace({ state: 'not_connected', standing_since_ms: null, grace_ends_ms: null }));
    expect(alone.lede).toBe('Kettle Drive ends for listeners.');
    expect(alone.lines.at(-1)).toBe('Automation takes the air.');
  });

  it('afterwards says where the air went', () => {
    const v = grace();
    expect(endShowNote({ view: v, off_air: 1, automation_in_ms: null, passed_to: 'Heather Douglas' })).toEqual({ title: 'Heather has the air', message: '1 source is off air. The air passed to Heather; nothing went to automation.' });
    // Without `passed_to`, a null `automation_in_ms` is still a station with no automation.
    expect(endShowNote({ view: v, off_air: 1, automation_in_ms: null }).message).toContain('no automation to hand the air to');
    expect(endShowNote({ view: v, off_air: 1, automation_in_ms: 9000, passed_to: null }).message).toContain('Automation takes the air in 9 s');
  });
});

// ---- the desk, rendered on the server ----

describe('the desk before the post', () => {
  it('names the show on the clock and gives it a row, with how its source stands', () => {
    show(before());
    const { body } = render(DeskB);
    expect(text(panel(body, 'desk.clock', '</section>'))).toContain(`Steeped · the post ${hms(at(14, 0))} −8:00`);
    const row = text(panel(body, 'desk.rundown', '</section>'));
    expect(row).toContain(`${hm(at(14, 0))} Steeped Finlay Grant · own studio Standby`);
    expect(row).toContain('the schedule adds the next show’s post');
    expect(body).toContain('title="Finlay is standing by: Steeped starts as you end."');
    expect(body).not.toContain('data-testid="late-count"');
    expect(body).not.toContain('is-kept');
  });

  it('the rundown’s own hard post keeps the line when it is the nearer', () => {
    const v = before();
    show({ ...v, show: { title: 'The Long Lunch', rundown: [{ at: hms(at(13, 55)), title: 'Travel', detail: '', hard_post: true, key: null }] } });
    studio.zone = '';
    const clock = text(panel(render(DeskB).body, 'desk.clock', '</section>'));
    expect(clock).toContain('Travel · hard post');
    expect(clock).not.toContain('the post 14');
  });

  it('a station older than 1.1 sends no post, and the desk is as it was', () => {
    show(view(BEFORE, [duncan], undefined, { show: { title: 'The Long Lunch', rundown: [] } }));
    const { body } = render(DeskB);
    expect(text(panel(body, 'desk.clock', '</section>'))).toContain('No hard post ahead');
    expect(body).not.toContain('data-testid="post-row"');
    expect(body).toContain('Nothing scheduled');
    expect(body).toContain('the schedule does not fill this yet');
  });
});

describe('the desk in a grace', () => {
  it('counts on the clock, with Hold for a producer and no key', () => {
    show(grace());
    const clock = panel(render(DeskB).body, 'desk.clock', '</section>');
    expect(clock).toContain('data-post="grace"');
    expect(text(clock)).toContain(`Kettle Drive takes the air −0:42 by the schedule ${hms(at(16, 2))} Hold`);
    expect(clock).toMatch(/class="post grace"/);
    expect(clock).not.toContain('<kbd>');
    expect(clock).toMatch(/style="width: 65%;?"/);
  });

  it('is plain with more than 30 seconds, amber in the last 30 and red in the last 10, with the count’s mark before the figure and the words beside the colour (14a)', () => {
    const clockAt = (s: number) => {
      show({ ...grace(), now_ms: at(16, 1, 60 - s) });
      return panel(render(DeskB).body, 'desk.clock', '</section>');
    };
    const plain = clockAt(42);
    expect(plain).not.toContain('data-hurry');
    expect(plain).not.toContain('hurry');
    const amber = clockAt(25);
    expect(amber).toMatch(/class="left num" data-hurry="amber"[^>]*>.*class="hurry g-warning".*#g-delay.*−0:25/);
    expect(text(amber)).toContain('−0:25 Last 30 seconds.');
    const red = clockAt(8);
    // The same mark at both levels, in the level's colour: the triangle is FAULT's alone.
    expect(red).toMatch(/class="left num" data-hurry="red"[^>]*>.*class="hurry g-critical".*#g-delay.*−0:08/);
    expect(red).not.toContain('#g-fault');
    expect(text(red)).toContain('−0:08 Last 10 seconds.');
  });

  it('offers Hold to an admin and a producer only', () => {
    show(grace());
    account.me = staff('screener');
    expect(text(panel(render(DeskB).body, 'desk.clock', '</section>'))).not.toContain('Hold');
    account.me = staff('presenter');
    expect(text(panel(render(DeskB).body, 'desk.clock', '</section>'))).not.toContain('Hold');
    account.me = staff('admin', 'morag');
    expect(text(panel(render(DeskB).body, 'desk.clock', '</section>'))).toContain('Hold');
  });

  it('held, says by whom and that it is a press only, with Let it run', () => {
    show(grace({ grace_ends_ms: null, held_by: 'shona' }));
    let clock = panel(render(DeskB).body, 'desk.clock', '</section>');
    expect(clock).toContain('data-post="held"');
    expect(text(clock)).toContain('Handover held by you a press only Let it run');
    account.me = staff('admin', 'morag');
    expect(text(panel(render(DeskB).body, 'desk.clock', '</section>'))).toContain('Handover held by shona a press only Let it run');
    account.me = staff('screener', 'isla');
    clock = text(panel(render(DeskB).body, 'desk.clock', '</section>'));
    expect(clock).toContain('Handover held by shona a press only');
    expect(clock).not.toContain('Let it run');
  });

  it('offers the grace in the toast: On air on the handover’s key, Not now on Esc, in the warning tone', () => {
    show(grace());
    const { body } = render(DeskB);
    expect(deskToastNow()).toBe('grace');
    expect(body).toMatch(/class="toast warning" data-panel="desk\.toast" role="alert"/);
    const toast = panel(body, 'desk.toast', '</div></div>');
    expect(text(toast)).toContain(`Heather goes on air in 0:42 at ${hms(at(16, 2))} By the schedule, at ${hms(at(16, 2))}. Or bring them on now. On air ⇧⏎ Not now Esc`);
    // The moving figure is for the eye; a screen reader is told when it ends, once.
    expect(toast).toMatch(/Heather goes on air <span aria-hidden="true">in 0:42<\/span><span class="sr-only">at 16:02:00<\/span>/);
    expect(toast).toMatch(/class="btn primary"[^>]*>On air/);
    // The press is the handover's: its button is on the next host's tile and the rundown's row too.
    expect(tile(body, 1)).toMatch(/data-handover="on"[^>]*>.*?Bring Heather on air/s);
    expect(panel(body, 'desk.rundown', '</section>')).toMatch(/data-handover="on"[^>]*>Bring Heather on air/);
  });

  it('below Full the grace’s toast offers only Not now, and says the other way is End show', () => {
    show({ ...grace(), step: { ...NO_SHOW, step: 'guests' } });
    const toast = text(panel(render(DeskB).body, 'desk.toast', '</div></div>'));
    expect(toast).toContain('Or end the show, and the air passes now. Not now Esc');
    expect(toast).not.toContain('On air ⇧⏎');
  });

  it('Not now, and Esc, put the offer away and send nothing: the clock goes on counting', async () => {
    show(grace());
    await run({ type: 'escape' });
    expect(studio.graceDismissed).toEqual([`PN6E32MR@${at(16, 0)}`]);
    for (const f of [api.desk, api.holdPost, api.sourceNotYet, api.sourceGo]) expect(f).not.toHaveBeenCalled();
    const { body } = render(DeskB);
    expect(body).not.toContain('data-panel="desk.toast"');
    expect(panel(body, 'desk.clock', '</section>')).toContain('data-post="grace"');
    studio.graceDismissed = [];
    dismissGrace();
    expect(studio.graceDismissed).toEqual([`PN6E32MR@${at(16, 0)}`]);
    expect(deskToastNow()).toBeNull();
  });

  it('On air is the handover: ⇧⏎ brings the next host on beside the one on air, then hands over', async () => {
    show(grace());
    vi.mocked(api.desk).mockImplementation(async (c) => {
      const cmd = c as { identity: string; on: boolean };
      const v = studio.view!;
      return { ...v, sources: v.sources.map((s) => (s.identity === cmd.identity ? { ...s, on_air: cmd.on } : s)), post: gracePost({ state: 'on_air', grace_ends_ms: null, standing_since_ms: null }) };
    });
    await run({ type: 'handover' });
    expect(api.desk).toHaveBeenCalledWith({ op: 'on_air', identity: 'source-PN6E32MR', on: true });
    expect(studio.handoverTo).toBe('source-PN6E32MR');
    expect(studio.note).toMatchObject({ title: 'Heather is on air too' });
    expect(tile(render(DeskB).body, 1)).toMatch(/data-handover="off"[^>]*>.*?Hand over to Heather/s);
    await run({ type: 'handover' });
    expect(api.desk).toHaveBeenLastCalledWith({ op: 'on_air', identity: 'source-MREQM2YS', on: false });
    expect(studio.note).toMatchObject({ title: 'Handed over to Heather', message: 'Finlay off air; Heather holds the air from their own studio.' });
    expect(studio.handoverTo).toBeNull();
  });

  it('a preparing window reads the grace and is offered only Not now', () => {
    show(grace());
    account.me = { ...staff('producer'), preparing: true };
    const toast = text(panel(render(DeskB).body, 'desk.toast', '</div></div>'));
    expect(toast).toContain('Heather goes on air in 0:42');
    expect(toast).not.toContain('On air ⇧⏎');
    expect(toast).toContain('Not now Esc');
  });

  it('says the DJ on air keeps the air, and NOT CONNECTED on the row', () => {
    show(view(GRACE, [finlay(true)], gracePost({ state: 'not_connected', standing_since_ms: null, grace_ends_ms: null }), { show: { title: 'Kettle Drive', rundown: [] } }));
    const { body } = render(DeskB);
    expect(text(panel(body, 'desk.clock', '</section>'))).toContain('Heather has not connected. You keep the air.');
    const row = panel(body, 'desk.rundown', '</section>');
    expect(text(row)).toContain('Kettle Drive Heather Douglas · own studio Not connected');
    expect(row).toMatch(/class="state hollow hollow-standby dashed sm[^"]*"/);
    expect(body).toContain('title="Automation takes the air."');
  });
});

describe('the desk in a late count', () => {
  it('counts on the tile with Not yet, and Go on air now on the handover’s key', () => {
    show(late());
    const { body } = render(DeskB);
    const t = tile(body, 5);
    expect(text(t)).toContain('On air in 0:20 · by the schedule Last 30 seconds. Not yet');
    expect(t).toMatch(/data-handover="go"[^>]*>.*?Go on air now.*?<kbd>⇧⏎<\/kbd>/s);
    expect(text(panel(body, 'desk.clock', '</section>'))).toContain(`Steeped takes the air −0:20 Last 30 seconds. by the schedule ${hms(at(14, 10, 25))}`);
    // The late count has no Hold: that is the grace's.
    expect(text(panel(body, 'desk.clock', '</section>'))).not.toContain('Hold');
    const toast = text(panel(body, 'desk.toast', '</div></div>'));
    expect(toast).toContain(`Finlay goes on air in 0:20 at ${hms(at(14, 10, 25))} Last 30 seconds. They connected late. Automation holds their air. On air ⇧⏎ Not yet Esc`);
    // The toast is a live region: the moving figure is for the eye, and a screen reader is told when it ends.
    expect(panel(body, 'desk.toast', '</div></div>')).toContain(`<span aria-hidden="true">in 0:20</span><span class="sr-only">at ${hms(at(14, 10, 25))}</span>`);
    // (The chip carries the vocabulary's sentence for its word as a tooltip.)
    expect(text(panel(body, 'desk.rundown', '</section>'))).toMatch(/Steeped Finlay Grant · own studio Standby .*Go on air now ⇧⏎/);
  });

  it('below Full the handover is off: the count and Not yet stay, the press goes', () => {
    show({ ...late(), step: { ...NO_SHOW, step: 'guests' } });
    const { body } = render(DeskB);
    const t = tile(body, 5);
    expect(text(t)).toContain('On air in 0:20 · by the schedule Last 30 seconds. Not yet');
    expect(t).not.toContain('data-handover');
    const toast = text(panel(body, 'desk.toast', '</div></div>'));
    expect(toast).toContain('Not yet Esc');
    expect(toast).not.toContain('On air ⇧⏎');
  });

  it('told Not yet, the tile’s button is the handover’s own and the clock says a press only', () => {
    show(late({ late_ends_ms: null, on_air_at_ms: null, standing_since_ms: null }));
    const { body } = render(DeskB);
    expect(tile(body, 5)).toMatch(/data-handover="go"[^>]*>.*?Bring Finlay on air/s);
    expect(body).not.toContain('data-testid="late-count"');
    expect(text(panel(body, 'desk.clock', '</section>'))).toContain('Finlay is standing by · a press only');
  });
});

describe('the desk with a lost link', () => {
  it('keeps the tile on the stage: LINK LOST, its count, and until when the place is kept', () => {
    studio.sourceSlots = { MREQM2YS: 5 };
    show(lost());
    const { body } = render(DeskB);
    expect(body.indexOf('data-panel="desk.tile.5"')).toBeLessThan(body.indexOf('data-panel="desk.waiting"'));
    const t = tile(body, 5);
    expect(body).toMatch(/class="tile[^"]*is-source[^"]*is-kept[^"]*" data-panel="desk\.tile\.5"/);
    expect(t).toMatch(/hollow-serious[^>]*>.*?#g-link-degraded.*?Link lost/s);
    expect(text(t)).toContain(`0:07 · place kept until ${hm(at(14, 46))}`);
    expect(text(t)).toContain('Listeners hear automation until the stream is steady.');
    // Nothing sounds: no level, no leveller, and never LINK DEGRADED or a person's lines.
    expect(t).toContain('aria-label="No level"');
    expect(t).toMatch(/leveller <b>—<\/b> dB/);
    expect(t).not.toContain('Link degraded');
    expect(t).not.toContain('fader <b>');
    // Disconnect is there, by the key: the built way to let the place go at once.
    expect(t).toContain('aria-label="Disconnect Finlay Grant"');
  });

  it('says so in the toast, the subline, the foot and the rundown', () => {
    show(lost());
    const { body } = render(DeskB);
    const toast = panel(body, 'desk.toast', '</div></div>');
    expect(toast).toMatch(/^data-panel="desk.toast" role="alert"/);
    expect(body).toMatch(/class="toast serious"/);
    expect(text(toast)).toContain(`Finlay’s link is lost Automation covers. Their place is kept until ${hm(at(14, 46))}. Off air 5 Dismiss Esc`);
    // Off air is live, on the strip's own On key.
    expect(toast).toMatch(/<button type="button" class="btn">Off air <!--\[--><kbd>5<\/kbd>/);
    expect(toast).not.toContain('aria-disabled');
    expect(body).toContain(`Finlay’s place is kept until ${hm(at(14, 46))}`);
    expect(text(panel(body, 'desk.status', '</footer>'))).toContain('Own studios 1 link lost');
    expect(text(panel(body, 'desk.rundown', '</section>'))).toContain('Steeped Finlay Grant · own studio Link lost');
  });

  it('in the first seconds, and with someone else on air, only the tile changes', () => {
    show({ ...lost({ covered: false }, [duncan]), mode: 'live' });
    const { body } = render(DeskB);
    expect(body).not.toContain('data-panel="desk.toast"');
    const t = tile(body, 5);
    expect(text(t)).toContain('0:07 · place kept until');
    expect(t).not.toContain('class="extra kept"');
    expect(body.indexOf('data-panel="desk.tile.1"')).toBeLessThan(body.indexOf('data-panel="desk.tile.5"'));
  });

  it('reconnected, it stays on the stage and says it is back once steady', () => {
    show(lost({}, [finlay(false, { connected_ms: 1_000 })]));
    const { body } = render(DeskB);
    expect(body.indexOf('data-panel="desk.tile.5"')).toBeLessThan(body.indexOf('data-panel="desk.waiting"'));
    const t = tile(body, 5);
    expect(text(t)).toContain('Back on air once steady');
    expect(t).toMatch(/leveller <b>−7\.4<\/b> dB/);
    expect(body).not.toContain('data-panel="desk.toast"');
    expect(text(panel(body, 'desk.status', '</footer>'))).toContain('Own studios 1 standing by');
  });

  it('a returning source is the station’s to put back: no press is offered, and nothing says it has arrived', () => {
    studio.toldSources = [];
    const v = lost({}, [finlay(false, { connected_ms: 6_000 })]);
    // As the station sends it: standing by again, and its place still kept.
    show({ ...v, post: { ...v.post!, state: 'standing_by' } });
    expect(goNow(studio.view)).toBeNull();
    expect(postLine(studio.view!.post, LOST)).toBeNull();
    expect(postRow(studio.view, LOST)).toMatchObject({ chip: CHIP.linkLost });
    expect(deskToastNow()).toBeNull();
    const { body } = render(DeskB);
    expect(body).not.toContain('data-handover');
    expect(body).not.toContain('is standing by');
    expect(body).not.toContain('a press only');
    expect(text(tile(body, 5))).toContain('Back on air once steady');
  });

  it('leaves when the place is let go', () => {
    show(view(at(14, 46, 1), [], post({ show: steeped(true), source: 'MREQM2YS', source_name: 'Finlay Grant', state: 'not_connected' }), { mode: 'auto' }));
    const { body } = render(DeskB);
    expect(body).not.toContain('is-kept');
    expect(body).not.toContain('data-panel="desk.tile.5"');
    expect(body).not.toContain('Own studios');
  });
});

// ---- what the desk sends ----

// ---- `desk.next`: the next show's source in the guest's seat, at Solo ----

describe('the next show’s source at Solo', () => {
  const solo = (v: View): View => ({ ...v, step: { ...NO_SHOW, step: 'solo' } });
  const card = (v: View) => nextCard(v, tileLink);

  it('is the post’s own source, at the desk and off air; never one whose place is kept', () => {
    expect(nextSource(before())).toMatchObject({ identity: 'source-MREQM2YS' });
    expect(nextSource(grace())).toMatchObject({ identity: 'source-PN6E32MR' });
    expect(nextSource(late())).toMatchObject({ identity: 'source-MREQM2YS' });
    expect(nextSource(view(GRACE, [finlay(true), heather(true)], gracePost({ state: 'on_air' })))).toBeNull();
    expect(nextSource(grace({ state: 'not_connected', source: 'NOTHERE1' }))).toBeNull();
    expect(nextSource(lost({}, [finlay(false)]))).toBeNull();
    expect(nextSource(view(BEFORE, [duncan, finlay(false)], undefined))).toBeNull();
    expect(nextSource(null)).toBeNull();
  });

  it('says when the schedule brings it on, case by case as the clock does, and only what the view carries', () => {
    // Before the post behind a presenter: the grace's length is the station's setting, so no time is guessed.
    expect(card(before())).toMatchObject({ key: 'MREQM2YS', title: 'Steeped · Finlay Grant', link: 'Own studio · SRT · steady 4:51', words: `The post is at ${hms(at(14, 0))}. Or on air when you end the show.`, count: null });
    // Before the post with automation holding the air: at the post, to the second.
    expect(card(view(BEFORE, [finlay(false)], post({ show: steeped(false), source: 'MREQM2YS', source_name: 'Finlay Grant', state: 'standing_by', standing_since_ms: at(13, 47), on_air_at_ms: at(14, 0) }), { mode: 'auto' }))?.words).toBe(`On air by the schedule at ${hms(at(14, 0))}.`);
    // The grace: the design's own sentence.
    expect(card(grace())?.words).toBe(`On air by the schedule at ${hms(at(16, 2))}, or when you end the show.`);
    expect(card(grace())?.link).toBe('Own studio · Icecast · steady 3:23 · about 2 s behind');
    // Held, and the grace off: End show is the way.
    expect(card(grace({ grace_ends_ms: null, held_by: 'shona' }))?.words).toBe('Handover held by shona: on air when you end the show.');
    expect(card(grace({ grace_ends_ms: null }))?.words).toBe('On air when you end the show.');
    // Told Not yet behind a presenter: the station no longer passes the air to them by itself.
    expect(card(grace({ grace_ends_ms: null, standing_since_ms: null }))?.words).toBe('The schedule is not counting them on.');
    // The late count is a moving figure, with Not yet; stopped, it waits for the host's own press.
    expect(card(late())).toMatchObject({ words: 'by the schedule', count: { at_ms: at(14, 10, 25), key: 'MREQM2YS' } });
    expect(card(late({ late_ends_ms: null, on_air_at_ms: null, standing_since_ms: null }))?.words).toBe('Nothing is counted: Finlay goes on air by their own press.');
    expect(card(grace({ state: 'connecting', grace_ends_ms: null, standing_since_ms: null }))?.words).toBe('Standing by once their link is steady.');
  });

  it('takes the source’s tile off the stage, and leaves every other tile where it was', () => {
    const placed = stageOf([duncan, finlay(false)], 'solo');
    expect(withoutNext(placed, finlay(false)).stage.map((s) => s.identity)).toEqual(['duncan']);
    expect(withoutNext(placed, null)).toBe(placed);
  });

  it('is drawn in the guest’s empty seat: its name and state, its meter, Cue on its strip’s key, when, and no On', () => {
    show(solo(before()));
    const { body } = render(DeskB);
    expect(body).not.toContain('data-panel="desk.tile.5"');
    expect(body).not.toContain('data-panel="desk.step-note"');
    expect(body).not.toContain('data-panel="desk.waiting"');
    expect(body).toContain('data-panel="desk.tile.1"');
    const next = panel(body, 'desk.next', '</section>');
    expect(text(next)).toContain(`Next Steeped · Finlay Grant Standby`);
    expect(text(next)).toContain(`Own studio · SRT · steady 4:51 The post is at ${hms(at(14, 0))}. Or on air when you end the show. Cue Finlay T`);
    expect(next).toContain('aria-label="Short-term −22.6 LUFS, target −23"');
    expect(next).toMatch(/<b data-panel-title="">Steeped · Finlay Grant<\/b>/);
    // One button, Cue, on the strip's own key; no On, no Talk, no Mute, no Disconnect.
    expect([...next.matchAll(/<button/g)]).toHaveLength(1);
    expect(next).not.toContain('<kbd>5</kbd>');
    expect(next).not.toContain('Disconnect');
  });

  it('cued, it says so, and the button is lit', () => {
    const v = solo(before());
    show({ ...v, sources: v.sources.map((s) => (s.role === 'source' ? { ...s, cue: true } : s)) });
    const next = panel(render(DeskB).body, 'desk.next', '</section>');
    expect(next).toMatch(/class="btn lit-cue" aria-pressed="true"/);
    expect(text(next)).toContain('Cue');
  });

  it('counts a late host in the card, with Not yet', async () => {
    show(solo(late()));
    const { body } = render(DeskB);
    expect(body).not.toContain('data-panel="desk.tile.5"');
    const next = panel(body, 'desk.next', '</section>');
    expect(text(next)).toContain('On air in 0:20 · by the schedule Last 30 seconds. Cue Finlay T Not yet');
    vi.mocked(api.sourceNotYet).mockResolvedValue({ waiting: true });
    await notYet('MREQM2YS');
    expect(text(panel(render(DeskB).body, 'desk.next', '</section>'))).toContain('Nothing is counted: Finlay goes on air by their own press.');
  });

  it('its strip’s number is no On key at Solo: it sends nothing and says how the air passes; Cue still works', async () => {
    show(solo(before()));
    await run({ type: 'on', slot: 5 });
    expect(api.desk).not.toHaveBeenCalled();
    expect(studio.note).toMatchObject({ title: 'Finlay has no On key at Solo', message: 'The air passes to Finlay when you end the show, or by the schedule.' });
    vi.mocked(api.desk).mockResolvedValue(studio.view!);
    await run({ type: 'cue', slot: 5 });
    expect(api.desk).toHaveBeenCalledWith({ op: 'cue', identity: 'source-MREQM2YS', on: true });
  });

  it('above Solo, and with everything shown, the source is a tile as before, with its On key', async () => {
    show({ ...before(), step: { ...NO_SHOW, step: 'guests' } });
    let body = render(DeskB).body;
    expect(body).not.toContain('data-panel="desk.next"');
    expect(body).toContain('data-panel="desk.tile.5"');
    show(solo(before()));
    studio.showAll = true;
    body = render(DeskB).body;
    expect(body).not.toContain('data-panel="desk.next"');
    expect(body).toContain('data-panel="desk.tile.5"');
    vi.mocked(api.desk).mockResolvedValue(studio.view!);
    await run({ type: 'on', slot: 5 });
    expect(api.desk).toHaveBeenCalledWith({ op: 'on_air', identity: 'source-MREQM2YS', on: true });
  });

  it('on air, the source is the show’s own tile, and the guest’s seat says Solo again', () => {
    show(solo(view(at(14, 0, 30), [duncan, finlay(true)], post({ show: kettle(false), source: 'PN6E32MR', source_name: 'Heather Douglas', state: 'not_connected' }))));
    const { body } = render(DeskB);
    expect(body).toContain('data-panel="desk.tile.5"');
    expect(body).not.toContain('data-panel="desk.next"');
    expect(body).toContain('data-panel="desk.step-note"');
  });

  it('a kept place stays a tile on the stage at Solo', () => {
    studio.sourceSlots = { MREQM2YS: 5 };
    show(solo(lost()));
    const { body } = render(DeskB);
    expect(body).toContain('is-kept');
    expect(body).not.toContain('data-panel="desk.next"');
  });
});

describe('the desk’s actions on the post', () => {
  it('Hold asks once and waits for the view; Let it run asks the other way', async () => {
    show(grace());
    vi.mocked(api.holdPost).mockResolvedValue({ held: true });
    await holdPost(true);
    expect(api.holdPost).toHaveBeenCalledTimes(1);
    expect(studio.holdAsked).toMatchObject({ was: null });
    // A second press before the view follows would undo the first: it does nothing.
    await holdPost(false);
    expect(api.releasePost).not.toHaveBeenCalled();
    expect(render(DeskB).body).toMatch(/class="btn sm hold" disabled/);
    takeView(grace({ grace_ends_ms: null, held_by: 'shona' }));
    expect(studio.holdAsked).toBeNull();
    vi.mocked(api.releasePost).mockResolvedValue({ held: false });
    await holdPost(false);
    expect(api.releasePost).toHaveBeenCalledTimes(1);
    expect(studio.holdAsked).toMatchObject({ was: 'shona' });
  });

  it('shows the station’s own sentence when it refuses a hold, and frees the button', async () => {
    show(grace());
    vi.mocked(api.holdPost).mockRejectedValue(new api.ApiError(403, 'Only an admin or a producer holds a handover.'));
    await holdPost(true);
    expect(studio.note).toMatchObject({ title: 'Not done', message: 'Only an admin or a producer holds a handover.' });
    expect(studio.holdAsked).toBeNull();
  });

  it('⇧⏎ is Go on air now, by the key, and never the desk’s own switch', async () => {
    show(late());
    vi.mocked(api.sourceGo).mockResolvedValue({ on_air: true });
    await run({ type: 'handover' });
    expect(api.sourceGo).toHaveBeenCalledWith('MREQM2YS');
    expect(api.desk).not.toHaveBeenCalled();
    expect(studio.note).toMatchObject({ title: 'Finlay is on air' });
  });

  it('shows the station’s sentence when it refuses the press', async () => {
    show(late());
    vi.mocked(api.sourceGo).mockRejectedValue(new api.ApiError(409, 'that source is not steady yet: it stands by after a few seconds of steady sound'));
    await goOnAir('MREQM2YS', 'Finlay');
    expect(studio.note).toMatchObject({ title: 'Not done', message: 'that source is not steady yet: it stands by after a few seconds of steady sound' });
  });

  it('Esc on the late toast stops the count, by the key, and the count leaves at once', async () => {
    show(late());
    expect(deskToastNow()).toBe('late');
    vi.mocked(api.sourceNotYet).mockResolvedValue({ waiting: true });
    await run({ type: 'escape' });
    expect(api.sourceNotYet).toHaveBeenCalledWith('MREQM2YS');
    expect(studio.view?.post).toMatchObject({ late_ends_ms: null, on_air_at_ms: null });
    expect(studio.note).toMatchObject({ title: 'The count is stopped' });
    // The arrival was not dismissed by the same press.
    expect(studio.toldSources).toEqual(['source-MREQM2YS', 'source-PN6E32MR']);
  });

  it('Not yet on the tile is the same send', async () => {
    show(late());
    vi.mocked(api.sourceNotYet).mockResolvedValue({ waiting: true });
    await notYet('MREQM2YS');
    expect(api.sourceNotYet).toHaveBeenCalledWith('MREQM2YS');
    expect(lateToast(studio.view, LATE)).toBeNull();
  });

  it('Esc answers the toast on screen: a lost link is dismissed, and no count behind it is stopped', async () => {
    // A lost link that automation covers, while another show's late host is counted (rare, and
    // the reason for one rule): the lost link is the toast, so Esc dismisses that.
    show({ ...late(), post: latePost({ link_lost: lostLink({ key_id: 'PN6E32MR', name: 'Heather Douglas' }) }) });
    expect(deskToastNow()).toBe('lost');
    await run({ type: 'escape' });
    expect(studio.lostDismissed).toEqual([`PN6E32MR@${at(14, 41)}`]);
    expect(api.sourceNotYet).not.toHaveBeenCalled();
    // The place is still kept; only the toast went. The next Esc answers the late count.
    expect(keptPlace(studio.view, {})).not.toBeNull();
    expect(deskToastNow()).toBe('late');
  });

  it('Dismiss keeps the place', () => {
    show(lost());
    dismissLost();
    expect(studio.lostDismissed).toEqual([`MREQM2YS@${at(14, 41)}`]);
    expect(render(DeskB).body).toContain('is-kept');
    expect(render(DeskB).body).not.toContain('data-panel="desk.toast"');
  });

  it('the kept strip’s own On key is Off air: one send by the key, the place leaves at once, and the desk says where the air is', async () => {
    studio.sourceSlots = { MREQM2YS: 5 };
    show({ ...lost({ covered: false }), mode: 'live' });
    vi.mocked(api.sourceOffAir).mockResolvedValue({ let_go: true, covered: true });
    await run({ type: 'on', slot: 5 });
    expect(api.sourceOffAir).toHaveBeenCalledTimes(1);
    expect(api.sourceOffAir).toHaveBeenCalledWith('MREQM2YS');
    expect(api.desk).not.toHaveBeenCalled();
    expect(studio.note).toMatchObject({ title: 'Finlay’s place is let go', message: 'Automation has taken the air. If Finlay reconnects, they stand by.' });
    // The reply has no view: the kept place leaves this window's at once, so the tile goes and
    // a second press is an empty strip's, not a second send for the station to refuse.
    expect(studio.view?.post?.link_lost).toBeNull();
    expect(studio.view?.post?.state).toBe('not_connected');
    expect(render(DeskB).body).not.toContain('is-kept');
    expect(text(panel(render(DeskB).body, 'desk.rundown', '</section>'))).not.toContain('Link lost');
    await run({ type: 'on', slot: 5 });
    expect(api.sourceOffAir).toHaveBeenCalledTimes(1);
    expect(studio.note?.title).toBe('Nobody is on strip 5');
  });

  it('after Off air a view that still carries the place is read without it: no tile, no toast, no second send', async () => {
    studio.sourceSlots = { MREQM2YS: 5 };
    show(lost());
    vi.mocked(api.sourceOffAir).mockResolvedValue({ let_go: true, covered: false });
    await offAirKept('MREQM2YS');
    expect(studio.letGo).toEqual([`MREQM2YS@${at(14, 41)}`]);
    // The station's `post` lags a decision by up to a second: its next view still says lost.
    takeView({ ...lost(), now_ms: at(14, 41, 8) });
    expect(studio.view?.post?.link_lost).toBeNull();
    expect(keptPlace(studio.view, studio.sourceSlots)).toBeNull();
    studio.note = null;
    const { body } = render(DeskB);
    expect(body).not.toContain('is-kept');
    expect(body).not.toContain('data-panel="desk.toast"');
    await run({ type: 'on', slot: 5 });
    expect(api.sourceOffAir).toHaveBeenCalledTimes(1);
    // The view catches up, and the window forgets: a later loss of the same key is said again.
    takeView(view(at(14, 41, 9), [], post({ show: steeped(true), source: 'MREQM2YS', source_name: 'Finlay Grant', state: 'not_connected' }), { mode: 'auto' }));
    expect(studio.letGo).toEqual([]);
    takeView(lost({ lost_ms: at(14, 50) }));
    expect(keptPlace(studio.view, studio.sourceSlots)).not.toBeNull();
  });

  it('Off air with automation already covering says so, and the station’s refusal is shown as given', async () => {
    studio.sourceSlots = { MREQM2YS: 5 };
    show(lost());
    vi.mocked(api.sourceOffAir).mockResolvedValue({ let_go: true, covered: false });
    await offAirKept('MREQM2YS');
    expect(studio.note?.message).toBe('Automation holds the air. If Finlay reconnects, they stand by.');
    show(lost());
    vi.mocked(api.sourceOffAir).mockRejectedValue(new api.ApiError(409, 'no place is being kept for that source: its link has not been lost, or its place is let go already'));
    await offAirKept('MREQM2YS');
    expect(studio.note).toMatchObject({ title: 'Not done', message: 'no place is being kept for that source: its link has not been lost, or its place is let go already' });
    // The place is still drawn: the station did not let it go.
    expect(keptPlace(studio.view, studio.sourceSlots)).not.toBeNull();
  });

  it('the kept strip’s other keys send nothing and say why', async () => {
    studio.sourceSlots = { MREQM2YS: 5 };
    show(lost());
    await run({ type: 'cue', slot: 5 });
    expect(studio.note).toMatchObject({ title: 'Nothing to cue', message: 'Finlay’s link is lost: nothing arrives from their software.' });
    await run({ type: 'mute', slot: 5 });
    expect(studio.note?.title).toBe('Nothing to mute');
    expect(api.desk).not.toHaveBeenCalled();
    // An empty strip elsewhere still says it is empty.
    await run({ type: 'on', slot: 2 });
    expect(studio.note?.title).toBe('Nobody is on strip 2');
  });

  it('Disconnect on the kept tile goes by the key and names the host', async () => {
    show(lost());
    vi.mocked(api.disconnectSource).mockResolvedValue({ disconnected: true, until_ms: at(18, 0) });
    await run({ type: 'disconnect', identity: 'source-MREQM2YS', key: 'MREQM2YS' });
    expect(api.disconnectSource).toHaveBeenCalledWith('MREQM2YS');
    expect(studio.note?.title).toBe('Finlay is disconnected');
  });
});

// ---- the driver's stand-in against the station's own description ----

describe('the stand-in station’s post (tests/hosts/station.mjs)', () => {
  interface Schema {
    properties?: Record<string, unknown>;
    required?: string[];
  }
  const spec = openapi as unknown as { components: { schemas: Record<string, Schema> }; paths: Record<string, Record<string, unknown>> };
  function matches(value: object, name: string) {
    const s = spec.components.schemas[name];
    expect(s, name).toBeTruthy();
    const got = Object.keys(value);
    expect((s.required ?? []).filter((k) => !got.includes(k)), `${name}: missing`).toEqual([]);
    expect(got.filter((k) => !Object.keys(s.properties ?? {}).includes(k)), `${name}: not the station’s`).toEqual([]);
  }

  it('the studio’s own shapes carry the fields the station sends', () => {
    matches(gracePost(), 'PostView');
    matches(kettle(true), 'PostShow');
    matches(lostLink(), 'LinkLostView');
    expect(Object.keys(post({})).sort()).toEqual(Object.keys(spec.components.schemas.PostView.properties ?? {}).sort());
    expect((spec.components.schemas.SourceState as unknown as { enum: string[] }).enum).toEqual(['not_connected', 'connecting', 'standing_by', 'on_air', 'link_lost']);
    expect(Object.keys(spec.components.schemas.EndShowResult.properties ?? {})).toContain('passed_to');
    expect(Object.keys(spec.components.schemas.View.properties ?? {})).toContain('post');
  });

  it('answers the post in the station’s own field names, and the routes it has', async () => {
    const path = new URL('../../../tests/hosts/station.mjs', import.meta.url).href;
    const { HostStation } = (await import(/* @vite-ignore */ path)) as { HostStation: new (o?: object) => any };
    const st = new HostStation({ step: 'full', nowMinutes: 16 * 60 + 1 });
    st.now += 18;
    const f = st.give('steeped', 'Finlay Grant', 'srt');
    const h = st.give('kettle-drive', 'Heather Douglas', 'icecast');
    st.connect(f.key_id, 'srt', 'mpegts/aac 48 kHz stereo', { ago_s: 7340 });
    st.people[0].on_air = true;
    st.acted.add(f.key_id);
    st.connect(h.key_id, 'icecast', 'mp3 44.1 kHz stereo', { ago_s: 203 });
    let p = st.studioView().post as PostView;
    matches(p, 'PostView');
    matches(p.show!, 'PostShow');
    expect(p).toMatchObject({ state: 'standing_by', source_name: 'Heather Douglas', grace_ends_ms: at(16, 2), held_by: null, holders: ['Finlay Grant'] });
    // The studio reads it as the design's moment.
    expect(postLine(p, st.now * 1000)).toMatchObject({ kind: 'count', why: 'grace', at_ms: at(16, 2) });

    for (const route of ['/api/v1/air/post/hold', '/api/v1/sources/{key}/go', '/api/v1/sources/{key}/not-yet', '/api/v1/sources/{key}/off-air', '/api/v1/show/end']) expect(spec.paths[route], route).toBeTruthy();
    expect(Object.keys(spec.paths['/api/v1/air/post/hold']).sort()).toEqual(['delete', 'post']);
    expect(st.handle('POST', '/api/v1/air/post/hold')).toEqual([200, { held: true, by: 'shona', airing_start_ms: at(16, 0) }]);
    p = st.studioView().post;
    expect(p).toMatchObject({ held_by: 'shona', grace_ends_ms: null });
    expect(st.handle('DELETE', '/api/v1/air/post/hold')[0]).toBe(200);
    // Someone presents: Go on air now is refused, as the station refuses it.
    expect(st.handle('POST', `/api/v1/sources/${h.key_id}/go`)[0]).toBe(409);
    expect(st.handle('POST', '/api/v1/sources/NOSUCHKEY/go')[0]).toBe(404);

    const ended = st.handle('POST', '/api/v1/show/end', {})[1];
    matches(ended, 'EndShowResult');
    expect(ended).toMatchObject({ off_air: 1, automation_in_ms: null, passed_to: 'Heather Douglas' });

    // A lost link, in the station's shape.
    st.lose(h.key_id);
    st.cover();
    p = st.studioView().post;
    matches(p.link_lost!, 'LinkLostView');
    expect(p.link_lost).toMatchObject({ key_id: h.key_id, covered: true, place_kept_until_ms: st.now * 1000 + 300_000 });
    // A screener may not hold.
    st.role = 'screener';
    expect(st.handle('POST', '/api/v1/air/post/hold')[0]).toBe(403);
    // A station older than 1.1 sends no post at all.
    expect('post' in new HostStation({ routes: false }).studioView()).toBe(false);
  });
});
