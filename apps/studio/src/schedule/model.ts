// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The schedule calendar's model, with no page: the station's wall clock in its own zone (so a
// clocks-back Sunday has 25 hours and the axis stays in wall-clock time, calendar.md §1), the
// week's blocks from the control plane's episodes, automation filling every gap by the clock
// of each local hour (ADR-0045), conflicts, a moved instance, and the editor's field markers
// (inherited, set here, as aired; ADR-0055). Pure, so it is tested on its own.

import type { CardFields, Clock, Resolved, Slot, WeekEpisode, WeekHour } from '../lib/schedule.ts';

export const HOUR_MS = 3_600_000;
export const DAY_MS = 24 * HOUR_MS;

/** A moment as the station's wall clock reads it. */
export interface Parts {
  y: number;
  m: number;
  d: number;
  hh: number;
  mm: number;
  /** Monday is 0. */
  wd: number;
}

const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
const formats = new Map<string, Intl.DateTimeFormat>();

function formatter(zone: string): Intl.DateTimeFormat {
  let f = formats.get(zone);
  if (!f) {
    f = new Intl.DateTimeFormat('en-GB', {
      timeZone: zone,
      hourCycle: 'h23',
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      weekday: 'short',
    });
    formats.set(zone, f);
  }
  return f;
}

/** The wall clock in `zone` at `ms`. */
export function parts(ms: number, zone: string): Parts {
  const p: Record<string, string> = {};
  for (const x of formatter(zone).formatToParts(new Date(ms))) p[x.type] = x.value;
  return {
    y: Number(p.year),
    m: Number(p.month),
    d: Number(p.day),
    hh: Number(p.hour) % 24,
    mm: Number(p.minute),
    wd: (WEEKDAYS.indexOf(p.weekday) + 7) % 7,
  };
}

/** The moment at which `zone`'s wall clock reads `p` (the first one, on a clocks-back night). */
export function instant(p: Pick<Parts, 'y' | 'm' | 'd' | 'hh' | 'mm'>, zone: string): number {
  const asUtc = (q: Pick<Parts, 'y' | 'm' | 'd' | 'hh' | 'mm'>) => Date.UTC(q.y, q.m - 1, q.d, q.hh, q.mm);
  const want = asUtc(p);
  let guess = want;
  for (let i = 0; i < 3; i++) {
    const got = asUtc(parts(guess, zone));
    if (got === want) break;
    guess += want - got;
  }
  // On a clocks-back night two moments read the same; take the earlier.
  const earlier = guess - HOUR_MS;
  return asUtc(parts(earlier, zone)) === want ? earlier : guess;
}

/** A floating Date for a calendar library that draws local time: the station's wall clock at
 *  `ms`, in this machine's zone, so the grid shows station time whatever the machine's zone. */
export function floating(ms: number, zone: string): Date {
  const p = parts(ms, zone);
  return new Date(p.y, p.m - 1, p.d, p.hh, p.mm);
}

/** The moment a floating Date (station wall clock, in this machine's zone) names. */
export function fromFloating(d: Date, zone: string): number {
  return instant({ y: d.getFullYear(), m: d.getMonth() + 1, d: d.getDate(), hh: d.getHours(), mm: d.getMinutes() }, zone);
}

/** The week around `ms`: Monday 00:00 station time and each day's start, eight entries so the
 *  last is the next Monday; a clocks-back day is 25 hours between its start and the next. */
export function weekOf(ms: number, zone: string): { start: number; days: number[] } {
  const p = parts(ms, zone);
  const monday = new Date(Date.UTC(p.y, p.m - 1, p.d - p.wd));
  const days = Array.from({ length: 8 }, (_, i) => {
    const d = new Date(monday.getTime() + i * DAY_MS);
    return instant({ y: d.getUTCFullYear(), m: d.getUTCMonth() + 1, d: d.getUTCDate(), hh: 0, mm: 0 }, zone);
  });
  return { start: days[0], days };
}

/** `25 h · clocks go back`, `23 h · clocks go forward`, or `24 h`. */
export function dayLength(days: number[], i: number): string {
  const h = Math.round((days[i + 1] - days[i]) / HOUR_MS);
  return h === 24 ? '24 h' : h > 24 ? `${h} h · clocks go back` : `${h} h · clocks go forward`;
}

const pad = (n: number) => String(n).padStart(2, '0');

export function hm(ms: number, zone: string): string {
  const p = parts(ms, zone);
  return `${pad(p.hh)}:${pad(p.mm)}`;
}

