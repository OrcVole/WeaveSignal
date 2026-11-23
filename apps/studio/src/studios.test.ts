// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Studios as columns and the rehearsal studio's bookings in the Schedule tab (1.3.0, issue 24;
// the project folder's `docs/reference/design/rehearsal-mode-build-plan.md`, section 2), tested
// without a station or a calendar library: which columns a station has and what the view says
// when one is missing, who may book for which show, what the booking editor sends and what it
// refuses before the station is asked, the Rehearsal column's blocks and their words, the
// Studios view in the tab's store, and the tab and the editor rendered on the server in each
// of their states. The design's own day: Wednesday 25 November 2026, The Morning Kettle on air
// while Finlay Grant's Steeped is booked to rehearse at 13:30.
//
// Nothing in the rehearsal studio reaches the air, so none of its words is ON AIR or OPEN and
// nothing of it is drawn as a lamp (ADR-0062): the tests look for that in every state.
import { render } from 'svelte/server';
import { beforeEach, describe, expect, it } from 'vitest';
import Schedule from './schedule/Schedule.svelte';
import RehearsalBooking from './schedule/RehearsalBooking.svelte';
import { rehearsalAria, rehearsalClasses, rehearsalHtml, studioHeadHtml } from './schedule/blockHtml.ts';
import { instant, isRehearsalId, rehearsalBlocks, weekOf, type Block, type Rehearsal } from './schedule/model.ts';
import {
  NO_SHOW_TO_BOOK,
  REHEARSAL_OFF,
  STUDIOS_OLDER,
  airsAfterWords,
  bookableShows,
  bookingAsk,
  bookingChange,
  columnCaption,
  fieldsOf,
  firstBooking,
  fromDrag,
  mayBook,
  offeredShow,
  readOnlyWhy,
  refusalOf,
  rehearsalKind,
  sessionWords,
  sharesWith,
  stretchOf,
  stretchWords,
  studiosState,
  type Refusal,
} from './schedule/studios.ts';
import { FieldError, bookingClash, type Me, type RehearsalBlock, type StudioColumn } from './lib/api.ts';
import type { Show } from './lib/schedule.ts';
import { account } from './lib/account.svelte.ts';
import { studio } from './lib/studio.svelte.ts';
import { NEW_BOOKING, beginBooking, closeOpen, keyView, noteUnsaved, openDay, sched, select, setView, shownRehearsal, step, weekRehearsals } from './lib/schedule.svelte.ts';

const ZONE = 'Europe/London';
const at = (d: number, hh: number, mm = 0) => instant({ y: 2026, m: 11, d, hh, mm }, ZONE);
/** Wednesday 25 November 2026, 12:10: The Morning Kettle is on air. */
const NOW = at(25, 12, 10);
const { days } = weekOf(NOW, ZONE);
const WED = 2;

const show = (slug: string, name: string, colour_slot: number, presenter_account: string | null = null): Show => ({
  id: slug,
  slug,
  name,
  description: '',
  colour_slot,
  layout: 'poster',
  slot: null,
  hosts: [],
  tags: [],
  explicit: false,
  feed_guid: null,
  cards: null,
  presenter: '',
  contact: '',
  links: [],
  commercial_music: false,
  presenter_account,
});
const KETTLE = show('the-morning-kettle', 'The Morning Kettle', 4, 'acc-eilidh');
const STEEPED = show('steeped', 'Steeped', 9, 'acc-finlay');
const DRIVE = show('kettle-drive', 'Kettle Drive', 7);
const SHOWS = [KETTLE, STEEPED, DRIVE];

const staff = (id: string, name: string, role: Me['role']): Me => ({ kind: 'staff', role, account: { id, username: id, name, role, disabled_ms: null, created_ms: 0 } });
const FINLAY = staff('acc-finlay', 'Finlay Grant', 'presenter');
const EILIDH = staff('acc-eilidh', 'Eilidh Fraser-MacLeod', 'presenter');
const KIRSTY = staff('acc-kirsty', 'Kirsty Wilson', 'producer');
const GREGOR = staff('acc-gregor', 'Gregor Stewart', 'screener');
const KEY: Me = { kind: 'station', role: 'admin' };

const COLUMNS: StudioColumn[] = [
  { id: 'studio', name: 'Studio' },
  { id: 'rehearsal', name: 'Rehearsal' },
];
/** Steeped booked 13:30 to 14:00 by Finlay; a session of The Morning Kettle that morning. */
const BOOKED: RehearsalBlock = { id: 'b1', kind: 'booking', show: 'steeped', show_name: 'Steeped', colour_slot: 9, start_ms: at(25, 13, 30), end_ms: at(25, 14), by: 'Finlay', open: false, ended: null };
const REHEARSED: RehearsalBlock = { id: 's1', kind: 'session', show: 'the-morning-kettle', show_name: 'The Morning Kettle', colour_slot: 4, start_ms: at(25, 8, 2), end_ms: at(25, 8, 40), by: 'Eilidh', open: false, ended: 'moved' };
const OPEN_NOW: RehearsalBlock = { id: 's2', kind: 'session', show: 'steeped', show_name: 'Steeped', colour_slot: 9, start_ms: at(25, 11, 58), end_ms: at(25, 12, 1), by: 'Finlay', open: true, ended: null };

/** An on-air block of the week on screen, for what a booking is beside. */
const onAir = (s: Show, d: number, from: number, to: number): Block => ({ id: `ep:${s.slug}-${d}`, kind: 'episode', start: at(d, from), end: at(d, to), show: s.name, slug: s.slug, colour: s.colour_slot, series: 1, number: 1, title: '', hosts: [], weekly: true, aired: false, frozen: false, conflict: false, moved: null, clock: null, studio: 'studio', from: 'room' });
const AIR = [onAir(KETTLE, 25, 10, 14), onAir(STEEPED, 25, 14, 16)];

const [booking, rehearsed] = rehearsalBlocks([BOOKED, REHEARSED], NOW);
const openNow = rehearsalBlocks([OPEN_NOW], NOW)[0];

