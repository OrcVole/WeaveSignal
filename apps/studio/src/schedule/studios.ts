// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Studios as columns and the rehearsal studio's bookings (1.3.0, issue 24; the project
// folder's `docs/reference/design/rehearsal-mode-build-plan.md`, section 2), as pure rules with
// no page: which columns the Studios view has and why one may be missing, who may book the
// rehearsal studio for which show, what a booking's editor may change and what it sends, the
// times a new booking starts with, and the words for a session as it happened.
//
// The station stays the authority (the bookings' routes refuse what they refuse, in their own
// sentences): these mirror its rules so that a slip is caught before the round trip, and so
// that nothing is offered that would only be refused. Nothing in the rehearsal studio reaches
// the air, so nothing here says "on air" of it (ADR-0062).

import type { BookingRequest, Me, RehearsalBlock, StudioColumn } from '../lib/api.ts';
import type { Show } from '../lib/schedule.ts';
import { DAY_MS, HOUR_MS, REHEARSAL, STUDIO, fromLocalInput, hm, lengthWords, longDayWords, parts, rehearsalId, type Block, type Rehearsal } from './model.ts';

const pad = (n: number) => String(n).padStart(2, '0');

// ---- the columns ----

/** What the Studios view can show at this station:
 *  - `unknown`: the station has not answered yet;
 *  - `older`: the station is older than 1.3.0 and sends no columns; it has the one studio and
 *    no Studios view;
 *  - `off`: it has the view, but its rehearsal studio is turned off (the installation's
 *    `REHEARSAL` setting), so there is the one column;
 *  - `both`: Studio and Rehearsal. */
export type StudiosState = 'unknown' | 'older' | 'off' | 'both';

/** `answered`: the station has sent a week, so columns it did not send are columns it has not. */
export function studiosState(columns: readonly StudioColumn[] | null, answered = true): StudiosState {
  if (!columns?.length) return answered ? 'older' : 'unknown';
  return columns.some((c) => c.id === REHEARSAL) ? 'both' : 'off';
}

/** What each column's head says under its name. */
export function columnCaption(id: string): string {
  if (id === STUDIO) return 'On air by the schedule';
  if (id === REHEARSAL) return 'Never on air';
  return '';
}

/** The sentence for a station whose rehearsal studio is off, and the way to turn it on. */
export const REHEARSAL_OFF = 'The rehearsal studio is off at this station, so there is the one column.';
export const REHEARSAL_OFF_HOW = 'Whoever runs the installation turns it on: REHEARSAL=true in its settings, then a restart.';
/** Why the Studios button waits at an older station. */
export const STUDIOS_OLDER = 'This station is older than 1.3.0: it has the one studio, and sends no columns to show.';

// ---- who may book ----

/** Whether this window may book the rehearsal studio for a show, as the station's rule has it
 *  (`steps::may_change`): an admin, a producer, the station key, or the member of staff who
 *  presents the show. A window the station has not named yet (`me` null) is as the key was. */
export function mayBook(me: Me | null | undefined, show: Pick<Show, 'presenter_account'> | null | undefined): boolean {
  if (!show) return false;
  if (!me) return true;
  if (me.kind === 'station' || me.role === 'admin' || me.role === 'producer') return true;
  return !!me.account?.id && show.presenter_account === me.account.id;
}

/** The shows this window may book for, by name. None: the Rehearsal column is read-only. */
export function bookableShows<S extends Pick<Show, 'presenter_account' | 'name'>>(me: Me | null | undefined, shows: readonly S[]): S[] {
  return shows.filter((s) => mayBook(me, s)).sort((a, b) => a.name.localeCompare(b.name));
}

/** Why a window with no show of its own cannot book, for the column's head and the pane. */
export const NO_SHOW_TO_BOOK = 'Only an admin, a producer or a show’s presenter books the rehearsal studio. You present no show yet, so the column is to read.';

// ---- what may change ----

