// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The Schedule tab's state: the week shown, what the control plane gave for it, and the
// selection. Here rather than in the component so the keymap's actions (← → T on the
// schedule tab) and the tests reach it.

import * as api from './schedule.ts';
import { noteStationVersion, speaksRules, stationVersion, type ScheduleDate } from './api.ts';
import {
  automationBlocks,
  dateBlocks,
  episodeBlocks,
  markConflicts,
  weekOf,
  type Block,
} from '../schedule/model.ts';
import { dayIndex, shownBlock } from '../schedule/clash.ts';
import type { Clock, Show, WeekEpisode, WeekHour } from './schedule.ts';
import { onStationChange } from './reset.ts';

/** What the editor column holds. `show` is a show's own pane, with its Dates and its Details;
 *  it stays while a row of its dates selects a block in the week. */
export type Pane = 'episode' | 'card' | 'log' | 'show';
export type ScheduleView = 'week' | 'day' | 'studios';
export type ShowTab = 'dates' | 'details';

/** What `sched` is when the page first loads: a change of station puts it back (ADR-0285). */
const fresh = () => ({
  /** The station's zone, from the control plane's publishing settings; UTC until it answers. */
  zone: 'UTC',
  /** Whether anyone has chosen the zone (ADR-0189); null until the station says. */
  zoneSet: null as boolean | null,
  /** A moment in the week shown. */
  anchor: Date.now(),
  episodes: [] as WeekEpisode[],
  /** Every rule's dates in the window that no episode fills yet: the dotted blocks, worked out
   *  by the control plane (ADR-0278). Null from a station too old to send them. */
  dates: null as ScheduleDate[] | null,
  /** The station's public address, from which the calendar links given out are made
   *  (Subscribe…, ADR-0281): null when the station has none, undefined until the week says and
   *  from a station older than 1.0.5, which has no calendar files. */
  publicUrl: undefined as string | null | undefined,
  /** Whether the station stores rules (1.0.4 or later, from `/health`); null until it says.
   *  An older one keeps a weekly time only, and is never sent `rule` or `draft_first`. */
  rules: null as boolean | null,
  shows: [] as Show[],
  clocks: [] as Clock[],
  hours: [] as WeekHour[],
  defaultClock: null as string | null,
  /** The station is a US statutory webcaster (up next is never announced), or unknown. */
  us: null as boolean | null,
  loading: false,
  error: '' as string,
  /** The selected block's id. */
  selected: null as string | null,
  pane: 'episode' as Pane,
  /** The show the `show` pane holds: one with nothing coming up draws no block to open it from. */
  editShow: null as string | null,
  /** Which of the show pane's two tabs is open; it opens at Dates (week-clashes.md section 3). */
  showTab: 'dates' as ShowTab,
  /** The week, or one day of it with a lane for each clashing show. In the Day view `anchor`
   *  is a moment in the day shown. The week is the default every time the tab opens. */
  view: 'week' as ScheduleView,
  /** A block the grid is asked to bring into view (a row of a show's dates was pressed). */
  reveal: null as { id: string; n: number } | null,
  /** `Esc` or a press on the empty grid was not obeyed, because an editor holds changes that are
   *  not saved: the tab says so in its own line. `n` makes the same refusal twice a new one. */
  notClosed: null as { what: string; n: number } | null,
  /** Bumped when the week should be fetched again (a save, a draft). */
  version: 0,
});

export const sched = $state(fresh());

export function weekBlocks(now: number): { days: number[]; blocks: Block[] } {
  const { days } = weekOf(sched.anchor, sched.zone);
  const eps = episodeBlocks(sched.episodes, sched.zone, now);
  const rules = dateBlocks(sched.dates, sched.shows);
  // A dotted date that shares time with another show clashes too, and says so.
  markConflicts([...eps, ...rules]);
  const auto = automationBlocks([...eps, ...rules], days, sched.zone, sched.hours, sched.clocks, sched.defaultClock);
  return { days, blocks: [...auto, ...rules, ...eps] };
}

const NOON = 12 * 3_600_000;

/** `←` `→` on the tab: a week. In the Day view the day shown stays the same weekday. */
export function moveWeek(delta: number): void {
  const { days } = weekOf(sched.anchor, sched.zone);
  if (sched.view === 'day') {
    const i = Math.max(0, days.findIndex((d, k) => k < 7 && sched.anchor >= d && sched.anchor < days[k + 1]));
    const there = weekOf(days[0] + delta * 7 * 24 * 3_600_000 + NOON, sched.zone).days;
    sched.anchor = there[i] + NOON;
  } else sched.anchor = days[0] + delta * 7 * 24 * 3_600_000 + NOON;
  sched.version++;
}

