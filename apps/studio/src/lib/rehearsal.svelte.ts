// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The rehearsal studio's store (1.3.0, #23): the control plane's rehearsal view, fed by its
// event stream, and the presses that open, drive and end a session. The rules that read it are
// `rehearsal.ts`; the reference is the docs page `reference/studio-rehearsal-data`.
//
// Use: `startRehearsal()` once the window is signed in (it reads the view once, then follows
// `/api/v1/rehearsal/events`); it returns its stop. A station older than 1.3.0 answers 404 and
// the store settles on `absent` without reconnecting. A change of station closes the stream and
// empties the store; start it again on the new station. The Desk tab's choice of studio is
// `studio.deskStudio` (`studio.svelte.ts`), set with `chooseDeskStudio`.

import {
  FieldError,
  endRehearsal,
  followRehearsal,
  getRehearsal,
  openRehearsal,
  rehearsalCommand,
  type LinkState,
  type RehearsalCommand,
  type RehearsalView,
} from './api.ts';
import { onLeave, onStationChange } from './reset.ts';
import { phaseOf, type RehearsalPhase } from './rehearsal.ts';
import { studio } from './studio.svelte.ts';

/** What `rehearsal` is when the page first loads: a change of station puts it back. */
const fresh = () => ({
  /** The rehearsal view as last sent; null until read. */
  view: null as RehearsalView | null,
  /** The station has no rehearsal routes (older than 1.3.0). */
  absent: false,
  /** The event stream's link. */
  link: 'connecting' as LinkState,
  linkWhy: '',
  /** A press is on its way (open, a command, end): the buttons wait. */
  busy: false,
  /** The last refusal, in the station's words (403 no rights, 409 busy or booked, 503 off);
   *  cleared by the next press that succeeds, or `clearRehearsalError`. */
  error: null as string | null,
  /** The status of that refusal, so a screen can tell "busy" (409) from "not yours" (403). */
  errorStatus: null as number | null,
});

export const rehearsal = $state(fresh());

let stopStream: (() => void) | null = null;
let offLeave: (() => void) | null = null;
let generation = 0;

/** Take a view the station sent: the stream's, or a press's answer. */
export function takeRehearsalView(v: RehearsalView): void {
  rehearsal.view = v;
  rehearsal.absent = false;
}

/** The studio's phase now (`rehearsal.ts`, `phaseOf`). */
export function rehearsalPhase(): RehearsalPhase {
  return phaseOf(rehearsal.view, rehearsal.absent);
}

/** Read the rehearsal studio once, then follow it. Calling it again while it runs does
 *  nothing. Returns the stop. */
export function startRehearsal(): () => void {
  if (stopStream) return stopRehearsal;
  const mine = ++generation;
  let stopped = false;
  stopStream = () => {
    stopped = true;
  };
  offLeave = onLeave('rehearsal events', stopRehearsal);
  void (async () => {
    try {
      takeRehearsalView(await getRehearsal());
    } catch (e) {
      if (e instanceof FieldError && e.status === 404) {
        if (mine === generation) {
          rehearsal.absent = true;
          rehearsal.link = 'lost';
          rehearsal.linkWhy = 'This station has no rehearsal studio: it is older than 1.3.0.';
        }
        return;
      }
      // Anything else: the stream says what it finds.
    }
    if (stopped || mine !== generation) return;
    const stop = followRehearsal(
      (v) => {
        if (mine === generation) takeRehearsalView(v);
      },
      (s, why) => {
        if (mine !== generation) return;
        rehearsal.link = s;
        rehearsal.linkWhy = why ?? '';
      },
    );
    stopStream = stop;
  })();
  return stopRehearsal;
}

/** Stop following the rehearsal studio (sign-out, leaving the station). The view stays until
 *  the store is emptied. */
export function stopRehearsal(): void {
  generation += 1;
  stopStream?.();
  stopStream = null;
  offLeave?.();
  offLeave = null;
}

async function press(act: () => Promise<RehearsalView>): Promise<boolean> {
  rehearsal.busy = true;
  try {
    takeRehearsalView(await act());
    rehearsal.error = null;
    rehearsal.errorStatus = null;
    return true;
  } catch (e) {
    rehearsal.error = e instanceof Error ? e.message : String(e);
    rehearsal.errorStatus = e instanceof FieldError ? e.status : null;
    return false;
  } finally {
    rehearsal.busy = false;
  }
}

/** Open the rehearsal studio for a show (its slug). False with `rehearsal.error` set when
 *  refused: another show is rehearsing, a booking holds the studio for another show (409), no
 *  rights over the show (403), the studio is off (503). */
export const openRehearsalFor = (show: string) => press(() => openRehearsal(show));

/** A switch or a cart at the rehearsal desk (`rehearsal.ts` has `mixCommand`). */
export const sendRehearsal = (c: RehearsalCommand) => press(() => rehearsalCommand(c));

/** End the rehearsal: everyone out of the rehearsal room. */
export const endRehearsalNow = () => press(() => endRehearsal());

/** Forget the last refusal (its toast was dismissed). */
export function clearRehearsalError(): void {
  rehearsal.error = null;
  rehearsal.errorStatus = null;
}

/** The Desk tab's studio: the live desk, or the rehearsal desk. A window's own choice, never
 *  remembered; a change of station puts it back to live. */
export function chooseDeskStudio(which: 'live' | 'rehearsal'): void {
  studio.deskStudio = which;
}

// A change of station (ADR-0285): the stream is closed and nothing of the last station stays.
onStationChange('rehearsal', () => {
  stopRehearsal();
  Object.assign(rehearsal, fresh());
});