/** `Sat 24 Oct`. */
export function dayWords(ms: number, zone: string): string {
  const p = parts(ms, zone);
  return `${WEEKDAYS[p.wd]} ${p.d} ${MONTHS[p.m - 1]}`;
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/** `Sat 24 Oct 2026 · 20:00–23:00`. */
export function whenWords(start: number, end: number, zone: string): string {
  const p = parts(start, zone);
  return `${WEEKDAYS[p.wd]} ${p.d} ${MONTHS[p.m - 1]} ${p.y} · ${hm(start, zone)}–${hm(end, zone)}`;
}

/** `19 – 25 October 2026` for the toolbar. */
export function rangeWords(days: number[], zone: string): string {
  const a = parts(days[0], zone);
  const b = parts(days[6], zone);
  const month = (p: Parts) => new Intl.DateTimeFormat('en-GB', { month: 'long' }).format(new Date(Date.UTC(p.y, p.m - 1, 1)));
  if (a.m === b.m) return `${a.d} – ${b.d} ${month(a)} ${a.y}`;
  if (a.y === b.y) return `${a.d} ${month(a)} – ${b.d} ${month(b)} ${a.y}`;
  return `${a.d} ${month(a)} ${a.y} – ${b.d} ${month(b)} ${b.y}`;
}

/** `7 h 30`, `1 h`, `45 min`. */
export function lengthWords(ms: number): string {
  const m = Math.round(ms / 60_000);
  const h = Math.floor(m / 60);
  const r = m % 60;
  if (!h) return `${r} min`;
  return r ? `${h} h ${pad(r)}` : `${h} h`;
}

/** The weekday's name for a slot (1 Monday to 7 Sunday). */
export function slotWords(s: Slot | null): string {
  if (!s) return 'no weekly rule';
  const start = `${pad(Math.floor(s.start / 60))}:${pad(s.start % 60)}`;
  const end = s.minutes === null ? '' : `–${pad(Math.floor(((s.start + s.minutes) % 1440) / 60))}:${pad((s.start + s.minutes) % 60)}`;
  return `Weekly on ${['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'][s.weekday - 1]}, ${start}${end}`;
}

/** The slot's next instance strictly after `after`, in station time. */
export function nextInstance(s: Slot, after: number, zone: string): number {
  const p = parts(after, zone);
  for (let i = 0; i < 15; i++) {
    const d = new Date(Date.UTC(p.y, p.m - 1, p.d + i));
    const wd = (d.getUTCDay() + 6) % 7;
    if (wd !== s.weekday - 1) continue;
    const at = instant(
      { y: d.getUTCFullYear(), m: d.getUTCMonth() + 1, d: d.getUTCDate(), hh: Math.floor(s.start / 60), mm: s.start % 60 },
      zone,
    );
    if (at > after) return at;
  }
  return after;
}

/** What the grid draws: an episode, a weekly rule's instance nobody has drafted yet, or
 *  automation filling a gap by the clock of each local hour. */
export interface Block {
  id: string;
  kind: 'episode' | 'rule' | 'automation';
  /** Epoch ms. */
  start: number;
  end: number;
  show: string;
  slug: string;
  /** The show's bank slot, 1 to 12 (ADR-0066); null for automation. */
  colour: number | null;
  series: number | null;
  number: number | null;
  title: string;
  hosts: string[];
  /** An instance of a weekly rule (the show or series has a slot). */
  weekly: boolean;
  aired: boolean;
  frozen: boolean;
  conflict: boolean;
  /** `was 07:00` on an instance moved off its rule's time this week. */
  moved: string | null;
  /** Automation: the clock's name. */
  clock: string | null;
  /** The studio the block is in; the model has none yet. */
  studio: string | null;
}

/** The hour when a slot's instance would have started, for the MOVED chip. */
function movedWords(slot: Slot | null, start: number, zone: string): string | null {
  if (!slot) return null;
  const p = parts(start, zone);
  const minutes = p.hh * 60 + p.mm;
  if (p.wd === slot.weekday - 1 && minutes === slot.start) return null;
  const was = `${pad(Math.floor(slot.start / 60))}:${pad(slot.start % 60)}`;
  return p.wd === slot.weekday - 1 ? `was ${was}` : `was ${WEEKDAYS[slot.weekday - 1]} ${was}`;
}

/** The week's episodes as blocks; an episode with no air time or length draws nothing. */
export function episodeBlocks(eps: WeekEpisode[], zone: string, now: number): Block[] {
  const out: Block[] = [];
  for (const w of eps) {
    const r = w.resolved;
    if (w.episode.air_at === null) continue;
    const start = w.episode.air_at * 1000;
    const minutes = r.episode?.minutes ?? w.episode.minutes ?? r.slot?.minutes ?? 60;
    const end = start + minutes * 60_000;
    out.push({
      id: `ep:${w.episode.id}`,
      kind: 'episode',
      start,
      end,
      show: r.show,
      slug: r.slug,
      colour: r.colour_slot,
      series: w.series.number,
      number: w.episode.number,
      title: r.episode?.title ?? w.episode.title,
      hosts: r.hosts,
      weekly: !!r.slot,
      aired: w.episode.aired_at !== null || end <= now,
      frozen: w.episode.aired_at !== null,
      conflict: false,
      moved: movedWords(r.slot, start, zone),
      clock: null,
      studio: null,
    });
  }
  return markConflicts(out);
}

/** Two episodes overlapping in time both carry CONFLICT (the model has no studios yet, so
 *  every overlap is one); automation never conflicts, it yields. */
export function markConflicts(blocks: Block[]): Block[] {
  const eps = blocks.filter((b) => b.kind === 'episode');
  for (const a of eps) {
    for (const b of eps) {
      if (a !== b && a.start < b.end && b.start < a.end && (a.studio === null || a.studio === b.studio)) a.conflict = true;
    }
  }
  return blocks;
}

/** A show's weekly rule drawn where no episode is drafted this week: a dotted instance that
 *  offers to draft it. `shows` gives each show's slot and colour. */
export function ruleBlocks(
  shows: { slug: string; name: string; colour_slot: number; slot: Slot | null }[],
  blocks: Block[],
  days: number[],
  zone: string,
  now: number,
): Block[] {
  const out: Block[] = [];
  for (const s of shows) {
    if (!s.slot) continue;
    let at = nextInstance(s.slot, days[0] - 1, zone);
    while (at < days[7]) {
      const end = at + (s.slot.minutes ?? 60) * 60_000;
      const taken = blocks.some((b) => b.kind === 'episode' && b.slug === s.slug && b.start < end && at < b.end);
      if (!taken && end > now) {
        out.push({
          id: `rule:${s.slug}:${at}`,
          kind: 'rule',
          start: at,
          end,
          show: s.name,
          slug: s.slug,
          colour: s.colour_slot,
          series: null,
          number: null,
          title: '',
          hosts: [],
          weekly: true,
          aired: false,
          frozen: false,
          conflict: false,
          moved: null,
          clock: null,
          studio: null,
        });
      }
      at = nextInstance(s.slot, at, zone);
    }
  }
  return out;
}

/** The clock of a local hour, by name; `default` for the hours the grid leaves to the default
 *  clock, and null for hours nothing fills (the fallback's). */
export function clockAt(ms: number, zone: string, hours: WeekHour[], clocks: Clock[], fallback: string | null): string | null {
  const p = parts(ms, zone);
  const id = hours.find((h) => h.weekday === p.wd && h.hour === p.hh)?.clock ?? fallback;
  if (!id) return null;
  return clocks.find((c) => c.id === id)?.name ?? 'Automation';
}

/** Automation fills every gap between the week's blocks, one block per run of local hours
 *  with the same clock, so the week is never silent and nobody schedules it by hand. */
export function automationBlocks(
  blocks: Block[],
  days: number[],
  zone: string,
  hours: WeekHour[],
  clocks: Clock[],
  defaultClock: string | null,
): Block[] {
  const out: Block[] = [];
  const busy = blocks.filter((b) => b.kind !== 'automation').sort((a, b) => a.start - b.start);
  for (let day = 0; day < 7; day++) {
    const dayStart = days[day];
    const dayEnd = days[day + 1];
    let cursor = dayStart;
    const gaps: [number, number][] = [];
    for (const b of busy) {
      if (b.end <= cursor || b.start >= dayEnd) continue;
      if (b.start > cursor) gaps.push([cursor, Math.min(b.start, dayEnd)]);
      cursor = Math.max(cursor, b.end);
    }
    if (cursor < dayEnd) gaps.push([cursor, dayEnd]);
    for (const [from, to] of gaps) {
      // Split the gap where the clock of the local hour changes.
      let runStart = from;
      let runClock = clockAt(from, zone, hours, clocks, defaultClock);
      let t = from - (from % HOUR_MS) + HOUR_MS;
      for (; t < to; t += HOUR_MS) {
        const c = clockAt(t, zone, hours, clocks, defaultClock);
        if (c !== runClock) {
          out.push(auto(day, runStart, t, runClock));
          runStart = t;
          runClock = c;
        }
      }
      out.push(auto(day, runStart, to, runClock));
    }
  }
  return out;
}

function auto(day: number, start: number, end: number, clock: string | null): Block {
  return {
    id: `auto:${day}:${start}`,
    kind: 'automation',
    start,
    end,
    show: clock ?? 'Automation',
    slug: '',
    colour: null,
    series: null,
    number: null,
    title: '',
    hosts: [],
    weekly: false,
    aired: false,
    frozen: false,
    conflict: false,
    moved: null,
    clock,
    studio: null,
  };
}

/** Which marker an editor field carries (calendar.md §4). */
export type Marker = 'inherited' | 'set' | 'frozen' | 'own';

export function marker(r: Pick<Resolved, 'inherited' | 'frozen'>, field: string, inheritable = true): Marker {
  if (r.frozen) return 'frozen';
  if (!inheritable) return 'own';
  return r.inherited.includes(field) ? 'inherited' : 'set';
}

/** `Ep 212`, `S3 Ep 7 · Ghost Train night`. */
export function episodeLine(b: Block): string {
  if (b.number === null) return '';
  const n = b.series !== null && b.series > 1 ? `S${b.series} Ep ${b.number}` : `Ep ${b.number}`;
  return b.title ? `${n} · ${b.title}` : n;
}

/** Initials for a person chip: `SC`, `CM` (the first and the last word). */
export function initials(name: string): string {
  const w = name.trim().split(/\s+/).filter(Boolean);
  if (!w.length) return '?';
  const first = w[0].replace(/^(Dr|Mr|Mrs|Ms|Prof)\.?$/i, '') || w[0];
  const last = w.length > 1 ? w[w.length - 1] : '';
  return ((first[0] ?? '') + (last[0] ?? '')).toUpperCase();
}

/** A show's path segment from its name: `saturday-night-kettle`. */
export function slugOf(name: string): string {
  return name
    .toLowerCase()
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 60);
}

