// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A show's dates as a list (`calendar.show-dates`; week-clashes.md section 3): the station's
// preview of the stored rule joined, by the rule date, to the series' episodes. Nothing here
// expands a rule, words a sentence or judges a refusal (section 7): every date, state and clash
// is the station's, and this only joins and words them. The changes a row makes (skip, put
// back, move, back to the rule) are small edits of the rule, each read again from the station
// before it is saved, so two windows cannot lose each other's change.

import type { Clash, IsoDate, PreviewDate, Rule, RulePreview } from '../lib/api.ts';
import type { EpisodeView, Series } from '../lib/schedule.ts';
import { clashesOf } from './clash.ts';
import { hm, instant, lengthWords, parts, type Block } from './model.ts';
import { dateWords, ymd } from './rule/form.ts';
import { airDate, dateIn } from './rule/view.ts';

// ---- the rule, edited one date at a time -----------------------------------------------------

/** The rule with a date taken out. A skipped date is not also a moved one. */
export const skipIn = (rule: Rule, on: IsoDate): Rule => ({ ...rule, skip: [...rule.skip.filter((d) => d !== on), on].sort(), moves: rule.moves.filter((m) => m.from !== on) });

/** The rule with a skipped date put back. */
export const putBackIn = (rule: Rule, on: IsoDate): Rule => ({ ...rule, skip: rule.skip.filter((d) => d !== on) });

/** The rule with one date aired at another date and time (`2026-12-05`, `20:00`). */
export const moveIn = (rule: Rule, on: IsoDate, to: IsoDate, at: string): Rule => ({
  ...rule,
  skip: rule.skip.filter((d) => d !== on),
  moves: [...rule.moves.filter((m) => m.from !== on), { from: on, to: `${to}T${at}:00`, minutes: null }].sort((a, b) => a.from.localeCompare(b.from)),
});

/** The rule with a moved date back on the rule's own date and time. */
export const unmoveIn = (rule: Rule, on: IsoDate): Rule => ({ ...rule, moves: rule.moves.filter((m) => m.from !== on) });

/** What reads a show's rule and saves it: the station's routes, given so this is tested alone. */
export interface RuleStore {
  /** The series' own rule when it has one, else the show's; null when there is none. */
  read: (slug: string, series: number | null) => Promise<{ rule: Rule; series: number | null } | null>;
  /** Save it where it was read from, leaving drafted episodes where they are. */
  write: (slug: string, series: number | null, rule: Rule) => Promise<void>;
}

/** Read the rule, change it, save it. False when there was no rule to change. */
export async function changeRule(store: RuleStore, slug: string, series: number | null, change: (rule: Rule) => Rule): Promise<boolean> {
  const got = await store.read(slug, series);
  if (!got) return false;
  await store.write(slug, got.series, change(got.rule));
  return true;
}

// ---- the rows ------------------------------------------------------------------------------

/** One line of a show's dates. */
export interface DateRow {
  /** The row's identity: the rule date, or the episode's id when no rule date made it. */
  key: string;
  /** The rule's own date, which Skip, Put back and Move… act on; null on an episode made by hand. */
  on: IsoDate | null;
  /** `open`: the rule gives it and nobody has drafted it. `drafted`: an episode fills it.
   *  `hand`: an episode no rule date made. `skipped`: taken out, and still listed. */
  kind: 'open' | 'drafted' | 'hand' | 'skipped';
  /** `Ep 7`, or empty on a skipped date. */
  ep: string;
  number: number | null;
  series: number | null;
  /** `Wed 25 Nov`, with the year on the first row of a new year. */
  day: string;
  /** `Wednesday 25 November`, for the toast and the row's accessible name. */
  dayLong: string;
  /** `21:00–23:00`, and the same said aloud: `21:00 to 23:00`. */
  times: string;
  timesLong: string;
  /** Epoch ms. */
  start: number;
  end: number;
  /** The date it airs on, and its start, for Move…. */
  air: IsoDate;
  at: string;
  title: string;
  /** `was Fri 4 Dec`. */
  moved: string | null;
  /** `St Andrew’s Weekend, 22:00 to 23:30 · overlaps 1 h`. */
  clash: string | null;
  /** The station refuses this session a draft over that show: `The Tea Break has that time.` */
  refused: string | null;
  /** `CLOCKS FORWARD · 21:00 does not happen`. */
  clock: string | null;
  onAir: boolean;
  /** The block that stands for it in the week. */
  blockId: string | null;
  /** The next date to draft: drafting stays in date order, so Draft is on this row alone. */
  first: boolean;
}

