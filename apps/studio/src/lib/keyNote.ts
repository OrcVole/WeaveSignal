// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The station key has no voice (1.3.1; K2 of the first-sound run). A window signed in with the
// station key cannot make "Join with my microphone…": the link is made for an account, and the
// key is none. Where that button would be, the desk says what to do instead and takes the person
// to the place: Settings, Staff, Add someone. Where a Not on air row is drawn the note is one
// quiet line in its head, as the DJ's own link is; at Solo, which has none, it keeps a seat.

import { tick } from 'svelte';
import type { Me } from './api.ts';
import { studio } from './studio.svelte.ts';

export const KEY_NOTE = {
  /** The whole note, on an empty Desk and in the seat at Solo, where there is room for both steps. */
  text: 'The station key has no microphone. To speak, add yourself in Staff, then sign in as yourself.',
  /** The one line in the Not on air head, where the row's legend stood (design review F2). */
  head: 'The station key has no microphone: sign in as yourself to speak.',
  /** The button that goes there. */
  action: 'Open Staff',
};

/** Whether the note stands where the button would be: a window on the station key, at a moment
 *  when the desk would offer a signed-in account its own microphone link (`offered`). */
export const keyNoteShown = (me: Me | null | undefined, offered: boolean): boolean => !!me && me.kind === 'station' && offered;

/** Settings, scrolled to Staff, with the focus on Add someone… (the next thing to press). */
export async function openStaff(): Promise<void> {
  studio.settings = true;
  await tick();
  for (let i = 0; i < 10; i++) {
    const panel = document.querySelector<HTMLElement>('[data-panel="settings.staff"]');
    if (panel) {
      panel.scrollIntoView({ block: 'start' });
      panel.querySelector<HTMLElement>('.add')?.focus({ preventScroll: true });
      return;
    }
    // Staff draws once the station has said who this window is.
    await new Promise((r) => setTimeout(r, 100));
  }
}
