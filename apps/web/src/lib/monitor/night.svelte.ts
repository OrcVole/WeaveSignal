// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The monitor link's look (the design's section 4.5): it follows the device's light or dark
// setting like the other public pages (ADR-0112), and adds a Night switch, because this page is
// open through a late show (ADR-0270). The choice is remembered on the device.

const KEPT = 'weavesignal.monitor.night';

/** Whether the host has switched Night on. */
export const look = $state({ night: false });

function paint(): void {
  const root = document.documentElement;
  if (look.night) root.dataset.theme = 'night';
  else if (globalThis.matchMedia?.('(prefers-color-scheme: light)').matches) root.dataset.theme = 'light';
  else delete root.dataset.theme;
}

/** Set the page's theme now and keep it right when the device changes its own. */
export function followDeviceOrNight(): void {
  try {
    look.night = localStorage.getItem(KEPT) === '1';
  } catch {
    // a private window: the device's setting, until the switch is pressed
  }
  paint();
  globalThis.matchMedia?.('(prefers-color-scheme: light)').addEventListener?.('change', paint);
}

export function setNight(on: boolean): void {
  look.night = on;
  paint();
  try {
    if (on) localStorage.setItem(KEPT, '1');
    else localStorage.removeItem(KEPT);
  } catch {
    // as above
  }
}
