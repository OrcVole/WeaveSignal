// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// When a show airs (ADR-0278; new-show.md), tested without a page: the questions as a form
// and the rule they send, the Repeats rows made from one date, the preview asked 250 ms after
// the last change with old answers dropped, what the control plane's reply becomes on the
// page (the rows, the small months, the line about clashes, what holds the save), the choice
// to move or leave drafted episodes, the version gate, the replies made whole, and the
// dialogs rendered on the server from a seeded preview. The API is a stand-in: a fetch that
// records what was sent. Sample shows and people are Kettle FM's (ADR-0071); every date was
// checked with `date`: 1 December 2026 is a Tuesday, and the first Tuesdays after it are
// 5 January, 2 February, 2 March, 6 April and 4 May 2027.
import { render } from 'svelte/server';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CALENDAR, PANELS, panel } from '../../../packages/ui/panels.ts';
import * as api from './lib/api.ts';
import { FieldError, asNextOn, asPreview, asScheduleDates, refusedClash, speaksRules } from './lib/api.ts';
import type { Clash, Me, PreviewDate, Rule, RulePreview, ShowSaved } from './lib/api.ts';
import * as schedApi from './lib/schedule.ts';
import type { EpisodeView, Show, ShowView } from './lib/schedule.ts';
import { sched } from './lib/schedule.svelte.ts';
import { instant } from './schedule/model.ts';
import Schedule from './schedule/Schedule.svelte';
import ShowEditor from './schedule/ShowEditor.svelte';
import NewShow from './schedule/rule/NewShow.svelte';
import RuleChange from './schedule/rule/RuleChange.svelte';
import RuleDialog from './schedule/rule/RuleDialog.svelte';
import RulePart from './schedule/rule/RulePart.svelte';
import Unscheduled from './schedule/rule/Unscheduled.svelte';
import {
  addDays,
  addMonths,
  choiceKey,
  choices,
  dateWords,
  dropGone,
  fitsOldStation,
  formProblems,
  fromRule,
  lengthWords,
  minutesWords,
  newForm,
  newShowInput,
  pickChoice,
  pickKind,
  previewRequest,
  ruleOfSlot,
  setFirst,
  shortOfSlot,
  slotOf,
  times,
  toList,
  toRule,
  toggleDate,
  toggleWeekday,
  weekdayOf,
  whenFields,
  withMove,
  withSkip,
  withoutMove,
  withoutSkip,
  type RuleForm,
} from './schedule/rule/form.ts';
import { enterIsPrimary, escStep, isTextField } from './schedule/rule/keys.ts';
import { EMPTY_PREVIEW, Previewer, type PreviewState } from './schedule/rule/previewer.ts';
import {
  afterWords,
  airedWords,
  anyClash,
  boundsWords,
  byHandLines,
  changeLines,
  clashNotice,
  countWords,
  firstWords,
  held,
  months,
  movable,
  moveBounds,
  rows,
  saveWords,
  firstState,
  madeInAll,
  ruleStatus,
  unscheduled,
} from './schedule/rule/view.ts';
import ShowWizard from './settings/ShowWizard.svelte';

/** A rendered body without the markers Svelte leaves between its blocks. */
const clean = (body: string) => body.replace(/<!--.*?-->/g, '');
const ZONE = 'Europe/London';
const TODAY = '2026-11-13';
/** A wall-clock time in Edinburgh as epoch seconds. */
const at = (iso: string, hh: number, mm = 0) => {
  const [y, m, d] = iso.split('-').map(Number);
  return instant({ y, m, d, hh, mm }, ZONE) / 1000;
};

const FIRTH: Clash = { show: 'Firth Sessions', slug: 'firth-sessions', start: at('2026-12-01', 20, 30), minutes: 60, overlap_minutes: 30, when: 'Tuesdays, 20:30 to 21:30', with: [], refused: false };
const day = (on: string, number: number | null, o: Partial<PreviewDate> = {}): PreviewDate => ({
  on,
  number,
  start: at(on, 20),
  minutes: 60,
  state: 'airs',
  now: false,
  moved_from: null,
  clock: null,
  episode: null,
  clashes: [],
  ...o,
});

/** The Folk Club: every month on the first Tuesday, 20:00 to 21:00, for 10 episodes, 5 January skipped. */
const FOLK_RULE: Rule = {
  starts_on: '2026-12-01',
  start: 1200,
  minutes: 60,
  repeat: { kind: 'monthly_nth', every: 1, nth: 1, weekday: 2 },
  end: { kind: 'count', count: 10 },
  skip: ['2027-01-05'],
  moves: [],
};
const FOLK: RulePreview = {
  ...EMPTY_PREVIEW,
  sentence: 'Every month on the first Tuesday, 20:00 to 21:00, from 1 December, for 10 episodes. Not on 5 January 2027.',
  short: 'First Tue monthly, 20:00–21:00',
  last: 'The last is Tuesday 5 October 2027.',
  total: 10,
  dates: [
    day('2026-12-01', 1),
    day('2027-01-05', null, { state: 'skipped' }),
    day('2027-02-02', 2, { start: at('2027-02-09', 20), moved_from: '2027-02-02T20:00:00' }),
    day('2027-03-02', 3, { clashes: [{ ...FIRTH, show: 'Six Nations Preview', slug: 'six-nations-preview', start: at('2027-03-02', 19, 30), when: null }] }),
    day('2027-04-06', 4),
    day('2027-05-04', 5),
  ],
};

const show = (over: Partial<Show> = {}): Show => ({
  id: 'id-folk',
  slug: 'the-folk-club',
  name: 'The Folk Club',
  description: '',
  colour_slot: 11,
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
  presenter_account: null,
  ...over,
});

describe('one date and two times', () => {
  it('reads a date for what it says of itself, with no station', () => {
    expect(weekdayOf('2026-12-01')).toBe(2);
    expect(weekdayOf('2026-11-30')).toBe(1);
    expect(weekdayOf('2026-12-31')).toBe(4);
    expect(weekdayOf('not a date')).toBe(0);
    expect(weekdayOf('2027-02-30')).toBe(0);
    expect(dateWords('2026-12-01', { weekday: 'long' })).toBe('Tuesday 1 December');
    expect(dateWords('2027-01-05', { weekday: 'short', month: 'short', year: true })).toBe('Tue 5 Jan 2027');
    expect(addDays('2026-12-31', 1)).toBe('2027-01-01');
    expect(addMonths('2027-01-31', 1)).toBe('2027-02-28');
  });

  it('makes a length from from and until, and asks about an end before the start (ADR-0244)', () => {
    expect(times({ from: '20:00', until: '21:00' })).toEqual({ start: 1200, minutes: 60, overnight: false });
    expect(times({ from: '22:00', until: '00:30' })).toEqual({ start: 1320, minutes: 150, overnight: true });
    expect(times({ from: '20:00', until: '' })).toBeNull();
    expect(lengthWords(60)).toBe('1 hour');
    expect(lengthWords(150)).toBe('2 h 30 min');
    expect(lengthWords(45)).toBe('45 min');
    expect(minutesWords(30)).toBe('30 minutes');
    expect(minutesWords(60)).toBe('an hour');
    const night = { ...newForm('2026-11-16', '20:00', '19:00') };
    expect(formProblems(night).map((p) => p.field)).toEqual(['overnight']);
    expect(toRule(night)).toBeNull();
    expect(toRule({ ...night, overnightOk: true })).toMatchObject({ start: 1200, minutes: 1380 });
  });
});

describe('Repeats, as a list made from the date', () => {
  it('offers the sentences for Tuesday 1 December 2026, with Once only picked', () => {
    expect(choices('2026-12-01').map((c) => c.label)).toEqual([
      'Once only',
      'Every week on Tuesday',
      'Every month on the first Tuesday',
      'Every year on 1 December',
      'On dates I choose…',
    ]);
    const f = newForm('2026-12-01');
    expect(choiceKey(f)).toBe('once');
    expect(f.mode).toBe('list');
  });

  it('adds the last of its weekday, and offers a fifth only as the last', () => {
    // 24 November 2026 is the fourth and the last Tuesday of its month.
    expect(choices('2026-11-24').map((c) => c.label)).toContain('Every month on the fourth Tuesday');
    expect(choices('2026-11-24').map((c) => c.label)).toContain('Every month on the last Tuesday');
    // 29 December 2026 is a fifth Tuesday.
    const fifth = choices('2026-12-29').map((c) => c.label);
    expect(fifth).toContain('Every month on the last Tuesday');
    expect(fifth.some((l) => /fifth|fourth/.test(l))).toBe(false);
  });

  it('turns each row into the rule the design gives (section 9)', () => {
    const f = newForm('2026-12-01');
    expect(toRule(f)).toEqual({ starts_on: '2026-12-01', start: 1200, minutes: 60, repeat: { kind: 'once' }, end: { kind: 'never' }, skip: [], moves: [] });
    expect(toRule(pickChoice(f, 'weekly'))?.repeat).toEqual({ kind: 'weekly', every: 1, weekdays: [2] });
    expect(toRule(pickChoice(f, 'nth1'))?.repeat).toEqual({ kind: 'monthly_nth', every: 1, nth: 1, weekday: 2 });
    expect(toRule(pickChoice(newForm('2026-11-27'), 'nth-1'))?.repeat).toEqual({ kind: 'monthly_nth', every: 1, nth: -1, weekday: 5 });
    expect(toRule(pickChoice(newForm('2026-12-31'), 'yearly'))?.repeat).toEqual({ kind: 'yearly', month: 12, day: 31 });
    // On dates I choose… opens the whole rule, with the first date chosen.
    const dates = pickChoice(f, 'dates');
    expect(dates.mode).toBe('full');
    expect(toRule(dates)?.repeat).toEqual({ kind: 'dates', dates: ['2026-12-01'] });
  });

  it('follows First on while it is a list, and keeps the pattern in the whole rule', () => {
    const weekly = pickChoice(newForm('2026-12-01'), 'weekly');
    expect(toRule(setFirst(weekly, '2026-12-02'))?.repeat).toEqual({ kind: 'weekly', every: 1, weekdays: [3] });
    expect(toRule(setFirst({ ...weekly, mode: 'full' }, '2026-12-02'))?.repeat).toEqual({ kind: 'weekly', every: 1, weekdays: [2] });
    // The last Friday stays the last Friday when the new date is one too.
    const last = pickChoice(newForm('2026-11-27'), 'nth-1');
    expect(choiceKey(setFirst(last, '2026-12-25'))).toBe('nth-1');
  });
});

