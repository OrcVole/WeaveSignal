// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The version in the frame, apart from the screen: what it prints and what its tooltip says.
//
// The frame always says the studio app's own version. The station's is printed beside it only
// when the two differ by their numbers (`versus`, lib/stations.ts: `1.1.0-test.4` is 1.1.0), because
// that is when a person needs it: a station older than the app has parts hidden or switched off,
// and one newer has parts this app cannot show. When they match, a second number would be noise
// in a bar that is already full at 1,440 px; the tooltip still gives both in full. A difference
// is said in words and takes no state glyph and no status colour: it is not a fault, and a
// station is never refused on its version alone (ADR-0287, question 3).

import { versus } from './stations.ts';

export interface VersionLabel {
  /** The studio app's version: always drawn. */
  app: string;
  /** The station's version, drawn beside the app's: only when its numbers differ, else null. */
  station: string | null;
  /** Its numbers alone (`1.2.0` of `1.2.0-test.1`), for where the frame has little room. */
  stationShort: string | null;
  /** The tooltip, a sentence a line. */
  tip: string[];
}

export function versionLabel(app: string, station: string | null | undefined): VersionLabel {
  const tip = [`This studio app is WeaveSignal ${app}.`];
  const how = versus(app, station);
  switch (how) {
    case 'older':
      tip.push(`The station runs ${station}, older than this app: what it does not have yet is hidden or switched off here.`);
      break;
    case 'newer':
      tip.push(`The station runs ${station}, newer than this app: update the app to use everything there.`);
      break;
    case 'major':
      tip.push(`The station runs ${station}, a different major version: some screens may not work until the two match.`);
      break;
    case 'same':
      tip.push(station === app ? 'The station runs the same version.' : `The station runs ${station}, the same release.`);
      break;
    default:
      // Not read yet, or not a version this app can compare: said in full if there is one.
      if (station) tip.push(`The station runs ${station}.`);
  }
  const differs = how === 'older' || how === 'newer' || how === 'major';
  const shown = differs ? (station ?? null) : null;
  return { app, station: shown, stationShort: shown && (/^\d+\.\d+\.\d+/.exec(shown.trim())?.[0] ?? shown), tip };
}

/** How long a reading of the station's version stands before a link that comes back asks again. */
export const REREAD_MS = 30_000;

/**
 * Whether to read the station's version again when the link to it comes back. A station is
 * updated by restarting it, which drops the link, so the return of the link is when its version
 * may have changed. Not before the first reading (the start reads it once by itself), and not
 * more often than `REREAD_MS`, so a link that comes and goes does not ask each time.
 */
export function rereadNow(readAt: number, now: number): boolean {
  return readAt > 0 && now - readAt >= REREAD_MS;
}