/** An RFC 9557 zoned time for the control plane, `2026-10-24T20:00:00+01:00[Europe/London]`. */
export function zonedString(ms: number, zone: string): string {
  const p = parts(ms, zone);
  const wall = Date.UTC(p.y, p.m - 1, p.d, p.hh, p.mm);
  const offsetMin = Math.round((wall - ms) / 60_000);
  const sign = offsetMin < 0 ? '-' : '+';
  const a = Math.abs(offsetMin);
  return `${p.y}-${pad(p.m)}-${pad(p.d)}T${pad(p.hh)}:${pad(p.mm)}:00${sign}${pad(Math.floor(a / 60))}:${pad(a % 60)}[${zone}]`;
}

/** A `datetime-local` value for the station's wall clock at `ms`. */
export function localInputValue(ms: number, zone: string): string {
  const p = parts(ms, zone);
  return `${p.y}-${pad(p.m)}-${pad(p.d)}T${pad(p.hh)}:${pad(p.mm)}`;
}

/** The moment a `datetime-local` value names in the station's zone. */
export function fromLocalInput(v: string, zone: string): number | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/.exec(v);
  if (!m) return null;
  return instant({ y: Number(m[1]), m: Number(m[2]), d: Number(m[3]), hh: Number(m[4]), mm: Number(m[5]) }, zone);
}