describe('Repeats, as the whole rule', () => {
  it('builds the design’s example: the first Tuesday, ten episodes, one skipped, one moved', () => {
    let f = pickChoice(newForm('2026-12-01'), 'nth1');
    f = { ...f, end: { kind: 'count', until: '', count: 10 } };
    f = withSkip(f, '2027-01-05');
    f = withMove(f, '2027-04-06', '2027-04-13', '20:00');
    expect(toRule(f)).toEqual({
      starts_on: '2026-12-01',
      start: 1200,
      minutes: 60,
      repeat: { kind: 'monthly_nth', every: 1, nth: 1, weekday: 2 },
      end: { kind: 'count', count: 10 },
      skip: ['2027-01-05'],
      moves: [{ from: '2027-04-06', to: '2027-04-13T20:00:00', minutes: null }],
    });
    expect(toRule(withoutMove(withoutSkip(f, '2027-01-05'), '2027-04-06'))).toMatchObject({ skip: [], moves: [] });
    // Skipping a moved date takes its move away.
    expect(withSkip(f, '2027-04-06').moves).toEqual([]);
  });

  it('takes several weekdays, every second week, to a date', () => {
    let f = pickKind({ ...newForm('2026-11-18', '21:00', '23:00'), mode: 'full' }, 'weekly');
    f = toggleWeekday(f, 5);
    f = { ...f, every: 2, end: { kind: 'until', until: '2027-02-26', count: 10 } };
    expect(toRule(f)).toMatchObject({ repeat: { kind: 'weekly', every: 2, weekdays: [3, 5] }, end: { kind: 'until', until: '2027-02-26' } });
    // The list cannot say it; Fewer choices remakes it from the date, in its own kind.
    expect(choiceKey(f)).toBeNull();
    expect(toRule(toList(f))?.repeat).toEqual({ kind: 'weekly', every: 1, weekdays: [3] });
    expect(formProblems(toggleWeekday(toggleWeekday(f, 3), 5)).map((p) => p.message)).toEqual(['Press at least one day.']);
  });

  it('keeps a day of the month and the last day, a date each year, and chosen dates', () => {
    const f = { ...newForm('2026-12-31'), mode: 'full' as const };
    expect(toRule({ ...pickKind(f, 'monthly'), kind: 'monthly_day', day: 31 })?.repeat).toEqual({ kind: 'monthly_day', every: 1, day: 31 });
    expect(toRule({ ...pickKind(f, 'monthly'), kind: 'monthly_day', day: -1 })?.repeat).toEqual({ kind: 'monthly_day', every: 1, day: -1 });
    expect(toRule(pickKind(f, 'yearly'))?.repeat).toEqual({ kind: 'yearly', month: 12, day: 31 });
    let d = pickKind({ ...newForm('2026-12-05', '14:00', '16:00'), mode: 'full' }, 'dates');
    for (const iso of ['2026-12-24', '2026-12-12', '2026-12-19']) d = toggleDate(d, iso);
    const rule = toRule(d);
    expect(rule?.repeat).toEqual({ kind: 'dates', dates: ['2026-12-05', '2026-12-12', '2026-12-19', '2026-12-24'] });
    // Chosen dates and a one-off have no end and nothing skipped.
    expect(rule?.end).toEqual({ kind: 'never' });
    expect(toRule({ ...d, end: { kind: 'count', until: '', count: 3 }, skip: ['2026-12-12'] })).toMatchObject({ end: { kind: 'never' }, skip: [] });
    expect(formProblems(toggleDate(pickKind({ ...newForm('2026-12-05'), mode: 'full' }, 'dates'), '2026-12-05')).map((p) => p.message)).toEqual(['Pick at least one date in the calendar.']);
  });

  it('reads a stored rule back into the questions, and an old weekly slot too', () => {
    const f = fromRule(FOLK_RULE, '2026-12-01');
    expect(f.mode).toBe('list');
    expect(choiceKey(f)).toBe('nth1');
    expect(toRule(f)).toEqual(FOLK_RULE);
    // A person who last used the whole rule opens on it.
    expect(fromRule(FOLK_RULE, '2026-12-01', true).mode).toBe('full');
    const slot = ruleOfSlot({ weekday: 6, start: 1200, minutes: 180 });
    expect(slotOf(slot)).toEqual({ weekday: 6, start: 1200, minutes: 180 });
    expect(shortOfSlot({ weekday: 6, start: 1200, minutes: 180 })).toBe('Sat, 20:00–23:00');
    // Its next date stands in for the start date it never had; the pattern is its own.
    const old = fromRule(slot, '2026-11-14');
    expect(toRule(old)).toEqual({ ...slot, starts_on: '2026-11-14' });
    // A date that is not its weekday cannot be said on the list.
    expect(fromRule(slot, '2026-11-13').mode).toBe('full');
  });

  it('drops skipped and moved dates the new pattern no longer gives, and says so once', () => {
    const f = withMove(withSkip(fromRule(FOLK_RULE, '2026-12-01'), '2027-03-02'), '2027-04-06', '2027-04-13', '20:00');
    const d = dropGone(f, { skip: ['2027-01-05'], moves: [] });
    expect(d.words).toBe('1 skipped date no longer applies.');
    expect(d.form.skip).toEqual(['2027-03-02']);
    expect(dropGone(f, { skip: ['2027-01-05', '2027-03-02'], moves: ['2027-04-06'] }).words).toBe('2 skipped dates and 1 moved date no longer apply.');
    expect(dropGone(f, { skip: [], moves: [] })).toEqual({ form: f, words: '' });
  });
});

describe('what is sent to the station', () => {
  it('asks for a preview of the rule, with the show when it exists and ten dates', () => {
    const f = fromRule(FOLK_RULE, '2026-12-01');
    expect(previewRequest(f)).toEqual({ rule: FOLK_RULE, limit: 10 });
    expect(previewRequest(f, { show: 'the-folk-club', series: 2, limit: 30 })).toEqual({ rule: FOLK_RULE, show: 'the-folk-club', series: 2, limit: 30 });
    expect(previewRequest(f, { limit: 90 })?.limit).toBe(60);
    // A form with a problem of its own asks nothing.
    expect(previewRequest({ ...f, from: '' })).toBeNull();
  });

  it('makes a new show with its rule and the first episode drafted in one save', () => {
    const body = newShowInput({ name: ' The Folk Club ', colour: 11, step: 'solo' }, FOLK_RULE, true);
    expect(body).toEqual({ name: 'The Folk Club', description: '', colour_slot: 11, layout: 'poster', hosts: [], tags: [], explicit: false, step: 'solo', rule: FOLK_RULE, draft_first: true });
    // No dates yet: the show alone, with no rule and nothing drafted.
    const none = newShowInput({ name: 'Bothy Ballads', colour: 2, step: 'guests' }, null, true);
    expect(none.rule).toBeNull();
    expect('draft_first' in none).toBe(false);
  });

  it('never sends rule or draft_first to a station older than 1.0.4', () => {
    const weekly = ruleOfSlot({ weekday: 2, start: 1260, minutes: 60 });
    const old = newShowInput({ name: 'Late Tales', colour: 5, step: 'solo' }, { ...weekly, starts_on: '2026-11-17' }, false);
    expect(old.slot).toEqual({ weekday: 2, start: 1260, minutes: 60 });
    expect('rule' in old || 'draft_first' in old).toBe(false);
    // A rule that station cannot keep is not sent as something else.
    expect(newShowInput({ name: 'The Folk Club', colour: 11, step: 'solo' }, FOLK_RULE, false).slot).toBeNull();
    expect(fitsOldStation(pickChoice(newForm('2026-11-17'), 'weekly'))).toBe(true);
    expect(fitsOldStation(newForm('2026-11-17'))).toBe(false);
    expect(fitsOldStation(pickChoice(newForm('2026-12-01'), 'nth1'))).toBe(false);
  });

  it('carries the stored rule with every other save, so a change of name cannot clear it', () => {
    // Section 9: a body with neither `rule` nor `slot` clears a weekly rule on a newer station.
    const weekly = ruleOfSlot({ weekday: 6, start: 1200, minutes: 180 });
    expect(whenFields(weekly, { rules: true })).toEqual({ rule: weekly });
    expect(whenFields(null, { rules: true })).toEqual({ rule: null });
    expect(whenFields(weekly, { rules: false })).toEqual({ slot: { weekday: 6, start: 1200, minutes: 180 } });
    const view = { show: show({ rule: weekly, slot: { weekday: 6, start: 1200, minutes: 180 }, presenter: 'Moira Henderson' }), resolved: { hosts: ['Moira Henderson'] }, series: [] } as unknown as ShowView;
    const body = { ...schedApi.inputOf(view), ...whenFields(view.show.rule ?? null, { rules: true }), slot: view.show.slot };
    expect(body).toMatchObject({ name: 'The Folk Club', hosts: ['Moira Henderson'], presenter: 'Moira Henderson', rule: weekly, slot: { weekday: 6, start: 1200, minutes: 180 } });
    expect('draft_first' in body).toBe(false);
  });
});

describe('the station’s version', () => {
  it('sends a rule only to 1.0.4 or later, counting the numbers alone', () => {
    expect(speaksRules('1.0.4')).toBe(true);
    expect(speaksRules('1.0.4-test.2')).toBe(true);
    expect(speaksRules('1.1.0')).toBe(true);
    expect(speaksRules('2.0.0')).toBe(true);
    expect(speaksRules('1.0.3')).toBe(false);
    expect(speaksRules('1.0.3-test.9')).toBe(false);
    expect(speaksRules('0.9.9')).toBe(false);
    expect(speaksRules(null)).toBe(false);
    expect(speaksRules('nightly')).toBe(false);
  });
});

