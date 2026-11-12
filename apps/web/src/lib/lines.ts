// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What the public pages say when a show does not take calls (ADR-0271): a show runs at one of
// four steps, and below Callers the station closes call-in while it is on air. The player says
// the lines are closed, a form sent a moment too late is told so, and a caller who was only
// waiting when the lines closed is thanked and told they were not on air. Pure, so the words
// are tested.

import { ApiError, type Released } from './api.ts';

/** The station's own sentence for call-in closed by the show's step
 *  (`crates/control/src/steps.rs`, `LINES_CLOSED`): the player's `call_in.why_not`, and what
 *  the call-in request itself answers. */
export const LINES_CLOSED = 'The lines are closed for this show.';

/** Added while the chat is open to write in. */
export const CHAT_STILL = 'You can still write in the chat below.';

/** What the player says under Talk to the studio while call-in is closed: the station's
 *  sentence, and for a show that takes no calls, the chat as the way to reach it. */
export function closedLine(whyNot: string | null, chatOpen: boolean): string {
  const why = whyNot ?? '';
  return why === LINES_CLOSED && chatOpen ? `${why} ${CHAT_STILL}` : why;
}

/** A call-in form sent after the lines closed: the page it was filled in on opened earlier. */
export const CLOSED_WHILE_FILLING = 'The lines closed while you were filling this in. This show is not taking calls.';

/** Whether a refused call-in was refused because the show takes no calls. The station answers
 *  that with the one sentence the player's facts carry, so it is told apart by it. */
export function closedUnderForm(e: unknown): boolean {
  return e instanceof ApiError && e.message === LINES_CLOSED;
}

/** What a caller who was only waiting reads when the lines close under them. */
export function releasedWords(cause: Released, name: string): { headline: string; thanks: string; detail: string } {
  const who = name.trim().split(/\s+/)[0] ?? '';
  return {
    headline: 'The lines have closed',
    thanks: who ? `Thank you for waiting, ${who}.` : 'Thank you for waiting.',
    detail:
      cause === 'show_took_air'
        ? 'The show that is starting now is not taking calls, so your place in the queue has gone. You were not on air.'
        : 'This show has stopped taking calls, so your place in the queue has gone. You were not on air.',
  };
}
