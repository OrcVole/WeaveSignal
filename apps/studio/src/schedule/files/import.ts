// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Import a calendar… (ADR-0281), as pure rules: the preview's reply made whole, what is
// ticked and what it is called, the request that makes the shows, and every refusal as a
// sentence under the right row. The studio works out no dates (ADR-0278): each proposed show's
// rule, sentence, next dates and clashes are the control plane's, and are drawn with the same
// helpers as New show's preview.

import { FieldError, asClash, asPreview, asRuleWords, type Clash, type Rule, type RulePreview, type RuleWords } from '../../lib/api.ts';
import { asShow, type Show } from '../../lib/schedule.ts';
import { SHOW_NAME_MAX } from '../../lib/wizard.ts';
import { slugOf } from '../model.ts';

/** The station's limits, said before a file is sent: 1 MB and 200 events. */
export const MAX_FILE_BYTES = 1024 * 1024;
export const MAX_EVENTS = 200;
/** How many of each show's next dates the preview asks for. */
export const DATES_SHOWN = 4;

/** How a calendar's event was read: a rule that gives exactly its dates, its dates for the
 *  next twelve months as chosen dates, or not a show at all. */
export type Mapping = 'exact' | 'dates' | 'refused';

/** One show the file proposes. */
export interface ImportItem {
  /** Its place in the preview's `items`, from 0. */
  index: number;
  name: string;
  description: string;
  mapping: Mapping;
  /** The rule on the station's clock; null when refused. */
  rule: Rule | null;
  /** Why it is chosen dates, or why it is refused. */
  reason: string | null;
  /** Things to say that change nothing: dates left out, a zone converted. */
  notes: string[];
  /** The zone the calendar gave its times in, when it is not the station's. */
  zone: string | null;
  /** That zone changes its clocks on other dates than the station's. */
  clockDiffers: boolean;
  /** How many of the file's events went into it. */
  events: number;
  /** The path it would take if made now with this name. */
  slug: string | null;
  words: RuleWords | null;
  /** Its sentence, next dates and clashes, in the shape New show's preview draws. */
  preview: RulePreview;
  /** How many of the dates shown clash with a show the station has. */
  clashCount: number;
  /** The show the station already has that this is (the same name with case and spaces folded,
   *  the same dates and times): the station's to say, never worked out here. Its own times are
   *  not among the clashes. */
  already: { slug: string; name: string } | null;
  /** The other items of the file whose dates overlap this one's, by `index`. */
  overlaps: number[];
}

export interface ImportPreview {
  /** The station's zone: every time is on its clock. */
  zone: string;
  /** The events read. */
  events: number;
  /** About the file as a whole. */
  notes: string[];
  items: ImportItem[];
  /** How many can be made. */
  ready: number;
}

type Raw = Record<string, unknown>;
const obj = (v: unknown): Raw => (v && typeof v === 'object' && !Array.isArray(v) ? (v as Raw) : {});
const list = (v: unknown): unknown[] => (Array.isArray(v) ? v : []);
const str = (v: unknown): string | null => (typeof v === 'string' && v ? v : null);
const num = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null);
const strings = (v: unknown): string[] => list(v).filter((x): x is string => typeof x === 'string' && x !== '');

/** The preview's reply (`POST /api/v1/schedule/import/preview`), made whole. */
export function asImportPreview(raw: unknown): ImportPreview {
  const r = obj(raw);
  const items = list(r.items).map((x, index): ImportItem => {
    const i = obj(x);
    const rule = i.rule && typeof i.rule === 'object' ? (i.rule as Rule) : null;
    const mapping: Mapping = !rule || i.mapping === 'refused' ? 'refused' : i.mapping === 'dates' ? 'dates' : 'exact';
    const preview = asPreview({ words: i.words, dates: i.dates, more: i.more });
    const there = obj(i.already);
    const thereSlug = str(there.slug);
    return {
      index,
      name: str(i.name) ?? '',
      description: str(i.description) ?? '',
      mapping,
      rule: mapping === 'refused' ? null : rule,
      reason: str(i.reason),
      notes: strings(i.notes),
      zone: str(i.zone),
      clockDiffers: i.clock_differs === true,
      events: num(i.events) ?? 1,
      slug: str(i.slug),
      words: asRuleWords(i.words),
      preview,
      clashCount: num(i.clash_count) ?? preview.dates.filter((d) => d.state === 'airs' && d.clashes.length).length,
      already: mapping !== 'refused' && thereSlug ? { slug: thereSlug, name: str(there.name) ?? str(i.name) ?? thereSlug } : null,
      overlaps: list(i.overlaps).flatMap((n) => (typeof n === 'number' ? [n] : [])),
    };
  });
  return {
    zone: str(r.zone) ?? 'UTC',
    events: num(r.events) ?? items.reduce((n, i) => n + i.events, 0),
    notes: strings(r.notes),
    items,
    ready: num(r.ready) ?? items.filter((i) => i.mapping !== 'refused').length,
  };
}