const pad = (n: number) => String(n).padStart(2, '0');
const hhmm = (n: number) => `${pad(Math.floor(n / 60))}:${pad(n % 60)}`;

const clashLine = (show: string, start: number, end: number, minutes: number, zone: string) => `${show}, ${hm(start, zone)} to ${hm(end, zone)} · overlaps ${lengthWords(minutes * 60_000)}`;
const ofClash = (c: Clash, zone: string) => clashLine(c.show, c.start * 1000, (c.start + c.minutes * 60) * 1000, c.overlap_minutes, zone);

export interface RowsContext {
  slug: string;
  zone: string;
  /** The station's date today, which settles the rows that carry their year. */
  today: IsoDate;
  now: number;
  /** The rule's own start, minutes after midnight, for the clocks' line and a moved episode. */
  nominal: number | null;
  /** The week on screen: a drafted episode's clash is known only while its block is in it. */
  blocks: readonly Block[];
}

/** The rows still to come: the preview's dates in its order, each filled date joined to its
 *  episode, and after them in time the episodes still to air that no rule date made. */
export function dateRows(preview: RulePreview | null, episodes: readonly EpisodeView[], o: RowsContext): DateRow[] {
  const thisYear = ymd(o.today)?.y ?? 0;
  const byOn = new Map<string, EpisodeView>();
  const byNumber = new Map<number, EpisodeView>();
  for (const e of episodes) {
    if (e.episode.occurrence) byOn.set(e.episode.occurrence, e);
    byNumber.set(e.episode.number, e);
  }
  const used = new Set<string>();
  const nominal = o.nominal === null ? '' : hhmm(o.nominal);
  const rows: Omit<DateRow, 'day' | 'first'>[] = [];

  const ofEpisode = (e: EpisodeView, on: IsoDate | null, d: PreviewDate | null): Omit<DateRow, 'day' | 'first'> | null => {
    if (e.episode.air_at === null) return null;
    const start = e.episode.air_at * 1000;
    const minutes = e.resolved.episode?.minutes ?? e.episode.minutes ?? e.resolved.rule?.minutes ?? e.resolved.slot?.minutes ?? d?.minutes ?? 60;
    const end = start + minutes * 60_000;
    const air = dateIn(start, o.zone);
    const block = o.blocks.find((b) => b.id === `ep:${e.episode.id}`) ?? null;
    const other = block ? clashesOf(block, o.blocks)[0] : undefined;
    // Off its rule's date, or on it at another time: said as the week's MOVED chip says it.
    const p = parts(start, o.zone);
    const usual = o.nominal;
    const moved =
      on && on !== air ? `was ${dateWords(on, { weekday: 'short', month: 'short' })}`
      : on && usual !== null && p.hh * 60 + p.mm !== usual && instant({ ...ymd(on)!, hh: Math.floor(usual / 60), mm: usual % 60 }, o.zone) !== start ? `was ${hhmm(usual)}`
      : null;
    return {
      key: on ?? `ep:${e.episode.id}`,
      on,
      kind: on ? 'drafted' : 'hand',
      ep: `Ep ${e.episode.number}`,
      number: e.episode.number,
      series: e.resolved.series?.number ?? null,
      dayLong: dateWords(air, { weekday: 'long' }),
      times: `${hm(start, o.zone)}–${hm(end, o.zone)}`,
      timesLong: `${hm(start, o.zone)} to ${hm(end, o.zone)}`,
      start,
      end,
      air,
      at: hm(start, o.zone),
      title: e.resolved.episode?.title ?? e.episode.title,
      moved,
      clash: other ? clashLine(other.block.show, other.block.start, other.block.end, other.minutes, o.zone) : null,
      refused: null,
      clock: null,
      onAir: start <= o.now && end > o.now,
      blockId: `ep:${e.episode.id}`,
    };
  };

  for (const d of preview?.dates ?? []) {
    const filled = d.state === 'filled' ? (byOn.get(d.on) ?? (d.episode ? byNumber.get(d.episode.number) : undefined)) : undefined;
    if (filled) {
      // An aired episode is under Aired, not here.
      used.add(filled.episode.id);
      if (filled.episode.aired_at !== null) continue;
      const row = ofEpisode(filled, d.on, d);
      if (row) rows.push(row);
      continue;
    }
    if (d.state === 'filled' && d.episode?.aired) continue;
    const start = d.start * 1000;
    const end = start + d.minutes * 60_000;
    const air = airDate(d, o.zone);
    const skipped = d.state === 'skipped';
    const refused = d.clashes.find((c) => c.refused) ?? null;
    rows.push({
      key: d.on,
      on: d.on,
      kind: skipped ? 'skipped' : d.state === 'filled' ? 'drafted' : 'open',
      ep: d.number !== null && !skipped ? `Ep ${d.number}` : '',
      number: skipped ? null : d.number,
      series: d.episode?.series ?? null,
      dayLong: dateWords(air, { weekday: 'long' }),
      times: `${hm(start, o.zone)}–${hm(end, o.zone)}`,
      timesLong: `${hm(start, o.zone)} to ${hm(end, o.zone)}`,
      start,
      end,
      air,
      at: hm(start, o.zone),
      title: '',
      moved: d.moved_from && !skipped ? `was ${dateWords(d.moved_from.slice(0, 10), { weekday: 'short', month: 'short' })}` : null,
      clash: d.state === 'airs' && d.clashes.length ? ofClash(d.clashes[0], o.zone) : null,
      refused: refused ? `${refused.show} has that time.` : null,
      clock: skipped ? null : d.clock === 'forward' ? `CLOCKS FORWARD · ${nominal} does not happen` : d.clock === 'back' ? `CLOCKS BACK · the first ${nominal} to the second` : null,
      onAir: d.now && !skipped,
      blockId: skipped || d.state !== 'airs' ? null : `rule:${o.slug}:${d.on}`,
    });
  }

  // Episodes still to air that no rule date in the list made: made by hand, or the show has no rule.
  for (const e of episodes) {
    if (used.has(e.episode.id) || e.episode.aired_at !== null || e.episode.air_at === null) continue;
    const start = e.episode.air_at * 1000;
    const minutes = e.resolved.episode?.minutes ?? e.episode.minutes ?? 60;
    if (start + minutes * 60_000 <= o.now) continue;
    const row = ofEpisode(e, null, null);
    if (!row) continue;
    const i = rows.findIndex((r) => r.start > row.start);
    rows.splice(i < 0 ? rows.length : i, 0, row);
  }

  let seenYear = thisYear;
  let firstOpen = true;
  return rows.map((r) => {
    const y = ymd(r.air)?.y ?? thisYear;
    const year = y !== thisYear && y !== seenYear;
    seenYear = y;
    const first = r.kind === 'open' && firstOpen;
    if (r.kind === 'open') firstOpen = false;
    return { ...r, day: dateWords(r.air, { weekday: 'short', month: 'short', year }), first };
  });
}

