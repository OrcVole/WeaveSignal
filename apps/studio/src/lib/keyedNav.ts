// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The two jumps between a show and its keys (keyed shows, 1.3.2; the operator's click-through
// of 2026-11-25): from anywhere to a show's Who can listen, and from anywhere to the Keys tab
// with one of its dialogs open. Apart from the keys' store, which the keymap's actions read.

import { run } from './actions.ts';
import { accessUi, ks, keysUi } from './keys.svelte.ts';
import { openShow } from './schedule.svelte.ts';
import { studio } from './studio.svelte.ts';

/** Open a show's Who can listen: the Schedule tab, the show's pane at Details, the fieldset in
 *  view with the focus in it. `pick`: People with a key is chosen for the person and waits for
 *  Save who can listen, since saving changes at once who may hear the show. */
export function toWhoCanListen(slug: string, pick = false): void {
  accessUi.jump = { slug, pick };
  openShow(slug, 'details');
  studio.settings = false;
  if (studio.tab !== 'schedule') void run({ type: 'tab', tab: 'schedule' });
}

/** Go to the Keys tab and open one of its dialogs there: Make a key, or the list of shows to
 *  keep for people with a key. `show`: the list is narrowed to the keys that open that show. */
export function toKeys(ask: 'make' | 'keep' | null = null, show = ''): void {
  if (show) ks.filter = { q: '', state: '', kind: '', family: '', show };
  ks.view = 'keys';
  keysUi.ask = ask;
  studio.settings = false;
  if (studio.tab !== 'keys') void run({ type: 'tab', tab: 'keys' });
}

/** New keyed show: the Schedule tab, with New show open and Keyed show ticked. */
export function toNewKeyedShow(): void {
  accessUi.newKeyed = true;
  studio.settings = false;
  if (studio.tab !== 'schedule') void run({ type: 'tab', tab: 'schedule' });
}