/** No word of the rehearsal studio is a word of the air. */
const neverOnAir = (html: string) => {
  expect(html).not.toMatch(/on[ -]air/i);
  expect(html).not.toMatch(/\bOPEN\b/);
  expect(html).not.toMatch(/g-onair|fill-onair|g-open|flag onair/);
};

describe('the columns a station has', () => {
  it('none at an older station, the one where the rehearsal studio is off, both where it is on', () => {
    expect(studiosState(null)).toBe('older');
    expect(studiosState([])).toBe('older');
    expect(studiosState([COLUMNS[0]])).toBe('off');
    expect(studiosState(COLUMNS)).toBe('both');
    // Until the station has answered, columns not sent are not columns it lacks.
    expect(studiosState(null, false)).toBe('unknown');
    expect(studiosState(COLUMNS, false)).toBe('both');
  });

  it('says what each column is, and never that the rehearsal studio is on air', () => {
    expect(columnCaption('studio')).toBe('On air by the schedule');
    expect(columnCaption('rehearsal')).toBe('Never on air');
    expect(columnCaption('a-third-studio')).toBe('');
    expect(STUDIOS_OLDER).toContain('older than 1.3.0');
    expect(REHEARSAL_OFF).toContain('off at this station');
  });

  it('heads the Rehearsal column with Book… for whoever may, and says why for whoever may not', () => {
    const may = studioHeadHtml(COLUMNS[1], { book: true });
    expect(may).toContain('<b>Rehearsal</b><span class="cap">Never on air</span>');
    expect(may).toMatch(/<button type="button" class="act2 bookbtn" data-book aria-label="Book the rehearsal studio…" title="[^"]+">Book…<\/button>/);
    const not = studioHeadHtml(COLUMNS[1], { bookWhy: NO_SHOW_TO_BOOK });
    expect(not).not.toContain('data-book');
    expect(not).toContain('To read');
    expect(not).toContain('title="Only an admin, a producer or a show’s presenter books the rehearsal studio.');
    // The Studio column has no button: shows are made with New show.
    expect(studioHeadHtml(COLUMNS[0], { book: true })).toBe('<div class="studiohd" data-studio="studio"><span class="nm"><b>Studio</b><span class="cap">On air by the schedule</span></span></div>');
    // A studio's name is the station's, and is escaped.
    expect(studioHeadHtml({ id: 'x', name: '<i>B</i>' })).toContain('&lt;i&gt;B&lt;/i&gt;');
  });
});

describe('who may book the rehearsal studio', () => {
  it('an admin, a producer, the station key, and the show’s own presenter: nobody else', () => {
    expect(mayBook(KEY, STEEPED)).toBe(true);
    expect(mayBook(staff('a', 'Morag Campbell', 'admin'), STEEPED)).toBe(true);
    expect(mayBook(KIRSTY, STEEPED)).toBe(true);
    expect(mayBook(FINLAY, STEEPED)).toBe(true);
    expect(mayBook(FINLAY, KETTLE)).toBe(false);
    expect(mayBook(FINLAY, DRIVE)).toBe(false);
    expect(mayBook(GREGOR, STEEPED)).toBe(false);
    // A window the station has not named is as the key was; a show that is gone is nobody's.
    expect(mayBook(null, STEEPED)).toBe(true);
    expect(mayBook(KEY, null)).toBe(false);
  });

  it('offers a presenter their own shows only, by name, and a screener none', () => {
    expect(bookableShows(KIRSTY, SHOWS).map((s) => s.name)).toEqual(['Kettle Drive', 'Steeped', 'The Morning Kettle']);
    expect(bookableShows(FINLAY, SHOWS).map((s) => s.slug)).toEqual(['steeped']);
    expect(bookableShows(GREGOR, SHOWS)).toEqual([]);
  });
});

describe('what of a booking may change', () => {
  it('all of one still to come, only the end of one under way, nothing of one that is over', () => {
    expect(bookingChange(booking, NOW, true)).toBe('all');
    expect(bookingChange(booking, at(25, 13, 45), true)).toBe('end');
    expect(bookingChange(booking, at(25, 14), true)).toBe('none');
    expect(bookingChange(booking, NOW, false)).toBe('none');
    // A session is the studio as it was used: never changed from here, open or over.
    expect(bookingChange(rehearsed, NOW, true)).toBe('none');
    expect(bookingChange(openNow, NOW, true)).toBe('none');
  });

  it('says why something is only to read', () => {
    expect(readOnlyWhy(booking, NOW, true)).toBeNull();
    expect(readOnlyWhy(booking, at(25, 13, 45), true)).toBeNull();
    expect(readOnlyWhy(booking, at(25, 15), true)).toBe('This booking is over: it stays in the schedule as it was.');
    expect(readOnlyWhy(booking, NOW, false)).toBe('Only an admin, a producer or Steeped’s presenter changes this booking.');
    expect(readOnlyWhy(rehearsed, NOW, true)).toBe('A rehearsal that has happened stays in the schedule as it was.');
    expect(readOnlyWhy(openNow, NOW, true)).toContain('run from the Desk tab');
  });
});

