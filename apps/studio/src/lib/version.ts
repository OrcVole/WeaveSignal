// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The version in the frame, apart from the screen: what it prints and what its tooltip says.
//
// The frame always says the studio app's own version. The station's is printed beside it only
// when the two differ by their numbers (`versus`, lib/stations.ts: `1.1.0-test.4` is 1.1.0), because
// that is when a person needs it: a station older than the app has parts hidden or switched off,
// and one newer has parts this app cannot show. When they match, a second number would be noise
// in a bar that is already full at 1,440 px; the tooltip still gives both in full. A station is
// never refused on its version alone (ADR-0287, question 3).
//
// Since 1.3.2 the version's ink says where the app stands (the operator, 2026-11-24, reversing
// ADR-0287's "no state glyph and no status colour" for this one word): green with a tick while
// it is up to date, red with an arrow while it is behind, and the frame's own ink, with no
// glyph, while that is not known or the app is ahead. The colour is never the only cue: the
// glyph is drawn with it, the word is in the label for a screen reader, and the tooltip says it
// in a sentence (`standing`, below). The glyph stands before the v (`✓ v1.3.2`; the operator,
// 2026-11-25), not in its place as it first did, and the v is never given up for room: a longer
// version in a tight frame gives up the gap after the glyph (`✓v1.3.10`), and is not drawn in
// less room than that. The room each form asks is in App.svelte's styles.

import { versus } from './stations.ts';

/**
 * Where the studio app stands, for the ink of its version in the frame.
 *
 * - `behind`: a newer app exists that this person should have. Either the update feed offers one
 *   (`offered`, the packaged app's own check, not rolled back from), or the station runs a newer
 *   release than the app (a later minor or patch, or a later major version).
 * - `current`: the station runs the same release as the app (the numbers alone: `1.3.2-test.1`
 *   is 1.3.2), and no update is offered.
 * - `ahead`: the app is newer than the station. Nothing for this person to do to the app.
 * - `unknown`: the station's version is not read yet, or is not one this app can compare.
 */
export type Standing = 'current' | 'behind' | 'ahead' | 'unknown';

/** Where the app stands. `offered` is the newer version the update feed offers, if it does. */
export function standing(app: string, station: string | null | undefined, offered: string | null = null): Standing {
  if (offered) return 'behind';
  switch (versus(app, station)) {
    case 'same':
      return 'current';
    case 'newer':
      return 'behind';
    case 'older':
      return 'ahead';
    case 'major':
      return major(station) > major(app) ? 'behind' : 'ahead';
    default:
      return 'unknown';
  }
}

const major = (v: string | null | undefined): number => Number(/^(\d+)\./.exec((v ?? '').trim())?.[1] ?? 0);

/** The word beside the version for someone who cannot see its ink or its glyph; none while the
 *  standing takes neither. */
export const STANDING_WORD: Record<Standing, string> = { current: 'up to date', behind: 'behind', ahead: '', unknown: '' };

export interface VersionLabel {
  /** The studio app's version: always drawn. */
  app: string;
  /** The station's version, drawn beside the app's: only when its numbers differ, else null. */
  station: string | null;
  /** Its numbers alone (`1.2.0` of `1.2.0-test.1`), for where the frame has little room. */
  stationShort: string | null;
  /** Where the app stands: the ink and the glyph. */
  standing: Standing;
  /** The tooltip, a sentence a line. */
  tip: string[];
}

export function versionLabel(app: string, station: string | null | undefined, offered: string | null = null): VersionLabel {
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
  const stands = standing(app, station, offered);
  // The last line says the standing in words, so the ink and the glyph are never the only cue.
  if (offered) tip.push(`Behind: WeaveSignal ${offered} is available. Settings › Updates installs it.`);
  else if (stands === 'behind') tip.push('This app is behind the station.');
  else if (stands === 'current') tip.push('This app is up to date.');
  return { app, station: shown, stationShort: shown && (/^\d+\.\d+\.\d+/.exec(shown.trim())?.[0] ?? shown), standing: stands, tip };
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