/** What may still be changed of a block in the Rehearsal column:
 *  - `all`: a booking still to come: its show, its times, and Delete;
 *  - `end`: a booking under way keeps its start; its end and its show may change, and Delete
 *    frees the studio;
 *  - `none`: a booking that is over, any session, or a booking for a show this window has no
 *    rights over. */
export type BookingChange = 'all' | 'end' | 'none';

export function bookingChange(r: Pick<Rehearsal, 'kind' | 'start' | 'end'>, now: number, may: boolean): BookingChange {
  if (r.kind !== 'booking' || !may || r.end <= now) return 'none';
  return r.start <= now ? 'end' : 'all';
}

/** Why a block in the Rehearsal column is only to read; null when it can be changed. */
export function readOnlyWhy(r: Pick<Rehearsal, 'kind' | 'start' | 'end' | 'show' | 'open'>, now: number, may: boolean): string | null {
  if (r.kind === 'session') return r.open ? 'A rehearsal under way is run from the Desk tab’s rehearsal studio, not from here.' : 'A rehearsal that has happened stays in the schedule as it was.';
  if (r.end <= now) return 'This booking is over: it stays in the schedule as it was.';
  if (!may) return `Only an admin, a producer or ${r.show}’s presenter changes this booking.`;
  return null;
}

// ---- the editor's fields ----

/** What the booking editor holds: the show's slug, the date, and the two times, station time.
 *  An end at or before the start is the day after (a rehearsal over midnight). */
export interface BookingFields {
  show: string;
  /** `YYYY-MM-DD`. */
  date: string;
  /** `HH:MM`. */
  from: string;
  until: string;
}

/** `2026-11-25` for the station's date at `ms`. */
export function isoDate(ms: number, zone: string): string {
  const p = parts(ms, zone);
  return `${String(p.y).padStart(4, '0')}-${pad(p.m)}-${pad(p.d)}`;
}

/** The editor's fields for a stretch of time. */
export function fieldsOf(show: string, start: number, end: number, zone: string): BookingFields {
  return { show, date: isoDate(start, zone), from: hm(start, zone), until: hm(end, zone) };
}

/** The stretch the fields name, or null while they cannot be read. */
export function stretchOf(f: Pick<BookingFields, 'date' | 'from' | 'until'>, zone: string): { start: number; end: number } | null {
  if (!f.date || !f.from || !f.until) return null;
  const start = fromLocalInput(`${f.date}T${f.from}`, zone);
  let end = fromLocalInput(`${f.date}T${f.until}`, zone);
  if (start === null || end === null) return null;
  if (end <= start && f.until !== f.from) {
    // The day after, by the calendar and not by 24 hours: a clocks-change night is 23 or 25.
    const next = new Date(Date.UTC(Number(f.date.slice(0, 4)), Number(f.date.slice(5, 7)) - 1, Number(f.date.slice(8, 10)) + 1));
    end = fromLocalInput(`${next.toISOString().slice(0, 10)}T${f.until}`, zone);
    if (end === null) return null;
  }
  return { start, end };
}

/** The station's limits (`rehearsal_bookings.rs`): five minutes to a day. */
export const SHORTEST_MS = 5 * 60_000;
export const LONGEST_MS = DAY_MS;
/** A booking may start a minute before now: the time it took to fill in the editor. */
const GRACE_MS = 60_000;

export type BookingAsk =
  /** The fields cannot be a booking: what to say, and which of them it is about. */
  | { problem: string; about: 'show' | 'from' | 'until' }
  /** A booking as it is already: nothing to send. */
  | { same: true }
  /** What to send: a whole booking when it is new, only what changed otherwise. */
  | { request: BookingRequest; patch: Partial<BookingRequest> };

/** What Save sends, checked as the station checks it. `was` is the booking being changed (null
 *  for a new one); one under way keeps its start, whatever the fields say. */
