// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The sandbox's scenarios (ADR-0222), in the order the chooser lists them: by the four steps a
// show can run at (ADR-0271), Solo first, each rung with its scenarios. The show itself, as the
// demo video runs it, opens the Full rung.

import type { ShowStep } from '../../lib/api.ts';
import { STEPS } from '../../lib/steps.ts';
import { ALONE } from './alone.ts';
import { AUTO } from './auto.ts';
import { BANNED } from './banned.ts';
import { CARTS } from './carts.ts';
import { DUMP } from './dump.ts';
import { GUEST } from './guest.ts';
import { HANDOVER } from './handover.ts';
import { LINK } from './link.ts';
import { MIC } from './mic.ts';
import { NEXT_DJ } from './nextdj.ts';
import { QUEUE } from './queue.ts';
import type { Scenario } from './scenario.ts';
import { SHOW } from './show.ts';

export const SCENARIOS: readonly Scenario[] = [MIC, CARTS, AUTO, GUEST, ALONE, DUMP, LINK, SHOW, QUEUE, BANNED, NEXT_DJ, HANDOVER];

/** The chooser's rungs: one line under each step's word. */
export const RUNG_HEAD: Record<ShowStep, string> = {
  solo: 'Just you',
  guests: 'You and a guest',
  callers: 'Listeners call in',
  full: 'A screener, the picture, the next DJ',
};

export interface Rung {
  step: ShowStep;
  head: string;
  /** Each scenario with its number in the whole list, so the numbers run on down the ladder. */
  scenarios: { n: number; scenario: Scenario }[];
}

/** The scenarios as four rungs, Solo at the top. */
export function rungs(all: readonly Scenario[] = SCENARIOS): Rung[] {
  let n = 0;
  return STEPS.map((step) => ({
    step,
    head: RUNG_HEAD[step],
    scenarios: all.filter((s) => s.step === step).map((scenario) => ({ n: ++n, scenario })),
  }));
}

/** The scenario a hash names (`#alone`), or the show. */
export function scenarioFor(id: string): Scenario {
  return SCENARIOS.find((s) => s.id === id) ?? SHOW;
}

/** The step a hash names (`#solo`, `#guests`, `#callers`, `#full`): the chooser opens at that
 *  rung. Null for a scenario's hash, or none. */
export function rungFor(id: string): ShowStep | null {
  return STEPS.includes(id as ShowStep) ? (id as ShowStep) : null;
}

export type { GuideFacts, Lever, Part, Scenario, Step } from './scenario.ts';
