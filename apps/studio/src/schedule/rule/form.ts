// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// When a show airs (ADR-0278; new-show.md), as the questions hold it and with no page: the one
// date and two times a person gives, the choice of how it comes back, and how those become the
// rule the control plane stores. The studio does not work out a rule's dates, write its
// sentence or judge a clash (section 8): what is here is the form's own arithmetic, on the one
// date typed. Its weekday and its place in its month make the Repeats rows (`Every month on
// the first Tuesday`), and two times make a length. Pure, so it is tested on its own.

import type { IsoDate, Rule, RuleEnd, RuleMove, RulePreview, RulePreviewRequest, Repeat } from '../../lib/api.ts';
import type { ShowStep } from '../../lib/api.ts';
import type { ShowInput, Slot } from '../../lib/schedule.ts';

export type RepeatKind = Repeat['kind'];

/** The answers to When it airs. Every pattern's own values are kept while another is shown,
 *  so going from Weekly to Monthly and back loses nothing. */
export interface RuleForm {
  /** First on (or Changes from): a date on the station's wall clock. */
  first: IsoDate;
  /** `HH:MM`, station time. */
  from: string;
  until: string;
  /** An end before the start, confirmed: it runs past midnight (ADR-0244). */
  overnightOk: boolean;
  /** Repeats as the list of sentences, or as the whole rule. */
  mode: 'list' | 'full';
  kind: RepeatKind;
  every: number;
  /** 1 (Monday) to 7. */
  weekdays: number[];
  /** 1 to 4, or -1 for the last. */
  nth: number;
  weekday: number;
  /** 1 to 31, or -1 for the last day. */
  day: number;
  yearDay: number;
  /** 1 to 12. */
  yearMonth: number;
  dates: IsoDate[];
  end: { kind: RuleEnd['kind']; until: IsoDate | ''; count: number };
  skip: IsoDate[];
  moves: RuleMove[];
}

export const WEEKDAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'] as const;
export const WEEKDAYS_SHORT = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'] as const;
export const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'] as const;
export const MONTHS_SHORT = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'] as const;
export const NTH_WORD: Record<number, string> = { 1: 'first', 2: 'second', 3: 'third', 4: 'fourth', [-1]: 'last' };
export const NTHS = [1, 2, 3, 4, -1] as const;

const pad = (n: number) => String(n).padStart(2, '0');

// ---- one calendar date --------------------------------------------------------------------

/** `2026-12-01` as its year, month (1 to 12) and day; null when it is not a date. */
export function ymd(iso: string): { y: number; m: number; d: number } | null {
  const a = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso.trim());
  if (!a) return null;
  const [y, m, d] = [Number(a[1]), Number(a[2]), Number(a[3])];
  if (m < 1 || m > 12 || d < 1 || d > daysIn(y, m)) return null;
  return { y, m, d };
}

export const isoOf = (y: number, m: number, d: number): IsoDate => `${String(y).padStart(4, '0')}-${pad(m)}-${pad(d)}`;

/** How many days a month has; `m` is 1 to 12. */
export const daysIn = (y: number, m: number): number => new Date(Date.UTC(y, m, 0)).getUTCDate();

/** A date's weekday, 1 (Monday) to 7; 0 when it is not a date. */
export function weekdayOf(iso: string): number {
  const p = ymd(iso);
  return p ? ((new Date(Date.UTC(p.y, p.m - 1, p.d)).getUTCDay() + 6) % 7) + 1 : 0;
}

/** A date some days on, for moving round the small months with the arrows. */
export function addDays(iso: IsoDate, n: number): IsoDate {
  const p = ymd(iso);
  if (!p) return iso;
  const t = new Date(Date.UTC(p.y, p.m - 1, p.d + n));
  return isoOf(t.getUTCFullYear(), t.getUTCMonth() + 1, t.getUTCDate());
}

/** The same day a month on or back, or that month's last day when it is shorter. */
export function addMonths(iso: IsoDate, n: number): IsoDate {
  const p = ymd(iso);
  if (!p) return iso;
  const t = new Date(Date.UTC(p.y, p.m - 1 + n, 1));
  const [y, m] = [t.getUTCFullYear(), t.getUTCMonth() + 1];
  return isoOf(y, m, Math.min(p.d, daysIn(y, m)));
}

