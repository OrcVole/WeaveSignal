// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Start now (New show, part 2; 1.3.2, the operator's answer of 2026-11-24; the DJ's setup, part
// 3, from 1.3.3): a first show on air
// within a minute, with one press, for the person who has just named it. The rule is the
// station's (`GET /api/v1/schedule/rule/now`): once, from the station's next whole minute, on
// its own clock and zone, so a studio whose clock is wrong still starts the show when it says.
// It is asked for when the button is pressed, never when the dialog opens, so however long
// the person reads, the show starts within a minute of the press.
//
// Nothing here judges a clash (new-show.md section 8): the station is asked for a preview of
// the rule, and what it says of another show at that time is worded here and asked about once.
// A station older than 1.3.2 has no such route; there the same rule is made here from the
// station's clock as this window follows it. Pure, so it is tested on its own.

import type { IsoDate, Rule, RulePreview } from '../../lib/api.ts';
import { hm, parts } from '../model.ts';
import { isoOf, lengthWords, times, type RuleForm } from './form.ts';
import { CLASH_COST } from './view.ts';

/** What the station answers: the rule to save, and when the show starts. */
export interface RuleNow {
  rule: Rule;
  starts_at_ms: number;
  /** The station's date and the clock of the start (`20:41`). */
  date: IsoDate;
  time: string;
  minutes: number;
  zone: string;
}

/** The lengths the route takes; anything else asks for an hour. */
export const NOW_MIN = 15;
export const NOW_MAX = 480;

/** How long a show that starts now runs: the length the two times on the form make, when it is
 *  one the station takes for this (15 minutes to 8 hours), else an hour. */
export function nowMinutes(form: Pick<RuleForm, 'from' | 'until'>): number {
  const t = times(form);
  return t && t.minutes >= NOW_MIN && t.minutes <= NOW_MAX ? t.minutes : 60;
}

/** The rule of a show that starts now, made here for a station that has no route for it (older
 *  than 1.3.2): once, from the next whole minute on the station's clock, in its zone. A start
 *  that rolls into the next day (23:59:30) is that day's, at 00:00. */
export function ruleNowHere(nowMs: number, zone: string, minutes: number): RuleNow {
  const at = (Math.floor(nowMs / 60_000) + 1) * 60_000;
  const p = parts(at, zone);
  const date = isoOf(p.y, p.m, p.d);
  return {
    rule: { starts_on: date, start: p.hh * 60 + p.mm, minutes, repeat: { kind: 'once' }, end: { kind: 'never' }, skip: [], moves: [] },
    starts_at_ms: at,
    date,
    time: hm(at, zone),
    minutes,
    zone,
  };
}

/** The words of the offer at the head of part 2. */
export const START_NOW = {
  button: 'Start now',
  anyway: 'Start now anyway',
  busy: 'Starting…',
  lead: 'On air straight away?',
  /** What one press does, with the length the form gives. */
  what: (minutes: number) => `Makes the show and starts it at the next whole minute, station time, for ${lengthWords(minutes)}, once. Give it more dates later, from the show’s pane.`,
  /** The same in the DJ's setup, where the press saves the DJ's own show. */
  whatMine: (minutes: number) => `Saves your show and starts it at the next whole minute, station time, for ${lengthWords(minutes)}, once. Give it more dates later, from Settings, under Your show.`,
  notNow: 'Not now',
} as const;

/** What the station's preview says of starting now. */
export type NowVerdict =
  | { kind: 'go' }
  /** Another show has the time and this session may not draft over it, or the rule has a problem. */
  | { kind: 'held'; why: string }
  /** Another show has the time: said once, then the next press goes on. */
  | { kind: 'clash'; title: string; body: string };

/** Read the preview of a start-now rule. The station decides what is refused; this words it. */
export function nowVerdict(preview: RulePreview, zone: string): NowVerdict {
  if (preview.problems.length) return { kind: 'held', why: preview.problems[0].message };
  const first = preview.dates.find((d) => d.state === 'airs');
  if (!first) return { kind: 'held', why: 'The station gives no date for a show that starts now. Give it a date and times below.' };
  const c = first.clashes[0];
  if (!c) return { kind: 'go' };
  const when = `${hm(c.start * 1000, zone)} to ${hm((c.start + c.minutes * 60) * 1000, zone)}`;
  const refused = first.clashes.find((x) => x.refused);
  if (refused) return { kind: 'held', why: `${refused.show} has the air now (${hm(refused.start * 1000, zone)} to ${hm((refused.start + refused.minutes * 60) * 1000, zone)}), and this account may not schedule over it. Give your show a later time below, or ask the station.` };
  return {
    kind: 'clash',
    title: `${c.show} is on the schedule now (${when}).`,
    body: CLASH_COST,
  };
}

// ---- the press ----

/** The three things a press of Start now asks of the station. */
export interface NowCalls<T> {
  /** The station's rule for a show that starts now. */
  ask: () => Promise<RuleNow>;
  /** The station's preview of a rule: its date, its problems, another show at that time. */
  preview: (rule: Rule) => Promise<RulePreview>;
  /** Save the show with the rule; throws the station's refusal. */
  save: (rule: Rule) => Promise<T>;
}

/** How a press ended. A refusal that is not about the minute is thrown, for the dialog to place. */
export type NowOutcome<T> = { kind: 'made'; now: RuleNow; saved: T } | { kind: 'held'; why: string } | { kind: 'clash'; title: string; body: string };

/** The station's rule, or the one made here where the station has no such route (`absent`). */
export async function ruleNowOr(route: () => Promise<RuleNow>, absent: (e: unknown) => boolean, here: () => RuleNow): Promise<RuleNow> {
  try {
    return await route();
  } catch (e) {
    if (!absent(e)) throw e;
    return here();
  }
}

/**
 * One press of Start now: ask for the rule, ask what the station says of it, save.
 *
 * - Another show at that time is returned as `clash` and nothing is saved; the next press comes
 *   with `anyway`, which saves without asking for the preview again.
 * - What the station holds (a clash it refuses for this account, a rule with a problem) is `held`.
 * - The minute may turn between the question and the answer, and the station takes no start
 *   that has passed. A preview that says so (a problem on `starts_on`), or a save refused for
 *   its rule (`minuteTurned`), asks for the rule once more and tries once more; a second such
 *   answer is the station's to say.
 */
export async function startNowFlow<T>(calls: NowCalls<T>, o: { anyway: boolean; zone: string; minuteTurned: (e: unknown) => boolean }): Promise<NowOutcome<T>> {
  for (let again = false; ; again = true) {
    const now = await calls.ask();
    if (!o.anyway) {
      const p = await calls.preview(now.rule);
      if (!again && p.problems.some((x) => x.field === 'starts_on')) continue;
      const said = nowVerdict(p, o.zone);
      if (said.kind !== 'go') return said;
    }
    try {
      return { kind: 'made', now, saved: await calls.save(now.rule) };
    } catch (e) {
      if (again || !o.minuteTurned(e)) throw e;
    }
  }
}

/** Done's line for a show started now: when it starts, in the station's own time. */
export function startedWords(now: Pick<RuleNow, 'time' | 'starts_at_ms' | 'minutes'>, zone: string): { lead: string; rest: string } {
  return {
    lead: `It starts at ${now.time}, station time:`,
    rest: `within a minute, until ${hm(now.starts_at_ms + now.minutes * 60_000, zone)}. When its time comes the step at the top of the studio becomes this show’s. ON AIR is the lamp, not the schedule: it lights when listeners can hear someone.`,
  };
}