/** One aired episode, for the closed group under the rows. */
export interface AiredRow {
  id: string;
  ep: string;
  series: number;
  day: string;
  times: string;
  start: number;
  title: string;
}

/** What has aired, newest first. */
export function airedRows(episodes: readonly EpisodeView[], series: Pick<Series, 'number'>, zone: string): AiredRow[] {
  return episodes
    .filter((e) => e.episode.aired_at !== null && e.episode.air_at !== null)
    .sort((a, b) => (b.episode.air_at ?? 0) - (a.episode.air_at ?? 0))
    .map((e) => {
      const start = e.episode.air_at! * 1000;
      const minutes = e.resolved.episode?.minutes ?? e.episode.minutes ?? e.resolved.rule?.minutes ?? e.resolved.slot?.minutes ?? 60;
      return {
        id: e.episode.id,
        ep: `Ep ${e.episode.number}`,
        series: series.number,
        day: dateWords(dateIn(start, zone), { weekday: 'short', month: 'short', year: true }),
        times: `${hm(start, zone)}–${hm(start + minutes * 60_000, zone)}`,
        start,
        title: e.resolved.episode?.title ?? e.episode.title,
      };
    });
}

/** Whether the rule gives more dates than the station sent. The station says so (`more`); a
 *  reply that only counts them (`total`) is read as well; one that says neither is taken to go
 *  on when the rule has no end. */
export function hasMore(preview: RulePreview | null, rule: Rule | null): boolean {
  if (!preview) return false;
  if (preview.more !== null) return preview.more;
  const airing = preview.dates.filter((d) => d.state !== 'skipped').length;
  if (preview.total !== null) return airing < preview.total;
  return !!rule && rule.end.kind === 'never' && rule.repeat.kind !== 'once' && rule.repeat.kind !== 'dates';
}

const endless = (rule: Rule | null) => !!rule && rule.end.kind === 'never' && rule.repeat.kind !== 'once' && rule.repeat.kind !== 'dates';

/** `29 to come: 2 drafted, 27 not drafted. 1 skipped. 1 clashes with another show.` The count
 *  to come is said only when the station has sent every date the rule still gives (it sends
 *  sixty at most, and says whether there are more): nothing is counted that was not sent. The
 *  skipped are the rule's own from today on. A clash is known only for the dates sent, so
 *  beyond them it is counted *of the next 60*. `draftedInAll` is how many episodes are drafted
 *  on the rule's dates to come, from the series. */