/** A date in words: `Tuesday 1 December`, `Tue 1 Dec 2026`. */
export function dateWords(iso: string, o: { weekday?: 'long' | 'short'; month?: 'long' | 'short'; year?: boolean } = {}): string {
  const p = ymd(iso);
  if (!p) return iso;
  const wd = weekdayOf(iso) - 1;
  const day = o.weekday === 'long' ? `${WEEKDAYS[wd]} ` : o.weekday === 'short' ? `${WEEKDAYS_SHORT[wd]} ` : '';
  const month = o.month === 'short' ? MONTHS_SHORT[p.m - 1] : MONTHS[p.m - 1];
  return `${day}${p.d} ${month}${o.year ? ` ${p.y}` : ''}`;
}

/** `21st`, `2nd`, `13th`. */
export function ordinal(n: number): string {
  const s = n % 100;
  if (s > 10 && s < 14) return `${n}th`;
  return `${n}${['th', 'st', 'nd', 'rd'][n % 10] ?? 'th'}`;
}

// ---- two times ----------------------------------------------------------------------------

/** `HH:MM` as minutes after midnight, or null when it is not a time. */
export function minutesOf(t: string): number | null {
  const m = /^(\d{1,2})[:.](\d{2})$/.exec(t.trim());
  if (!m) return null;
  const [h, mm] = [Number(m[1]), Number(m[2])];
  return h > 23 || mm > 59 ? null : h * 60 + mm;
}

export const hhmm = (minutes: number): string => {
  const n = ((Math.round(minutes) % 1440) + 1440) % 1440;
  return `${pad(Math.floor(n / 60))}:${pad(n % 60)}`;
};

/** The start, the length, and whether the end is before the start; null while either is not a time. */
export function times(f: Pick<RuleForm, 'from' | 'until'>): { start: number; minutes: number; overnight: boolean } | null {
  const start = minutesOf(f.from);
  const end = minutesOf(f.until);
  if (start === null || end === null) return null;
  return { start, minutes: end > start ? end - start : end + 1440 - start, overnight: end <= start };
}

/** `1 hour`, `2 hours`, `2 h 30 min`, `45 min`. */
export function lengthWords(minutes: number): string {
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  if (!m) return h === 1 ? '1 hour' : `${h} hours`;
  return h ? `${h} h ${m} min` : `${m} min`;
}

/** `30 minutes`, `an hour`, `2 hours`, `90 minutes`: an overlap, in a sentence. */
export function minutesWords(n: number): string {
  if (n % 60) return `${n} minute${n === 1 ? '' : 's'}`;
  return n === 60 ? 'an hour' : `${n / 60} hours`;
}

// ---- the form -----------------------------------------------------------------------------

/** What the date itself says: its weekday, which of its weekday it is in its month, and
 *  whether it is the last of them. A fifth is offered only as the last. */
export function ofDate(iso: IsoDate): { weekday: number; nth: number; last: boolean; day: number; month: number } {
  const p = ymd(iso) ?? { y: 2000, m: 1, d: 1 };
  const nth = Math.ceil(p.d / 7);
  const last = p.d + 7 > daysIn(p.y, p.m);
  return { weekday: weekdayOf(iso) || 1, nth: nth > 4 ? -1 : nth, last, day: p.d, month: p.m };
}

/** A new show's answers: once only, the one answer that is never wrong. */
export function newForm(first: IsoDate, from = '20:00', until = '21:00'): RuleForm {
  const d = ofDate(first);
  return {
    first,
    from,
    until,
    overnightOk: false,
    mode: 'list',
    kind: 'once',
    every: 1,
    weekdays: [d.weekday],
    nth: d.nth,
    weekday: d.weekday,
    day: d.day,
    yearDay: d.day,
    yearMonth: d.month,
    dates: [],
    end: { kind: 'never', until: '', count: 10 },
    skip: [],
    moves: [],
  };
}

/** One row of Repeats as a list: a sentence made from the date in First on. */
export interface Choice {
  key: string;
  label: string;
}