describe('the booking editor’s fields', () => {
  const f = fieldsOf('steeped', at(25, 13, 30), at(25, 14), ZONE);

  it('holds a stretch as a date and two times, station time, and reads it back', () => {
    expect(f).toEqual({ show: 'steeped', date: '2026-11-25', from: '13:30', until: '14:00' });
    expect(stretchOf(f, ZONE)).toEqual({ start: at(25, 13, 30), end: at(25, 14) });
    // An end at or before the start is the day after.
    expect(stretchOf({ ...f, from: '23:00', until: '00:30' }, ZONE)).toEqual({ start: at(25, 23), end: at(26, 0, 30) });
    // On the night the clocks go back the day after is 25 hours on, by the calendar.
    const night = stretchOf({ date: '2026-10-24', from: '23:00', until: '22:00' }, 'Europe/London')!;
    expect((night.end - night.start) / 3_600_000).toBe(24);
    expect(stretchOf({ ...f, until: '' }, ZONE)).toBeNull();
    expect(stretchOf({ ...f, date: '' }, ZONE)).toBeNull();
    expect(stretchWords(at(25, 13, 30), at(25, 14), ZONE)).toBe('Wednesday 25 November, 13:30 to 14:00');
    expect(stretchWords(at(25, 23), at(26, 0, 30), ZONE)).toBe('Wednesday 25 November, 23:00 to 00:30 the next day');
    expect(stretchWords(at(25, 22), at(26, 0), ZONE)).toBe('Wednesday 25 November, 22:00 to 00:00');
  });

  it('sends a whole booking when it is new', () => {
    expect(bookingAsk(f, ZONE, NOW)).toEqual({ request: { show: 'steeped', start_ms: at(25, 13, 30), end_ms: at(25, 14) }, patch: { show: 'steeped', start_ms: at(25, 13, 30), end_ms: at(25, 14) } });
  });

  it('refuses what the station refuses, in its words, and says which field it is about', () => {
    expect(bookingAsk({ ...f, show: '' }, ZONE, NOW)).toEqual({ problem: 'Choose the show the studio is held for.', about: 'show' });
    expect(bookingAsk({ ...f, until: '' }, ZONE, NOW)).toEqual({ problem: 'Give a date and both times, such as 13:30 and 14:00.', about: 'until' });
    expect(bookingAsk({ ...f, until: '13:30' }, ZONE, NOW)).toEqual({ problem: 'A booking ends after it starts.', about: 'until' });
    expect(bookingAsk({ ...f, until: '13:33' }, ZONE, NOW)).toEqual({ problem: 'A booking is at least five minutes long.', about: 'until' });
    expect(bookingAsk({ ...f, from: '09:00', until: '10:00' }, ZONE, NOW)).toEqual({ problem: 'A booking starts from now on: the past cannot be booked.', about: 'from' });
    // A minute back is let through: the time it took to fill in the editor.
    expect('request' in bookingAsk({ ...f, from: '12:10', until: '13:00' }, ZONE, NOW + 50_000)).toBe(true);
    // Five minutes to a day: 13:30 to 13:30 the next day is a day, and no booking is longer.
    expect('request' in bookingAsk({ ...f, until: '13:35' }, ZONE, NOW)).toBe(true);
    expect('request' in bookingAsk({ ...f, from: '13:30', until: '13:29' }, ZONE, NOW)).toBe(true);
  });

  it('sends only what changed of a booking, and nothing when nothing did', () => {
    expect(bookingAsk(f, ZONE, NOW, booking)).toEqual({ same: true });
    const later = bookingAsk({ ...f, until: '14:30' }, ZONE, NOW, booking);
    expect('patch' in later && later.patch).toEqual({ end_ms: at(25, 14, 30) });
    const other = bookingAsk({ ...f, show: 'kettle-drive', from: '13:00' }, ZONE, NOW, booking);
    expect('patch' in other && other.patch).toEqual({ show: 'kettle-drive', start_ms: at(25, 13) });
    expect('request' in other && other.request).toEqual({ show: 'kettle-drive', start_ms: at(25, 13), end_ms: at(25, 14) });
  });

  it('keeps the start of a booking under way, whatever the fields say, and its end is from now on', () => {
    const now = at(25, 13, 45);
    const longer = bookingAsk({ ...f, from: '13:40', until: '14:15' }, ZONE, now, booking);
    expect('patch' in longer && longer.patch).toEqual({ end_ms: at(25, 14, 15) });
    expect('request' in longer && longer.request.start_ms).toBe(at(25, 13, 30));
    expect(bookingAsk({ ...f, until: '13:40' }, ZONE, now, booking)).toEqual({ problem: 'A booking ends from now on.', about: 'until' });
  });

  it('starts a new booking on the next half hour today, at ten on a day to come, clear of a booking in the way', () => {
    const day = (d: number) => ({ from: days[d], to: days[d + 1] });
    expect(firstBooking(day(WED), NOW, ZONE, [])).toEqual({ start: at(25, 12, 30), end: at(25, 13, 30) });
    // 12:30 to 13:30 would run into Steeped's 13:30? It touches, and touching is not sharing.
    expect(firstBooking(day(WED), NOW, ZONE, [booking])).toEqual({ start: at(25, 12, 30), end: at(25, 13, 30) });
    expect(firstBooking(day(WED), at(25, 12, 40), ZONE, [booking])).toEqual({ start: at(25, 14), end: at(25, 15) });
    expect(firstBooking(day(WED), at(25, 12, 30), ZONE, [])).toEqual({ start: at(25, 13), end: at(25, 14) });
    expect(firstBooking(day(WED + 1), NOW, ZONE, [])).toEqual({ start: at(26, 10), end: at(26, 11) });
    // A session is not in a booking's way: only a booking holds the studio ahead.
    expect(firstBooking(day(WED), at(25, 7, 50), ZONE, [rehearsed])).toEqual({ start: at(25, 8), end: at(25, 9) });
    // A day that has gone cannot be booked.
    expect(firstBooking(day(WED - 1), NOW, ZONE, [])).toBeNull();
    expect(firstBooking(day(WED), at(25, 23, 45), ZONE, [])).toBeNull();
  });

  it('takes a drag from now on, and none that is all in the past', () => {
    expect(fromDrag(at(25, 13), at(25, 14), NOW)).toEqual({ start: at(25, 13), end: at(25, 14) });
    expect(fromDrag(at(25, 12), at(25, 13), NOW)).toEqual({ start: at(25, 12, 10), end: at(25, 13) });
    expect(fromDrag(at(25, 12), at(25, 13), at(25, 12, 11))).toEqual({ start: at(25, 12, 15), end: at(25, 13) });
    expect(fromDrag(at(25, 11), at(25, 12), NOW)).toBeNull();
    expect(fromDrag(at(25, 12), at(25, 12, 30), at(25, 12, 27))).toBeNull();
  });

  it('offers the show that goes on air soonest after the booking, among those the window may book', () => {
    expect(offeredShow(bookableShows(KIRSTY, SHOWS), AIR, at(25, 13, 30))).toBe('steeped');
    expect(offeredShow(bookableShows(KIRSTY, SHOWS), AIR, at(25, 9))).toBe('the-morning-kettle');
    // Nothing of theirs airs after it in the week on screen: the first by name.
    expect(offeredShow(bookableShows(KIRSTY, SHOWS), AIR, at(25, 18))).toBe('kettle-drive');
    expect(offeredShow(bookableShows(EILIDH, SHOWS), AIR, at(25, 13, 30))).toBe('the-morning-kettle');
    expect(offeredShow([], AIR, at(25, 13, 30))).toBe('');
  });

  it('says when the booked show goes on air after it, within the twelve hours a move needs', () => {
    expect(airsAfterWords('steeped', 'Steeped', at(25, 14), AIR, ZONE)).toBe('Steeped is on air at 14:00, as this ends.');
    expect(airsAfterWords('steeped', 'Steeped', at(25, 13, 30), AIR, ZONE)).toBe('Steeped is on air at 14:00, 30 min after this ends.');
    expect(airsAfterWords('steeped', 'Steeped', at(25, 15), AIR, ZONE)).toBe('');
    expect(airsAfterWords('kettle-drive', 'Kettle Drive', at(25, 13), AIR, ZONE)).toBe('');
    expect(airsAfterWords('steeped', 'Steeped', at(24, 23), AIR, ZONE)).toBe('');
  });

  it('sees a booking in the way before the station is asked; touching is not sharing', () => {
    const taken = [booking, rehearsed];
    expect(sharesWith(taken, at(25, 13), at(25, 13, 45))?.show).toBe('Steeped');
    expect(sharesWith(taken, at(25, 13), at(25, 13, 30))).toBeNull();
    expect(sharesWith(taken, at(25, 14), at(25, 15))).toBeNull();
    // The booking being changed is not in its own way, and a session is in nobody's.
    expect(sharesWith(taken, at(25, 13, 30), at(25, 14, 30), booking.id)).toBeNull();
    expect(sharesWith(taken, at(25, 8), at(25, 9))).toBeNull();
  });

  it('reads the station’s refusal: its sentence, the field it names, and the booking in the way', () => {
    const clash = new FieldError(409, 'That time is booked for Kettle Drive.', null, { error: 'That time is booked for Kettle Drive.', clash: { ...BOOKED, id: 'b9', show: 'kettle-drive', show_name: 'Kettle Drive', start_ms: at(25, 13), end_ms: at(25, 14) } });
    expect(refusalOf(clash, bookingClash(clash), ZONE)).toEqual({ message: 'That time is booked for Kettle Drive.', about: null, clash: { id: 'reh:b9', start: at(25, 13), words: 'Kettle Drive has the studio from 13:00 to 14:00.' } });
    const times = new FieldError(422, 'A booking starts from now on: the past cannot be booked.', 'start_ms');
    expect(refusalOf(times, bookingClash(times), ZONE)).toEqual({ message: 'A booking starts from now on: the past cannot be booked.', about: 'from', clash: null });
    expect(refusalOf(new FieldError(422, 'A booking is at most a day long.', 'end_ms'), null, ZONE).about).toBe('until');
    expect(refusalOf(new FieldError(403, 'Only an admin, a producer or the show’s presenter books the rehearsal studio for it.', null), null, ZONE).about).toBeNull();
    expect(refusalOf(new Error('the station did not answer'), null, ZONE).message).toBe('the station did not answer');
  });
});