describe('the calls, against a stand-in station', () => {
  const sent: { url: string; method: string; body: unknown }[] = [];
  let reply: { status: number; body: unknown } = { status: 200, body: {} };
  beforeEach(() => {
    sent.length = 0;
    vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
      sent.push({ url, method: init?.method ?? 'GET', body: init?.body ? JSON.parse(String(init.body)) : undefined });
      return new Response(JSON.stringify(reply.body), { status: reply.status, headers: { 'content-type': 'application/json' } });
    });
  });
  afterEach(() => vi.unstubAllGlobals());

  it('posts the rule for a preview and makes the reply whole', async () => {
    reply = { status: 200, body: { sentence: 'Once, on Monday 30 November, 19:00 to 21:00.', short: 'Once, Mon 30 Nov, 19:00–21:00', dates: [{ on: '2026-11-30', number: 1, start: at('2026-11-30', 19), minutes: 120 }], total: 1 } };
    const p = await api.previewRule({ rule: FOLK_RULE, limit: 10 });
    expect(sent[0]).toEqual({ url: '/api/v1/schedule/rule/preview', method: 'POST', body: { rule: FOLK_RULE, limit: 10 } });
    expect(p.sentence).toBe('Once, on Monday 30 November, 19:00 to 21:00.');
    expect(p.dates[0]).toEqual(day('2026-11-30', 1, { start: at('2026-11-30', 19), minutes: 120 }));
    expect(p).toMatchObject({ last: null, clear: null, problems: [], hints: [], dropped: { skip: [], moves: [] }, change: null, total: 1, more: null });
  });

  it('saves a show with draft_first and returns the drafted episode', async () => {
    const ep = { episode: { id: 'e1', number: 1, air_at: at('2026-12-01', 20), minutes: 60 }, resolved: {} } as unknown as EpisodeView;
    reply = { status: 200, body: { show: show({ rule: FOLK_RULE }), resolved: {}, series: [{ number: 1 }], drafted: ep } };
    const v = await api.saveShow('the-folk-club', newShowInput({ name: 'The Folk Club', colour: 11, step: 'solo' }, FOLK_RULE, true));
    expect(sent[0].url).toBe('/api/v1/shows/the-folk-club');
    expect(sent[0].method).toBe('PUT');
    expect(sent[0].body).toMatchObject({ name: 'The Folk Club', rule: FOLK_RULE, draft_first: true });
    expect(v.drafted?.episode.id).toBe('e1');
    // A reply with nothing drafted (No dates yet, or an older station) says null, never undefined.
    reply = { status: 200, body: { show: show(), resolved: {}, series: [] } };
    expect((await api.saveShow('the-folk-club', newShowInput({ name: 'The Folk Club', colour: 11, step: 'solo' }, null, true))).drafted).toBeNull();
  });

  it('saves the rule alone with what the drafted episodes do, for a show and for a series', async () => {
    reply = { status: 200, body: { rule: FOLK_RULE, rule_words: { sentence: 'Every month…', short: 'First Tue monthly, 20:00–21:00' }, moved: [{ series: 1, number: 4, from: 1, to: 2 }] } };
    const changed = await api.putShowRule('the-folk-club', FOLK_RULE, 'move');
    expect(sent[0]).toEqual({ url: '/api/v1/shows/the-folk-club/rule', method: 'PUT', body: { rule: FOLK_RULE, drafted: 'move' } });
    expect(changed.moved).toHaveLength(1);
    expect(changed.rule_words?.short).toBe('First Tue monthly, 20:00–21:00');
    await api.putSeriesRule('the-folk-club', 2, null, 'leave');
    expect(sent[1]).toEqual({ url: '/api/v1/shows/the-folk-club/series/2/rule', method: 'PUT', body: { rule: null, drafted: 'leave' } });
  });

  it('keeps a refusal’s field and the clash a presenter is refused over', async () => {
    reply = { status: 409, body: { error: 'Saturday Night Kettle has that time.', field: 'rule', clash: { show: 'Saturday Night Kettle', slug: 'saturday-night', start: 1, minutes: 180, overlap_minutes: 120, with: ['Shona Campbell'], refused: true } } };
    const e = await api.saveShow('the-wee-small-hours', newShowInput({ name: 'The Wee Small Hours', colour: 5, step: 'solo' }, FOLK_RULE, true)).catch((x: unknown) => x);
    expect(e).toBeInstanceOf(FieldError);
    expect((e as FieldError).field).toBe('rule');
    expect((e as FieldError).message).toBe('Saturday Night Kettle has that time.');
    expect(refusedClash(e)).toMatchObject({ show: 'Saturday Night Kettle', refused: true, with: ['Shona Campbell'] });
    expect(refusedClash(new Error('no'))).toBeNull();
  });

  it('reads the version from /health, and says null when it cannot', async () => {
    reply = { status: 200, body: { status: 'ok', version: '1.0.4', database: 'ok' } };
    expect(await api.stationVersion()).toBe('1.0.4');
    expect(sent[0].url).toBe('/health');
    reply = { status: 200, body: {} };
    expect(await api.stationVersion()).toBeNull();
  });

  it('takes the week’s dates beside its episodes, and none from an older station', async () => {
    reply = { status: 200, body: { zone: ZONE, episodes: [], dates: [{ show: 'the-folk-club', series: 1, on: '2027-01-05', number: 2, start: at('2027-01-05', 20), minutes: 60 }] } };
    const w = await schedApi.weekEpisodes(0, 1);
    expect(w.dates).toEqual([{ show: 'the-folk-club', series: 1, on: '2027-01-05', number: 2, start: at('2027-01-05', 20), minutes: 60, moved_from: null, clock: null }]);
    reply = { status: 200, body: { zone: ZONE, episodes: [] } };
    expect((await schedApi.weekEpisodes(0, 1)).dates).toBeNull();
  });
});

describe('the replies made whole', () => {
  it('fills what a preview left out, and keeps what it said', () => {
    expect(asPreview(null)).toEqual(EMPTY_PREVIEW);
    const p = asPreview({ words: { sentence: 'S.', short: 's' }, last: { on: '2027-10-05', words: 'The last is Tuesday 5 October 2027.' }, clear: { start: 1320, minutes: 60 }, problems: [{ field: 'end', message: 'It ends before it starts.' }, { nonsense: true }], dates: [{ on: '2026-12-01', start: 5, state: 'filled', episode: { series: 1, number: 4 }, clashes: [{ name: 'Firth Sessions', overlap: 30 }] }] });
    expect(p).toMatchObject({ sentence: 'S.', short: 's', last: 'The last is Tuesday 5 October 2027.', clear: { start: 1320, minutes: 60 }, problems: [{ field: 'end', message: 'It ends before it starts.' }] });
    expect(p.dates[0]).toMatchObject({ state: 'filled', minutes: 60, episode: { series: 1, number: 4, aired: false } });
    expect(p.dates[0].clashes[0]).toMatchObject({ show: 'Firth Sessions', overlap_minutes: 30, refused: false, with: [] });
  });

  it('reads a next date however the station sends it', () => {
    expect(asNextOn({ on: '2027-03-02', start: 9, minutes: 60 }, undefined, true)).toEqual({ on: '2027-03-02', start: 9, minutes: 60, reason: null, last_on: null });
    expect(asNextOn('2027-03-02', undefined, true).on).toBe('2027-03-02');
    expect(asNextOn({ on: null, reason: 'complete', last_on: '2027-10-05' }, undefined, true)).toMatchObject({ on: null, reason: 'complete', last_on: '2027-10-05' });
    expect(asNextOn(null, 'once_passed', true).reason).toBe('once_passed');
    // With no reason given: a show with a rule has ended, one without has no dates yet.
    expect(asNextOn(null, undefined, true).reason).toBe('ended');
    expect(asNextOn(null, undefined, false).reason).toBe('none');
    expect(asScheduleDates(undefined)).toBeNull();
    expect(asScheduleDates([{ slug: 'x', start: 1 }, { show: 'y' }])).toHaveLength(1);
  });
});