export function bookingAsk(f: BookingFields, zone: string, now: number, was: Pick<Rehearsal, 'slug' | 'start' | 'end'> | null = null): BookingAsk {
  if (!f.show) return { problem: 'Choose the show the studio is held for.', about: 'show' };
  const typed = stretchOf(f, zone);
  if (!typed) return { problem: 'Give a date and both times, such as 13:30 and 14:00.', about: !f.until ? 'until' : 'from' };
  const underWay = !!was && was.start <= now;
  const start = underWay ? was.start : typed.start;
  const end = typed.end;
  if (end <= start) return { problem: 'A booking ends after it starts.', about: 'until' };
  if (end - start < SHORTEST_MS) return { problem: 'A booking is at least five minutes long.', about: 'until' };
  if (end - start > LONGEST_MS) return { problem: 'A booking is at most a day long.', about: 'until' };
  if (!underWay && start < now - GRACE_MS) return { problem: 'A booking starts from now on: the past cannot be booked.', about: 'from' };
  if (end <= now) return { problem: 'A booking ends from now on.', about: 'until' };
  const request: BookingRequest = { show: f.show, start_ms: start, end_ms: end };
  if (!was) return { request, patch: request };
  const patch: Partial<BookingRequest> = {};
  if (f.show !== was.slug) patch.show = f.show;
  if (start !== was.start) patch.start_ms = start;
  if (end !== was.end) patch.end_ms = end;
  return Object.keys(patch).length ? { request, patch } : { same: true };
}

// ---- a new booking's first times ----

/** The times Book… starts with on the day shown (`from` to `to`, epoch ms): an hour from the
 *  next half hour when the day is today, from 10:00 station time on a day to come, moved past
 *  any booking in the way. Null when the day has gone: the past cannot be booked. */
export function firstBooking(day: { from: number; to: number }, now: number, zone: string, taken: readonly Pick<Rehearsal, 'kind' | 'start' | 'end'>[]): { start: number; end: number } | null {
  if (day.to <= now) return null;
  const HALF = HOUR_MS / 2;
  let start: number;
  if (now >= day.from) start = Math.ceil((now + 1) / HALF) * HALF;
  else {
    const p = parts(day.from, zone);
    start = fromLocalInput(`${String(p.y).padStart(4, '0')}-${pad(p.m)}-${pad(p.d)}T10:00`, zone) ?? day.from + 10 * HOUR_MS;
  }
  const bookings = taken.filter((t) => t.kind === 'booking').sort((a, b) => a.start - b.start);
  for (const b of bookings) if (b.start < start + HOUR_MS && start < b.end) start = Math.ceil(b.end / HALF) * HALF;
  if (start >= day.to) return null;
  return { start, end: start + HOUR_MS };
}

/** A stretch dragged on the grid, as a booking may have it: nothing of it before now (to the
 *  next five minutes), and null when all of it is past. */
export function fromDrag(start: number, end: number, now: number): { start: number; end: number } | null {
  if (end <= now) return null;
  const FIVE = 5 * 60_000;
  const from = start >= now ? start : Math.ceil(now / FIVE) * FIVE;
  return end - from >= SHORTEST_MS ? { start: from, end } : null;
}

/** The show Book… offers first, among those this window may book for: the one that goes on air
 *  soonest after the booking ends that day (rehearsing is for the show about to air), else the
 *  first by name. Empty when there is none to offer. */
export function offeredShow(bookable: readonly Pick<Show, 'slug'>[], blocks: readonly Pick<Block, 'kind' | 'slug' | 'start'>[], end: number): string {
  if (!bookable.length) return '';
  const mine = new Set(bookable.map((s) => s.slug));
  const next = blocks
    .filter((b) => (b.kind === 'episode' || b.kind === 'rule') && mine.has(b.slug) && b.start >= end && b.start < end + 12 * HOUR_MS)
    .sort((a, b) => a.start - b.start)[0];
  return next?.slug ?? bookable[0].slug;
}

/** When the booked show next goes on air after the booking, for the editor's line: `Steeped is
 *  on air at 14:00, 30 min after this ends.` Empty when it has no airing in the week on screen
 *  within twelve hours (the move to the live studio needs one). */