/** What the person has said of one proposed show: whether to make it, and its name. */
export interface Choice {
  on: boolean;
  name: string;
}

/** Every show that can be made starts ticked, under the calendar's own name; but not one the
 *  station already has, which made again would be a second show at the same times. */
export const firstChoices = (p: ImportPreview): Choice[] => p.items.map((i) => ({ on: i.mapping !== 'refused' && !i.already, name: i.name }));

/** Tick all and Untick all. Tick all ticks what is new to the station; a show the station
 *  already has stays as the person left it, since making it twice is a choice for its own row. */
export const allChoices = (p: ImportPreview, choices: readonly Choice[], on: boolean): Choice[] =>
  p.items.map((i) => ({ ...(choices[i.index] ?? { name: i.name }), on: on ? (i.already ? !!choices[i.index]?.on : i.mapping !== 'refused') : false }));

/** The shows of the file that the station already has. */
export const alreadyThere = (p: ImportPreview): ImportItem[] => p.items.filter((i) => !!i.already);

/** Whether every show the file can make is one the station already has. */
export const allThere = (p: ImportPreview): boolean => p.ready > 0 && alreadyThere(p).length >= p.ready;

/** What is said once for the file: how many of its shows the station already has. Null when
 *  it has none of them. */
export function alreadyNote(p: ImportPreview): string | null {
  const n = alreadyThere(p).length;
  if (!n) return null;
  if (allThere(p)) return n === 1 ? 'The show in this file is already on the station, with the same dates and times. There is nothing to make.' : `All ${n} shows in this file are already on the station, with the same dates and times. There is nothing to make.`;
  return n === 1 ? 'One of these shows is already on the station, with the same dates and times. It starts unticked, so it is not made twice.' : `${n} of these shows are already on the station, with the same dates and times. They start unticked, so they are not made twice.`;
}

/** What a row says beside *Already on the station*: which show it is when the names are not
 *  written alike, and what ticking it would do. */
export function alreadyWords(i: Pick<ImportItem, 'already'>, c: Pick<Choice, 'on' | 'name'>): string {
  if (!i.already) return '';
  const as = c.name.trim() === i.already.name ? '' : ` as ${i.already.name}`;
  return c.on ? `It is there${as}, with the same dates and times. Ticked, a second show is made, and the two clash on every date.` : `It is there${as}, with the same dates and times. Left unticked, so it is not made twice.`;
}

/** How many ticked shows clash with what the station has, for the file's one warning: a clash
 *  on a date shown, or a show made a second time over itself. */
export const clashingTicked = (p: ImportPreview, choices: readonly Choice[]): number => p.items.filter((i) => choices[i.index]?.on && i.rule && (i.clashCount > 0 || !!i.already)).length;

/** Why Make the shows is held when nothing is ticked. */
export const nothingTicked = (p: ImportPreview): string => (!p.ready ? 'Nothing in this file can be made into a show.' : allThere(p) ? 'Every show in this file is already on the station.' : 'Tick at least one show.');

/** Why a name will not do, or empty. The station's rule (1 to 160 characters) and New show's:
 *  a name needs a letter or a digit, because the show's path is made from it. */
export function nameProblem(name: string): string {
  const n = name.trim();
  if (!n) return 'Give the show a name.';
  if ([...n].length > SHOW_NAME_MAX) return `At most ${SHOW_NAME_MAX} characters.`;
  if (!slugOf(n)) return 'The name needs at least one letter or digit.';
  return '';
}

/** A show to make, with the row it came from. */
export interface Chosen {
  index: number;
  name: string;
  description: string;
  rule: Rule;
}

/** The ticked shows that can be made, in the file's order. */
export function chosen(p: ImportPreview, choices: readonly Choice[]): Chosen[] {
  return p.items.flatMap((i) => {
    const c = choices[i.index];
    return c?.on && i.rule ? [{ index: i.index, name: c.name.trim(), description: i.description, rule: i.rule }] : [];
  });
}

/** The rows whose names stop Make the shows, by `index`: an empty name, one too long, one with
 *  no letter or digit. */
export function nameProblems(p: ImportPreview, choices: readonly Choice[]): Map<number, string> {
  const out = new Map<number, string>();
  for (const c of chosen(p, choices)) {
    const problem = nameProblem(c.name);
    if (problem) out.set(c.index, problem);
  }
  return out;
}

/** The rows whose name another ticked show before them has, by `index`, with what to know: the
 *  station makes both (a calendar may hold a weekday *Breakfast* and a weekend one) and tells
 *  their paths apart with a number. Said, not refused. */
