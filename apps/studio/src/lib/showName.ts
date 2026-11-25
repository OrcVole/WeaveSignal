// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

import type { View } from './api';

/** The show the desk names: the station's own `studio.show` setting when it has one, else the
 *  show the schedule has on air now (the one the frame's step button names). Empty when neither:
 *  no show is on air, which the desk says in words. A show that was on air and is not now is not
 *  named here (the frame's tooltip says it was the last on air). */
export function showName(v: View | null | undefined): string {
  const set = v?.show?.title?.trim();
  if (set) return set;
  return v?.step?.on_air && v.step.show ? v.step.show.name : '';
}

/** What a heading says when no show is named. */
export const NO_SHOW_WORDS = 'No show on air';

/** A row of the rundown that is the schedule's own, for a station whose `studio.rundown` setting
 *  has none (the schedule fills the desk's rundown only by the next show's post, and only with a
 *  host to bring on): the show on air now, and the next to start. The next carries its weekday
 *  when it is more than half a day away, so a show on Monday does not read as today's. */
export interface ScheduleRow {
  /** `now`, or the time. */
  at: string;
  title: string;
  detail: string;
  now: boolean;
}

export function scheduleRows(v: View | null | undefined, hm: (ms: number) => string): ScheduleRow[] {
  const step = v?.step;
  if (!v || !step) return [];
  const rows: ScheduleRow[] = [];
  if (step.on_air && step.show) rows.push({ at: 'now', title: step.show.name, detail: 'on air by the schedule', now: true });
  const next = step.next;
  if (next) {
    const far = next.start_ms - v.now_ms > 12 * 3600_000;
    const day = far ? ` · ${new Date(next.start_ms).toLocaleDateString('en-GB', { weekday: 'long' })}` : '';
    rows.push({ at: hm(next.start_ms), title: next.name, detail: `next by the schedule${day}`, now: false });
  }
  return rows;
}