export function countWords(rows: readonly DateRow[], preview: RulePreview | null, rule: Rule | null, today: IsoDate, draftedInAll: number | null = null): { lead: string; rest: string } {
  const drafted = draftedInAll ?? rows.filter((r) => r.kind === 'drafted').length;
  const hand = rows.filter((r) => r.kind === 'hand').length;
  const sent = rows.filter((r) => r.kind !== 'skipped' && r.kind !== 'hand').length;
  const more = hasMore(preview, rule);
  const skipped = rule ? rule.skip.filter((d) => d >= today).length : rows.filter((r) => r.kind === 'skipped').length;
  const clashes = rows.filter((r) => r.clash && r.kind !== 'skipped').length;
  const bits: string[] = [];
  let lead: string;
  if (!more) {
    lead = `${sent} to come:`;
    bits.push(`${drafted} drafted, ${Math.max(0, sent - drafted)} not drafted.`);
  } else {
    lead = `${drafted} drafted`;
    bits.push(endless(rule) ? `of the next ${sent}; the dates go on with no end.` : `of the next ${sent}; more follow.`);
  }
  if (hand) bits.push(`${hand} made by hand.`);
  if (skipped) bits.push(`${skipped} skipped.`);
  if (clashes) bits.push(more ? `${clashes} of the next ${sent} ${clashes === 1 ? 'clashes' : 'clash'} with another show.` : `${clashes} ${clashes === 1 ? 'clashes' : 'clash'} with another show.`);
  return { lead, rest: bits.join(' ') };
}

/** The first `shown` dates that air, with the skipped dates among them: the list shows ten,
 *  then ten more at a time. */
export function firstRows(rows: readonly DateRow[], shown: number): DateRow[] {
  let n = 0;
  const out: DateRow[] = [];
  for (const r of rows) {
    if (r.kind !== 'skipped') {
      if (n >= shown) break;
      n++;
    }
    out.push(r);
  }
  return out;
}

/** What follows the rows shown: `and 19 more, to Friday 26 February 2027`, or that the dates go
 *  on. `lastLine` is the station's own second line (`The last is Friday 26 February 2027.`). */
export function moreWords(rows: readonly DateRow[], shown: number, preview: RulePreview | null, rule: Rule | null, lastLine: string | null = null): { more: boolean; words: string } {
  if (!preview || !rule) return { more: false, words: '' };
  const airing = rows.filter((r) => r.kind !== 'skipped').length;
  const hidden = Math.max(0, airing - shown);
  const beyond = hasMore(preview, rule);
  const last = /^The last is (.+?)\.?$/.exec(lastLine ?? '')?.[1] ?? '';
  if (endless(rule)) return { more: hidden > 0, words: 'and more after, with no end' };
  if (hidden > 0) return { more: true, words: `and ${hidden}${beyond ? ' or' : ''} more${last ? `, to ${last}` : ''}` };
  if (beyond) return { more: false, words: `and more after${last ? `, to ${last}` : ''}` };
  return { more: false, words: airing > 1 ? `Those are all ${airing}.` : '' };
}

/** A row said aloud: `Episode 7, Wednesday 25 November, 21:00 to 23:00, not drafted.` */
export function rowAria(r: DateRow): string {
  const what = r.kind === 'skipped' ? 'skipped' : r.kind === 'open' ? 'not drafted' : r.kind === 'hand' ? 'drafted, made by hand' : 'drafted';
  const bits = [r.number !== null ? `Episode ${r.number}` : '', r.dayLong, r.timesLong, what, r.moved ? `moved, ${r.moved}` : '', r.clash ? `conflict: ${r.clash}` : '', r.onAir ? 'on air' : ''].filter(Boolean);
  return `${bits.join(', ')}.${r.blockId ? ' Space shows it in the week.' : ''}`;
}

/** How many of a series' episodes are drafted on a rule's date and still to air. */
export function draftedToCome(episodes: readonly EpisodeView[], now: number): number {
  return episodes.filter((e) => {
    if (e.episode.aired_at !== null || e.episode.air_at === null || !e.episode.occurrence) return false;
    const minutes = e.resolved.episode?.minutes ?? e.episode.minutes ?? e.resolved.rule?.minutes ?? e.resolved.slot?.minutes ?? 60;
    return (e.episode.air_at + minutes * 60) * 1000 > now;
  }).length;
}