describe('the control plane’s replies, as branch newshow-api built them', () => {
  const ms = (iso: string, hh: number, mm = 0) => at(iso, hh, mm) * 1000;
  const theirClash = { show: 'Saturday Night Kettle', slug: 'saturday-night', kind: 'rule', series: 3, episode: null, on: '2026-11-14', start_ms: ms('2026-11-14', 20), end_ms: ms('2026-11-14', 23), overlap_minutes: 120, short: 'Sat, 20:00–23:00', hosts: ['Shona Campbell', 'Catriona MacKenzie-Buchanan'], presenter: '', refused: true };

  it('reads a preview: words nested, instants in milliseconds, notes, more, the refused first date', () => {
    const p = asPreview({
      ok: true,
      zone: ZONE,
      words: { sentence: 'Every week on Saturday, 21:00 to 23:00, from 14 November.', short: 'Sat, 21:00–23:00', last: null, last_on: null, notes: ['Months without a 31st are skipped.'] },
      problems: [],
      notes: ['The first date has passed. The first episode will be Tuesday 5 January.'],
      dates: [
        { on: '2026-11-14', number: 1, state: 'airs', episode: null, start_ms: ms('2026-11-14', 21), end_ms: ms('2026-11-14', 23), minutes: 120, start: '21:00', end: '23:00', end_next_day: false, moved_from: null, clock: null, first: true, under_way: false, clashes: [theirClash], refused: true },
        { on: '2026-11-21', number: 2, state: 'filled', episode: 2, start_ms: ms('2026-11-21', 21), end_ms: ms('2026-11-21', 23), minutes: 120, start: '21:00', end: '23:00', end_next_day: false, moved_from: null, clock: null, first: false, under_way: true, clashes: [], refused: false },
      ],
      more: true,
      clash_count: 1,
      change: null,
    });
    expect(p).toMatchObject({ sentence: 'Every week on Saturday, 21:00 to 23:00, from 14 November.', short: 'Sat, 21:00–23:00', more: true, total: null, clear: null, change: null });
    // Their `start` is a clock time in words; the instant is `start_ms`.
    expect(p.dates[0]).toMatchObject({ on: '2026-11-14', start: at('2026-11-14', 21), minutes: 120, state: 'airs', now: false });
    expect(p.dates[0].clashes[0]).toEqual({ show: 'Saturday Night Kettle', slug: 'saturday-night', start: at('2026-11-14', 20), minutes: 180, overlap_minutes: 120, when: 'Sat, 20:00–23:00', with: ['Shona Campbell', 'Catriona MacKenzie-Buchanan'], refused: true });
    expect(p.dates[1]).toMatchObject({ state: 'filled', now: true, episode: { series: 1, number: 2, aired: false } });
    // The notes on the dates sit under First on; what the calendar cannot give, under Repeats.
    expect(p.hints).toEqual([
      { field: 'starts_on', message: 'The first date has passed. The first episode will be Tuesday 5 January.' },
      { field: 'repeat', message: 'Months without a 31st are skipped.' },
    ]);
    expect(clashNotice(p, ZONE)).toMatchObject({ kind: 'refused', title: 'Saturday Night Kettle has that time.', body: 'Sat, 20:00–23:00, with Shona Campbell and Catriona MacKenzie-Buchanan. Pick another time, or choose No dates yet and ask the station.' });
    expect(held(p, { unreachable: false, stale: false, local: false })).toBe('Another show has that time.');
    // With no more dates, the dates given are all of them.
    expect(asPreview({ dates: [{ on: '2026-11-30', number: 1, state: 'airs', start_ms: 1, end_ms: 3_600_001 }], more: false }).total).toBe(1);
  });

  it('cuts a problem’s path to its field, and reads why a rule makes no dates', () => {
    const p = asPreview({ ok: false, problems: [{ field: 'repeat.every', message: 'repeat every 1 to 12 weeks' }, { field: 'end', message: 'This makes no dates. It ends on 1 November, before its first date, 1 December.' }], notes: [], dates: [], more: false });
    expect(p.problems.map((x) => x.field)).toEqual(['repeat', 'end']);
    expect(p.sentence).toBe('');
    expect(held(p, { unreachable: false, stale: false, local: false })).toBe('repeat every 1 to 12 weeks');
  });

  it('drops a skipped or moved date the rule no longer gives, named in the refusal', () => {
    const p = asPreview({ ok: false, problems: [{ field: 'skip', message: 'the rule gives no date on 2027-01-05' }, { field: 'moves', message: 'the rule gives no date on 2027-04-06' }], dates: [] });
    expect(p.dropped).toEqual({ skip: ['2027-01-05'], moves: ['2027-04-06'] });
    // They are the studio's to put right, so they are not shown as problems.
    expect(p.problems).toEqual([]);
    const f = withMove(withSkip(fromRule(FOLK_RULE, '2026-12-01'), '2027-01-05'), '2027-04-06', '2027-04-13', '20:00');
    expect(dropGone(f, p.dropped)).toMatchObject({ words: '1 skipped date and 1 moved date no longer apply.', form: { skip: [], moves: [] } });
    // A move that must stay between its neighbours is a problem of its own: the studio takes the move back.
    const bounds = asPreview({ ok: false, problems: [{ field: 'moves', message: 'keep the date moved from 2027-04-06 between 2027-03-02 and 2027-05-04, so the episodes stay in order' }], dates: [] });
    expect(bounds.dropped).toEqual({ skip: [], moves: [] });
    expect(bounds.problems).toHaveLength(1);
  });

  it('reads what a save would do to the episodes: moved, with no date, left, by hand', () => {
    const p = asPreview({
      ok: true,
      words: { sentence: 'S.', short: 's' },
      dates: [],
      change: {
        aired: 3,
        moved: [{ series: 1, number: 5, from_on: '2027-04-06', to_on: '2027-04-07', from_ms: ms('2027-04-06', 20), to_ms: ms('2027-04-07', 20) }, { series: 1, number: 4, from_on: '2027-03-02', to_on: '2027-03-03', from_ms: ms('2027-03-02', 20), to_ms: ms('2027-03-03', 20) }],
        by_hand: [{ series: 1, number: 6, on: '2027-05-06', start_ms: ms('2027-05-06', 19) }],
        no_date: [{ series: 1, number: 9, on: '2027-08-03', start_ms: ms('2027-08-03', 20) }],
        left: [],
      },
    });
    expect(changeLines(p.change!, ZONE).map((l) => [l.ep, l.from, l.to])).toEqual([
      ['Ep 4', 'Tue 2 March', 'Wed 3 March'],
      ['Ep 5', 'Tue 6 April', 'Wed 7 April'],
      ['Ep 9', 'Tue 3 August', null],
    ]);
    expect(byHandLines(p.change!, ZONE)).toEqual(['Ep 6 · Thu 6 May, 19:00']);
    expect(saveWords(p.change, 'move')).toBe('Save and move 2 episodes');
    // The station gives no date for the first aired episode, so the line says none.
    expect(airedWords(p.change)).toBe('3 episodes have aired. They are not changed.');
  });

  it('reads a show’s words and next date, and the last date a finished show had', () => {
    const complete = schedApi.asShow(show({ rule: FOLK_RULE, rule_words: { sentence: 'S.', short: 's', last: 'The last is Tuesday 5 October 2027.', last_on: '2027-10-05', notes: [] }, next_on: { on: null, start_ms: null, reason: 'ended' } as never }));
    expect(complete.next_on).toEqual({ on: null, start: null, minutes: null, reason: 'ended', last_on: '2027-10-05' });
    expect(unscheduled([complete], true).finished[0].why).toBe('ended 5 October');
    const once = schedApi.asShow(show({ rule: { ...FOLK_RULE, starts_on: '2026-11-30', repeat: { kind: 'once' }, end: { kind: 'never' } }, rule_words: { sentence: 'Once.', short: 'once', last: null, last_on: null, notes: [] }, next_on: { on: null, start_ms: null, reason: 'once_passed' } as never }));
    expect(unscheduled([once], true).finished[0].why).toBe('was once, on 30 November');
    const next = schedApi.asShow(show({ rule: FOLK_RULE, rule_words: { sentence: 'S.', short: 's', last: null, last_on: null, notes: [] }, next_on: { on: '2027-03-02', start_ms: ms('2027-03-02', 20), reason: null } as never }));
    expect(next.next_on).toMatchObject({ on: '2027-03-02', start: at('2027-03-02', 20), reason: null });
    // A station older than 1.0.4 sends none of it, and the show is left as it came.
    const old = show({ slot: { weekday: 6, start: 1200, minutes: 180 } });
    expect(schedApi.asShow(old)).toBe(old);
  });

  it('reads the week’s dates, a saved rule’s moves and a refusal’s clashes', () => {
    expect(asScheduleDates([{ show: 'the-folk-club', name: 'The Folk Club', colour_slot: 11, series: 1, on: '2027-01-05', number: 2, start_ms: ms('2027-01-05', 20), end_ms: ms('2027-01-05', 21), minutes: 60, moved_from: null, clock: null, short: 'First Tue monthly, 20:00–21:00' }])).toEqual([
      { show: 'the-folk-club', series: 1, on: '2027-01-05', number: 2, start: at('2027-01-05', 20), minutes: 60, moved_from: null, clock: null },
    ]);
    const e = new FieldError(409, 'Saturday Night Kettle has that time. Sat, 20:00–23:00, with Shona Campbell.', 'rule', { clashes: [theirClash] });
    expect(refusedClash(e)).toMatchObject({ show: 'Saturday Night Kettle', when: 'Sat, 20:00–23:00', refused: true });
  });

  it('words a later date’s clash for a session the station would refuse', () => {
    const later: RulePreview = { ...EMPTY_PREVIEW, dates: [day('2026-11-14', 1), day('2026-11-21', 2, { clashes: [FIRTH] })] };
    // The station marks only the first date refused; the wording of a later one follows the session.
    expect(clashNotice(later, ZONE, true)).toMatchObject({ kind: 'later', body: 'You can save your show; ask the station about that date before you draft it.' });
    expect(clashNotice(later, ZONE, false)?.kind).toBe('warn');
    expect(held(later, { unreachable: false, stale: false, local: false })).toBe('');
  });
});

describe('asking for the preview', () => {
  const req = (n: number) => ({ rule: { ...FOLK_RULE, start: n }, limit: 10 });
  const answer = (n: number): RulePreview => ({ ...EMPTY_PREVIEW, sentence: `rule ${n}` });
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  /** A station whose every answer waits to be let go, in any order. */
  function station() {
    const asked: number[] = [];
    const waiting = new Map<number, { ok: (p: RulePreview) => void; no: (e: unknown) => void }>();
    const states: PreviewState[] = [];
    const p = new Previewer(
      (r) =>
        new Promise<RulePreview>((ok, no) => {
          asked.push(r.rule.start);
          waiting.set(r.rule.start, { ok, no });
        }),
      (s) => states.push(s),
    );
    const last = () => states[states.length - 1];
    return { p, asked, waiting, last };
  }

  it('waits 250 ms after the last change, and asks once', async () => {
    const s = station();
    s.p.changed(req(1));
    await vi.advanceTimersByTimeAsync(200);
    s.p.changed(req(2));
    await vi.advanceTimersByTimeAsync(200);
    s.p.changed(req(3));
    expect(s.asked).toEqual([]);
    await vi.advanceTimersByTimeAsync(249);
    expect(s.asked).toEqual([]);
    await vi.advanceTimersByTimeAsync(1);
    expect(s.asked).toEqual([3]);
    expect(s.last()).toMatchObject({ stale: true, working: false });
    s.waiting.get(3)!.ok(answer(3));
    await vi.advanceTimersByTimeAsync(0);
    expect(s.last()).toMatchObject({ stale: false, working: false, unreachable: false });
    expect(s.last().answer?.sentence).toBe('rule 3');
    // The same questions again ask nothing.
    s.p.changed(req(3));
    await vi.advanceTimersByTimeAsync(500);
    expect(s.asked).toEqual([3]);
  });

  it('drops an answer older than the newest request', async () => {
    const s = station();
    s.p.changed(req(1), true);
    expect(s.asked).toEqual([1]);
    s.p.changed(req(2));
    await vi.advanceTimersByTimeAsync(250);
    expect(s.asked).toEqual([1, 2]);
    // The newer answer arrives first; the older one, late, is not shown.
    s.waiting.get(2)!.ok(answer(2));
    await vi.advanceTimersByTimeAsync(0);
    s.waiting.get(1)!.ok(answer(1));
    await vi.advanceTimersByTimeAsync(0);
    expect(s.last().answer?.sentence).toBe('rule 2');
    // An old answer that arrives while a new question waits its 250 ms is dropped too.
    s.p.changed(req(4), true);
    s.p.changed(req(5));
    s.waiting.get(4)!.ok(answer(4));
    await vi.advanceTimersByTimeAsync(0);
    expect(s.last().answer?.sentence).toBe('rule 2');
    expect(s.last().stale).toBe(true);
  });

  it('says Working out the dates… after 400 ms, with the last answer dimmed', async () => {
    const s = station();
    s.p.seed(answer(0));
    s.p.changed(req(1), true);
    await vi.advanceTimersByTimeAsync(399);
    expect(s.last()).toMatchObject({ stale: true, working: false });
    await vi.advanceTimersByTimeAsync(1);
    expect(s.last()).toMatchObject({ stale: true, working: true });
    expect(s.last().answer?.sentence).toBe('rule 0');
    s.waiting.get(1)!.ok(answer(1));
    await vi.advanceTimersByTimeAsync(0);
    expect(s.last()).toMatchObject({ stale: false, working: false });
  });

  it('keeps the answers when the station cannot be reached, and tries again', async () => {
    const s = station();
    s.p.seed(answer(0));
    s.p.changed(req(1), true);
    s.waiting.get(1)!.no(new TypeError('network'));
    await vi.advanceTimersByTimeAsync(0);
    expect(s.last()).toMatchObject({ unreachable: true, working: false });
    expect(s.last().answer?.sentence).toBe('rule 0');
    s.p.retry();
    expect(s.asked).toEqual([1, 1]);
    s.waiting.get(1)!.ok(answer(1));
    await vi.advanceTimersByTimeAsync(0);
    expect(s.last()).toMatchObject({ unreachable: false, stale: false });
  });

  it('shows a refusal in the station’s words as a problem by its field', async () => {
    const s = station();
    s.p.changed(req(1), true);
    s.waiting.get(1)!.no(new FieldError(422, 'It ends on 1 November, before its first date, 1 December.', 'end'));
    await vi.advanceTimersByTimeAsync(0);
    expect(s.last().unreachable).toBe(false);
    expect(s.last().answer?.problems).toEqual([{ field: 'end', message: 'It ends on 1 November, before its first date, 1 December.' }]);
  });

  it('asks nothing while the form has a problem of its own, and nothing after it stops', async () => {
    const s = station();
    s.p.seed(answer(0));
    s.p.changed(null);
    await vi.advanceTimersByTimeAsync(1000);
    expect(s.asked).toEqual([]);
    expect(s.last()).toMatchObject({ stale: true });
    s.p.changed(req(1));
    s.p.stop();
    await vi.advanceTimersByTimeAsync(1000);
    expect(s.asked).toEqual([]);
  });
});