/** The Day view's day, a step either way; the week is read again only when the day leaves it. */
export function moveDay(delta: number): void {
  openDay(dayStart(sched.anchor) + delta * 24 * 3_600_000 + NOON);
}

/** The toolbar's arrows: a week in the Week view, a day in the Day view. */
export function step(delta: number): void {
  if (sched.view === 'day') moveDay(delta);
  else moveWeek(delta);
}

function dayStart(ms: number): number {
  const { days } = weekOf(ms, sched.zone);
  return days[Math.max(0, days.findIndex((d, k) => k < 7 && ms >= d && ms < days[k + 1]))];
}

/** Show the moment's day or week, reading the week again only when it is another one. */
function goTo(ms: number): void {
  const before = weekOf(sched.anchor, sched.zone).days[0];
  sched.anchor = sched.view === 'day' ? dayStart(ms) + NOON : ms;
  if (weekOf(sched.anchor, sched.zone).days[0] !== before) sched.version++;
}

/** Open one day in the Day view: the day that holds `ms`. */
export function openDay(ms: number): void {
  sched.view = 'day';
  goTo(ms);
}

/** Week, or Day on the day the caller chose (week-clashes.md: the selected block's day; else
 *  today, when today is in the week shown; else the week's Monday). The selection is kept. */
export function setView(view: 'week' | 'day', dayMs: number | null = null): void {
  if (view === 'day') openDay(dayMs ?? sched.anchor);
  else sched.view = 'week';
}

/** `D` and `W` on the tab (ADR-0283), and the toolbar's **Day** and **Week**: Day opens on the
 *  selected block's day wherever in the week it is, the open day or not. */
export function keyView(view: 'week' | 'day', now: number): void {
  if (view === 'week') return setView('week');
  const { days, blocks } = weekBlocks(now);
  const selected = blocks.find((b) => b.id === sched.selected) ?? null;
  setView('day', dayToOpen(selected?.start ?? null, now, days));
}

/** The day Day opens on when nobody named one. */
export function dayToOpen(selectedStart: number | null, now: number, days: readonly number[]): number {
  if (selectedStart !== null && selectedStart >= days[0] && selectedStart < days[7]) return selectedStart;
  if (now >= days[0] && now < days[7]) return now;
  return days[0] + NOON;
}

export function today(now: number): void {
  sched.anchor = now;
  sched.version++;
}

export function select(id: string | null): void {
  sched.selected = id;
  if (id && sched.pane !== 'card') sched.pane = id.startsWith('auto:') ? 'log' : 'episode';
  if (id && sched.pane === 'card' && id.startsWith('auto:')) sched.pane = 'log';
}

/** A show's own pane, at its Dates unless Details is asked for. */
export function openShow(slug: string, tab: ShowTab = 'dates'): void {
  sched.editShow = slug;
  sched.showTab = tab;
  sched.pane = 'show';
}

/** ‹ Shows: close what is open and clear the selection, so the list of shows is back. */
export function backToShows(): void {
  sched.pane = 'episode';
  sched.editShow = null;
  sched.selected = null;
}

// ---- Esc, and a press on the grid where no block is ----
/** What the editors hold that is typed and not saved, by editor. Not state: nothing draws it,
 *  and `closeOpen` reads it when asked. */
const unsaved = new Map<object, string>();
/** An editor says what it holds that is not saved (*The episode*), or null once that is saved,
 *  discarded, or the editor has closed. */
export function noteUnsaved(who: object, what: string | null): void {
  if (what) unsaved.set(who, what);
  else unsaved.delete(who);
}

/** What `closeOpen` did: nothing (the list of shows was there already), nothing because an editor
 *  holds changes that are not saved, or closed, with what was open for the focus to go back to. */
export type Closed = { did: 'nothing' } | { did: 'unsaved'; what: string } | { did: 'closed'; block: string | null; show: string | null };

let refusals = 0;
/** `Esc` on the tab, and a press on the grid where no block is: as **‹ Shows**, it closes what
 *  the editor column holds and clears the selection, so the list of shows is back. `Esc` is
 *  never destructive (keymap.md), and after a first `Esc` has left a text field a second one
 *  follows by habit: so while an editor holds changes that are not saved nothing closes, and
 *  the tab says why. A selection the grid does not draw (another week, or another day in the
 *  Day view) holds nothing open, and is left as it is. */
