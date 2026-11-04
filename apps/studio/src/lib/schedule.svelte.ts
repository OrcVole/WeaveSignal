// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The Schedule tab's state: the week shown, what the control plane gave for it, and the
// selection. Here rather than in the component so the keymap's actions (← → T on the
// schedule tab) and the tests reach it.

import * as api from './schedule.ts';
import {
  automationBlocks,
  episodeBlocks,
  ruleBlocks,
  weekOf,
  type Block,
} from '../schedule/model.ts';
import type { Clock, Show, WeekEpisode, WeekHour } from './schedule.ts';

export type Pane = 'episode' | 'card' | 'log' | 'new-show';

export const sched = $state({
  /** The station's zone, from the control plane's publishing settings; UTC until it answers. */
  zone: 'UTC',
  /** Whether anyone has chosen the zone (ADR-0189); null until the station says. */
  zoneSet: null as boolean | null,
  /** A moment in the week shown. */
  anchor: Date.now(),
  episodes: [] as WeekEpisode[],
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
  view: 'week' as 'week' | 'studios',
  /** Bumped when the week should be fetched again (a save, a draft). */
  version: 0,
});

export function weekBlocks(now: number): { days: number[]; blocks: Block[] } {
  const { days } = weekOf(sched.anchor, sched.zone);
  const eps = episodeBlocks(sched.episodes, sched.zone, now);
  const rules = ruleBlocks(sched.shows, eps, days, sched.zone, now);
  const auto = automationBlocks([...eps, ...rules], days, sched.zone, sched.hours, sched.clocks, sched.defaultClock);
  return { days, blocks: [...auto, ...rules, ...eps] };
}

export function moveWeek(delta: number): void {
  const { days } = weekOf(sched.anchor, sched.zone);
  sched.anchor = days[0] + delta * 7 * 24 * 3_600_000 + 12 * 3_600_000;
  sched.version++;
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

let fetching: Promise<void> | null = null;

/** Fetch the week and what frames it; errors are the tab's to show, not the caller's. */
export async function loadWeek(): Promise<void> {
  if (fetching) return fetching;
  sched.loading = true;
  fetching = (async () => {
    try {
      const settings = await api.playoutSettings().catch(() => null);
      const chosen = await api.stationZone().catch(() => null);
      const { days } = weekOf(sched.anchor, sched.zone);
      const [week, shows, clocks, hours] = await Promise.all([
        api.weekEpisodes(days[0] - 24 * 3_600_000, days[7] + 24 * 3_600_000),
        api.shows(),
        api.clocks().catch(() => [] as Clock[]),
        api.weekHours().catch(() => [] as WeekHour[]),
      ]);
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