describe('the Rehearsal column’s blocks', () => {
  it('are the station’s bookings and sessions, a session still open running to now', () => {
    const list = rehearsalBlocks([REHEARSED, OPEN_NOW, BOOKED], NOW);
    expect(list.map((r) => [r.id, r.kind, r.show, r.by])).toEqual([
      ['reh:b1', 'booking', 'Steeped', 'Finlay'],
      ['reh:s1', 'session', 'The Morning Kettle', 'Eilidh'],
      ['reh:s2', 'session', 'Steeped', 'Finlay'],
    ]);
    const [b, s, o] = list;
    expect([b.past, b.open, b.ended, b.colour]).toEqual([false, false, null, 9]);
    expect([s.past, s.open, s.ended]).toEqual([true, false, 'moved']);
    // The station's end for an open session was the moment the week was read: it runs to now.
    expect([o.past, o.open, o.ended, o.end]).toEqual([false, true, null, NOW]);
    expect(rehearsalBlocks([OPEN_NOW], at(25, 12, 40))[0].end).toBe(at(25, 12, 40));
    expect(rehearsalBlocks([BOOKED], at(25, 14))[0].past).toBe(true);
    // A thirteenth bank has no slot of the twelve: the block takes the de-emphasis colour.
    expect(rehearsalBlocks([{ ...BOOKED, colour_slot: 13 }], NOW)[0].colour).toBeNull();
    expect(isRehearsalId('reh:b1')).toBe(true);
    expect(isRehearsalId('ep:b1')).toBe(false);
    expect(isRehearsalId(null)).toBe(false);
  });

  it('say what they are: booked, rehearsed, rehearsing now', () => {
    expect(rehearsalKind(booking)).toBe('Booked');
    expect(rehearsalKind({ ...booking, past: true })).toBe('Was booked');
    expect(rehearsalKind(rehearsed)).toBe('Rehearsed · moved to live');
    expect(rehearsalKind({ ...rehearsed, ended: 'idle' })).toBe('Rehearsed');
    expect(rehearsalKind(openNow)).toBe('Rehearsing now');
    expect(sessionWords(rehearsed, ZONE)).toBe('Opened by Eilidh at 08:02. At 08:40 everyone moved to the live studio, standing by until the handover.');
    expect(sessionWords({ ...rehearsed, ended: 'ended' }, ZONE)).toBe('Opened by Eilidh at 08:02 and ended at 08:40.');
    expect(sessionWords({ ...rehearsed, ended: 'idle', by: '' }, ZONE)).toBe('Opened at 08:02. It ended by itself at 08:40, with nobody left in it.');
    expect(sessionWords(openNow, ZONE)).toBe('Opened by Finlay at 11:58, and still open.');
  });

  it('draws a booking hatched, a session plain, the past muted, and none of them as on air', () => {
    const ctx = { now: NOW, zone: ZONE, selected: 'reh:b1' };
    expect(rehearsalClasses(booking, ctx)).toEqual(['ev', 'reh', 'booking', 'sel']);
    expect(rehearsalClasses(rehearsed, ctx)).toEqual(['ev', 'reh', 'session', 'past']);
    expect(rehearsalClasses(openNow, { ...ctx, tiny: true, short: true })).toEqual(['ev', 'reh', 'session', 'open', 'short', 'tiny']);
    expect(rehearsalClasses({ ...booking, id: NEW_BOOKING }, { ...ctx, selected: NEW_BOOKING, draft: true })).toEqual(['ev', 'reh', 'booking', 'draft', 'sel']);
    const b = rehearsalHtml(booking, ctx);
    expect(b).toContain('<span class="num">13:30–14:00</span>');
    expect(b).toContain('<b>Steeped</b>');
    expect(b).toMatch(/<span class="kind"><svg[^>]*>.*<\/svg>Booked<\/span><span class="who">by Finlay<\/span>/);
    expect(rehearsalHtml(rehearsed, ctx)).toContain('Rehearsed · moved to live');
    // Half an hour is one line tall: what it is stands on that line, and who by is the tooltip's.
    const tiny = rehearsalHtml(booking, { ...ctx, tiny: true, short: true });
    expect(tiny).toMatch(/<b>Steeped<\/b><span class="kind inl"><svg[^>]*>.*<\/svg>Booked<\/span><\/div>$/);
    expect(tiny).not.toContain('class="rest"');
    expect(tiny).not.toContain('by Finlay');
    // A session under way has no end yet, and is not "live", "on air" or "open".
    const o = rehearsalHtml(openNow, ctx);
    expect(o).toContain('<span class="num">11:58–now</span>');
    expect(o).toContain('Rehearsing now');
    expect(o).not.toMatch(/\blive\b/i);
    for (const html of [b, o, rehearsalHtml(rehearsed, ctx)]) neverOnAir(html);
    for (const r of [booking, rehearsed, openNow]) expect(rehearsalClasses(r, ctx).join(' ')).not.toMatch(/onair|conflict|auto|rule/);
    // The booking being made says so, and names nobody yet.
    const d = rehearsalHtml({ ...booking, id: NEW_BOOKING, by: '', show: '' }, { ...ctx, draft: true });
    expect(d).toContain('New booking');
    expect(d).toContain('<b>No show yet</b>');
    expect(d).not.toContain('class="who"');
    // A show's name is the station's, and is escaped.
    expect(rehearsalHtml({ ...booking, show: '<b>x</b>', by: '<i>' }, ctx)).toContain('&lt;b&gt;x&lt;/b&gt;');
    expect(rehearsalHtml({ ...booking, show: '<b>x</b>', by: '<i>' }, ctx)).toContain('by &lt;i&gt;');
  });

  it('tells a screen reader the same, with the times a short block has no room for', () => {
    const ctx = { now: NOW, zone: ZONE, selected: null };
    expect(rehearsalAria(booking, ctx)).toBe('Steeped, the rehearsal studio booked, 13:30 to 14:00, by Finlay');
    expect(rehearsalAria({ ...booking, past: true }, ctx)).toBe('Steeped, the rehearsal studio was booked, 13:30 to 14:00, by Finlay');
    expect(rehearsalAria(rehearsed, ctx)).toBe('The Morning Kettle, a rehearsal. Opened by Eilidh at 08:02. At 08:40 everyone moved to the live studio, standing by until the handover.');
    expect(rehearsalAria(openNow, ctx)).toBe('Steeped, a rehearsal under way. Opened by Finlay at 11:58, and still open.');
    expect(rehearsalAria({ ...booking, id: NEW_BOOKING }, { ...ctx, draft: true })).toBe('A new booking of the rehearsal studio for Steeped, 13:30 to 14:00, not saved yet');
    for (const r of [booking, rehearsed, openNow]) neverOnAir(rehearsalAria(r, ctx));
  });
});