export function airsAfterWords(slug: string, name: string, end: number, blocks: readonly Pick<Block, 'kind' | 'slug' | 'start'>[], zone: string): string {
  const next = blocks.filter((b) => (b.kind === 'episode' || b.kind === 'rule') && b.slug === slug && b.start >= end && b.start < end + 12 * HOUR_MS).sort((a, b) => a.start - b.start)[0];
  if (!next) return '';
  const gap = next.start - end;
  return gap === 0 ? `${name} is on air at ${hm(next.start, zone)}, as this ends.` : `${name} is on air at ${hm(next.start, zone)}, ${lengthWords(gap)} after this ends.`;
}

// ---- words ----

/** `Wednesday 25 November, 13:30 to 14:00`; an end on the day after says so. */
export function stretchWords(start: number, end: number, zone: string): string {
  const over = isoDate(start, zone) !== isoDate(end, zone) && hm(end, zone) !== '00:00';
  return `${longDayWords(start, zone)}, ${hm(start, zone)} to ${hm(end, zone)}${over ? ' the next day' : ''}`;
}

/** A session in one sentence, as it happened: `Opened by Kirsty at 11:02 and ended at 11:40.` */
export function sessionWords(r: Pick<Rehearsal, 'start' | 'end' | 'by' | 'open' | 'ended'>, zone: string): string {
  const by = r.by ? `Opened by ${r.by}` : 'Opened';
  const at = hm(r.start, zone);
  if (r.open) return `${by} at ${at}, and still open.`;
  const end = hm(r.end, zone);
  if (r.ended === 'moved') return `${by} at ${at}. At ${end} everyone moved to the live studio, standing by until the handover.`;
  if (r.ended === 'idle') return `${by} at ${at}. It ended by itself at ${end}, with nobody left in it.`;
  return `${by} at ${at} and ended at ${end}.`;
}

/** The kind line on a block of the Rehearsal column, and its word for a screen reader. */
export function rehearsalKind(r: Pick<Rehearsal, 'kind' | 'open' | 'ended' | 'past'>): string {
  if (r.kind === 'booking') return r.past ? 'Was booked' : 'Booked';
  if (r.open) return 'Rehearsing now';
  return r.ended === 'moved' ? 'Rehearsed · moved to live' : 'Rehearsed';
}

/** The booking in the way, in the station's own block: `Kettle Drive has the studio from 13:00
 *  to 14:00.` */
export function inTheWayWords(c: Pick<RehearsalBlock, 'show_name' | 'start_ms' | 'end_ms'>, zone: string): string {
  return `${c.show_name} has the studio from ${hm(c.start_ms, zone)} to ${hm(c.end_ms, zone)}.`;
}

// ---- the station's refusal ----

/** A refused save, for the editor: the station's sentence, the field it is about when it named
 *  one, and the booking in the way when that is why (409 with `clash`). */
export interface Refusal {
  message: string;
  about: 'show' | 'from' | 'until' | null;
  clash: { id: string; start: number; words: string } | null;
}

/** A refusal from what the bookings' routes threw. `clash` is the booking in the way, as
 *  `bookingClash` (lib/api.ts) reads it from the error. */
export function refusalOf(e: unknown, clash: RehearsalBlock | null, zone: string): Refusal {
  const message = e instanceof Error ? e.message : String(e);
  const field = e && typeof e === 'object' && 'field' in e ? (e as { field: unknown }).field : null;
  const about = field === 'start_ms' ? 'from' : field === 'end_ms' ? 'until' : field === 'show' ? 'show' : null;
  return { message, about, clash: clash ? { id: rehearsalId(clash.id), start: clash.start_ms, words: inTheWayWords(clash, zone) } : null };
}

/** The booking that a stretch would share time with, before the station is asked: touching is
 *  not sharing, and the booking being changed is not in its own way. */
export function sharesWith(taken: readonly Rehearsal[], start: number, end: number, except: string | null = null): Rehearsal | null {
  return taken.find((t) => t.kind === 'booking' && t.id !== except && t.start < end && start < t.end) ?? null;
}
