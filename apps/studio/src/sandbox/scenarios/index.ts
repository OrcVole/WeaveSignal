// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The sandbox's scenarios (ADR-0222), in the order the chooser lists them: the show first,
// unchanged, then running everything alone, then the smaller ones, one skill each (the two
// handover ones together, setting up before doing).

import { ALONE } from './alone.ts';
import { AUTO } from './auto.ts';
import { BANNED } from './banned.ts';
import { CARTS } from './carts.ts';
import { DUMP } from './dump.ts';
import { HANDOVER } from './handover.ts';
import { LINK } from './link.ts';
import { MIC } from './mic.ts';
import { NEXT_DJ } from './nextdj.ts';
import { QUEUE } from './queue.ts';
import type { Scenario } from './scenario.ts';
import { SHOW } from './show.ts';

export const SCENARIOS: readonly Scenario[] = [SHOW, ALONE, QUEUE, DUMP, BANNED, CARTS, AUTO, NEXT_DJ, HANDOVER, LINK, MIC];

/** The scenario a hash names (`#alone`), or the show. */
export function scenarioFor(id: string): Scenario {
  return SCENARIOS.find((s) => s.id === id) ?? SHOW;
}

export type { GuideFacts, Lever, Part, Scenario, Step } from './scenario.ts';
