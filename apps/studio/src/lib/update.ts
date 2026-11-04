// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The update screen's rules (ADR-0015, ADR-0140), apart from the screen: whether an install may
// start now, and the words. The Rust side (src-tauri/src/update.rs) finds, downloads, verifies
// and installs; this decides when the person may ask.
//
// An update never starts by itself, and never while a show is on: restarting the studio app
// during a show would drop the presenter's desk, and a download shares the machine's line with
// the show's own audio.

import { isOpen } from '../../../../packages/ui/states.ts';
import type { View } from './api.ts';

/** What `update_check` answers (src-tauri/src/update.rs, `Check`). */
export interface UpdateCheck {
  /** The build has a key and a feed; without them it never updates itself. */
  enabled: boolean;
  current: string;
  available: UpdateAvailable | null;
  /** The version a rollback would go back to, if one is kept (AppImage only). */
  previous: string | null;
}

export interface UpdateAvailable {
  version: string;
  notes: string | null;
  /** Seconds since 1970, if the feed gives a date. */
  date: number | null;
  /** The person rolled back from this version: the frame does not nag about it. */
  skipped: boolean;
  /** Downloaded and verified, waiting for the install. */
  downloaded: boolean;
}

export interface UpdateGate {
  /** Why nothing may start now; null when it may. */
  blocked: string | null;
  /** Something the person should know before going ahead; null when nothing. */
  caution: string | null;
}

const plural = (n: number, one: string, many: string) => (n === 1 ? one : many);

/**
 * Whether the app may be updated, rolled back or restarted now. Refused while the desk has a
 * strip open (listeners hear it, or the room does while playout is down) or a talk key is held:
 * the desk's view is the judge, and the same facts the master lamp reads.
 *
 * When the studio cannot see the desk at all (no view yet, or the control plane unreachable)
 * the rule cannot be applied; the person is told so and may go ahead. A desk that is down is
 * not on air.
 */
export function updateGate(view: View | null, talking: readonly string[] = []): UpdateGate {
  if (talking.length > 0) {
    return { blocked: 'A talk key is held. Updating waits until nothing is open on the desk.', caution: null };
  }
  if (!view) {
    return {
      blocked: null,
      caution: 'The studio cannot see the desk, so it cannot tell whether a show is on air.',
    };
  }
  const open = view.sources.filter(isOpen).length;
  if (open > 0) {
    const live = view.desk.reachable && view.desk.playout.connected;
    const what = `${open} ${plural(open, 'strip is', 'strips are')} open on the desk`;
    return {
      blocked: `${live ? 'The station is on air' : 'The room is open'}: ${what}. Updating waits until nothing is open.`,
      caution: null,
    };
  }
  return { blocked: null, caution: null };
}

/** `12.3 MB`, in decimal megabytes as a download page gives them. */
export function megabytes(n: number): string {
  return `${(n / 1_000_000).toFixed(1)} MB`;
}

export function progressWords(done: number, total: number | null): string {
  return total ? `${megabytes(done)} of ${megabytes(total)} (${Math.min(100, Math.floor((done / total) * 100))} %)` : megabytes(done);
}

/** A release date in the person's locale, or '' when the feed gave none. */
export function dateWords(seconds: number | null, locale?: string): string {
  if (seconds === null) return '';
  return new Date(seconds * 1000).toLocaleDateString(locale, { year: 'numeric', month: 'long', day: 'numeric' });
}