/** The rows for a date. The last of its weekday in its month adds *the last*; a fifth offers only that. */
export function choices(first: IsoDate): Choice[] {
  const d = ofDate(first);
  const p = ymd(first);
  const wd = WEEKDAYS[d.weekday - 1];
  const rows: Choice[] = [
    { key: 'once', label: 'Once only' },
    { key: 'weekly', label: `Every week on ${wd}` },
  ];
  if (p && Math.ceil(p.d / 7) <= 4) rows.push({ key: `nth${Math.ceil(p.d / 7)}`, label: `Every month on the ${NTH_WORD[Math.ceil(p.d / 7)]} ${wd}` });
  if (d.last) rows.push({ key: 'nth-1', label: `Every month on the last ${wd}` });
  rows.push({ key: 'yearly', label: `Every year on ${d.day} ${MONTHS[d.month - 1]}` });
  rows.push({ key: 'dates', label: 'On dates I choose…' });
  return rows;
}

/** The row the answers match, or null when only the whole rule can show them. */
export function choiceKey(f: RuleForm): string | null {
  const d = ofDate(f.first);
  const p = ymd(f.first);
  if (f.kind === 'once' || f.kind === 'dates') return f.kind;
  if (f.kind === 'weekly') return f.every === 1 && f.weekdays.length === 1 && f.weekdays[0] === d.weekday ? 'weekly' : null;
  if (f.kind === 'monthly_nth' && f.every === 1 && f.weekday === d.weekday && p) {
    if (f.nth === -1) return d.last ? 'nth-1' : null;
    return f.nth === Math.ceil(p.d / 7) ? `nth${f.nth}` : null;
  }
  if (f.kind === 'yearly') return f.yearDay === d.day && f.yearMonth === d.month ? 'yearly' : null;
  return null;
}

const noEnd = (f: RuleForm): RuleForm => ({ ...f, end: { ...f.end, kind: 'never' } });

/** A row picked. *On dates I choose…* opens the whole rule, since the dates are picked in the calendar. */
export function pickChoice(f: RuleForm, key: string): RuleForm {
  const d = ofDate(f.first);
  if (key === 'once') return noEnd({ ...f, kind: 'once' });
  if (key === 'dates') return noEnd({ ...f, kind: 'dates', mode: 'full', dates: f.dates.length ? f.dates : [f.first] });
  if (key === 'weekly') return { ...f, kind: 'weekly', every: 1, weekdays: [d.weekday] };
  if (key === 'yearly') return { ...f, kind: 'yearly', yearDay: d.day, yearMonth: d.month };
  const nth = /^nth(-?\d)$/.exec(key);
  if (nth) return { ...f, kind: 'monthly_nth', every: 1, nth: Number(nth[1]), weekday: d.weekday };
  return f;
}

/** The five kinds of the whole rule; Monthly is two patterns. */
export type KindTab = 'once' | 'weekly' | 'monthly' | 'yearly' | 'dates';
export const KIND_TABS: readonly { key: KindTab; label: string }[] = [
  { key: 'once', label: 'Once' },
  { key: 'weekly', label: 'Weekly' },
  { key: 'monthly', label: 'Monthly' },
  { key: 'yearly', label: 'Yearly' },
  { key: 'dates', label: 'Chosen dates' },
];
export const kindTab = (k: RepeatKind): KindTab => (k === 'monthly_nth' || k === 'monthly_day' ? 'monthly' : k);

/** One of the five pressed. */
export function pickKind(f: RuleForm, tab: KindTab): RuleForm {
  if (tab === 'once') return noEnd({ ...f, kind: 'once' });
  if (tab === 'dates') return noEnd({ ...f, kind: 'dates', dates: f.dates.length ? f.dates : [f.first] });
  if (tab === 'monthly') return { ...f, kind: f.kind === 'monthly_day' ? 'monthly_day' : 'monthly_nth' };
  return { ...f, kind: tab };
}

/** First on changed. As a list the rows are made from the date, so the pattern follows it
 *  (a weekly Tuesday becomes a weekly Wednesday); as the whole rule the pattern stays. */
