// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The move from the rehearsal studio to the live studio (1.3), the page's part: the person's
// link carries their pass in its fragment (`/join/#<pass>`), and the move swaps the rehearsal
// pass for the live one there, so a reload after the move lands in the live studio. Pure
// apart from the two browser objects it is handed, so it is tested.

import type { MoveTo } from './api.ts';

/** How long the page keeps trying to join the live studio after a move: the new pass lasts
 *  hours, but a person who cannot get in after this is told so rather than left on "Moving". */
export const MOVE_RETRY_MS = 120_000;

/** What of `location` the swap reads. */
export interface Where {
  pathname: string;
  search: string;
}

/** What of `history` the swap uses. */
export interface Past {
  state: unknown;
  replaceState(state: unknown, unused: string, url: string): void;
}

/**
 * Put `pass` in the page's fragment in place of the old one. `replaceState`, never assigning
 * `location.hash` and never going to the move's `link`: those would add a Back entry that
 * holds a dead pass, or load the page again and lose the microphone the move keeps.
 */
export function adoptPass(loc: Where | undefined, past: Past | undefined, pass: string): void {
  if (!loc || !past) return;
  past.replaceState(past.state, '', `${loc.pathname}${loc.search}#${pass}`);
}

/** The page's own swap, from the real browser objects; nothing where there are none. */
export const adoptInBrowser = (pass: string): void => adoptPass(globalThis.location, globalThis.history, pass);

/** What the page remembers of a move, for its words. */
export type Moved = Pick<MoveTo, 'show' | 'airing_start_ms'>;
