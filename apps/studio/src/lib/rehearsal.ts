// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The rehearsal studio's rules as the studio app reads them (1.3.0, #23 and #24): what state the
// studio is in, who is in the mix, what a move to the live studio would carry, and the bookings.
// Pure: the store is `rehearsal.svelte.ts`. Nothing in the rehearsal studio reaches the air, so
// nothing here says "on air" (ADR-0062). The reference is the docs page
// `reference/studio-rehearsal-data`; the design is the project folder's
// `docs/reference/design/rehearsal-mode-build-plan.md`.

import type { RehearsalBlock, RehearsalCommand, RehearsalPerson, RehearsalView } from './api.ts';

/** Where the rehearsal studio stands, for the screens:
 *  - `unknown`: not read yet;
 *  - `absent`: the station is older than 1.3.0 and has none;
 *  - `off`: the station has it turned off (the installation's `REHEARSAL` setting);
 *  - `desk-down`: on, but its desk is not answering ("The rehearsal desk is starting again");
 *  - `closed`: on, its desk answering, no session open ("Open the rehearsal studio for…");
 *  - `open`: a session is open for a show. */
export type RehearsalPhase = 'unknown' | 'absent' | 'off' | 'desk-down' | 'closed' | 'open';

/** The phase from the last view, and whether the station said it has no such route. An open
 *  session wins over a desk that is not answering: the session is still the show's, and the
 *  screen says both. */
export function phaseOf(view: RehearsalView | null, absent: boolean): RehearsalPhase {
  if (absent) return 'absent';
  if (!view) return 'unknown';
  if (!view.enabled) return 'off';
  if (view.session) return 'open';
  return view.desk.linked ? 'closed' : 'desk-down';
}

/** The people at the rehearsal desk now (left the room: not present). */
export function presentPeople(view: RehearsalView | null): RehearsalPerson[] {
  return (view?.people ?? []).filter((p) => p.present);
}

/** The people the others hear: present and in the mix. */
export function inTheMix(view: RehearsalView | null): RehearsalPerson[] {
  return presentPeople(view).filter((p) => p.on);
}

/** The room is full: as many people present as it takes. */
export function roomFull(view: RehearsalView | null): boolean {
  return !!view && view.max_people > 0 && presentPeople(view).length >= view.max_people;
}

/** What a move to the live studio would carry as the opening state, from the people present
 *  now: who goes on at the handover (`on`) and who is muted (`mute`), each sorted. The control
 *  plane works it out the same way when the move is pressed (`Opening::from_people`); this is
 *  for the move dialog to show before it is. */
export function openingPreview(view: RehearsalView | null): { on: string[]; mute: string[] } {
  const here = presentPeople(view);
  const sorted = (xs: string[]) => [...new Set(xs)].sort();
  return { on: sorted(here.filter((p) => p.on).map((p) => p.identity)), mute: sorted(here.filter((p) => p.mute).map((p) => p.identity)) };
}

/** The session is open for this show. */
export function rehearsing(view: RehearsalView | null, show: string | null | undefined): boolean {
  return !!show && view?.session?.show === show;
}

/** Into the rehearsal's mix, or out of it: the desk's own op is `on_air`, though the rehearsal
 *  desk has no air. */
export const mixCommand = (identity: string, on: boolean): RehearsalCommand => ({ op: 'on_air', identity, on });

/** The bookings among a week's rehearsal blocks. */
export const bookingsOf = (blocks: readonly RehearsalBlock[]): RehearsalBlock[] => blocks.filter((b) => b.kind === 'booking');

/** The sessions among a week's rehearsal blocks. */
export const sessionsOf = (blocks: readonly RehearsalBlock[]): RehearsalBlock[] => blocks.filter((b) => b.kind === 'session');

/** The booking that holds the studio at `nowMs`, if any: Open is refused for any other show
 *  while it does. A booking holds from its start until (not at) its end. */
export function holdingBooking(blocks: readonly RehearsalBlock[], nowMs: number): RehearsalBlock | null {
  return bookingsOf(blocks).find((b) => b.start_ms <= nowMs && nowMs < b.end_ms) ?? null;
}

/** Whether `[startMs, endMs)` would share time with a booking (other than `except`), as the
 *  control plane refuses it: touching is not sharing. The editor can say so before the 409. */
export function bookingClashAt(blocks: readonly RehearsalBlock[], startMs: number, endMs: number, except: string | null = null): RehearsalBlock | null {
  return bookingsOf(blocks).find((b) => b.id !== except && b.start_ms < endMs && startMs < b.end_ms) ?? null;
}

/** A booking can still be changed or deleted: it is not over. */
export const bookingChangeable = (b: RehearsalBlock, nowMs: number): boolean => b.kind === 'booking' && b.end_ms > nowMs;