export function setFirst(f: RuleForm, first: IsoDate): RuleForm {
  if (!ymd(first)) return { ...f, first };
  if (f.mode === 'full') return { ...f, first };
  const was = choiceKey(f);
  const d = ofDate(first);
  const next: RuleForm = { ...f, first, weekdays: [d.weekday], weekday: d.weekday, nth: d.nth, day: d.day, yearDay: d.day, yearMonth: d.month };
  // The last Tuesday stays the last Tuesday when the new date is one too.
  if (was === 'nth-1' && d.last) next.nth = -1;
  return next;
}

export function toggleWeekday(f: RuleForm, wd: number): RuleForm {
  const has = f.weekdays.includes(wd);
  return { ...f, weekdays: has ? f.weekdays.filter((x) => x !== wd) : [...f.weekdays, wd].sort((a, b) => a - b) };
}

/** A date added to or taken from the chosen dates. */
export function toggleDate(f: RuleForm, iso: IsoDate): RuleForm {
  const has = f.dates.includes(iso);
  return { ...f, dates: has ? f.dates.filter((x) => x !== iso) : [...f.dates, iso].sort() };
}

export const withSkip = (f: RuleForm, on: IsoDate): RuleForm => (f.skip.includes(on) ? f : { ...f, skip: [...f.skip, on].sort(), moves: f.moves.filter((m) => m.from !== on) });
export const withoutSkip = (f: RuleForm, on: IsoDate): RuleForm => ({ ...f, skip: f.skip.filter((x) => x !== on) });
/** A date aired at another time: a date and a start, on the station's wall clock. */
export const withMove = (f: RuleForm, on: IsoDate, to: IsoDate, at: string): RuleForm => ({
  ...f,
  moves: [...f.moves.filter((m) => m.from !== on), { from: on, to: `${to}T${hhmm(minutesOf(at) ?? 0)}:00`, minutes: null }].sort((a, b) => a.from.localeCompare(b.from)),
});
export const withoutMove = (f: RuleForm, on: IsoDate): RuleForm => ({ ...f, moves: f.moves.filter((m) => m.from !== on) });

/** Skipped and moved dates the new pattern no longer gives are dropped, and counted, so the
 *  preview can say so once (`1 skipped date no longer applies.`). */
export function dropGone(f: RuleForm, dropped: RulePreview['dropped']): { form: RuleForm; words: string } {
  const skip = f.skip.filter((d) => !dropped.skip.includes(d));
  const moves = f.moves.filter((m) => !dropped.moves.includes(m.from));
  const [s, m] = [f.skip.length - skip.length, f.moves.length - moves.length];
  if (!s && !m) return { form: f, words: '' };
  const said = [s ? `${s} skipped date${s === 1 ? '' : 's'}` : '', m ? `${m} moved date${m === 1 ? '' : 's'}` : ''].filter(Boolean).join(' and ');
  return { form: { ...f, skip, moves }, words: `${said} no longer ${s + m === 1 ? 'applies' : 'apply'}.` };
}

/** A problem the form can see without the station: something not yet a time, a date or a number. */
export interface FormProblem {
  field: 'first' | 'from' | 'until' | 'overnight' | 'repeat' | 'end';
  message: string;
}

export function formProblems(f: RuleForm): FormProblem[] {
  const out: FormProblem[] = [];
  if (!ymd(f.first)) out.push({ field: 'first', message: 'Give the first date.' });
  if (minutesOf(f.from) === null) out.push({ field: 'from', message: 'The start is a time, such as 20:00.' });
  else if (minutesOf(f.until) === null) out.push({ field: 'until', message: 'The end is a time, such as 21:00.' });
  else if (times(f)?.overnight && !f.overnightOk) out.push({ field: 'overnight', message: 'It ends before it starts, so it runs past midnight. Tick the box if it does, or change the times.' });
  if (f.kind === 'weekly' && !f.weekdays.length) out.push({ field: 'repeat', message: 'Press at least one day.' });
  if (f.kind === 'dates' && !f.dates.length) out.push({ field: 'repeat', message: 'Pick at least one date in the calendar.' });
  if ((f.kind === 'weekly' || f.kind === 'monthly_nth' || f.kind === 'monthly_day') && !(Number.isInteger(f.every) && f.every >= 1 && f.every <= 12)) {
    out.push({ field: 'repeat', message: 'Every 1 to 12.' });
  }
  if (repeats(f) && f.end.kind === 'until' && !ymd(f.end.until)) out.push({ field: 'end', message: 'Give the date it ends on.' });
  if (repeats(f) && f.end.kind === 'count' && !(Number.isInteger(f.end.count) && f.end.count >= 1 && f.end.count <= 520)) {
    out.push({ field: 'end', message: 'Between 1 and 520 episodes.' });
  }
  return out;
}