describe('the preview on the page', () => {
  it('words each row: number, day, times, skipped, moved, a clash', () => {
    const r = rows(FOLK, ZONE, TODAY, '20:00');
    expect(r.map((x) => [x.ep, x.day, x.times, x.state])).toEqual([
      ['Ep 1', 'Tue 1 Dec', '20:00–21:00', 'airs'],
      // The year is given on the first row of each new year.
      ['', 'Tue 5 Jan 2027', '20:00–21:00', 'skipped'],
      ['Ep 2', 'Tue 9 Feb', '20:00–21:00', 'airs'],
      ['Ep 3', 'Tue 2 Mar', '20:00–21:00', 'airs'],
      ['Ep 4', 'Tue 6 Apr', '20:00–21:00', 'airs'],
      ['Ep 5', 'Tue 4 May', '20:00–21:00', 'airs'],
    ]);
    expect(r[2].moved).toBe('was Tue 2 Feb');
    expect(r[2].on).toBe('2027-02-02');
    expect(r[3].clash).toBe('Six Nations Preview, 19:30 to 20:30 · overlaps 30 min');
    expect(r[1].moved).toBeNull();
  });

  it('gives the real times on a night the clocks change, and says which', () => {
    // 01:30 for an hour on Europe/London's change nights (new-show.md section 9).
    const p: RulePreview = {
      ...EMPTY_PREVIEW,
      dates: [
        day('2027-03-28', 1, { start: Date.UTC(2027, 2, 28, 1, 30) / 1000, clock: 'forward' }),
        day('2027-10-31', 2, { start: Date.UTC(2027, 9, 31, 0, 30) / 1000, clock: 'back' }),
      ],
    };
    const r = rows(p, ZONE, '2027-01-01', '01:30');
    expect([r[0].times, r[0].clock]).toEqual(['02:30–03:30', 'CLOCKS FORWARD · 01:30 does not happen']);
    expect([r[1].times, r[1].clock]).toEqual(['01:30–01:30', 'CLOCKS BACK · the first 01:30 to the second']);
  });

  it('draws only the months that hold one of the next six dates', () => {
    const m = months(FOLK, { first: '2026-12-01', kind: 'monthly_nth' }, ZONE, TODAY);
    expect(m.map((x) => x.caption)).toEqual(['December 2026', 'January 2027', 'February 2027', 'March 2027', 'April 2027', 'May 2027']);
    const dec = m[0];
    // 1 December 2026 is a Tuesday: one empty cell before it.
    expect(dec.lead).toBe(1);
    expect(dec.days).toHaveLength(31);
    expect(dec.days[0]).toMatchObject({ state: 'air', label: 'Tuesday 1 December: airs', row: '2026-12-01' });
    expect(m[1].days[4]).toMatchObject({ state: 'skip', label: 'Tuesday 5 January: skipped' });
    // A moved date is hollow where it was and filled where it went.
    expect(m[2].days[1]).toMatchObject({ state: 'from', label: 'Tuesday 2 February: moved to 9 February', row: '2027-02-02' });
    expect(m[2].days[8]).toMatchObject({ state: 'air', row: '2027-02-02' });
    expect(m[3].days[1]).toMatchObject({ state: 'clash', label: 'Tuesday 2 March: clashes with Six Nations Preview' });
    // A yearly show draws six Decembers, not seventy months.
    const yearly: RulePreview = { ...EMPTY_PREVIEW, dates: [2026, 2027, 2028, 2029, 2030, 2031, 2032].map((y, i) => day(`${y}-12-31`, i + 1, { start: at(`${y}-12-31`, 22) })) };
    expect(months(yearly, { first: '2026-12-31', kind: 'yearly' }, ZONE, TODAY).map((x) => x.caption)).toEqual(['December 2026', 'December 2027', 'December 2028', 'December 2029', 'December 2030', 'December 2031']);
    // Chosen dates draw three months in a row at least, to pick from; nothing yet draws First on's.
    expect(months(null, { first: '2026-12-05', kind: 'dates' }, ZONE, TODAY).map((x) => x.caption)).toEqual(['December 2026', 'January 2027', 'February 2027']);
    expect(months(null, { first: '2026-12-05', kind: 'once' }, ZONE, TODAY).map((x) => x.caption)).toEqual(['December 2026']);
  });

  it('counts the clashes, names the one show in every date’s way, and offers a clear time only when every date clashes', () => {
    expect(clashNotice({ ...FOLK, dates: FOLK.dates.filter((d) => !d.clashes.length) }, ZONE)).toBeNull();
    expect(clashNotice(FOLK, ZONE)).toMatchObject({ kind: 'warn', title: '1 of the next 5 dates clashes.', offer: null });
    expect(clashNotice(FOLK, ZONE)?.body).toMatch(/the one that starts later is on air by the schedule/);
    const every: RulePreview = { ...EMPTY_PREVIEW, clear: { start: 1320, minutes: 60 }, dates: ['2026-12-01', '2026-12-08', '2026-12-15'].map((d, i) => day(d, i + 1, { clashes: [{ ...FIRTH, start: at(d, 20, 30) }] })) };
    expect(clashNotice(every, ZONE)).toEqual({
      kind: 'warn',
      title: 'Every date overlaps Firth Sessions (Tuesdays, 20:30 to 21:30) by 30 minutes.',
      body: 'Where two shows overlap, the one that starts later is on air by the schedule: its presenter is named to callers and its step shapes the studio.',
      offer: { start: 1320, label: 'Try 22:00 to 23:00' },
    });
    expect(clashNotice({ ...EMPTY_PREVIEW, dates: [day('2026-12-01', 1, { clashes: [FIRTH] })] }, ZONE)?.title).toBe('This date clashes.');
    // A one-off has no Skip or Move on its row, so its one clashing date is offered a clear time.
    expect(clashNotice({ ...EMPTY_PREVIEW, clear: { start: 1260, minutes: 60 }, dates: [day('2026-12-01', 1, { clashes: [FIRTH] })] }, ZONE)?.offer).toEqual({ start: 1260, label: 'Try 21:00 to 22:00' });
    const two = { ...FOLK, dates: FOLK.dates.map((d) => (d.on === '2027-04-06' ? { ...d, clashes: [FIRTH] } : d)) };
    expect(clashNotice(two, ZONE)?.title).toBe('2 of the next 5 dates clash.');
    expect(anyClash(FOLK)).toBe(true);
    expect(anyClash(null)).toBe(false);
  });

  it('refuses a presenter whose first date is another show’s, and only warns about a later one', () => {
    const snk: Clash = { show: 'Saturday Night Kettle', slug: 'saturday-night', start: at('2026-11-14', 20), minutes: 180, overlap_minutes: 120, when: 'Saturdays, 20:00 to 23:00', with: ['Shona Campbell', 'Catriona MacKenzie-Buchanan'], refused: true };
    const first: RulePreview = { ...EMPTY_PREVIEW, clear: { start: 1380, minutes: 120 }, dates: [day('2026-11-14', 1, { start: at('2026-11-14', 21), minutes: 120, clashes: [snk] }), day('2026-11-21', 2)] };
    expect(clashNotice(first, ZONE)).toEqual({
      kind: 'refused',
      title: 'Saturday Night Kettle has that time.',
      body: 'Saturdays, 20:00 to 23:00, with Shona Campbell and Catriona MacKenzie-Buchanan. Pick another time, or choose No dates yet and ask the station.',
      offer: { start: 1380, label: 'Try 23:00 to 01:00' },
    });
    expect(held(first, { unreachable: false, stale: false, local: false })).toBe('Another show has that time.');
    const later: RulePreview = { ...EMPTY_PREVIEW, dates: [day('2026-11-14', 1), day('2026-11-21', 2, { clashes: [snk] })] };
    expect(clashNotice(later, ZONE)).toMatchObject({ kind: 'later', title: '1 of the next 2 dates clashes.', body: 'You can save your show; ask the station about that date before you draft it.' });
    expect(held(later, { unreachable: false, stale: false, local: false })).toBe('');
  });

  it('holds the save while there is nothing sure to save', () => {
    const ok = { unreachable: false, stale: false, local: false };
    expect(held(FOLK, ok)).toBe('');
    expect(held(FOLK, { ...ok, local: true })).toBe('Put right what is marked first.');
    expect(held(FOLK, { ...ok, unreachable: true })).toBe('The station cannot be reached.');
    expect(held(FOLK, { ...ok, stale: true })).toBe('Working out the dates…');
    expect(held(null, ok)).toBe('Working out the dates…');
    expect(held({ ...EMPTY_PREVIEW }, ok)).toBe('This makes no dates.');
    expect(held({ ...FOLK, problems: [{ field: 'end', message: 'It ends on 1 November, before its first date, 1 December.' }] }, ok)).toBe('It ends on 1 November, before its first date, 1 December.');
  });

  it('says what follows the rows, and between which dates a moved date stays', () => {
    const never = { kind: 'never' as const, until: '' as const, count: 10 };
    const counted = { kind: 'count' as const, until: '' as const, count: 10 };
    expect(afterWords({ ...FOLK, total: null, more: true }, { kind: 'monthly_nth', every: 1, end: never })).toEqual({ more: true, words: 'and every month after, with no end' });
    expect(afterWords({ ...FOLK, total: null, more: true }, { kind: 'weekly', every: 2, end: never }).words).toBe('and every 2 weeks after, with no end');
    // A rule that ends, with more dates than are shown: only the button.
    expect(afterWords({ ...FOLK, total: null, more: true }, { kind: 'monthly_nth', every: 1, end: counted })).toEqual({ more: true, words: '' });
    expect(afterWords({ ...FOLK, more: null }, { kind: 'monthly_nth', every: 1, end: counted })).toEqual({ more: true, words: '' });
    expect(afterWords({ ...FOLK, total: 5, more: false }, { kind: 'monthly_nth', every: 1, end: counted })).toEqual({ more: false, words: 'Those are all 5.' });
    expect(afterWords({ ...EMPTY_PREVIEW, total: 1, dates: [day('2026-11-30', 1)] }, { kind: 'once', every: 1, end: never })).toEqual({ more: false, words: 'It airs once, so this is the only date.' });
    // Draft all 10 takes the rule's own count; a rule that ends on a date, the dates given once there are no more.
    expect(madeInAll(FOLK_RULE, FOLK)).toBe(10);
    expect(madeInAll({ ...FOLK_RULE, end: { kind: 'until', until: '2027-02-26' } }, { ...FOLK, total: 5, more: false })).toBe(5);
    expect(madeInAll({ ...FOLK_RULE, end: { kind: 'never' } }, { ...FOLK, total: null })).toBeNull();
    expect(madeInAll({ ...FOLK_RULE, repeat: { kind: 'once' } }, FOLK)).toBeNull();
    // 6 April sits between 2 March and 4 May, as the rows give them.
    const b = moveBounds(FOLK, '2027-04-06', ZONE, TODAY);
    expect(b).toEqual({ after: '2027-03-02', before: '2027-05-04' });
    expect(boundsWords(b)).toBe('Keep it between 2 March and 4 May, so the episodes stay in order.');
    expect(moveBounds(FOLK, '2026-12-01', ZONE, TODAY)).toEqual({ after: TODAY, before: '2027-02-09' });
  });

  it('says when episode 1 is, tonight for today', () => {
    expect(firstWords({ start: at('2026-12-01', 20), minutes: 60 }, ZONE, TODAY)).toBe('Tuesday 1 December, 20:00 to 21:00');
    expect(firstWords({ start: at(TODAY, 23), minutes: 120 }, ZONE, TODAY)).toBe('tonight, Friday 13 November, 23:00 to 01:00');
    expect(firstWords({ start: at('2027-01-05', 20), minutes: 60 }, ZONE, TODAY)).toBe('Tuesday 5 January 2027, 20:00 to 21:00');
  });
});