/** The tab's store as the design's Wednesday has it. */
function seed(o: Partial<typeof sched> = {}, me: Me | null = KIRSTY) {
  Object.assign(sched, {
    zone: ZONE,
    anchor: NOW,
    view: 'week',
    rota: false,
    selected: null,
    booking: null,
    pane: 'episode',
    editShow: null,
    showTab: 'dates',
    episodes: [],
    dates: null,
    hours: [],
    clocks: [],
    defaultClock: null,
    shows: SHOWS,
    studios: COLUMNS,
    rehearsals: [BOOKED, REHEARSED],
    rules: true,
    loading: false,
    error: '',
    notClosed: null,
    reveal: null,
    ...o,
  });
  account.me = me;
  account.loaded = true;
  // The studio's clock reads the design's Wednesday.
  Object.assign(studio, { tab: 'schedule', now: NOW, skew: 0, view: null });
}

describe('the Studios view in the tab’s store', () => {
  beforeEach(() => seed());

  it('opens on a day, as the Day view does, and the arrows step a day', () => {
    setView('studios', days[WED] + 1000);
    expect([sched.view, sched.rota]).toEqual(['studios', false]);
    expect(sched.anchor).toBe(days[WED] + 12 * 3_600_000);
    step(1);
    expect([sched.view, sched.anchor]).toEqual(['studios', days[WED + 1] + 12 * 3_600_000]);
    step(-2);
    expect(sched.anchor).toBe(days[WED - 1] + 12 * 3_600_000);
  });

  it('reads the week again on the way in: a rehearsal opens and ends without the tab being told', () => {
    const read = sched.version;
    setView('studios', NOW);
    expect(sched.version).toBe(read + 1);
    // Another day of the same week is not another read.
    step(1);
    expect(sched.version).toBe(read + 1);
    // Sunday to the next Monday is another week, read once.
    setView('studios', days[6] + 1000);
    step(1);
    expect(sched.version).toBe(read + 2);
  });

  it('from the toolbar it opens on today in this week, and keeps the day between Day and Studios', () => {
    keyView('studios', NOW);
    expect([sched.view, sched.anchor]).toEqual(['studios', days[WED] + 12 * 3_600_000]);
    step(2);
    keyView('day', NOW);
    expect([sched.view, sched.anchor]).toEqual(['day', days[WED + 2] + 12 * 3_600_000]);
    keyView('studios', NOW);
    expect([sched.view, sched.anchor]).toEqual(['studios', days[WED + 2] + 12 * 3_600_000]);
    keyView('week', NOW);
    expect(sched.view).toBe('week');
  });

  it('begins a booking as the selection, and holds it only while it is the selection', () => {
    setView('studios', NOW);
    beginBooking('steeped', at(25, 15), at(25, 16));
    expect([sched.selected, sched.pane, sched.booking]).toEqual([NEW_BOOKING, 'episode', { show: 'steeped', start: at(25, 15), end: at(25, 16) }]);
    expect(shownRehearsal(weekRehearsals(NOW), days, WED)).toBe(NEW_BOOKING);
    select('reh:b1');
    expect([sched.selected, sched.booking]).toEqual(['reh:b1', null]);
    expect(shownRehearsal(weekRehearsals(NOW), days, WED)).toBe('reh:b1');
    // On another day the grid does not draw it, so the editor column does not hold it.
    expect(shownRehearsal(weekRehearsals(NOW), days, WED + 1)).toBeNull();
    select('reh:gone');
    expect(shownRehearsal(weekRehearsals(NOW), days, WED)).toBeNull();
  });

  it('drops the Rehearsal column’s selection and the booking being made when the view is left', () => {
    setView('studios', NOW);
    beginBooking('steeped', at(25, 15), at(25, 16));
    setView('week');
    expect([sched.view, sched.selected, sched.booking]).toEqual(['week', null, null]);
    setView('studios', NOW);
    select('reh:b1');
    openDay(NOW);
    expect([sched.view, sched.selected]).toEqual(['day', null]);
    // The week draws nothing of the rehearsal studio, so nothing of it is ever shown there.
    sched.selected = 'reh:b1';
    expect(shownRehearsal(weekRehearsals(NOW), days, WED)).toBeNull();
    // A show that is selected is kept, as ever.
    setView('studios', NOW);
    select('ep:something');
    setView('week');
    expect(sched.selected).toBe('ep:something');
  });

  it('Esc closes a booking, and never over changes that are not saved', () => {
    setView('studios', NOW);
    select('reh:b1');
    expect(closeOpen(NOW)).toEqual({ did: 'closed', block: 'reh:b1', show: null });
    expect(sched.selected).toBeNull();
    beginBooking('steeped', at(25, 15), at(25, 16));
    const editor = {};
    noteUnsaved(editor, 'The booking');
    expect(closeOpen(NOW)).toEqual({ did: 'unsaved', what: 'The booking' });
    expect(sched.booking).not.toBeNull();
    noteUnsaved(editor, null);
    expect(closeOpen(NOW)).toEqual({ did: 'closed', block: NEW_BOOKING, show: null });
    expect([sched.selected, sched.booking]).toEqual([null, null]);
    expect(closeOpen(NOW)).toEqual({ did: 'nothing' });
  });
});

