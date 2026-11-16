// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Stepping a schedule time (`TimeField.svelte`): WebKitGTK draws a time field with no arrows
// of its own, so the studio draws two, and each press moves the time to the quarter-hour
// before or after it. Pure, with no page, so it is tested on its own.

/** A press moves the time to the next quarter-hour: radio schedules sit on them. */
export const STEP_MINUTES = 15;

export interface Stepped {
  /** `HH:MM`, as a time field holds it. */
  time: string;
  /** The step went past midnight: the day after (1) or the day before (-1); else 0. */
  carry: -1 | 0 | 1;
}

const pad = (n: number) => String(n).padStart(2, '0');

/** The time a press gives. From a time on a step it is one step on or back (18:15 to 18:30);
 *  from one between two it is the nearer step that way (18:07 to 18:15 later, to 18:00
 *  earlier), so an odd minute is two presses from the quarter beyond it and never skips one.
 *  Past midnight the time wraps and `carry` says which way, for a field that has a date beside
 *  it. A field that is empty or unreadable starts at 00:00, whichever arrow is pressed. */
export function stepTime(value: string, dir: 1 | -1, step = STEP_MINUTES): Stepped {
  const m = /^(\d{1,2}):(\d{2})/.exec(value.trim());
  const at = m ? Number(m[1]) * 60 + Number(m[2]) : null;
  if (at === null || at >= 1440 || Number(m![2]) > 59) return { time: '00:00', carry: 0 };
  const to = dir > 0 ? (Math.floor(at / step) + 1) * step : (Math.ceil(at / step) - 1) * step;
  const carry = to >= 1440 ? 1 : to < 0 ? -1 : 0;
  const n = ((to % 1440) + 1440) % 1440;
  return { time: `${pad(Math.floor(n / 60))}:${pad(n % 60)}`, carry };
}

/** What a step's button says, to a pointer's tooltip and to a screen reader. */
export const stepWords = (dir: 1 | -1, step = STEP_MINUTES): string => `${step} minutes ${dir > 0 ? 'later' : 'earlier'}`;
