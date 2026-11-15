// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What the control plane's preview of a rule becomes on the page (`calendar.rule.preview`;
// new-show.md sections 1 and 6): the rows of next dates, the small months, the line about
// clashes, and whether the save is held. Nothing here works out a date or judges a clash
// (section 8): every date, state and clash is the preview's, and this only words and places
// them. Pure, so it is tested on its own.

import type { Clash, DraftedChoice, IsoDate, PreviewDate, Rule, RuleChangePlan, RulePreview } from '../../lib/api.ts';
import type { Show } from '../../lib/schedule.ts';
import { hm, parts } from '../model.ts';
import { MONTHS, dateWords, daysIn, fitsOldStation, formProblems, hhmm, isoOf, lengthWords, minutesWords, shortOfSlot, toRule, weekdayOf, ymd, type RuleForm } from './form.ts';
import type { PreviewState } from './previewer.ts';

/** The station's date at a moment. */
export function dateIn(ms: number, zone: string): IsoDate {
  const p = parts(ms, zone);
  return isoOf(p.y, p.m, p.d);
}

/** The date a preview date really airs on: its own, or where it was moved to. */
export const airDate = (d: PreviewDate, zone: string): IsoDate => (d.state === 'skipped' ? d.on : dateIn(d.start * 1000, zone));

/** One line of Next dates. */
export interface Row {
  /** The rule's own date: the row's identity, and what Skip and Move… act on. */
  on: IsoDate;
  /** `Ep 2`, or empty when it takes no number (a skipped date). */
  ep: string;
  /** `Tue 5 Jan`, with the year on the first row of each new year. */
  day: string;
  /** `20:00–21:00`, the real times. */
  times: string;
  state: PreviewDate['state'];
  now: boolean;
  /** `was Tue 6 Apr`, on a moved date. */
  moved: string | null;
  /** `Six Nations Preview, 19:30 to 20:30 · overlaps 30 min`. */
  clash: string | null;
  /** `CLOCKS FORWARD · 01:30 does not happen`. */
  clock: string | null;
  /** The date it airs on, for the small months. */
  air: IsoDate;
  /** `Ep 4 · drafted`, on a date an episode already fills. */
  filled: string | null;
}

const clashLine = (c: Clash, zone: string): string =>
  `${c.show}, ${hm(c.start * 1000, zone)} to ${hm((c.start + c.minutes * 60) * 1000, zone)} · overlaps ${lengthWords(c.overlap_minutes)}`;

/** The rows, in the order given. `today` settles which rows carry their year; `nominal` is
 *  the rule's own start (`20:00`), for the line on a night the clocks change. */
export function rows(preview: RulePreview, zone: string, today: IsoDate, nominal: string): Row[] {
  let seenYear = ymd(today)?.y ?? 0;
  const thisYear = seenYear;
  return preview.dates.map((d) => {
    const air = airDate(d, zone);
    const y = ymd(air)?.y ?? thisYear;
    const year = y !== thisYear && y !== seenYear;
    seenYear = y;
    const end = (d.start + d.minutes * 60) * 1000;
    return {
      on: d.on,
      ep: d.number !== null && d.state !== 'skipped' ? `Ep ${d.number}` : '',
      day: dateWords(air, { weekday: 'short', month: 'short', year }),
      times: `${hm(d.start * 1000, zone)}–${hm(end, zone)}`,
      state: d.state,
      now: d.now && d.state !== 'skipped',
      moved: d.moved_from && d.state !== 'skipped' ? `was ${dateWords(d.moved_from.slice(0, 10), { weekday: 'short', month: 'short' })}` : null,
      clash: d.state === 'airs' && d.clashes.length ? clashLine(d.clashes[0], zone) : null,
      clock: d.clock === 'forward' ? `CLOCKS FORWARD · ${nominal} does not happen` : d.clock === 'back' ? `CLOCKS BACK · the first ${nominal} to the second` : null,
      air,
      filled: d.state === 'filled' ? `${d.episode ? `Ep ${d.episode.number} · ` : ''}${d.episode?.aired ? 'aired' : 'drafted'}` : null,
    };
  });
}

/** The dates that air: not skipped, and not already an episode's. */
export const live = (preview: RulePreview): PreviewDate[] => preview.dates.filter((d) => d.state !== 'skipped');

