// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The audience figure the desk shows (ADR-0046) and the warning that goes with it (ADR-0017):
// a station streaming video to more than about 500 viewers needs a CDN or edges, and the
// interface says so. The control plane's view carries both figures (`audience` in `View`): the
// listeners connected to its stream now, and whether edges or a CDN are configured.

import type { View } from './api.ts';

/** "More than about 500 viewers" (ADR-0017): the warning starts above this. */
export const VIDEO_VIEWERS_WITHOUT_CDN = 500;

export interface CdnWarning {
  title: string;
  message: string;
}

/**
 * The warning, or null. It shows when the station has video on, nothing carries the picture
 * but this one server (no edges or CDN configured), and more than about 500 are connected.
 * The count is of listeners on the station's stream; the control plane does not yet count video
 * viewers apart, so it is the nearest figure and an upper bound. A control plane too old to
 * report it, or a count that is not known, gives no warning.
 */
export function cdnWarning(v: View | null | undefined): CdnWarning | null {
  const a = v?.audience;
  const videoOn = !!v?.desk.picture;
  if (!a || !videoOn || a.edges || a.listeners === null || a.listeners <= VIDEO_VIEWERS_WITHOUT_CDN) return null;
  return {
    title: 'Video needs a CDN above about 500 viewers',
    message:
      `${a.listeners.toLocaleString('en-GB')} people are connected and the picture is on, but this station has no CDN or edges set up. ` +
      'One server cannot carry video to that many. Set up a CDN or edges (the guide "Put Bunny in front of the station"), or switch video off.',
  };
}
