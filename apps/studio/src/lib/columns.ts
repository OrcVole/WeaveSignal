// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The desk in one column. Where the studio is 1,400 px wide or less (a narrow window, or a large
// text size, which is zoom) styles/desk-b.css folds the desk's three columns into one that
// scrolls under a status bar that stays. The style sheet decides that, with a container query,
// and says which it drew in `--desk-columns` on the desk; the desk reads it here, because a
// toast has a different place in each (DeskB.svelte): in three columns the slot at the right
// column's foot, which is always in view; in one column that slot is the foot of a long page, so
// the toast has a row of its own above the status bar instead.

/** The widest studio drawn in one column, in CSS pixels: the style sheet's own figure
 *  (`@container studio (max-width: 1400px)`; columns.test.ts holds the two together). */
export const ONE_COLUMN_MAX = 1400;

/** Before the desk is drawn: whether a window this wide, at this zoom, will be one column. The
 *  studio is as wide as the window, and zoom divides it. A guess that saves a toast showing at
 *  the first moment from being drawn in one place and then the other; `oneColumn` is the answer. */
export function guessOneColumn(windowWidth: number | undefined, zoom: number): boolean {
  return typeof windowWidth === 'number' && windowWidth > 0 && zoom > 0 && windowWidth / zoom <= ONE_COLUMN_MAX;
}

/** Whether the style sheet has drawn this desk in one column. */
export function oneColumn(desk: Element): boolean {
  return getComputedStyle(desk).getPropertyValue('--desk-columns').trim() === '1';
}