/** How a day in the small months is drawn. Shape carries each state; colour only repeats it. */
export type DayState = 'air' | 'skip' | 'clash' | 'filled' | 'from' | '';

export interface Day {
  iso: IsoDate;
  d: number;
  state: DayState;
  today: boolean;
  /** What a pointer or a screen reader is told: `Tuesday 5 January: skipped`. */
  label: string;
  /** The row this day belongs to, for ⏎. */
  row: IsoDate | null;
}

export interface Month {
  y: number;
  m: number;
  caption: string;
  /** Empty cells before the first, so the 1st sits under its weekday. */
  lead: number;
  days: Day[];
}

/** Only the months that hold one of the next six dates, twelve at most, so a yearly show
 *  draws six Decembers and not seventy months. Chosen dates draw three months in a row at
 *  least, to pick from. With nothing to show, the month of First on. */
export function months(preview: RulePreview | null, form: Pick<RuleForm, 'first' | 'kind'>, zone: string, today: IsoDate): Month[] {
  const dates = preview?.dates ?? [];
  const by = new Map<IsoDate, { d: PreviewDate; as: DayState }>();
  const airing = dates.filter((d) => d.state !== 'skipped');
  const limit = airing.length ? airDate(airing[Math.min(5, airing.length - 1)], zone) : form.first;
  const key = (iso: IsoDate) => {
    const p = ymd(iso);
    return p ? p.y * 12 + (p.m - 1) : null;
  };
  const want = new Set<number>();
  const add = (iso: IsoDate) => {
    const k = key(iso);
    if (k !== null) want.add(k);
  };
  for (const d of dates) {
    const air = airDate(d, zone);
    const from = d.moved_from?.slice(0, 10) ?? null;
    if (d.state === 'skipped') by.set(d.on, { d, as: 'skip' });
    else {
      if (from && from !== air && !by.has(from)) by.set(from, { d, as: 'from' });
      by.set(air, { d, as: d.state === 'filled' ? 'filled' : d.clashes.length ? 'clash' : 'air' });
    }
    if (air <= limit) {
      add(air);
      if (from) add(from);
    }
  }
  if (!want.size) add(form.first);
  if (form.kind === 'dates') {
    const first = Math.min(...want, key(form.first) ?? Infinity);
    if (Number.isFinite(first)) for (let i = 0; i < 3; i++) want.add(first + i);
  }
  return [...want]
    .sort((a, b) => a - b)
    .slice(0, 12)
    .map((k) => {
      const [y, m] = [Math.floor(k / 12), (k % 12) + 1];
      const days: Day[] = [];
      for (let d = 1; d <= daysIn(y, m); d++) {
        const iso = isoOf(y, m, d);
        const hit = by.get(iso);
        const said = dateWords(iso, { weekday: 'long' });
        const what =
          hit?.as === 'skip' ? 'skipped'
          : hit?.as === 'filled' ? `episode ${hit.d.episode?.number ?? hit.d.number ?? ''}, ${hit.d.episode?.aired ? 'aired' : 'drafted'}`.replace(' ,', ',')
          : hit?.as === 'clash' ? `clashes with ${hit.d.clashes[0].show}`
          : hit?.as === 'air' ? 'airs'
          : hit?.as === 'from' ? `moved to ${dateWords(airDate(hit.d, zone), {})}`
          : '';
        days.push({ iso, d, state: hit?.as ?? '', today: iso === today, label: what ? `${said}: ${what}` : said, row: hit ? hit.d.on : null });
      }
      return { y, m, caption: `${MONTHS[m - 1]} ${y}`, lead: weekdayOf(isoOf(y, m, 1)) - 1, days };
    });
}

/** The line above the months when a date clashes, and whether it stops the save. */
export interface ClashNotice {
  /** `refused`: this session may not draft the first date over the other show. `later`: a
   *  later date would be refused when it comes to be drafted. `warn`: it may go on. */
  kind: 'refused' | 'later' | 'warn';
  title: string;
  body: string;
  /** `Try 22:00 to 23:00`: a start that is clear on every date shown. */
  offer: { start: number; label: string } | null;
}