describe('changing a rule: what happens to episodes', () => {
  const plan = {
    aired: 3,
    since: '2026-12-01',
    drafted: [
      { series: 1, number: 4, from: at('2027-03-02', 20), to: at('2027-03-03', 20), clash: null },
      { series: 1, number: 5, from: at('2027-04-06', 20), to: at('2027-04-07', 20), clash: { ...FIRTH, show: 'Kettle Drive', start: at('2027-04-07', 19, 30), when: null } },
      { series: 1, number: 9, from: at('2027-08-03', 20), to: null, clash: null },
    ],
    by_hand: [{ series: 1, number: 6, at: at('2027-05-06', 19) }],
  };

  it('lists each drafted episode’s move, the one with no date, and the one a person placed', () => {
    expect(changeLines(plan, ZONE)).toEqual([
      { ep: 'Ep 4', from: 'Tue 2 March', to: 'Wed 3 March', stays: null, clash: null },
      { ep: 'Ep 5', from: 'Tue 6 April', to: 'Wed 7 April', stays: null, clash: 'Kettle Drive, 19:30 to 20:30 · overlaps 30 min' },
      { ep: 'Ep 9', from: 'Tue 3 August', to: null, stays: 'has no date under the new rule. It stays on Tue 3 August.', clash: null },
    ]);
    expect(byHandLines(plan, ZONE)).toEqual(['Ep 6 · Thu 6 May, 19:00']);
    // A change of time alone says the times.
    expect(changeLines({ ...plan, drafted: [{ series: 1, number: 4, from: at('2027-03-02', 20), to: at('2027-03-02', 21), clash: null }] }, ZONE)[0]).toMatchObject({ from: 'Tue 2 March, 20:00', to: 'Tue 2 March, 21:00' });
  });

  it('moves them by default, offers to leave them, and words the button by the choice', () => {
    expect(movable(plan)).toBe(2);
    expect(saveWords(plan, 'move')).toBe('Save and move 2 episodes');
    expect(saveWords(plan, 'leave')).toBe('Save, and leave them');
    expect(saveWords({ ...plan, drafted: plan.drafted.slice(0, 1) }, 'move')).toBe('Save and move 1 episode');
    expect(saveWords({ aired: 3, since: null, drafted: [], by_hand: [] }, 'move')).toBe('Save');
    expect(airedWords(plan)).toBe('3 episodes have aired since 1 December. They are not changed.');
    expect(airedWords({ ...plan, aired: 1, since: null })).toBe('1 episode has aired. It is not changed.');
    expect(countWords(plan, 10)).toBe('for 10 episodes in all: 3 aired, 4 drafted, 3 to come');
    const { body: raw } = render(RuleChange, { props: { plan, zone: ZONE } });
    const body = clean(raw);
    expect(body).toContain('data-panel="calendar.rule.change"');
    expect(body).toContain('<b>3 aired.</b>');
    expect(body).toContain('Not changed: what was broadcast is not rewritten.');
    expect(body).toContain('3 drafted, on the old dates.');
    expect(body).toMatch(/<input[^>]*value="move"[^>]*checked[^>]*\/?> Move them to the new dates/);
    expect(body).toMatch(/<input[^>]*value="leave"[^>]*\/?> Leave them where they are/);
    expect(body).not.toMatch(/<input[^>]*value="leave"[^>]*checked/);
    expect(body).toContain('They keep their numbers, titles, guests and everything set on them.');
    expect(body).toContain('<b>1 moved by hand.</b>');
    expect(body).toContain('Thu 6 May, 19:00. It stays where you put it.');
    expect(body).toContain('Conflict');
    // Left where they are, the list still shows what would have moved.
    expect(render(RuleChange, { props: { plan, zone: ZONE, choice: 'leave' } }).body).toMatch(/<input[^>]*value="leave"[^>]*checked/);
  });
});

