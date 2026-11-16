// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The list of shows (`calendar.shows`), with no page: which shows have dates to come, in name
// order, and the line that counts them. The station says whether a show has a next date
// (`next_on`); nothing is worked out here. Pure, so it is tested on its own.

import type { Show } from '../lib/schedule.ts';
import type { Unscheduled } from './rule/view.ts';

/** A name as it is filed: without a leading *The*, so The Bothy Sessions sits under B. */
const filed = (name: string) => name.replace(/^the\s+/i, '');

/** The shows with a date to come, by name. On a station older than 1.0.4 every show is in one
 *  of the groups below instead, since that station says no next date. */
export function listRows(shows: readonly Show[], rules: boolean): Show[] {
  if (!rules) return [];
  return shows.filter((s) => !!s.next_on?.on).sort((a, b) => filed(a.name).localeCompare(filed(b.name), 'en-GB', { sensitivity: 'base' }));
}

/** `12 with dates to come · 1 with none yet · 2 finished`. */
export function showCounts(coming: number, u: Unscheduled): string {
  const bits: string[] = [];
  if (coming) bits.push(`${coming} with dates to come`);
  if (u.none.length) bits.push(`${u.none.length} with none yet`);
  if (u.toDraft.length) bits.push(`${u.toDraft.length} with a weekly time`);
  if (u.finished.length) bits.push(`${u.finished.length} finished`);
  return bits.join(' · ');
}