const whenOf = (c: Clash, zone: string): string => c.when ?? `${hm(c.start * 1000, zone)} to ${hm((c.start + c.minutes * 60) * 1000, zone)}`;
const people = (names: string[]): string => (names.length < 2 ? names.join('') : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`);

/** What a clash costs, said once (section 1). */
export const CLASH_COST = 'Where two shows overlap, the one that starts later is on air by the schedule: its presenter is named to callers and its step shapes the studio.';

/** `mayRefuse`: this session is one the station refuses a clash (a presenter's), so a later
 *  date that clashes is worded as theirs to ask about, not as a schedule's to pre-empt. The
 *  station still decides what is refused: only its `refused` holds the save. */
export function clashNotice(preview: RulePreview, zone: string, mayRefuse = false): ClashNotice | null {
  const airs = preview.dates.filter((d) => d.state === 'airs');
  const bad = airs.filter((d) => d.clashes.length);
  if (!bad.length) return null;
  const first = airs[0];
  const refusedFirst = first.clashes.find((c) => c.refused) ?? null;
  const offer = preview.clear ? { start: preview.clear.start, label: `Try ${hhmm(preview.clear.start)} to ${hhmm(preview.clear.start + preview.clear.minutes)}` } : null;
  if (refusedFirst) {
    const who = refusedFirst.with.length ? `, with ${people(refusedFirst.with)}` : '';
    return {
      kind: 'refused',
      title: `${refusedFirst.show} has that time.`,
      body: `${whenOf(refusedFirst, zone)}${who}. Pick another time, or choose No dates yet and ask the station.`,
      offer,
    };
  }
  const one = bad.length === 1;
  const count = airs.length === 1 ? 'This date clashes.' : `${bad.length} of the next ${airs.length} dates clash${one ? 'es' : ''}.`;
  if (mayRefuse || bad.some((d) => d.clashes.some((c) => c.refused))) {
    return { kind: 'later', title: count, body: `You can save your show; ask the station about ${one ? 'that date' : 'those dates'} before you draft ${one ? 'it' : 'them'}.`, offer: null };
  }
  const other = bad[0].clashes[0];
  const every = bad.length === airs.length && airs.length > 1 && bad.every((d) => d.clashes[0].slug === other.slug && d.clashes[0].show === other.show);
  return {
    kind: 'warn',
    title: every ? `Every date overlaps ${other.show} (${whenOf(other, zone)}) by ${minutesWords(other.overlap_minutes)}.` : count,
    body: CLASH_COST,
    // Only when the whole show is in another's way is another time offered; one date of several
    // is put right on its row. A one-off has no Skip or Move on its row, so it is offered too.
    offer: bad.length === airs.length ? offer : null,
  };
}

/** Whether any date that airs clashes: *Make the show* then reads *Make the show anyway*. */
export const anyClash = (preview: RulePreview | null): boolean => !!preview && preview.dates.some((d) => d.state === 'airs' && d.clashes.length > 0);

/** Why the save is held, or empty when it may go on. The station's problems come first. */
export function held(preview: RulePreview | null, o: { unreachable: boolean; stale: boolean; local: boolean }): string {
  if (o.local) return 'Put right what is marked first.';
  if (o.unreachable) return 'The station cannot be reached.';
  if (!preview || o.stale) return 'Working out the dates…';
  if (preview.problems.length) return preview.problems[0].message;
  const airs = preview.dates.filter((d) => d.state !== 'skipped');
  if (!airs.length) return 'This makes no dates.';
  if (airs.find((d) => d.state === 'airs')?.clashes.some((c) => c.refused)) return 'Another show has that time.';
  return '';
}

/** What the host of `calendar.rule` needs to word and enable its buttons. */
export interface RuleStatus {
  /** Why the save is held, or empty when it may go on. */
  held: string;
  /** A date that airs clashes: *Make the show anyway*. */
  clash: boolean;
  /** The rule the answers make, or null while the form has a problem of its own. */
  rule: Rule | null;
  /** The newest answer that matches the questions; null while one is awaited. */
  answer: RulePreview | null;
}

/** The answers and what the station last said of them, as the buttons need it. A station
 *  older than 1.0.4 is asked nothing: there the form alone decides, and only a weekly time fits. */
export function ruleStatus(form: RuleForm, st: PreviewState, rules: boolean): RuleStatus {
  const local = formProblems(form).length > 0;
  const rule = toRule(form);
  if (!rules) return { held: local ? 'Put right what is marked first.' : fitsOldStation(form) ? '' : 'This station keeps a weekly time only.', clash: false, rule, answer: null };
  return { held: held(st.answer, { unreachable: st.unreachable, stale: st.stale, local }), clash: !st.stale && anyClash(st.answer), rule, answer: st.stale ? null : st.answer };
}

/** A preview's state before the station has been asked, with a first answer if there is one. */
export const firstState = (answer: RulePreview | null = null): PreviewState => ({ answer, stale: false, working: false, unreachable: false });

/** The first date that will air and is not yet an episode: episode 1's, for Done. */
export const firstToAir = (preview: RulePreview | null): PreviewDate | null => preview?.dates.find((d) => d.state === 'airs') ?? null;

/** What follows the rows: more to show, no end, or all of them. */
export function afterWords(preview: RulePreview, form: Pick<RuleForm, 'kind' | 'every' | 'end'>): { more: boolean; words: string } {
  const shown = live(preview).length;
  if (form.kind === 'once') return { more: false, words: 'It airs once, so this is the only date.' };
  const endless = form.kind !== 'dates' && form.end.kind === 'never';
  // The station says whether there are more; a reply that only counts them is read as well.
  const more = shown < 60 && (preview.more ?? (preview.total === null ? endless : shown < preview.total));
  if (endless) {
    const each = form.kind === 'weekly' ? (form.every === 1 ? 'every week' : `every ${form.every} weeks`) : form.kind === 'yearly' ? 'every year' : form.every === 1 ? 'every month' : `every ${form.every} months`;
    return { more, words: `and ${each} after, with no end` };
  }
  if (more) return { more, words: '' };
  return { more, words: shown === 1 ? 'That is the only date.' : `Those are all ${shown}.` };
}

/** How many episodes a new show's rule makes in all, for *Draft all 10*: the rule's own
 *  count, or the dates given when the station says there are no more. Null with no end. */
export function madeInAll(rule: Rule | null, preview: RulePreview | null): number | null {
  if (!rule || rule.repeat.kind === 'once') return null;
  if (rule.end.kind === 'count') return rule.end.count;
  return preview?.total ?? null;
}

/** Between which dates a moved date must stay, from its neighbours in the rows shown, so the
 *  hint can say so before the station is asked. The station is still the judge. */
export function moveBounds(preview: RulePreview, on: IsoDate, zone: string, today: IsoDate): { after: IsoDate; before: IsoDate | null } {
  const airs = live(preview);
  const i = airs.findIndex((d) => d.on === on);
  return {
    after: i > 0 ? airDate(airs[i - 1], zone) : today,
    before: i >= 0 && i < airs.length - 1 ? airDate(airs[i + 1], zone) : null,
  };
}

/** `Keep it between 2 March and 4 May, so the episodes stay in order.` */
export function boundsWords(b: { after: IsoDate; before: IsoDate | null }): string {
  return `Keep it ${b.before ? `between ${dateWords(b.after)} and ${dateWords(b.before)}` : `after ${dateWords(b.after)}`}, so the episodes stay in order.`;
}

/** The first line of Done and the DJ's Done: when episode 1 is. `tonight` for today's date. */
export function firstWords(d: { start: number; minutes: number }, zone: string, today: IsoDate): string {
  const on = dateIn(d.start * 1000, zone);
  const day = dateWords(on, { weekday: 'long', year: (ymd(on)?.y ?? 0) !== (ymd(today)?.y ?? 0) });
  return `${on === today ? (parts(d.start * 1000, zone).hh >= 17 ? 'tonight, ' : 'today, ') : ''}${day}, ${hm(d.start * 1000, zone)} to ${hm((d.start + d.minutes * 60) * 1000, zone)}`;
}

// ---- Before you save (`calendar.rule.change`) -----------------------------------------------


/** One drafted episode on the old dates, as the list shows it. */
export interface ChangeLine {
  ep: string;
  /** `Tue 2 March`, with the time when only the time changes. */
  from: string;
  /** Where it goes; null when the new rule has no date for it. */
  to: string | null;
  /** After the number: `has no date under the new rule. It stays on Tue 3 August.` */
  stays: string | null;
  clash: string | null;
}

const dayOf = (s: number, zone: string) => dateWords(dateIn(s * 1000, zone), { weekday: 'short' });

/** The drafted episodes, each on one line: `Ep 4 · Tue 2 March → Wed 3 March`. */
export function changeLines(plan: RuleChangePlan, zone: string): ChangeLine[] {
  return plan.drafted.map((e) => {
    const ep = `Ep ${e.number}`;
    const from = dayOf(e.from, zone);
    if (e.to === null) return { ep, from, to: null, stays: `has no date under the new rule. It stays on ${from}.`, clash: null };
    const to = dayOf(e.to, zone);
    const same = from === to;
    return {
      ep,
      from: same ? `${from}, ${hm(e.from * 1000, zone)}` : from,
      to: same ? `${to}, ${hm(e.to * 1000, zone)}` : to,
      stays: null,
      clash: e.clash ? clashLine(e.clash, zone) : null,
    };
  });
}

/** The episodes moved or made by hand: `Ep 6 · Thu 6 May, 19:00`. */
export const byHandLines = (plan: RuleChangePlan, zone: string): string[] => plan.by_hand.map((e) => `Ep ${e.number} · ${dayOf(e.at, zone)}, ${hm(e.at * 1000, zone)}`);

/** How many drafted episodes would move. */
export const movable = (plan: RuleChangePlan | null): number => plan?.drafted.filter((e) => e.to !== null && e.to !== e.from).length ?? 0;

/** Whether a changed rule touches any episode at all: without one, Save needs no second look. */
export const hasEpisodes = (plan: RuleChangePlan | null): boolean => !!plan && (plan.aired > 0 || plan.drafted.length > 0 || plan.by_hand.length > 0);

/** `Save and move 2 episodes`, `Save, and leave them`, or `Save`. */
export function saveWords(plan: RuleChangePlan | null, choice: DraftedChoice): string {
  const n = movable(plan);
  if (!n) return 'Save';
  return choice === 'move' ? `Save and move ${n} episode${n === 1 ? '' : 's'}` : 'Save, and leave them';
}

/** `3 episodes have aired since 1 December. They are not changed.` */
export function airedWords(plan: RuleChangePlan | null): string {
  if (!plan || !plan.aired) return '';
  const since = plan.since ? ` since ${dateWords(plan.since)}` : '';
  return `${plan.aired} episode${plan.aired === 1 ? ' has' : 's have'} aired${since}. ${plan.aired === 1 ? 'It is' : 'They are'} not changed.`;
}

/** `for 10 episodes in all: 3 aired, 3 drafted, 4 to come`: After, for a show with a past. */
export function countWords(plan: RuleChangePlan | null, count: number): string {
  if (!plan || !hasEpisodes(plan)) return '';
  const drafted = plan.drafted.length + plan.by_hand.length;
  return `for ${count} episodes in all: ${plan.aired} aired, ${drafted} drafted, ${Math.max(0, count - plan.aired - drafted)} to come`;
}

// ---- Nothing coming up (`calendar.unscheduled`) ---------------------------------------------


export interface Unscheduled {
  /** Shows without a rule: *No dates yet*. */
  none: { show: Show; why: string }[];
  /** A one-off that has aired, or a rule that has ended: not a fault. */
  finished: { show: Show; why: string }[];
  /** On a station older than 1.0.4, which sends no dates for the week: the shows with a
   *  weekly time, each offered its next episode to draft. */
  toDraft: { show: Show; why: string }[];
}

/** The shows with nothing coming up, in the station's own account of each (`next_on`): the
 *  studio decides nothing here. A show leaves the list when it has a date to come or a drafted
 *  episode still to air, which is when the station gives it a next date. */
export function unscheduled(shows: readonly Show[], rules: boolean): Unscheduled {
  const out: Unscheduled = { none: [], finished: [], toDraft: [] };
  for (const show of shows) {
    const whose = show.presenter ? `${show.presenter}’s show` : '';
    if (!rules) {
      if (show.slot) out.toDraft.push({ show, why: shortOfSlot(show.slot) });
      else out.none.push({ show, why: whose });
      continue;
    }
    const next = show.next_on;
    if (!next || next.on) continue;
    const last = next.last_on ? dateWords(next.last_on) : '';
    if (next.reason === 'once_passed') out.finished.push({ show, why: last ? `was once, on ${last}` : 'was once, and has aired' });
    else if (next.reason === 'complete') {
      const n = show.rule?.end.kind === 'count' ? show.rule.end.count : null;
      out.finished.push({ show, why: n ? `its ${n} episode${n === 1 ? ' is' : 's are'} made` : 'its episodes are made' });
    } else if (next.reason === 'ended') out.finished.push({ show, why: last ? `ended ${last}` : 'its dates have ended' });
    else out.none.push({ show, why: whose });
  }
  return out;
}