export function closeOpen(now: number): Closed {
  const { days, blocks } = weekBlocks(now);
  const block = shownBlock(blocks, days, sched.selected, sched.view === 'day' ? Math.max(0, dayIndex(sched.anchor, days)) : null);
  const show = sched.pane === 'show' && sched.shows.some((s) => s.slug === sched.editShow) ? sched.editShow : null;
  if (!block && !show) return { did: 'nothing' };
  const [what] = unsaved.values();
  if (what) {
    sched.notClosed = { what, n: ++refusals };
    return { did: 'unsaved', what };
  }
  backToShows();
  return { did: 'closed', block: block?.id ?? null, show };
}

let reveals = 0;
/** Show a block in the week without leaving the pane that is open: the week (or the Day view)
 *  moves to its date, the block is selected there, and the grid scrolls to it. */
export function showInWeek(id: string | null, ms: number): void {
  goTo(ms);
  sched.selected = id;
  if (id) sched.reveal = { id, n: ++reveals };
}

let fetching: Promise<void> | null = null;

/** The station's version is asked once a window, and again only while it could not be read. */
let version: string | null = null;
async function knownVersion(): Promise<string | null> {
  version ??= await stationVersion();
  return version;
}

/** What a dialog outside the Schedule tab needs of the station (the DJ's setup opens over the
 *  desk): its zone, whether anyone chose it, and whether it stores rules. A station that
 *  cannot answer leaves what was there. */
export async function loadStationFacts(): Promise<void> {
  const [chosen, v] = await Promise.all([api.stationZone().catch(() => null), knownVersion()]);
  if (chosen) {
    sched.zone = chosen.zone || sched.zone;
    sched.zoneSet = chosen.set;
  }
  if (v !== null) sched.rules = speaksRules(v);
  noteStationVersion(v);
}

/** The station's public address, for a link made outside the Schedule tab (a host's monitor
 *  link, from Settings or the desk). It comes with the week (1.0.5), so a window that has read
 *  no week yet asks for an empty one. Null when the station has none, or could not be asked. */
export async function loadPublicAddress(): Promise<string | null> {
  if (sched.publicUrl === undefined) {
    try {
      const now = Date.now();
      const asked = (await api.weekEpisodes(now, now + 1000)).publicUrl;
      if (sched.publicUrl === undefined) sched.publicUrl = asked;
    } catch {
      // Not known: the caller says how the link is formed instead of giving one.
    }
  }
  return sched.publicUrl ?? null;
}

/** Fetch the week and what frames it; errors are the tab's to show, not the caller's. */
export async function loadWeek(): Promise<void> {
  if (fetching) return fetching;
  sched.loading = true;
  fetching = (async () => {
    try {
      const settings = await api.playoutSettings().catch(() => null);
      const chosen = await api.stationZone().catch(() => null);
      const { days } = weekOf(sched.anchor, sched.zone);
      const [week, shows, clocks, hours, v] = await Promise.all([
        api.weekEpisodes(days[0] - 24 * 3_600_000, days[7] + 24 * 3_600_000),
        api.shows(),
        api.clocks().catch(() => [] as Clock[]),
        api.weekHours().catch(() => [] as WeekHour[]),
        knownVersion(),
      ]);
      // The version decides (new-show.md section 8). While it cannot be read, a week that
      // carries `dates` can only have come from a station that stores rules.
      sched.rules = v !== null ? speaksRules(v) : week.dates !== null;
      noteStationVersion(v);
      sched.dates = week.dates;
      sched.publicUrl = week.publicUrl;
      sched.zone = week.zone || settings?.time_zone || sched.zone;
      sched.zoneSet = chosen?.set ?? sched.zoneSet;
      sched.episodes = week.episodes;
      sched.shows = shows;
      sched.clocks = clocks;
      sched.hours = hours;
      sched.defaultClock = settings?.default_clock ?? null;
      sched.error = '';
    } catch (e) {
      sched.error = e instanceof Error ? e.message : String(e);
    } finally {
      sched.loading = false;
      fetching = null;
    }
  })();
  return fetching;
}

// A change of station (ADR-0285): the week, the shows and the zone go, and so does the
// station's version, which decides which fields an older station is sent.
onStationChange('schedule', () => {
  Object.assign(sched, fresh());
  unsaved.clear();
  reveals = 0;
  fetching = null;
  version = null;
});

/** The station's version as this window last read it (a test's; null until read). */
export function versionRead(): string | null {
  return version;
}
