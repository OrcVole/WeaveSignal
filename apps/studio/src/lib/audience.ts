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

/** What the Listeners panel says (ADR-0046, ADR-0272). */
export interface AudienceWords {
  /** The count, or null when there is none. */
  count: number | null;
  /** The sentence under the count: by mount, or why nobody could count. */
  detail: string;
  /** What to say about edges or a CDN, or ''. */
  edges: string;
}

/** The mount's name without its slash: `programme.opus`. */
const mountName = (m: string) => m.replace(/^\/+/, '') || m;

/**
 * The panel's words. A count is the listeners on the station's own listener process, by mount;
 * with no count the reason is said plainly: the listener process did not answer, or this control
 * plane is too old to count at all. Edges and a CDN carry listeners the origin does not see, so
 * the figure is the origin's.
 */
export function audienceWords(v: View | null | undefined): AudienceWords {
  const a = v?.audience;
  if (!a) {
    return {
      count: null,
      detail: 'This station’s control plane is too old to count listeners. Update it.',
      edges: '',
    };
  }
  if (a.listeners === null) {
    return {
      count: null,
      detail: a.error ? `Not counted: ${a.error}.` : 'Not counted yet: waiting for the first answer from the listener process.',
      edges: '',
    };
  }
  const mounts = a.by_mount ?? [];
  const detail = mounts.length
    ? mounts.map((m) => `${mountName(m.mount)} ${m.listeners.toLocaleString('en-GB')}`).join(' · ')
    : 'on the station’s stream';
  return {
    count: a.listeners,
    detail,
    edges: a.edges
      ? 'Edges or a CDN are set up: this counts the listeners on the origin, and each edge counts as one. Listeners on an edge are not added here.'
      : '',
  };
}