describe('the dialogs rendered', () => {
  const noop = () => {};
  const base = { shows: [show({ slug: 'kettle-drive', name: 'Kettle Drive', colour_slot: 1 })], zone: ZONE, today: TODAY, live: false, onMade: noop, onOpen: noop, onClose: noop };

  it('opens New show on part 1: the name, the next free colour, the step', () => {
    const { body: raw } = render(NewShow, { props: base });
    const body = clean(raw);
    expect(body).toContain('data-panel="calendar.new-show"');
    expect(body).toContain('Part 1 of 3: The show');
    expect(body).toContain('Name it and pick how much of the studio it starts with. The rest can wait.');
    expect(body).toContain('Up to three lines on the card at its smallest size.');
    expect((body.match(/aria-label="Bank \d+"/g) ?? []).length).toBe(12);
    // Bank 1 is Kettle Drive's, so bank 2 is the next one free and is picked.
    expect(body).toMatch(/aria-pressed="true" aria-label="Bank 2"/);
    expect(body).toContain('Bank 2 is the next one free.');
    expect(body).toContain('Next: when it airs');
    expect(body).not.toContain('Make the show');
    // "Step" means the show's step: the dialog's stages are parts.
    expect(body).not.toMatch(/Step \d of \d/);
  });

  it('asks on part 2 when it airs, and draws what the station said back', () => {
    const form = fromRule(FOLK_RULE, '2026-12-01');
    const { body: raw } = render(NewShow, { props: { ...base, part: 2, name: 'The Folk Club', form, preview: FOLK } });
    const body = clean(raw);
    expect(body).toContain('Part 2 of 3: When it airs');
    expect(body).toContain('Give one date and time, then say whether it comes back.');
    for (const name of ['calendar.new-show', 'calendar.rule', 'calendar.rule.preview', 'calendar.rule.summary']) expect(body.includes(`data-panel="${name}"`), name).toBe(true);
    for (const m of body.matchAll(/data-panel="([^"]+)"/g)) expect(() => panel(m[1])).not.toThrow();
    // The questions.
    expect(body).toContain('>First on</label>');
    expect(body).toContain('value="2026-12-01"');
    expect(body).toContain('1 hour');
    expect(body).toContain('Station time, Europe/London.');
    expect(body).toContain('Every month on the first Tuesday');
    expect(body).toMatch(/<input[^>]*value="nth1"[^>]*checked/);
    expect(body).toContain('More choices…');
    expect(body).toContain('>Ends</legend>');
    expect(body).toContain('Skipped dates do not count: the show runs until it has made 10 episodes.');
    // The preview: the control plane's sentence, as given, in a polite live region.
    expect(body).toContain('What this makes');
    expect(body).toMatch(/data-panel="calendar\.rule\.summary" role="status" aria-live="polite"/);
    expect(body).toContain('Every month on the first Tuesday, 20:00 to 21:00, from 1 December, for 10 episodes. Not on 5 January 2027.');
    expect(body).toContain('The last is Tuesday 5 October 2027.');
    expect(body).toContain('1 of the next 5 dates clashes.');
    expect(body).toContain('Six Nations Preview, 19:30 to 20:30 · overlaps 30 min');
    expect(body).toContain('Moved · was Tue 2 Feb');
    expect(body).toContain('>Skipped<');
    for (const b of ['>Skip</button>', '>Put back</button>', '>Move…</button>', '>Back to the rule</button>', '>Show more dates</button>']) expect(body.includes(b), b).toBe(true);
    expect(body).toContain('December 2026');
    expect(body).toContain('aria-label="Tuesday 5 January: skipped"');
    // A clash is a warning here: the button says so.
    expect(body).toContain('Make the show anyway');
    expect(body).toContain('>No dates yet</button>');
    expect(body).toContain('>Back</button>');
  });

  it('says under First on that the date and time are the selected block’s, with the way back to today', () => {
    // Thursday 19 November 2026, 15:00: the block that was selected in the week.
    const { body: raw } = render(NewShow, { props: { ...base, part: 2, name: 'The Folk Club', at: { date: '2026-11-19', from: '15:00' } } });
    const body = clean(raw);
    expect(body).toContain('value="2026-11-19"');
    expect(body).toContain('From the block selected in the week: <b>Thursday 19 November, 15:00 to 16:00</b>.');
    expect(body).toContain('>Use today instead</button>');
    expect(body).toMatch(/id="ns-first"[^>]*aria-describedby="ns-seeded ns-first-notes ns-zone"/);
  });

  it('says nothing of a block when none was selected, or when the answers were given', () => {
    expect(clean(render(NewShow, { props: { ...base, part: 2, name: 'The Folk Club' } }).body)).not.toContain('From the block selected');
    const form = fromRule(FOLK_RULE, '2026-12-01');
    expect(clean(render(NewShow, { props: { ...base, part: 2, name: 'The Folk Club', form, at: { date: '2026-12-01', from: '20:00' } } }).body)).not.toContain('From the block selected');
  });

  it('a block today says today, and offers the usual hour', () => {
    const body = clean(render(NewShow, { props: { ...base, part: 2, name: 'The Folk Club', at: { date: TODAY, from: '15:00' } } }).body);
    expect(body).toContain('From the block selected in the week: <b>today, 15:00 to 16:00</b>.');
    expect(body).toContain('>Use 20:00 instead</button>');
  });

  it('holds Make the show while the rule makes no dates, in the station’s words', () => {
    const form = { ...pickChoice(newForm('2026-12-01'), 'weekly'), end: { kind: 'until' as const, until: '2026-11-01', count: 10 } };
    const none: RulePreview = { ...EMPTY_PREVIEW, problems: [{ field: 'end', message: 'It ends on 1 November, before its first date, 1 December.' }] };
    const { body: raw } = render(NewShow, { props: { ...base, part: 2, name: 'The Bothy Sessions', form, preview: none } });
    const body = clean(raw);
    expect(body).toContain('This makes no dates.');
    expect(body).toContain('It ends on 1 November, before its first date, 1 December.');
    expect(body).toMatch(/<button type="submit" class="btn primary" disabled[^>]*>Make the show<\/button>/);
  });

  it('opens the whole rule for chosen dates, with the calendar as the picker', () => {
    let form = pickKind({ ...newForm('2026-12-05', '14:00', '16:00'), mode: 'full' as const }, 'dates');
    form = toggleDate(form, '2026-12-12');
    const { body: raw } = render(NewShow, { props: { ...base, part: 2, name: 'Christmas Market Live', form, preview: { ...EMPTY_PREVIEW, sentence: 'On 2 dates you chose, 14:00 to 16:00, from Saturday 5 December.', total: 2, more: false, dates: [day('2026-12-05', 1, { start: at('2026-12-05', 14), minutes: 120 }), day('2026-12-12', 2, { start: at('2026-12-12', 14), minutes: 120 })] } } });
    const body = clean(raw);
    expect(body).toMatch(/role="radio" aria-checked="true"[^>]*>Chosen dates</);
    expect(body).toContain('Pick the dates in the calendar. Each one airs at the time above.');
    expect(body).toContain('aria-label="Remove Saturday 12 December"');
    expect(body).toContain('aria-label="Saturday 12 December: chosen"');
    expect(body).toContain('Fewer choices');
    // Chosen dates have no end, and a date is taken out rather than skipped.
    expect(body).not.toContain('>Ends</legend>');
    expect(body).not.toContain('>Skip</button>');
    expect(body).toContain('Those are all 2.');
  });

  it('says on Done what was made, with episode 1 to open and the rest to draft', () => {
    const drafted = { episode: { id: 'e1', number: 1, air_at: at('2026-12-01', 20), minutes: 60 }, resolved: {} } as unknown as EpisodeView;
    const saved = { show: show({ rule: FOLK_RULE }), resolved: {}, series: [{ number: 1 }], drafted } as unknown as ShowSaved;
    const { body: raw } = render(NewShow, { props: { ...base, part: 3, name: 'The Folk Club', saved } });
    const body = clean(raw);
    expect(body).toContain('Part 3 of 3: Done');
    expect(body).toContain('<strong>The Folk Club</strong> is a show.');
    expect(body).toContain('Episode 1 is drafted</b> for Tuesday 1 December, 20:00 to 21:00. Give it a title and guests when you know them.');
    expect(body).toContain('The other dates wait in the week as dotted blocks. Draft each when you are ready');
    expect(body).toContain('The studio starts at Solo:');
    expect(body).toContain('Hosts, a description, the card and the presenter’s details are in the show editor.');
    expect(body).toContain('>Open episode 1</button>');
    expect(body).toContain('>Close</button>');
    // No dates yet: nothing is drafted, and Done says where to give it dates.
    const none = render(NewShow, { props: { ...base, part: 3, name: 'Bothy Ballads', saved: { show: show({ name: 'Bothy Ballads', rule: null }), resolved: {}, series: [], drafted: null } as unknown as ShowSaved } }).body;
    expect(none).toContain('It has no dates yet, so nothing is drafted. Give it dates from the Schedule tab.');
    expect(none).not.toContain('Open episode');
    // Once: there is nothing more to draft.
    const once = render(NewShow, { props: { ...base, part: 3, name: 'St Andrew’s Night Ceilidh', saved: { ...saved, show: show({ rule: { ...FOLK_RULE, repeat: { kind: 'once' }, end: { kind: 'never' } } }) } as unknown as ShowSaved } }).body;
    expect(once).toContain('It airs once, so there is nothing more to draft.');
  });

  it('keeps an older station to a weekly time, with no preview to ask for', () => {
    const { body: raw } = render(NewShow, { props: { ...base, part: 2, name: 'Late Tales', rules: false, form: pickChoice(newForm('2026-11-17', '21:30', '22:30'), 'weekly') } });
    const body = clean(raw);
    expect(body).toContain('This station keeps a weekly time only.');
    expect(body).toContain('Update it for other patterns.');
    expect(body).toMatch(/<input[^>]*value="weekly"[^>]*checked/);
    expect(body).toMatch(/<input[^>]*value="once"[^>]*disabled/);
    expect(body).not.toContain('More choices…');
    expect(body).not.toContain('data-panel="calendar.rule.preview"');
    expect(body).not.toContain('>Ends</legend>');
    expect(body).toMatch(/<button type="submit" class="btn primary"(?! disabled)[^>]*>Make the show<\/button>/);
  });

  it('says so when the station’s zone is not chosen, and offers an admin the way', () => {
    const { body: raw } = render(RulePart, { props: { form: newForm('2026-12-01'), state: firstState(), zone: ZONE, today: TODAY, zoneSet: false, canChooseZone: true, onzone: noop, live: false } });
    const body = clean(raw);
    expect(body).toContain('Times are in Europe/London, which nobody has confirmed yet.');
    expect(body).toContain('Choose the time zone…');
    expect(body).toContain('Pick a date and a time, and the next dates appear here.');
  });

  it('turns the rule dialog to Changes from for a show with a past, and then to Before you save', () => {
    const plan = { aired: 3, since: '2026-12-01', drafted: [{ series: 1, number: 4, from: at('2027-03-02', 20), to: at('2027-03-03', 20), clash: null }], by_hand: [] };
    const preview: RulePreview = { ...FOLK, change: plan, dates: [day('2027-03-02', 4, { state: 'filled', episode: { series: 1, number: 4, aired: false } }), day('2027-04-06', 5)] };
    const props = { slug: 'the-folk-club', name: 'The Folk Club', rule: FOLK_RULE, nextOn: '2027-03-02', zone: ZONE, today: '2027-02-10', preview, live: false, onSaved: noop, onClose: noop };
    const { body: raw } = render(RuleDialog, { props });
    const body = clean(raw);
    expect(body).toContain('When The Folk Club airs');
    expect(body).toContain('3 episodes have aired since 1 December. They are not changed.');
    expect(body).toContain('>Changes from</label>');
    // The new rule starts at the next date that has not aired.
    expect(body).toContain('value="2027-03-02"');
    expect(body).toContain('for 10 episodes in all: 3 aired, 1 drafted, 6 to come');
    expect(body).toContain('Ep 4 · drafted');
    expect(body).toContain('>Save…</button>');
    expect(body).toContain('>Cancel</button>');
    const change = render(RuleDialog, { props: { ...props, changing: true } }).body;
    expect(change).toContain('Before you save');
    expect(change).toContain('data-panel="calendar.rule.change"');
    expect(change).toContain('Tue 2 March');
    expect(change).toContain('Wed 3 March');
    expect(change).toContain('>Save and move 1 episode</button>');
    expect(change).toContain('>Back</button>');
    // A show with no episodes saves in one press; a series is named.
    const fresh = render(RuleDialog, { props: { ...props, rule: null, nextOn: null, preview: { ...FOLK, change: { aired: 0, since: null, drafted: [], by_hand: [] } } } }).body;
    expect(fresh).toContain('>First on</label>');
    expect(fresh).toContain('>Save</button>');
    expect(render(RuleDialog, { props: { ...props, series: 2 } }).body).toContain('When series 2 of The Folk Club airs');
  });

  it('gives the DJ’s setup the same part as its third of four, on the list', () => {
    const shona: Me = { kind: 'staff', role: 'presenter', account: { id: 'a-shona', username: 'shona', name: 'Shona Campbell', role: 'presenter', disabled_ms: null, created_ms: 1 } };
    const wizard = { me: shona, shows: [] as Show[], zone: ZONE, today: TODAY, rules: true, live: false, onDone: noop, onSkip: noop, onClose: noop };
    const form = pickChoice(newForm('2026-11-14', '21:00', '23:00'), 'weekly');
    const snk: Clash = { show: 'Saturday Night Kettle', slug: 'saturday-night', start: at('2026-11-14', 20), minutes: 180, overlap_minutes: 120, when: 'Saturdays, 20:00 to 23:00', with: ['Shona Campbell', 'Catriona MacKenzie-Buchanan'], refused: true };
    const preview: RulePreview = { ...EMPTY_PREVIEW, sentence: 'Every week on Saturday, 21:00 to 23:00, from 14 November.', clear: { start: 1380, minutes: 120 }, dates: [day('2026-11-14', 1, { start: at('2026-11-14', 21), minutes: 120, clashes: [snk] }), day('2026-11-21', 2, { start: at('2026-11-21', 21), minutes: 120, clashes: [{ ...snk, start: at('2026-11-21', 20) }] })] };
    const { body: raw } = render(ShowWizard, { props: { ...wizard, part: 3, form, preview } });
    const body = clean(raw);
    expect(body).toContain('data-panel="studio.show-setup"');
    expect(body).toContain('data-panel="calendar.rule"');
    expect(body).toContain('Part 3 of 4: When it is on');
    expect(body).toContain('When your show is on. Give its first date, then say whether it comes back.');
    expect(body).toContain('Every week on Saturday');
    expect(body).toContain('More choices…');
    // A first date that is another show's is refused, with the other show named.
    expect(body).toContain('data-clash="refused"');
    expect(body).toContain('Saturday Night Kettle has that time.');
    expect(body).toContain('Saturdays, 20:00 to 23:00, with Shona Campbell and Catriona MacKenzie-Buchanan. Pick another time, or choose No dates yet and ask the station.');
    expect(body).toContain('Try 23:00 to 01:00');
    expect(body).toMatch(/<button type="submit" class="btn primary" disabled[^>]*>Save my show<\/button>/);
    expect(body).toContain('>No dates yet</button>');
    // Done: episode 1, and the rule as the station says it.
    const drafted = { episode: { id: 'e1', number: 1, air_at: at(TODAY, 23), minutes: 120 }, resolved: {} } as unknown as EpisodeView;
    const saved = { show: show({ name: 'The Wee Small Hours', rule: ruleOfSlot({ weekday: 5, start: 1380, minutes: 120 }), rule_words: { sentence: 'Every week on Friday, 23:00 to 01:00 the next day.', short: 'Fri, 23:00–01:00', last: null, last_on: null, notes: [] } }), resolved: {}, series: [], drafted } as unknown as ShowSaved;
    const done = render(ShowWizard, { props: { ...wizard, part: 4, saved } }).body;
    expect(done).toContain('Part 4 of 4: Done');
    expect(done).toContain('Episode 1 is drafted</b> for tonight, Friday 13 November, 23:00 to 01:00.');
    expect(done).toContain('It is on every week on Friday, 23:00 to 01:00 the next day.');
    expect(done).toContain('The studio starts at');
  });
});

