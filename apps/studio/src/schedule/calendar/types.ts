// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The grid adapter (ADR-0021): the Schedule tab draws its week through one small interface, so
// the library behind it (Event Calendar, ADR-0185) can be swapped without touching the toolbar,
// the editors or the model.

import type { Block, Rehearsal } from '../model.ts';
import type { BookingChange } from '../studios.ts';

/** A block of the Rehearsal column as the grid is given it: what the station sent, what of it
 *  this window may still change by dragging, and whether it is the booking being made. */
export interface GridRehearsal extends Rehearsal {
  change: BookingChange;
  /** The new booking, not yet at the station: drawn so that its times can be seen and dragged. */
  draft?: boolean;
}

export interface GridProps {
  /** Everything the week shows, automation included. */
  blocks: Block[];
  /** Eight day starts, Monday to the next Monday, epoch ms in the station's zone. */
  days: number[];
  zone: string;
  /** The station's clock, for the now line, `aired` and ON AIR. */
  now: number;
  selected: string | null;
  onSelect: (id: string) => void;
  /** A press on the grid where no block is: the gap beside a day's blocks, or the plane beside
   *  the Day view's column. Never the heads, the time axis or a scrollbar. */
  onClear?: () => void;
  /** A future block dragged or resized (an episode, or in the Studios view a booking of the
   *  rehearsal studio, `reh:…`): resolves with null when the control plane took it, or the
   *  reason it refused, and the grid snaps the block back. */
  onMove: (id: string, start: number, end: number) => Promise<string | null>;
  /** A day asked for: its header in the week, a day of the Day view's strip, or the count on a
   *  fold. 0 (Monday) to 6. */
  onOpenDay?: (day: number) => void;
  /** The Studios view's columns, in the station's order (1.3.0): Studio, and Rehearsal when the
   *  station has a rehearsal studio. Null or empty, the view has the one column. */
  studios?: readonly { id: string; name: string }[] | null;
  /** The Rehearsal column's blocks: bookings and sessions, and the booking being made. Drawn
   *  only in the Studios view; never in the week. */
  rehearsals?: readonly GridRehearsal[];
  /** Book…: the Rehearsal column's own button (no stretch: the caller chooses the times), or a
   *  drag across its empty time. Absent when this window may book for no show. */
  onBook?: (stretch: { start: number; end: number } | null) => void;
  /** Why this window cannot book, for the Rehearsal column's head. */
  bookWhy?: string;
  /** The shows kept for people with a key now, by path (keyed shows, 1.3.2): each of their
   *  blocks carries the key. Absent or empty, none does. */
  keyed?: ReadonlySet<string>;
  /** The week; one day, a lane for each clashing show; or one day with a column per studio. */
  view?: 'week' | 'day' | 'studios';
  /** The day the Day view and the studios view show, 0 (Monday) to 6. */
  day?: number;
  /** A block to bring into view when it changes (a row of a show's dates was pressed): the grid
   *  scrolls to it. `n` makes the same block asked for twice a new ask. */
  reveal?: { id: string; n: number } | null;
}

/** A moment a block may still be moved: a future block that has not aired (calendar.md §5). */
export function movable(b: Block, now: number): boolean {
  return b.kind === 'episode' && !b.frozen && b.start > now;
}
