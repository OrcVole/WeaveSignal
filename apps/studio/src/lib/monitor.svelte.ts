// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The monitor links this window holds, for Open my monitor (ADR-0280; design section 4.4).
//
// A monitor link carries its own secret, and the station keeps only a scrambled form of it:
// no route gives a link back once the key's details have been closed. So Open my monitor works
// from the window that made the key, for the person it was made for, until that window is
// closed or changes station. The links live in this module's memory and nowhere else: nothing
// is written to storage, and a link is let go when its key is renewed or ended here.

import { onStationChange } from './reset.ts';

export const monitors = $state({ links: {} as Record<string, string> });

/** Hold the link of a key just made for the person signed in. */
export function holdMonitor(keyId: string, link: string): void {
  monitors.links = { ...monitors.links, [keyId]: link };
}

/** The key was renewed or ended: its link no longer opens anything. */
export function dropMonitor(keyId: string): void {
  if (!(keyId in monitors.links)) return;
  const { [keyId]: _gone, ...rest } = monitors.links;
  monitors.links = rest;
}

// A change of station (ADR-0285): nothing of the last one stays.
onStationChange('monitor', () => {
  monitors.links = {};
});