describe('the right panel', () => {
  it('lists shows with nothing coming up in two groups, by the station’s account of each', () => {
    const shows = [
      show({ slug: 'northern-lights', name: 'Northern Lights', presenter: 'Tara Buchanan', rule: null, next_on: asNextOn(null, 'none', false) }),
      show({ slug: 'ceilidh', name: 'St Andrew’s Night Ceilidh', rule: { ...FOLK_RULE, repeat: { kind: 'once' }, end: { kind: 'never' } }, next_on: asNextOn({ on: null, reason: 'once_passed', last_on: '2026-11-30' }, undefined, true) }),
      show({ slug: 'lectures', name: 'Autumn Lectures', rule: { ...FOLK_RULE, end: { kind: 'count', count: 6 } }, next_on: asNextOn({ on: null, reason: 'complete' }, undefined, true) }),
      show({ slug: 'warmers', name: 'Winter Warmers', rule: FOLK_RULE, next_on: asNextOn({ on: null, reason: 'ended', last_on: '2027-01-29' }, undefined, true) }),
      // A show with a date to come is not listed.
      show({ slug: 'the-folk-club', rule: FOLK_RULE, next_on: asNextOn({ on: '2027-03-02' }, undefined, true) }),
    ];
    const u = unscheduled(shows, true);
    expect(u.none.map((r) => [r.show.name, r.why])).toEqual([['Northern Lights', 'Tara Buchanan’s show']]);
    expect(u.finished.map((r) => [r.show.name, r.why])).toEqual([
      ['St Andrew’s Night Ceilidh', 'was once, on 30 November'],
      ['Autumn Lectures', 'its 6 episodes are made'],
      ['Winter Warmers', 'ended 29 January'],
    ]);
    const { body: raw } = render(Unscheduled, { props: { shows, ondates: () => {}, onedit: () => {}, ondraft: () => {} } });
    const body = clean(raw);
    expect(body).toContain('data-panel="calendar.unscheduled"');
    expect(body).toContain('<b data-panel-title="">No dates yet</b>');
    expect(body).toContain('Give it dates…');
    // Finished is closed by default and counted: a finished show is not a fault.
    expect(body).toMatch(/<details class="g done"><summary><b>Finished<\/b> · 3 shows<\/summary>/);
    expect(body).toContain('Give it new dates…');
    expect(render(Unscheduled, { props: { shows: [shows[4]], ondates: () => {}, onedit: () => {}, ondraft: () => {} } }).body).not.toContain('calendar.unscheduled');
  });

  it('on an older station offers each weekly show its next episode, since the week has no dates', () => {
    const old = [show({ slot: { weekday: 6, start: 1200, minutes: 180 } }), show({ slug: 'bothy', name: 'Bothy Ballads' })];
    const u = unscheduled(old, false);
    expect(u.toDraft.map((r) => r.why)).toEqual(['Sat, 20:00–23:00']);
    expect(u.none.map((r) => r.show.name)).toEqual(['Bothy Ballads']);
    const { body: raw } = render(Unscheduled, { props: { shows: old, rules: false, ondates: () => {}, onedit: () => {}, ondraft: () => {} } });
    const body = clean(raw);
    expect(body).toContain('This station runs an older WeaveSignal');
    expect(body).toContain('Draft the next episode');
  });

  it('shows When it airs in the show editor: the rule in short, the next date, Change…', () => {
    sched.rules = true;
    sched.zone = ZONE;
    const s = show({ rule: FOLK_RULE, rule_words: { sentence: 'Every month on the first Tuesday, 20:00 to 21:00, for 10 episodes.', short: 'First Tue monthly, 20:00–21:00', last: null, last_on: null, notes: [] }, next_on: asNextOn({ on: '2027-03-02' }, undefined, true) });
    const { body: raw } = render(ShowEditor, { props: { slug: s.slug, show: s, onSaved: () => {}, onCard: () => {} } });
    const body = clean(raw);
    const at0 = body.indexOf('data-panel="calendar.show-rule"');
    const field = body.slice(at0, body.indexOf('data-field="hosts"'));
    expect(field).toMatch(/<span data-panel-title[^>]*>When it airs<\/span>/);
    expect(field).toContain('First Tue monthly, 20:00–21:00');
    expect(field).toContain('· next Tue 2 March');
    expect(field).toContain('>Change…</button>');
    expect(field).toContain('>Clear</button>');
    expect(field).toContain('Every month on the first Tuesday, 20:00 to 21:00, for 10 episodes. Series and episodes follow it until set on their own level.');
    const none = render(ShowEditor, { props: { slug: s.slug, show: show({ rule: null, next_on: asNextOn(null, 'none', false) }), onSaved: () => {}, onCard: () => {} } }).body;
    expect(none).toContain('No dates yet');
    expect(none).toContain('>Give it dates…</button>');
    const done = render(ShowEditor, { props: { slug: s.slug, show: { ...s, next_on: asNextOn({ on: null, reason: 'complete' }, undefined, true) }, onSaved: () => {}, onCard: () => {} } }).body;
    expect(done).toContain('Finished: its 10 episodes are made');
    sched.rules = null;
  });

  it('opens New show over the Schedule tab, and words the legend by the rule', () => {
    sched.zone = ZONE;
    sched.anchor = Date.UTC(2026, 10, 13, 10);
    const { body: raw } = render(Schedule, { props: { live: false, dialog: 'new-show' } });
    const body = clean(raw);
    expect(body).toContain('data-panel="calendar.new-show"');
    expect(body).toContain('A date from the show’s rule');
    expect(body).not.toContain('weekly rule');
    for (const m of body.matchAll(/data-panel="([^"]+)"/g)) expect(() => panel(m[1])).not.toThrow();
  });

  it('registers the design’s seven new panel names (section 5)', () => {
    for (const name of ['calendar.new-show', 'calendar.rule', 'calendar.rule.summary', 'calendar.rule.preview', 'calendar.rule.change', 'calendar.show-rule', 'calendar.unscheduled']) {
      expect(CALENDAR.map((p) => p.name), name).toContain(name);
    }
    expect(PANELS['studio.show-setup']).toMatch(/in four parts.*when it is on, with the next dates/);
    expect(PANELS['calendar.series-editor']).toContain('when it airs');
  });
});

describe('the dialogs’ two keys', () => {
  const el = (tag: string, attrs: Record<string, string> = {}) => ({ tagName: tag, getAttribute: (k: string) => attrs[k] ?? null }) as unknown as Element;

  it('lets Esc leave a text field first, and close on the second', () => {
    expect(escStep(el('INPUT', { type: 'date' }))).toBe('leave-field');
    expect(escStep(el('INPUT'))).toBe('leave-field');
    expect(escStep(el('TEXTAREA'))).toBe('leave-field');
    expect(escStep(el('INPUT', { type: 'radio' }))).toBe('close');
    expect(escStep(el('BUTTON'))).toBe('close');
    expect(escStep(null)).toBe('close');
    expect(isTextField(el('INPUT', { type: 'checkbox' }))).toBe(false);
  });

  it('presses the primary button with ⏎, except on a control that takes ⏎ itself', () => {
    expect(enterIsPrimary(el('LI'))).toBe(true);
    expect(enterIsPrimary(el('DIALOG'))).toBe(true);
    expect(enterIsPrimary(el('BUTTON'))).toBe(false);
    expect(enterIsPrimary(el('TEXTAREA'))).toBe(false);
    expect(enterIsPrimary(el('SPAN', { role: 'gridcell' }))).toBe(false);
    expect(enterIsPrimary(el('BUTTON', { role: 'radio' }))).toBe(false);
  });
});
