// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A training scenario for the sandbox desk (ADR-0222): who is in the room, who rings, what they
// say and when (the beats), the guide's steps read from the station's state, and the two cards
// around it (what you will learn, what you did). The scenarios are data; `station.ts` runs
// them and `Guide.svelte` shows them. Nothing here touches the DOM.

import type { Role, RundownItem, ShowStep } from '../../lib/api.ts';
import type { StationMode } from '../../../../../packages/ui/states.ts';
import type { Beat } from '../lines.ts';
import type { Station } from '../station.ts';

/** Someone in the room from the start. */
export interface Cast {
  identity: string;
  name: string;
  role: Role;
}

/** A caller who may ring: what they typed on the player page. */
export interface CallerSeed {
  name: string;
  topic: string;
}

/** The levers a guide step may offer as a button: the station's own settings and the things
 *  only a server can do, stood in for here. */
export type Lever = 'pull-playout' | 'end-show' | 'go-live' | 'go-auto';

export type Part = string | { key: string };

export interface Step {
  id: string;
  say: Part[];
  done: boolean;
  action?: { label: string; run: Lever };
}

/** What the guide knows beyond the station: which path the visitor hears, and whether they
 *  switched to the listeners after a dump. */
export interface GuideFacts {
  hear: 'studio' | 'listeners';
  /** They have listened as the listeners do at some point. */
  listened: boolean;
  heardListenersAfterDump: boolean;
}

export const k = (key: string): Part => ({ key });

export interface Scenario {
  /** The hash that opens it (`/#alone`). */
  id: string;
  title: string;
  /** One line on the chooser. */
  blurb: string;
  /** About how long it takes. */
  minutes: number;
  /** The step the show runs at (ADR-0271): the chooser's rung, and what the simulated station
   *  switches on. Nobody in the cast, and no caller, may need a seat the step hides. */
  step: ShowStep;
  /** The start card: what you will learn. */
  learn: string[];
  /** Whose chair you sit in: their keys are yours and their recorded voice stands in for yours. */
  you: Cast;
  show: string;
  cast: Cast[];
  /** People of the cast who are not in the room at the start: each joins a few seconds after
   *  the desk's invite dialog makes an invite with their role (the first of them not yet in). */
  later?: string[];
  callers: Record<string, CallerSeed>;
  /** Accounts banned before, by identity (the station's `banned` setting on a real station). */
  bans?: Record<string, { date: string; reason: string }>;
  /** The station's mode at the start (ADR-0045): live unless said. */
  mode?: StationMode;
  rundown: (start: number) => RundownItem[];
  beats: Beat[];
  /** Runs when the sound starts: who rings when, who is already sent to the desk. */
  start?: (s: Station, now: number) => void;
  /** The guide's steps, read from the station each time it changes. */
  steps: (s: Station, g: GuideFacts) => Step[];
  /** The end card: what you did, as the station records it. */
  did: (s: Station, g: GuideFacts) => string[];
}

/** The show's rundown clock, from the moment the sandbox opened (station time, HH:MM). */
export function rundownAt(start: number): (min: number) => string {
  return (min) => {
    const d = new Date(start + min * 60_000);
    return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  };
}
