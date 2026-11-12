// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A modal dialog owns the keyboard (keymap rule 5). While one is open (the invite, the show
// setup, Settings' confirmations, RTMP out's destination), only ⌃T, the held talkback, still
// reaches the desk: a tab chord would throw away the dialog's work (an invite's link is shown
// once), and ⌃D ⌃D or ⌃⇧E behind it would act on air unseen (review 2026-11-10, U1).

export interface ChordKey {
  code: string;
  ctrl: boolean;
  meta: boolean;
  shift: boolean;
  alt: boolean;
}

/** Whether a key pressed while a modal dialog is open may go on to the keymap. */
export function dialogLets(k: ChordKey): boolean {
  return (k.ctrl || k.meta) && !k.shift && !k.alt && k.code === 'KeyT';
}