/** Names or tags typed as one line: commas, semicolons or new lines between them. */
export function splitList(s: string): string[] {
  return s
    .split(/[,;\n]/)
    .map((x) => x.trim())
    .filter(Boolean);
}

/** What a corrections-log line changed, one phrase per field: `title: The ferrys → The ferries`.
 *  A list shows its names; nothing prints a field the line did not change. */
export function changeWords(c: { old_values: Record<string, unknown>; new_values: Record<string, unknown> }): string[] {
  const show = (v: unknown): string =>
    Array.isArray(v) ? (v.length ? v.join(', ') : 'none') : v === '' || v === null || v === undefined ? 'empty' : String(v);
  return Object.keys(c.new_values).map((k) => `${k}: ${show(c.old_values[k])} → ${show(c.new_values[k])}`);
}

const WEEKDAY_NAMES = ['monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday', 'sunday'];

/** The card's fields (the cards crate's `CardData`, less the station) from a resolved show,
 *  series or episode, cut to the level asked for. */
export function cardFields(r: Resolved, level: 'show' | 'series' | 'episode', zone: string): CardFields {
  const slot = r.slot
    ? {
        weekday: WEEKDAY_NAMES[r.slot.weekday - 1],
        start: `${pad(Math.floor(r.slot.start / 60))}:${pad(r.slot.start % 60)}`,
        ...(r.slot.minutes !== null
          ? { end: `${pad(Math.floor(((r.slot.start + r.slot.minutes) % 1440) / 60))}:${pad((r.slot.start + r.slot.minutes) % 60)}` }
          : {}),
      }
    : null;
  const f: CardFields = {
    layout: r.layout === 'slate' ? 'slate' : 'poster',
    colour_slot: r.colour_slot,
    show: { name: r.show, slug: r.slug, description: r.description, slot },
    hosts: r.hosts,
    guests: level === 'episode' ? r.guests : [],
  };
  if (level !== 'show' && r.series) {
    f.series = { number: r.series.number, title: r.series.title, first: r.series.first, last: r.series.last, episodes: r.series.episodes };
  }
  if (level === 'episode' && r.episode) {
    f.episode = {
      number: r.episode.number,
      title: r.episode.title,
      air: r.episode.air_at === null ? null : zonedString(r.episode.air_at * 1000, zone),
    };
  }
  return f;
}