const text = (html: string) => html.replace(/<!--.*?-->/gs, '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();
const studiosButton = (body: string) => /<button[^>]*>Studios<\/button>/.exec(body)?.[0] ?? '';
const line = (body: string) => /<p class="studios-line"[^>]*>(.*?)<\/p>/s.exec(body)?.[1] ?? '';

describe('the Schedule tab with studios', () => {
  it('at an older station the Studios button waits, and says why', () => {
    seed({ studios: null, rehearsals: [] });
    const { body } = render(Schedule, { props: { live: false } });
    expect(studiosButton(body)).toContain('aria-disabled="true"');
    expect(studiosButton(body)).toContain('aria-pressed="false"');
    expect(studiosButton(body)).toContain('title="This station is older than 1.3.0: it has the one studio, and sends no columns to show."');
    expect(body).toContain('data-panel="calendar.week"');
    // Until the station has answered, nothing is said of its age.
    seed({ studios: null, rehearsals: [], rules: null });
    expect(studiosButton(render(Schedule, { props: { live: false } }).body)).not.toContain('aria-disabled');
  });

  it('with columns the button opens the view: one day, the day’s arrows, its own panel name', () => {
    seed();
    expect(studiosButton(render(Schedule, { props: { live: false } }).body)).not.toContain('aria-disabled');
    setView('studios', NOW);
    const { body } = render(Schedule, { props: { live: false } });
    expect(studiosButton(body)).toContain('aria-pressed="true"');
    expect(body).toContain('data-panel="calendar.studios"');
    expect(body).toContain('aria-label="Wednesday 25 November, a column for each studio"');
    expect(body).not.toContain('data-panel="calendar.week"');
    expect(body).not.toContain('data-panel="calendar.day"');
    expect(body).toContain('aria-label="Previous day"');
    expect(body).toContain('Wed 25 November 2026');
    // The Day and Week buttons are not pressed with it.
    expect(/<button[^>]*>Day<\/button>/.exec(body)?.[0]).toContain('aria-pressed="false"');
    // With a booking on the day and rights to book, the view has nothing to add under it.
    expect(body).not.toContain('class="studios-line"');
    // The editor column rests on the list of shows, as in every view.
    expect(body).toContain('data-panel="calendar.shows"');
  });

  it('where the rehearsal studio is off, says so under the one column, and how it is turned on', () => {
    seed({ studios: [COLUMNS[0]], rehearsals: [] });
    setView('studios', NOW);
    const { body } = render(Schedule, { props: { live: false } });
    expect(body).toContain('data-panel="calendar.studios"');
    expect(text(line(body))).toBe('The rehearsal studio is off at this station, so there is the one column. Whoever runs the installation turns it on: REHEARSAL=true in its settings, then a restart.');
  });

  it('for someone with no show of their own the Rehearsal column is to read, and the view says who books', () => {
    seed({}, GREGOR);
    setView('studios', NOW);
    const { body } = render(Schedule, { props: { live: false } });
    expect(text(line(body))).toBe(`The Rehearsal column is to read. ${NO_SHOW_TO_BOOK}`);
    // A presenter with a show of their own has nothing to be told.
    seed({}, FINLAY);
    setView('studios', NOW);
    expect(render(Schedule, { props: { live: false } }).body).not.toContain('class="studios-line"');
  });

  it('on a day with nothing booked it says how to begin, and on a day gone by that the past is not booked', () => {
    seed();
    setView('studios', days[WED + 1] + 1000);
    const free = render(Schedule, { props: { live: false } }).body;
    expect(text(line(free))).toBe('Nothing is booked in the rehearsal studio on Thursday 26 November. To hold it for a show, press Book…, or drag across the Rehearsal column. It airs nothing.');
    setView('studios', days[WED - 1] + 1000);
    const gone = render(Schedule, { props: { live: false } }).body;
    expect(text(line(gone))).toBe('Nothing was booked in the rehearsal studio on Tuesday 24 November. The past cannot be booked.');
  });

  it('opens a booking, the booking being made and a session in the editor column, under rehearsal.booking', () => {
    seed();
    setView('studios', NOW);
    select('reh:b1');
    let { body } = render(Schedule, { props: { live: false } });
    expect(body).toContain('data-panel="rehearsal.booking"');
    expect(body).not.toContain('data-panel="calendar.shows"');
    expect(body).toContain('Steeped · rehearsal booked');
    expect(body).toContain('Save booking');
    beginBooking('steeped', at(25, 15), at(25, 16));
    ({ body } = render(Schedule, { props: { live: false } }));
    expect(body).toContain('Book the rehearsal studio');
    expect(body).toContain('Book the studio');
    select('reh:s1');
    ({ body } = render(Schedule, { props: { live: false } }));
    expect(body).toContain('The Morning Kettle · rehearsed');
    expect(body).toContain('aria-label="A rehearsal"');
    expect(body).not.toContain('Save booking');
    // In the week the same selection opens nothing: the week does not draw it.
    seed();
    sched.selected = 'reh:b1';
    expect(render(Schedule, { props: { live: false } }).body).not.toContain('data-panel="rehearsal.booking"');
  });

  it('a booking for another presenter’s show is read-only in the tab', () => {
    seed({}, EILIDH);
    setView('studios', NOW);
    select('reh:b1');
    const { body } = render(Schedule, { props: { live: false } });
    expect(body).toContain('Only an admin, a producer or Steeped’s presenter changes this booking.');
    expect(body).not.toContain('Delete booking');
  });
});

describe('the booking editor', () => {
  const none = async () => null;
  const base = { zone: ZONE, now: NOW, may: true, shows: bookableShows(KIRSTY, SHOWS), taken: [booking, rehearsed], blocks: AIR, onSave: none, onDelete: none, onShows: () => {} };
  const draft = (o: Partial<Rehearsal> = {}) => ({ ...booking, id: NEW_BOOKING, ref: '', by: '', start: at(25, 15), end: at(25, 16), draft: true, ...o });

  it('new: the show, the day and the two times, and nothing booked until the press', () => {
    const { body } = render(RehearsalBooking, { props: { ...base, item: draft({ start: at(25, 12, 30), end: at(25, 13, 30) }) } });
    expect(body).toContain('<h2 data-panel-title="">Book the rehearsal studio</h2>');
    expect(text(body)).toContain('‹ Shows › Rehearsal studio › a new booking');
    expect(body).toContain('Wed 25 Nov 2026 · 12:30–13:30');
    for (const s of ['Kettle Drive', 'Steeped', 'The Morning Kettle']) expect(body).toContain(`>${s}</option>`);
    expect(body).toMatch(/<input class="date" type="date" value="2026-11-25" min="2026-11-25"/);
    expect(body).toContain('aria-label="From, station time"');
    expect(body).toContain('aria-label="Until, station time"');
    expect(text(body)).toContain('1 h. Station time.');
    expect(body).toMatch(/<button type="submit" class="btn primary"[^>]*>Book the studio<\/button>/);
    expect(body).toMatch(/<button type="button" class="btn quiet"[^>]*>Discard<\/button>/);
    expect(body).not.toContain('Delete booking');
    expect(text(body)).toContain('Not booked until Book the studio is pressed.');
    // Rehearsing is for the show about to air: it says when that is.
    expect(text(body)).toContain('Steeped is on air at 14:00, 30 min after this ends.');
    expect(text(body)).toContain('Nothing in it reaches the air.');
    expect(body).not.toContain('data-testid="booking-read-only"');
  });

  it('changing one: the same fields, Save and Discard waiting for a change, and Delete', () => {
    const { body } = render(RehearsalBooking, { props: { ...base, item: booking } });
    expect(body).toContain('<h2 data-panel-title="">Steeped · rehearsal booked</h2>');
    expect(text(body)).toContain('Rehearsal studio › a booking');
    expect(body).toMatch(/<button type="submit" class="btn primary" disabled[^>]*>Save booking<\/button>/);
    expect(body).toMatch(/<button type="button" class="btn quiet" disabled[^>]*>Discard<\/button>/);
    expect(body).toMatch(/<button type="button" class="btn del"[^>]*>Delete booking<\/button>/);
    expect(text(body)).toContain('Steeped is on air at 14:00, as this ends.');
    expect(text(body)).toContain('Drag the block in the Rehearsal column to move it');
    // Its own time is not in its own way.
    expect(body).not.toContain('data-testid="booking-shares"');
  });

  it('under way: the day and the start stay, and the end is what changes', () => {
    const { body } = render(RehearsalBooking, { props: { ...base, now: at(25, 13, 45), item: booking } });
    expect(body).toMatch(/<input class="date" type="date"[^>]*disabled/);
    expect(body).toMatch(/aria-label="From, station time"[^>]*disabled|disabled[^>]*aria-label="From, station time"/);
    expect(body).not.toMatch(/aria-label="Until, station time"[^>]*disabled|<input[^>]*disabled[^>]*aria-label="Until, station time"/);
    expect(text(body)).toContain('This booking is under way: its day and its start stay as they are.');
    expect(text(body)).toContain('Under way: shorten or lengthen it, give it to another show, or delete it to free the studio.');
    expect(body).toContain('Delete booking');
  });

  it('refused: the booking in the way, in the station’s block, with Show it', () => {
    const refused: Refusal = { message: 'That time is booked for Kettle Drive.', about: null, clash: { id: 'reh:b9', start: at(25, 15), words: 'Kettle Drive has the studio from 15:00 to 16:00.' } };
    const { body } = render(RehearsalBooking, { props: { ...base, item: draft(), refused, onShowIt: () => {} } });
    const alert = /<div class="problem serious" role="alert"[^>]*>(.*?)<\/div>\s*<\/div>/s.exec(body)?.[1] ?? '';
    expect(text(alert)).toContain('Not saved. Kettle Drive has the studio from 15:00 to 16:00.');
    expect(text(alert)).toContain('Two bookings cannot share the studio.');
    expect(alert).toContain('>Show it</button>');
    // The fields stay as typed, to be changed and sent again.
    expect(body).toContain('Book the studio');
  });

  it('refused for another reason: the station’s own sentence, on the field it names', () => {
    const refused: Refusal = { message: 'A booking starts from now on: the past cannot be booked.', about: 'from', clash: null };
    const { body } = render(RehearsalBooking, { props: { ...base, item: draft(), refused } });
    expect(text(body)).toContain('Not saved. A booking starts from now on: the past cannot be booked.');
    expect(body).toMatch(/aria-label="From, station time"[^>]*aria-invalid="true"|aria-invalid="true"[^>]*aria-label="From, station time"/);
    expect(body).not.toContain('>Show it</button>');
  });

  it('says a booking the week already shows in the way, before the station is asked', () => {
    const { body } = render(RehearsalBooking, { props: { ...base, item: draft({ start: at(25, 13), end: at(25, 14) }), onShowIt: () => {} } });
    const note = /<div class="problem" role="status"[^>]*>(.*?)<\/div>\s*<\/div>/s.exec(body)?.[1] ?? '';
    expect(text(note)).toContain('Steeped has the studio from 13:30 to 14:00. The station refuses two bookings at one time.');
    expect(note).toContain('>Show it</button>');
    // It is said, not enforced: the station decides, and the press stays.
    expect(body).toMatch(/<button type="submit" class="btn primary"[^>]*>Book the studio<\/button>/);
    expect(/<button type="submit"[^>]*>/.exec(body)?.[0]).not.toContain('disabled');
  });

  it('read-only: a booking that is over, and one for a show this window has no rights over', () => {
    const over = render(RehearsalBooking, { props: { ...base, now: at(25, 15), item: { ...booking, past: true } } }).body;
    expect(over).toContain('<h2 data-panel-title="">Steeped · was booked</h2>');
    expect(text(over)).toContain('This booking is over: it stays in the schedule as it was.');
    expect(text(over)).toContain('Booked by Finlay');
    expect(text(over)).toContain('To read');
    const theirs = render(RehearsalBooking, { props: { ...base, may: false, shows: [], item: booking } }).body;
    expect(text(theirs)).toContain('Only an admin, a producer or Steeped’s presenter changes this booking.');
    for (const body of [over, theirs]) {
      expect(body).not.toMatch(/<input|<select|type="submit"|Delete booking|Discard/);
      expect(text(body)).toContain('Only a booking still to come, or under way, can be changed.');
    }
  });

  it('read-only: a session as it happened, and one under way, which is run from the Desk tab', () => {
    const was = render(RehearsalBooking, { props: { ...base, item: rehearsed } }).body;
    expect(was).toContain('<h2 data-panel-title="">The Morning Kettle · rehearsed</h2>');
    expect(text(was)).toContain('Rehearsal studio › a rehearsal');
    expect(was).toContain('Wed 25 Nov 2026 · 08:02–08:40');
    expect(text(was)).toContain('As it happened');
    expect(text(was)).toContain('Opened by Eilidh at 08:02. At 08:40 everyone moved to the live studio, standing by until the handover.');
    expect(text(was)).toContain('A rehearsal that has happened stays in the schedule as it was.');
    expect(text(was)).toContain('Nothing in it reached the air.');
    const open = render(RehearsalBooking, { props: { ...base, item: openNow } }).body;
    expect(open).toContain('<h2 data-panel-title="">Steeped · rehearsing now</h2>');
    expect(open).toContain('Wed 25 Nov 2026 · 11:58–now');
    expect(text(open)).toContain('Opened by Finlay at 11:58, and still open.');
    expect(text(open)).toContain('open the Desk tab and choose the rehearsal studio there');
    for (const body of [was, open]) expect(body).not.toMatch(/<input|<select|type="submit"|Delete booking/);
  });

  it('in no state is it a lamp, red, ON AIR or OPEN', () => {
    const states = [
      { item: draft() },
      { item: booking },
      { item: booking, now: at(25, 13, 45) },
      { item: { ...booking, past: true }, now: at(25, 15) },
      { item: rehearsed },
      { item: openNow },
    ];
    for (const s of states) {
      const { body } = render(RehearsalBooking, { props: { ...base, ...s } });
      expect(body).not.toMatch(/fill-onair|g-onair|live-onair|flag onair|\bON AIR\b|\bOPEN\b/);
      // "on air" is said only of the show's own airing, never of the studio or the booking.
      for (const m of text(body).matchAll(/[^.]*\bon air\b[^.]*\./gi)) expect(m[0]).toMatch(/is on air at \d\d:\d\d/);
    }
  });
});