/** The show comes back by a pattern, so it can have an end. */
export const repeats = (f: Pick<RuleForm, 'kind'>): boolean => f.kind !== 'once' && f.kind !== 'dates';

function repeatOf(f: RuleForm): Repeat {
  switch (f.kind) {
    case 'once':
      return { kind: 'once' };
    case 'weekly':
      return { kind: 'weekly', every: f.every, weekdays: [...f.weekdays].sort((a, b) => a - b) };
    case 'monthly_nth':
      return { kind: 'monthly_nth', every: f.every, nth: f.nth, weekday: f.weekday };
    case 'monthly_day':
      return { kind: 'monthly_day', every: f.every, day: f.day };
    case 'yearly':
      return { kind: 'yearly', month: f.yearMonth, day: f.yearDay };
    case 'dates':
      return { kind: 'dates', dates: [...f.dates].sort() };
  }
}

/** The rule the answers make, or null while the form has a problem of its own. */
export function toRule(f: RuleForm): Rule | null {
  if (formProblems(f).length) return null;
  const t = times(f)!;
  const end: RuleEnd = !repeats(f) || f.end.kind === 'never' ? { kind: 'never' } : f.end.kind === 'until' ? { kind: 'until', until: f.end.until } : { kind: 'count', count: f.end.count };
  return {
    // Chosen dates start at the earliest of them.
    starts_on: f.kind === 'dates' ? [...f.dates].sort()[0] : f.first,
    start: t.start,
    minutes: t.minutes,
    repeat: repeatOf(f),
    end,
    // A one-off has no date to skip; a chosen date is taken out rather than skipped.
    skip: repeats(f) ? [...f.skip].sort() : [],
    moves: f.kind === 'once' ? [] : f.moves,
  };
}

/** A stored rule as the questions show it. `first` stands in for a rule with no start date
 *  (an old weekly slot): the next date it gives, or today. `whole` is the person's remembered
 *  choice of the whole rule; without it the list is shown whenever it can say the rule. */
export function fromRule(rule: Rule, first: IsoDate, whole = false): RuleForm {
  const start = rule.starts_on ?? first;
  const base = newForm(start, hhmm(rule.start), hhmm(rule.start + (rule.minutes ?? 60)));
  const r = rule.repeat;
  const f: RuleForm = {
    ...base,
    overnightOk: rule.start + (rule.minutes ?? 60) >= 1440,
    kind: r.kind,
    every: 'every' in r ? r.every : 1,
    weekdays: r.kind === 'weekly' ? [...r.weekdays] : base.weekdays,
    nth: r.kind === 'monthly_nth' ? r.nth : base.nth,
    weekday: r.kind === 'monthly_nth' ? r.weekday : base.weekday,
    day: r.kind === 'monthly_day' ? r.day : base.day,
    yearDay: r.kind === 'yearly' ? r.day : base.yearDay,
    yearMonth: r.kind === 'yearly' ? r.month : base.yearMonth,
    dates: r.kind === 'dates' ? [...r.dates] : [],
    end: { kind: rule.end.kind, until: rule.end.kind === 'until' ? rule.end.until : '', count: rule.end.kind === 'count' ? rule.end.count : 10 },
    skip: [...rule.skip],
    moves: rule.moves.map((m) => ({ ...m })),
  };
  return { ...f, mode: whole || choiceKey(f) === null || f.kind === 'dates' ? 'full' : 'list' };
}

// ---- what is sent -------------------------------------------------------------------------

/** The preview's request, or null while the form has a problem of its own. */
export function previewRequest(f: RuleForm, o: { show?: string | null; series?: number | null; limit?: number } = {}): RulePreviewRequest | null {
  const rule = toRule(f);
  if (!rule) return null;
  return { rule, ...(o.show ? { show: o.show } : {}), ...(o.show && o.series ? { series: o.series } : {}), limit: Math.min(60, Math.max(1, o.limit ?? 10)) };
}