export function sameNames(p: ImportPreview, choices: readonly Choice[]): Map<number, string> {
  const out = new Map<number, string>();
  const seen = new Set<string>();
  for (const c of chosen(p, choices)) {
    const key = c.name.toLowerCase();
    if (!key) continue;
    if (seen.has(key)) out.set(c.index, `Another ticked show is named ${c.name} too. Both are made; their paths are told apart by a number. Rename one to tell them apart in the lists.`);
    seen.add(key);
  }
  return out;
}

/** What `POST /api/v1/schedule/import` takes. */
export const importRequest = (picked: readonly Chosen[]): { items: { name: string; description: string; rule: Rule }[] } => ({
  items: picked.map(({ name, description, rule }) => ({ name, description, rule })),
});

/** The mark on a row, and what it means in a sentence (the pointer's and a screen reader's). */
export function mappingWords(i: Pick<ImportItem, 'mapping'>): { label: string; title: string } {
  if (i.mapping === 'exact') return { label: 'Exact', title: 'The rule gives exactly the calendar’s dates and times.' };
  if (i.mapping === 'dates') return { label: 'Chosen dates', title: 'The calendar’s rule is not one a show can have, so these are its dates for the next twelve months.' };
  return { label: 'Not a show', title: 'This cannot be made into a show.' };
}

