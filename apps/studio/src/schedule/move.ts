// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// An episode's Move… and Back to the rule in its editor (the operator's 1.0.5 click-through):
// each is saved as it is pressed, with Undo, as a show's dates are (ADR-0283). Before, Move…
// only put the new time in the editor's draft, and the move was kept only by Save episode at
// the foot of the column, which nobody found. What is sent, the way back, and what the tab's
// line says are worked out here, with no page, so they are tested on their own.
//
// Only the time goes to the station, and the length only when it was changed: the episode's
// other fields (a title half typed, its guests) stay in the editor's draft for Save episode,
// and a length the episode inherits stays inherited.

import type { Episode, EpisodePatch } from '../lib/schedule.ts';
import { fromLocalInput, hm, longDayWords, zonedString } from './model.ts';

/** What Move… holds when Move is pressed. */
export interface MoveFields {
  /** `YYYY-MM-DD`, station time. */
  date: string;
  /** `HH:MM`, station time. */
  time: string;
  /** The length in minutes, as the number field gives it: not a number while it is empty. */
  minutes: unknown;
}

export type MoveAsk =
  /** The fields cannot be read as a move: what to say, and which of them it is about. */
  | { problem: string; about: 'when' | 'length' }
  /** The fields are the time and the length the episode has already: nothing to send. */
  | { same: true }
  /** The change, and nothing else of the episode. */
  | { patch: EpisodePatch };

/** What Move sends. `start` (epoch ms, null with no air time yet) and `length` (minutes) are
 *  the episode's as the editor shows them. */
export function moveAsk(f: MoveFields, start: number | null, length: number, zone: string): MoveAsk {
  const at = f.date && f.time ? fromLocalInput(`${f.date}T${f.time}`, zone) : null;
  if (at === null) return { problem: 'Give a date and a time, such as 20:00.', about: 'when' };
  const minutes = typeof f.minutes === 'number' ? f.minutes : NaN;
  if (!Number.isInteger(minutes) || minutes < 1 || minutes > 1440) return { problem: 'Give a length in whole minutes, from 1 to 1,440.', about: 'length' };
  const resized = minutes !== length;
  if (at === start && !resized) return { same: true };
  // The time goes with a changed length even when it is the same: the station judges a
  // presenter's clash by the time it is sent, and a longer episode can reach another show.
  return { patch: { air_at: zonedString(at, zone), ...(resized ? { minutes } : {}) } };
}

/** Back to the rule: the rule's own time on the date the episode fills, and its own length. */
export const ruleAsk = (ruleTime: number, zone: string): EpisodePatch => ({ air_at: zonedString(ruleTime, zone), minutes: null });

/** The way back from a change: the same fields as the episode held them before it. A length
 *  the episode inherited goes back to inheriting (`null`), and so does a time it never had. */
export function wayBack(ep: Pick<Episode, 'air_at' | 'minutes'>, patch: EpisodePatch, zone: string): EpisodePatch {
  return {
    ...('air_at' in patch ? { air_at: ep.air_at === null ? null : zonedString(ep.air_at * 1000, zone) } : {}),
    ...('minutes' in patch ? { minutes: ep.minutes } : {}),
  };
}

/** The tab's line after a move: *Episode 5 is on Wednesday 18 November, 20:30 to 22:30.*
 *  After Back to the rule: *Episode 5 is back on Wednesday 18 November, as the rule has it.*
 *  Where it now shares time with other shows the line says so, as a dragged block's does. */
export function movedSaid(number: number, start: number, end: number, zone: string, o: { back?: boolean; clashes?: string } = {}): string {
  const day = longDayWords(start, zone);
  const said = o.back ? `Episode ${number} is back on ${day}, as the rule has it.` : `Episode ${number} is on ${day}, ${hm(start, zone)} to ${hm(end, zone)}.`;
  return o.clashes ? `${said} It now clashes with ${o.clashes}.` : said;
}