/** The weekly slot a rule is, for a station older than 1.0.4: every week, one weekday, no end,
 *  nothing skipped or moved. Any other rule is null: that station cannot keep it. */
export function slotOf(rule: Rule | null): Slot | null {
  if (!rule || rule.repeat.kind !== 'weekly' || rule.repeat.every !== 1 || rule.repeat.weekdays.length !== 1) return null;
  if (rule.end.kind !== 'never' || rule.skip.length || rule.moves.length) return null;
  return { weekday: rule.repeat.weekdays[0], start: rule.start, minutes: rule.minutes };
}

/** A weekly slot as the rule it is (the design's mapping, section 9). */
export const ruleOfSlot = (s: Slot): Rule => ({
  starts_on: null,
  start: s.start,
  minutes: s.minutes,
  repeat: { kind: 'weekly', every: 1, weekdays: [s.weekday] },
  end: { kind: 'never' },
  skip: [],
  moves: [],
});

/** What an older station can keep of the answers: only *Every week on* one day, with no end. */
export const fitsOldStation = (f: RuleForm): boolean => f.kind === 'weekly' && f.every === 1 && f.weekdays.length === 1 && f.end.kind === 'never';

/** When a show airs, as a save sends it. A station that stores rules takes `rule` (and
 *  `draft_first` when asked); an older one refuses a field it does not know, so it is sent the
 *  weekly `slot` the rule is, and nothing else. */
export function whenFields(rule: Rule | null, o: { rules: boolean; draftFirst?: boolean }): Pick<ShowInput, 'rule' | 'slot' | 'draft_first'> {
  if (!o.rules) return { slot: slotOf(rule) };
  return { rule, ...(o.draftFirst && rule ? { draft_first: true } : {}) };
}

/** New show's body: the three answers of part 1, when it airs, and nothing else; the rest of
 *  a show is the show editor's. With a rule the station drafts the first episode in the same
 *  transaction; with none (*No dates yet*) it only makes the show. */
export function newShowInput(a: { name: string; colour: number; step: ShowStep }, rule: Rule | null, rules: boolean): ShowInput {
  return {
    name: a.name.trim(),
    description: '',
    colour_slot: a.colour,
    layout: 'poster',
    hosts: [],
    tags: [],
    explicit: false,
    step: a.step,
    ...whenFields(rule, { rules, draftFirst: true }),
  };
}

/** The key under which this computer remembers that a person last used the whole rule. */
export const modeKey = (accountId: string | null | undefined): string => `weavesignal.rule.mode.${accountId ?? 'station'}`;

/** Fewer choices: back to the list. A rule the list cannot say is remade from the date, in
 *  its own kind (several weekdays become the date's one; a day of the month its weekday). */
export function toList(f: RuleForm): RuleForm {
  const list: RuleForm = { ...f, mode: 'list' };
  // Chosen dates stay chosen: the list says *On dates I choose…* and the calendar still picks.
  if (choiceKey(list) !== null) return list;
  const d = ofDate(f.first);
  if (f.kind === 'weekly') return pickChoice(list, 'weekly');
  if (f.kind === 'yearly') return pickChoice(list, 'yearly');
  return pickChoice(list, d.nth === -1 ? 'nth-1' : `nth${d.nth}`);
}

/** The old station's stand-in for the rule's short form, which only a newer station writes:
 *  `Sat, 20:00–23:00`. */
export function shortOfSlot(s: Slot): string {
  const end = s.minutes === null ? '' : `–${hhmm(s.start + s.minutes)}`;
  return `${WEEKDAYS_SHORT[s.weekday - 1] ?? ''}, ${hhmm(s.start)}${end}`;
}

/** Remember on this computer whether a person last used the whole rule. */
export function rememberMode(accountId: string | null | undefined, mode: RuleForm['mode']): void {
  try {
    globalThis.localStorage?.setItem(modeKey(accountId), mode);
  } catch {
    // A private window: the list is shown next time.
  }
}

export function remembersWhole(accountId: string | null | undefined): boolean {
  try {
    return globalThis.localStorage?.getItem(modeKey(accountId)) === 'full';
  } catch {
    return false;
  }
}