const and = (names: string[]): string => (names.length < 2 ? names.join('') : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`);

/** `Overlaps Folk Night and The Late Ceilidh in this file.`: the other ticked shows of the same
 *  file whose dates overlap this one's, under the names they have now. */
export function overlapWords(i: ImportItem, p: ImportPreview, choices: readonly Choice[]): string | null {
  const names = i.overlaps.flatMap((n) => (choices[n]?.on && p.items[n]?.rule ? [choices[n].name.trim() || p.items[n].name] : []));
  if (!names.length) return null;
  const shown = names.length > 3 ? [...names.slice(0, 2), `${names.length - 2} others`] : names;
  return `Overlaps ${and(shown)} in this file.`;
}

/** `3 events`, for a show made from a run of single events or an event with changed dates. */
export const eventsWords = (n: number): string => (n === 1 ? '1 event' : `${n} events`);

/** The line under the file's name: `12 events read · 5 shows proposed, 4 can be made`. */
export function fileWords(p: ImportPreview): string {
  const shows = p.items.length === 1 ? '1 show proposed' : `${p.items.length} shows proposed`;
  const can = p.ready === p.items.length ? '' : p.ready === 0 ? ', none can be made' : `, ${p.ready} can be made`;
  return `${eventsWords(p.events)} read · ${shows}${can}`;
}

/** The primary button: `Make 4 shows`, `Make the show`. */
export const makeLabel = (n: number): string => (n === 1 ? 'Make the show' : `Make ${n} shows`);

/** The file's text. A calendar file is UTF-8 (RFC 5545); one in another encoding is refused
 *  here in the station's own sentence, because read loosely its letters would come out as
 *  `\uFFFD` and the shows would be made under spoilt names without a word. */
export function decodeCalendar(bytes: ArrayBuffer | Uint8Array): string {
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    throw new Error('The file is not text in UTF-8, so it is not a calendar file we can read.');
  }
}

/** A file too large is said before it is sent; the station still judges the rest. */
export function fileProblem(file: { name: string; size: number }): string {
  if (file.size > MAX_FILE_BYTES) return `${file.name} is larger than 1 MB. Export a shorter range of dates, or one calendar at a time.`;
  if (file.size === 0) return `${file.name} is empty.`;
  return '';
}

/** A path's last part, as a file's name: `/home/iona/schedule (2).ics` is `schedule (2).ics`. */
export const pathName = (path: string): string => path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() ?? path;

/** Which of the files dropped on the window to read: the first that is named as a calendar
 *  file. The packaged app is given paths, a browser files; both are judged by the name here
 *  and by what they hold where they are read. */
export function droppedFile<T>(dropped: readonly T[], nameOf: (f: T) => string): { file: T } | { problem: string } {
  if (!dropped.length) return { problem: 'Nothing that was dropped is a file.' };
  const file = dropped.find((f) => /\.(ics|ical)$/i.test(nameOf(f)));
  if (file === undefined) return { problem: dropped.length === 1 ? `${pathName(nameOf(dropped[0]))} is not a calendar file: its name does not end in .ics.` : 'None of those is a calendar file: a calendar file’s name ends in .ics.' };
  return { file };
}

/** A refusal of the file as a sentence. The station's own sentence is shown as given; a bare
 *  status (a proxy's 413, an older station's 404) gets ours. */
export function refusalWords(e: unknown): string {
  if (e instanceof FieldError) {
    const own = !/^\d{3}\b/.test(e.message) ? e.message : '';
    if (e.status === 413) return own || 'That file is too large. A calendar file can be up to 1 MB and hold up to 200 events: export a shorter range of dates, or one calendar at a time.';
    if (e.status === 422) return own || 'That is not a calendar file the station can read. Export it again as iCalendar (.ics).';
    if (e.status === 403) return own || 'Only an admin or a producer imports a calendar.';
    if (e.status === 404 || e.status === 405) return 'This station cannot read calendar files yet: it is older than 1.0.5.';
    if (e.status === 503) return own || 'The station cannot read calendar files without its database.';
    return own || 'The station could not read the file. Try again.';
  }
  const m = e instanceof Error ? e.message : typeof e === 'string' ? e : '';
  return /did not answer|Failed to fetch|NetworkError|Load failed/i.test(m) ? 'The station did not answer. Nothing was read; try again.' : m || 'Something went wrong. Try again.';
}

/** A refusal of Make the shows, placed: the row it names (the station counts the shows as they
 *  were sent, so `items[1]` is the second ticked row, not the second row) and which of its
 *  parts, or the whole form. */
export function placeRefusal(e: unknown, sent: readonly Chosen[]): { index: number | null; part: 'name' | 'rule' | null; message: string } {
  const message = refusalWords(e);
  if (!(e instanceof FieldError) || !e.field) return { index: null, part: null, message };
  const m = /^items\[(\d+)\]\.(name|rule|description)$/.exec(e.field);
  const row = m ? sent[Number(m[1])] : undefined;
  if (!m || !row) return { index: null, part: null, message };
  return { index: row.index, part: m[2] === 'name' ? 'name' : 'rule', message };
}

/** A made show that overlaps another show's time. */
export interface ImportWarning {
  /** The made show's path and name. */
  show: string;
  name: string;
  /** Its overlaps in the twelve months from its first date (the first 60), one for each. */
  clashes: Clash[];
  /** On how many dates they fall. */
  dates: number;
}

export interface ImportMade {
  made: Show[];
  warnings: ImportWarning[];
}

/** The import's reply (`POST /api/v1/schedule/import`, 201), made whole. */
export function asImportMade(raw: unknown): ImportMade {
  const r = obj(raw);
  return {
    made: list(r.made).map((s) => asShow(s as Show)),
    warnings: list(r.warnings).map((w) => {
      const o = obj(w);
      const days = new Set(list(o.clashes).map((c) => str(obj(c).on) ?? String(num(obj(c).start_ms) ?? num(obj(c).start) ?? '')));
      return { show: str(o.show) ?? '', name: str(o.name) ?? '', clashes: list(o.clashes).map((c) => asClash(c)), dates: days.size };
    }),
  };
}

/** `4 shows are made.` */
export const madeWords = (n: number): string => (n === 1 ? '1 show is made.' : `${n} shows are made.`);

/** A warning as one line: `Folk Night overlaps The Late Kettle on 52 dates in its first twelve
 *  months.` The station lists a show's first 60 overlaps, so 60 is said as *60 or more*. */
export function warningWords(w: ImportWarning): string {
  const others = [...new Set(w.clashes.map((c) => c.show))];
  const shown = others.length > 3 ? [...others.slice(0, 2), `${others.length - 2} other shows`] : others;
  const n = w.dates;
  const count = w.clashes.length >= 60 ? `${n} dates or more` : n === 1 ? '1 date' : `${n} dates`;
  return `${w.name} overlaps ${and(shown)} on ${count} in its first twelve months.`;
}

/** What follows a show's dates shown: nothing for a show that airs once, that there are more,
 *  or that those are all. */
export function afterWords(i: Pick<ImportItem, 'rule' | 'preview'>): string {
  if (!i.rule || i.rule.repeat.kind === 'once') return '';
  const shown = i.preview.dates.filter((d) => d.state !== 'skipped').length;
  if (i.preview.more) return i.rule.end.kind === 'never' && i.rule.repeat.kind !== 'dates' ? 'and so on, with no end' : 'and more after these';
  return shown === 1 ? 'That is the only date.' : `Those are all ${shown}.`;
}

/** Whether every date shown clashes with one and the same show: New show's preview then says the
 *  clash once, in a sentence that names the show, and the rows need not each repeat it. */
export function sharedClash(preview: Pick<RulePreview, 'dates'>): boolean {
  const airs = preview.dates.filter((d) => d.state === 'airs');
  const first = airs[0]?.clashes[0];
  return airs.length > 1 && !!first && airs.every((d) => d.clashes[0]?.slug === first.slug && d.clashes[0]?.show === first.show);
}
