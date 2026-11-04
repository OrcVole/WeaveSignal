// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The grid adapter (ADR-0021): the Schedule tab draws its week through one small interface, so
// the library behind it (Event Calendar, ADR-0185) can be swapped without touching the toolbar,
// the editors or the model.

import type { Block } from '../model.ts';

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
  /** A future block dragged or resized: resolves with null when the control plane took it, or
   *  the reason it refused, and the grid snaps the block back. */
  onMove: (id: string, start: number, end: number) => Promise<string | null>;
  /** The resource view's columns, when the grid is asked for one (studios as columns). */
  studios?: string[] | null;
  /** The week, or one day with a column per studio. */
  view?: 'week' | 'studios';
  /** The day the studios view shows, 0 (Monday) to 6. */
  day?: number;
}

/** A moment a block may still be moved: a future block that has not aired (calendar.md §5). */
export function movable(b: Block, now: number): boolean {
  return b.kind === 'episode' && !b.frozen && b.start > now;
}
