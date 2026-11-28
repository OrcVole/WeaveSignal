// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Listener keys in the studio app (1.3.2, keyed shows), as pure rules: where a key stands now,
// the words for its window, limits and use, the forms' checks and the bodies they send, a
// pasted list read into recipients, the message a key is sent with, the leak report's words,
// and what a refusal of the whole screen means. The station stays the authority: these mirror
// its rules so a slip is caught before the round trip, and its own sentence is shown as given
// when it still refuses.
//
// The word is "key" everywhere. A listener key is not a show key (Settings, Own studios).

import { FieldError, type Me } from './api.ts';
import { emailProblem } from './account.ts';
import type { FamilySpec, KeyChange, KeyDevice, KeyedShow, KeyKind, KeyLeak, KeyLogEntry, KeyMedia, KeyRecipient, KeySpec, KeyTemplate, ListenerKey, Slate } from './keysApi.ts';

export const LABEL_MAX = 120;
export const NOTE_MAX = 500;
export const REASON_MAX = 200;
export const SHOWS_MAX = 32;
export const FAMILY_MAX = 1000;
/** How many days before its end a key "ends soon": the station's default (`keys.ends_soon_days`),
 *  until the station has said its own (`ks.signing.soonDays`). */
export const SOON_DAYS = 7;
const DAY = 86_400_000;

/** A producer, an admin or the station key manages keys; a presenter and a screener may not
 *  (keys hold names, email addresses and the means to listen), and the station refuses them. */
export function mayManageKeys(me: Me | null | undefined): boolean {
  return !!me && (me.kind === 'station' || me.role === 'admin' || me.role === 'producer');
}

/** Erasing a key and everything kept about its use is an admin's (ADR-0122). */
export function mayEraseKey(me: Me | null | undefined): boolean {
  return !!me && (me.kind === 'station' || me.role === 'admin');
}

// ---- where a key stands ----

/** What a key is now, for its chip: the station's state with its window and limits read in. */
export type Standing = 'works' | 'ends-soon' | 'not-started' | 'ended' | 'used-up' | 'paused' | 'replaced' | 'revoked';

export function standing(k: ListenerKey, now: number, soonDays = SOON_DAYS): Standing {
  if (k.state === 'revoked') return k.replaced_by_id ? 'replaced' : 'revoked';
  if (k.state === 'paused') return 'paused';
  if (k.valid_from_ms != null && now < k.valid_from_ms) return 'not-started';
  if (k.valid_until_ms != null && now >= k.valid_until_ms) return 'ended';
  if (k.max_listens != null && k.listens_used >= k.max_listens) return 'used-up';
  if (k.valid_until_ms != null && k.valid_until_ms - now <= soonDays * DAY) return 'ends-soon';
  return 'works';
}

/** A standing's word and chip. Nothing here is red or filled: a key is never on air. */
export const STANDING: Record<Standing, { word: string; cls: string; line: string }> = {
  works: { word: 'Works', cls: 'hollow hollow-good', line: 'Its holder can listen.' },
  'ends-soon': { word: 'Ends soon', cls: 'hollow hollow-warning', line: 'Its holder can listen, and is told when it ends.' },
  'not-started': { word: 'Not started', cls: 'hollow hollow-standby', line: 'Its holder is told when it opens.' },
  ended: { word: 'Ended', cls: 'hollow hollow-off', line: 'Its window is over. Extend it and it works again.' },
  'used-up': { word: 'Used up', cls: 'hollow hollow-off', line: 'Its listens are used. Raise the limit and it works again.' },
  paused: { word: 'Paused', cls: 'hollow hollow-standby dashed', line: 'Switched off for now, and kept. Resume brings it back.' },
  replaced: { word: 'Replaced', cls: 'hollow hollow-off', line: 'Ended, with a new key issued to the same person.' },
  revoked: { word: 'Revoked', cls: 'hollow hollow-off', line: 'Ended for good. It cannot be brought back.' },
};

/** How soon a change reaches someone who is listening: their player's next renewal, at the
 *  station's pace (`keys.renew_every_s`, a minute unless set otherwise). `null`: not known. A
 *  pause, a revoke or a replace stops them at once instead. */
export function cutWords(renewS: number | null | undefined): string {
  if (!renewS || renewS <= 0) return 'within a few minutes';
  if (renewS < 90) return renewS === 60 ? 'within a minute' : `within ${Math.round(renewS)} seconds`;
  const m = Math.round(renewS / 60);
  return `within ${m === 2 ? 'two' : m} minutes`;
}

/** Whether a key can still be changed: a revoked key cannot (the station answers 409). */
export const alive = (k: ListenerKey): boolean => k.state !== 'revoked';

/** How many of each standing, for the head of the list. */
export function tally(keys: readonly ListenerKey[], now: number, soonDays = SOON_DAYS): { works: number; paused: number; over: number } {
  let works = 0;
  let paused = 0;
  let over = 0;
  for (const k of keys) {
    const s = standing(k, now, soonDays);
    if (s === 'works' || s === 'ends-soon' || s === 'not-started') works += 1;
    else if (s === 'paused') paused += 1;
    else over += 1;
  }
  return { works, paused, over };
}

// ---- words ----

export const KIND_WORD: Record<KeyKind, string> = { personal: 'Personal', group: 'Group', machine: 'Machine' };
export const KIND_LINE: Record<KeyKind, string> = {
  personal: 'One named person: a member, a ticket holder, a reviewer.',
  group: 'Many people who share one key: a class, a club, a congregation. You set how many.',
  machine: 'A partner station’s software or another program, which redeems the key itself. No devices to count.',
};
/** What a key is for. A key for the picture carries the show's sound with it (the operator,
 *  2026-11-25: a video key also opens the show's sound), so `video` opens what `both` does and
 *  its words say so: there is no picture without the sound. The studio no longer offers
 *  `video` for a new key (1.3.3; ADR-0295); the word stays for the keys made with it, which go
 *  on working, and the station still takes it. */
export const MEDIA_WORD: Record<KeyMedia, string> = { both: 'Sound and picture', sound: 'Sound only', video: 'Picture, with its sound' };
/** What the public hears while a keyed show's window is open, as playout says it (the studio
 *  view's `keyed.public_hears`). */
export function publicHearsWords(kind: string | null | undefined): string {
  switch (kind) {
    case 'slate_file':
      return 'the station’s slate';
    case 'fallback':
      return 'the station’s fallback music';
    case 'default_loop':
      return 'the loop WeaveSignal ships';
    case 'silence':
      return 'silence, because the station has no slate or fallback files';
    case 'listener_fallback':
      return 'the listener process’s own fallback, because playout is not answering';
    default:
      return 'the station’s slate';
  }
}

export const SLATE_WORD: Record<Slate, string> = { announcement: 'an announcement', fallback: 'the fallback music', teaser: 'a teaser' };
export const SLATE_LINE: Record<Slate, string> = {
  announcement: 'An announcement that says when the public programme returns and how to get a key.',
  fallback: 'The station’s fallback music, as when nobody is on air.',
  teaser: 'A short teaser of the show.',
};

function fmt(zone: string, o: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  return new Intl.DateTimeFormat('en-GB', { ...o, ...(zone ? { timeZone: zone } : {}) });
}

/** `30 Nov 2026`, in the station's zone. */
export const dayWords = (ms: number, zone = ''): string => fmt(zone, { day: 'numeric', month: 'short', year: 'numeric' }).format(new Date(ms));
/** `30 Nov 2026, 22:00`, in the station's zone. */
export const timeWords = (ms: number, zone = ''): string => fmt(zone, { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit', hourCycle: 'h23' }).format(new Date(ms));

/** How long from now, roughly: `in 5 days`, `3 hours ago`. */
export function fromNow(ms: number, now: number): string {
  const d = ms - now;
  const a = Math.abs(d);
  const unit = a < 3_600_000 ? [Math.max(1, Math.round(a / 60_000)), 'minute'] : a < 2 * DAY ? [Math.round(a / 3_600_000), 'hour'] : [Math.round(a / DAY), 'day'];
  const words = `${unit[0]} ${unit[1]}${unit[0] === 1 ? '' : 's'}`;
  return d >= 0 ? `in ${words}` : `${words} ago`;
}

/** A key's window in a line: when it starts and when it ends. */
export function windowWords(k: Pick<ListenerKey, 'valid_from_ms' | 'valid_until_ms'>, now: number, zone = ''): string {
  const from = k.valid_from_ms ?? null;
  const until = k.valid_until_ms ?? null;
  const starts = from !== null && from > now ? `Opens ${timeWords(from, zone)}` : null;
  if (until === null) return starts ? `${starts} · no end` : 'No end';
  const ends = until <= now ? `Ended ${timeWords(until, zone)}` : `Ends ${timeWords(until, zone)}`;
  return starts ? `${starts} · ${ends[0].toLowerCase()}${ends.slice(1)}` : ends;
}

/** `28 Nov, 19:30`, with the year when it is not this one: for a row of the list. */
export function shortTime(ms: number, now: number, zone = ''): string {
  const year = (t: number) => fmt(zone, { year: 'numeric' }).format(new Date(t));
  return fmt(zone, { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit', hourCycle: 'h23', ...(year(ms) === year(now) ? {} : { year: 'numeric' }) }).format(new Date(ms));
}

/** A key's window for a row of the list: short, and the same facts as `windowWords`. */
export function windowShort(k: Pick<ListenerKey, 'valid_from_ms' | 'valid_until_ms'>, now: number, zone = ''): string {
  const from = k.valid_from_ms ?? null;
  const until = k.valid_until_ms ?? null;
  const later = from !== null && from > now;
  if (until === null) return later ? `From ${shortTime(from, now, zone)}` : 'No end';
  if (until <= now) return `Ended ${shortTime(until, now, zone)}`;
  return later ? `${shortTime(from, now, zone)} to ${shortTime(until, now, zone)}` : `Until ${shortTime(until, now, zone)}`;
}

const count = (n: number, one: string, many = `${one}s`): string => `${n} ${n === 1 ? one : many}`;

/** A key's limits in a line. A machine key has no devices to count. */
export function limitsWords(k: Pick<ListenerKey, 'kind' | 'max_devices' | 'max_listens' | 'max_people'>): string {
  const parts: string[] = [];
  if (k.kind !== 'machine') parts.push(k.max_devices == null ? 'any number of devices' : `${count(k.max_devices, 'device')} at once`);
  if (k.max_listens != null) parts.push(count(k.max_listens, 'listen'));
  if (k.kind === 'group') parts.push(k.max_people == null ? 'any number of people' : `up to ${count(k.max_people, 'person', 'people')}`);
  return parts.length ? parts.join(' · ') : 'No limits';
}

/** Where a keyed show's window stands, for the desk's *What the public hears now* (`desk.keyed`):
 *
 *  - `kept`: playout answered, and it is keeping the show from the public.
 *  - `not-kept`: playout answered, and it is not: everyone hears the show. The desk's alarm.
 *  - `not-confirmed` (1.3.2): playout did not answer the station's last look, so nothing it says
 *    stands behind either word. By design the listener process then plays its own fallback and
 *    not the show, which is why the station still reports `kept`; the desk says what should be
 *    so and that it is not confirmed, and raises no alarm it cannot back.
 *
 *  A station that does not send `confirmed` (before 1.3.2's last build) is read as confirmed. */
export type KeyedStanding = 'kept' | 'not-kept' | 'not-confirmed';
export function keyedStanding(k: { kept: boolean; confirmed?: boolean }): KeyedStanding {
  if (k.confirmed === false) return 'not-confirmed';
  return k.kept ? 'kept' : 'not-kept';
}

/** The words of the third state, after the show's name. */
export const NOT_CONFIRMED = {
  mark: 'not confirmed',
  body: 'is for people with a key. Playout is not answering the station, so this is not confirmed: the listeners’ stream should be playing the station’s own fallback, not the show.',
  act: 'It is asked again every two seconds. If this stays, restart playout, and see its log.',
} as const;

/** What a key opens, by the shows' names. `shows` is the station's keyed shows; a show the key
 *  names that is no longer keyed (or gone) is counted, not named. */
export function opensWords(k: Pick<ListenerKey, 'all_shows' | 'shows'>, shows: readonly Pick<KeyedShow, 'id' | 'name'>[]): string {
  if (k.all_shows) return 'Every keyed show';
  if (k.shows.length === 0) return 'No show';
  const names = k.shows.map((id) => shows.find((s) => s.id === id)?.name).filter((n): n is string => !!n);
  const unknown = k.shows.length - names.length;
  if (names.length === 0) return count(unknown, 'show that is not keyed now', 'shows that are not keyed now');
  const lead = names.length <= 2 ? names.join(' and ') : `${names[0]} and ${names.length - 1} more`;
  return unknown ? `${lead} (and ${count(unknown, 'show', 'shows')} not keyed now)` : lead;
}

/** A key's use in a line. */
export function useWords(k: Pick<ListenerKey, 'listens_used' | 'max_listens' | 'last_used_ms'>, now: number): string {
  if (k.last_used_ms == null) return 'Not used yet';
  const listens = k.max_listens != null ? `${k.listens_used} of ${count(k.max_listens, 'listen')}` : count(k.listens_used, 'listen');
  return `${listens} · last used ${fromNow(k.last_used_ms, now)}`;
}

/** A device's line on a key's page: live (it renewed within the hold), signed out, or gone quiet. */
export function deviceStanding(d: KeyDevice, now: number, holdMs = 360_000): { word: string; live: boolean } {
  if (d.ended_ms != null) return { word: 'Signed out', live: false };
  if (now - d.last_seen_ms <= holdMs) return { word: 'Listening now', live: true };
  return { word: `Last seen ${fromNow(d.last_seen_ms, now)}`, live: false };
}

const FIELD_WORD: Record<string, string> = {
  label: 'who it is for',
  note: 'the note',
  email: 'the email address',
  media: 'sound or picture',
  all_shows: 'the shows it opens',
  shows: 'the shows it opens',
  valid_from: 'when it starts',
  valid_until: 'when it ends',
  max_devices: 'the devices limit',
  max_listens: 'the listens limit',
  max_people: 'the people limit',
};

// ---- Lost your key?, on the staff's side (keyed-shows.md, section 4.2 and "Mail") ----

/** A key's Email line: whether an address is kept, and what *Lost your key?* does with it at
 *  this station. `mail` is whether the station sends email (the keyed shows' list, 1.3.2);
 *  unknown reads as it sends, as the screen always said. */
export function emailFact(hasEmail: boolean, mail: boolean | null | undefined): string {
  if (mail === false) {
    return hasEmail
      ? 'An address is kept, sealed. This station does not send email, so Lost your key? only notes the request here: you send the key yourself, with Send.'
      : 'None kept, so a Lost your key? request cannot be matched to this key.';
  }
  return hasEmail ? 'An address is kept, sealed. It is shown with the key’s text, in Send.' : 'None kept, so Lost your key? cannot send this one.';
}

/** What the email field promises when a key is made or changed. */
export function emailHint(mail: boolean | null | undefined): string {
  return mail === false
    ? 'Kept sealed. This station does not send email: with an address, a Lost your key? request on the player is noted on the key, for you to send it again.'
    : 'Kept sealed. With it, Lost your key? on the player can send the key to its holder again.';
}

/** A holder asked for their key again and the station sent nothing: the newest `lost_key_asked`
 *  in the key's log that says so (`detail.sent` false; where an entry does not say, the
 *  station-wide `mail` false), with no key text shown and no replacement since. Where the station
 *  emailed the key it answered by itself, and nothing waits. Null when nothing waits. */
export function lostWaiting(log: readonly KeyLogEntry[] | null | undefined, mail: boolean | null | undefined): { at_ms: number } | null {
  if (!log?.length) return null;
  const unsent = (e: KeyLogEntry) => (typeof e.detail?.sent === 'boolean' ? !e.detail.sent : mail === false);
  const asks = log.filter((e) => e.action === 'lost_key_asked');
  if (!asks.length) return null;
  const newest = asks.reduce((a, b) => (b.at_ms > a.at_ms ? b : a));
  if (!unsent(newest)) return null;
  const answered = log.some((e) => (e.action === 'text_shown' || e.action === 'replaced') && e.at_ms >= newest.at_ms);
  return answered ? null : { at_ms: newest.at_ms };
}

/** The words of that note. */
export const LOST_WAITING = {
  lead: 'Its holder asked for it again',
  body: 'with Lost your key? on the player. The station sent nothing, because it has no outgoing mail: send it to them yourself.',
  hint: 'Send… shows the link, the key and a message ready to copy. The station notes on the key that its text was shown.',
} as const;

/** One entry of a key's change log, in words. The station keeps the names of the fields that
 *  changed, never their values. */
export function logWords(e: KeyLogEntry, zone = ''): string {
  const d = e.detail ?? {};
  const reason = typeof d.reason === 'string' && d.reason ? `: ${d.reason}` : '';
  switch (e.action) {
    case 'created':
      return 'Made';
    case 'changed': {
      const fields = Array.isArray(d.fields) ? [...new Set((d.fields as unknown[]).map((f) => FIELD_WORD[String(f)] ?? String(f)))] : [];
      return fields.length ? `Changed ${fields.join(', ')}` : 'Changed';
    }
    case 'extended':
      return typeof d.to_ms === 'number' ? `Extended to ${timeWords(d.to_ms, zone)}` : 'Extended';
    case 'paused':
      return `Paused${reason}`;
    case 'resumed':
      return 'Resumed';
    case 'revoked':
      return `Revoked${reason}`;
    case 'replaced':
      return 'Replaced by a new key';
    case 'text_shown':
      return typeof d.export === 'string' ? 'Its text went out in the family’s export' : 'Its text was shown';
    case 'lost_key_asked':
      // The station says whether it emailed the key (1.3.2); an older entry does not.
      return `Its holder asked for it again (Lost your key?)${d.sent === true ? ': the station emailed it' : d.sent === false ? ': nothing was sent' : ''}`;
    case 'device_ended':
      return 'A device was signed out';
    default:
      return e.action.replaceAll('_', ' ');
  }
}

// ---- the forms ----

/** A key as a form holds it: every field a string or a choice, so a half-typed number is not a
 *  number yet. */
export interface KeyForm {
  label: string;
  kind: KeyKind;
  email: string;
  note: string;
  media: KeyMedia;
  /** `all`: every keyed show; `some`: the shows ticked. */
  opens: 'all' | 'some';
  shows: string[];
  /** A `datetime-local` value in the station's zone; empty: already, and no end. */
  from: string;
  until: string;
  /** `any` or a count, as text. */
  devices: string;
  listens: string;
  people: string;
  family: string;
}

/** The common case (the design's default): a personal key for every keyed show, sound and
 *  picture, two devices at a time, no end. */
export function freshForm(): KeyForm {
  return { label: '', kind: 'personal', email: '', note: '', media: 'both', opens: 'all', shows: [], from: '', until: '', devices: '2', listens: '', people: '', family: '' };
}

export type KeyField = 'label' | 'email' | 'note' | 'shows' | 'from' | 'until' | 'devices' | 'listens' | 'people';
export type KeyProblems = Partial<Record<KeyField, string>>;

/** A limit as typed: empty or `any` is none; else a whole number from 1. */
export function limitOf(text: string): number | null | 'bad' {
  const t = text.trim().toLowerCase();
  if (t === '' || t === 'any') return null;
  if (!/^\d{1,7}$/.test(t)) return 'bad';
  const n = Number(t);
  return n >= 1 ? n : 'bad';
}

/** What is wrong with a key's form, a sentence under each field. `toMs` reads a
 *  `datetime-local` value in the station's zone. `withLabel: false` is a family's template,
 *  whose names are its recipients'. */
export function keyProblems(f: KeyForm, toMs: (v: string) => number | null, withLabel = true): KeyProblems {
  const p: KeyProblems = {};
  if (withLabel) {
    const label = f.label.trim();
    if (!label) p.label = 'Say who the key is for: a name, a group or a ticket number.';
    else if ([...label].length > LABEL_MAX) p.label = `Up to ${LABEL_MAX} characters.`;
    if (f.email.trim() && emailProblem(f.email)) p.email = 'Enter an email address, like name@example.com, or leave it empty.';
    if ([...f.note].length > NOTE_MAX) p.note = `A note is up to ${NOTE_MAX} characters.`;
  }
  if (f.opens === 'some' && f.shows.length === 0) p.shows = 'Tick at least one show, or choose every keyed show.';
  if (f.opens === 'some' && f.shows.length > SHOWS_MAX) p.shows = `A key opens up to ${SHOWS_MAX} named shows. Choose every keyed show for more.`;
  const from = f.from ? toMs(f.from) : null;
  const until = f.until ? toMs(f.until) : null;
  if (f.from && from === null) p.from = 'Enter a date and a time.';
  if (f.until && until === null) p.until = 'Enter a date and a time.';
  if (from !== null && until !== null && until <= from) p.until = 'The end is after the start.';
  if (f.kind !== 'machine' && limitOf(f.devices) === 'bad') p.devices = 'A number from 1, or leave it empty for any number.';
  if (limitOf(f.listens) === 'bad') p.listens = 'A number from 1, or leave it empty for any number.';
  if (f.kind === 'group' && limitOf(f.people) === 'bad') p.people = 'A number from 1, or leave it empty for any number.';
  return p;
}

/** The first field with a problem, in the form's order, to put the focus on. */
export function firstProblem(p: KeyProblems): KeyField | null {
  return (['label', 'email', 'shows', 'from', 'until', 'devices', 'listens', 'people', 'note'] as const).find((k) => p[k]) ?? null;
}

const num = (text: string): number | null => {
  const n = limitOf(text);
  return n === 'bad' ? null : n;
};

/** What every key of the form is: the part a family's template shares with one key. */
export function templateOf(f: KeyForm, toMs: (v: string) => number | null): KeyTemplate {
  return {
    kind: f.kind,
    media: f.media,
    all_shows: f.opens === 'all',
    shows: f.opens === 'all' ? [] : [...f.shows],
    valid_from_ms: f.from ? toMs(f.from) : null,
    valid_until_ms: f.until ? toMs(f.until) : null,
    max_devices: f.kind === 'machine' ? null : num(f.devices),
    max_listens: num(f.listens),
    max_people: f.kind === 'group' ? num(f.people) : null,
  };
}

/** The body that makes the form's key. */
export function specOf(f: KeyForm, toMs: (v: string) => number | null): KeySpec {
  return {
    label: f.label.trim(),
    ...templateOf(f, toMs),
    ...(f.email.trim() ? { email: f.email.trim() } : {}),
    ...(f.note.trim() ? { note: f.note.trim() } : {}),
    ...(f.family ? { family: f.family } : {}),
  };
}

/** A key as its form, to change it. The email address is never read back, so the field starts
 *  empty and `keepEmail` says the address the station has stays as it is. */
export function formOf(k: ListenerKey, toInput: (ms: number) => string): KeyForm {
  return {
    label: k.label,
    kind: k.kind,
    email: '',
    note: k.note,
    media: k.media,
    opens: k.all_shows ? 'all' : 'some',
    shows: [...k.shows],
    from: k.valid_from_ms != null ? toInput(k.valid_from_ms) : '',
    until: k.valid_until_ms != null ? toInput(k.valid_until_ms) : '',
    devices: k.max_devices != null ? String(k.max_devices) : '',
    listens: k.max_listens != null ? String(k.max_listens) : '',
    people: k.max_people != null ? String(k.max_people) : '',
    family: k.family_id ?? '',
  };
}

/** What the form changes about the key: only the fields that differ are sent, and a window or
 *  a limit that was taken away is sent as `null`. `email`: `keep` leaves the address alone,
 *  `remove` takes it away, `set` sends the form's. Empty when nothing changed. */
export function changeOf(k: ListenerKey, f: KeyForm, toMs: (v: string) => number | null, email: 'keep' | 'remove' | 'set'): KeyChange {
  const c: KeyChange = {};
  const t = templateOf({ ...f, kind: k.kind }, toMs);
  if (f.label.trim() !== k.label) c.label = f.label.trim();
  if (f.note.trim() !== k.note) c.note = f.note.trim();
  if (email === 'remove') c.email = null;
  if (email === 'set' && f.email.trim()) c.email = f.email.trim();
  if (t.media !== k.media) c.media = t.media;
  const sameShows = t.all_shows === k.all_shows && (t.all_shows || (t.shows!.length === k.shows.length && t.shows!.every((s) => k.shows.includes(s))));
  if (!sameShows) {
    c.all_shows = t.all_shows;
    c.shows = t.shows;
  }
  if ((t.valid_from_ms ?? null) !== (k.valid_from_ms ?? null)) c.valid_from_ms = t.valid_from_ms ?? null;
  if ((t.valid_until_ms ?? null) !== (k.valid_until_ms ?? null)) c.valid_until_ms = t.valid_until_ms ?? null;
  if ((t.max_devices ?? null) !== (k.max_devices ?? null)) c.max_devices = t.max_devices ?? null;
  if ((t.max_listens ?? null) !== (k.max_listens ?? null)) c.max_listens = t.max_listens ?? null;
  if ((t.max_people ?? null) !== (k.max_people ?? null)) c.max_people = t.max_people ?? null;
  return c;
}

/** The quick extends on a key's page and row: a week, a month. */
export const EXTENDS: readonly { days: number; label: string }[] = [
  { days: 7, label: '+1 week' },
  { days: 30, label: '+1 month' },
];

/** Why a key cannot be extended by days, or null. A key with no end has nothing to extend. */
export function extendProblem(k: ListenerKey): string | null {
  if (k.state === 'revoked') return 'A revoked key cannot be extended. Replace it instead.';
  if (k.valid_until_ms == null) return 'This key has no end, so there is nothing to extend.';
  return null;
}

// ---- making many ----

export interface ParsedList {
  recipients: KeyRecipient[];
  /** A sentence for each line that cannot be a key, with its line number. */
  problems: string[];
  /** Whether a first line of column names was left out. */
  header: boolean;
}

/** One line of a pasted list into cells: tabs (pasted from a spreadsheet), else commas or
 *  semicolons, with double quotes round a cell that holds one. */
function cells(line: string): string[] {
  if (line.includes('\t')) return line.split('\t').map((c) => c.trim());
  const sep = line.includes(',') || !line.includes(';') ? ',' : ';';
  const out: string[] = [];
  let cur = '';
  let quoted = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (quoted) {
      if (ch === '"' && line[i + 1] === '"') {
        cur += '"';
        i += 1;
      } else if (ch === '"') quoted = false;
      else cur += ch;
    } else if (ch === '"' && cur.trim() === '') quoted = true;
    else if (ch === sep) {
      out.push(cur.trim());
      cur = '';
    } else cur += ch;
  }
  out.push(cur.trim());
  return out;
}

/** A pasted list read into recipients: one person a line, as `name`, `name, email` or
 *  `name, email, note` (commas, semicolons or a spreadsheet's tabs). A first line of column
 *  names is left out. Every line that cannot be a key is said, and none is dropped silently. */
export function parseRecipients(text: string): ParsedList {
  const recipients: KeyRecipient[] = [];
  const problems: string[] = [];
  let header = false;
  const seen = new Set<string>();
  const lines = text.split(/\r?\n/);
  lines.forEach((raw, i) => {
    const line = raw.trim();
    if (!line) return;
    const c = cells(line);
    const [label, email = '', ...rest] = c;
    if (recipients.length === 0 && problems.length === 0 && !header && /^(name|label|who)$/i.test(label) && (c.length === 1 || /^e-?mail/i.test(email))) {
      header = true;
      return;
    }
    const note = rest.join(', ').trim();
    const n = i + 1;
    if (!label) return void problems.push(`Line ${n} has no name.`);
    if ([...label].length > LABEL_MAX) return void problems.push(`Line ${n}: the name is longer than ${LABEL_MAX} characters.`);
    if (email && emailProblem(email)) return void problems.push(`Line ${n}: “${email}” is not an email address.`);
    if ([...note].length > NOTE_MAX) return void problems.push(`Line ${n}: the note is longer than ${NOTE_MAX} characters.`);
    const same = `${label.toLowerCase()}|${email.toLowerCase()}`;
    if (seen.has(same)) return void problems.push(`Line ${n} repeats ${label}${email ? ` (${email})` : ''}.`);
    seen.add(same);
    recipients.push({ label, ...(email ? { email } : {}), ...(note ? { note } : {}) });
  });
  if (recipients.length > FAMILY_MAX) problems.push(`A family is up to ${FAMILY_MAX} keys in one go, and this list has ${recipients.length}. Make it in parts.`);
  return { recipients, problems, header };
}

/** How many anonymous keys, as typed: 1 to 1,000. */
export function countProblem(text: string): string | null {
  const t = text.trim();
  if (!/^\d{1,5}$/.test(t) || Number(t) < 1) return 'Enter how many keys to make, from 1.';
  if (Number(t) > FAMILY_MAX) return `Up to ${FAMILY_MAX} keys in one go. Make more in a second family.`;
  return null;
}

/** A family's name, or the sentence for its field. */
export function familyLabelProblem(label: string, taken: readonly string[] = []): string | null {
  const l = label.trim();
  if (!l) return 'Name the family after who it is for: “Autumn patrons”, “Quiz night tickets”.';
  if ([...l].length > LABEL_MAX) return `Up to ${LABEL_MAX} characters.`;
  if (taken.some((t) => t.trim().toLowerCase() === l.toLowerCase())) return 'A family has that name already. Choose another.';
  return null;
}

/** The body that makes a family: named recipients, or a number of anonymous keys. */
export function familySpecOf(label: string, note: string, template: KeyTemplate, from: { recipients: KeyRecipient[] } | { count: number; prefix: string }): FamilySpec {
  return {
    label: label.trim(),
    ...(note.trim() ? { note: note.trim() } : {}),
    template,
    ...('recipients' in from ? { recipients: from.recipients } : { count: from.count, ...(from.prefix.trim() ? { prefix: from.prefix.trim() } : {}) }),
  };
}

// ---- CSV ----

/** One CSV cell, as the station writes its own export: quoted when it must be, and a cell that
 *  a spreadsheet would run as a formula written with a quote in front of it. */
export function csvCell(v: string | number | null | undefined): string {
  let s = v === null || v === undefined ? '' : String(v);
  if (/^[=+\-@\t\r]/.test(s)) s = `'${s}`;
  return /[",\r\n']/.test(s) ? `"${s.replaceAll('"', '""')}"` : s;
}

export function csv(head: readonly string[], rows: readonly (string | number | null | undefined)[][]): string {
  return [head, ...rows].map((r) => r.map(csvCell).join(',')).join('\r\n') + '\r\n';
}

/** A file name from a family's or a list's name: lower-case letters, digits and hyphens. */
export function fileStem(name: string, fallback = 'keys'): string {
  const s = name
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 60);
  return s || fallback;
}

/** The leak report as CSV: counts and reasons, never a key's text or an address. */
export function leaksCsv(entries: readonly KeyLeak[], zone = ''): string {
  const when = (ms: number | null | undefined) => (ms == null ? '' : timeWords(ms, zone));
  return csv(
    ['key', 'family', 'state', 'score', 'signs', 'addresses_7_days', 'addresses_24_hours', 'addresses_ever', 'devices', 'listens', 'most_new_addresses_in_an_hour', 'first_seen', 'last_seen'],
    entries.map((e) => [e.key.label, e.family ?? '', e.key.state, e.score, e.reasons.join('; '), e.addresses_7d, e.addresses_24h, e.addresses, e.devices, e.listens, e.peak_new_addresses_in_an_hour, when(e.first_seen_ms), when(e.last_seen_ms)]),
  );
}

// ---- the leak report ----

export type LeakLevel = 'likely' | 'look' | 'none' | 'unused';

/** How a row of the leak report reads. The score is a hint, not a verdict: a burst of new
 *  addresses is what a key posted in public looks like; any other sign is worth a look. */
export function leakLevel(e: KeyLeak): LeakLevel {
  if (e.first_seen_ms == null && e.listens === 0) return 'unused';
  if (e.score <= 0) return 'none';
  return e.spike ? 'likely' : 'look';
}

export const LEAK_WORD: Record<LeakLevel, { word: string; cls: string }> = {
  likely: { word: 'Likely passed on', cls: 'hollow hollow-serious' },
  look: { word: 'Worth a look', cls: 'hollow hollow-warning' },
  none: { word: 'No sign', cls: 'hollow hollow-standby' },
  unused: { word: 'Not used yet', cls: 'hollow hollow-off' },
};

/** The report's head: how many keys show a sign, of how many used. */
export function leakSummary(entries: readonly KeyLeak[]): { used: number; signs: number; likely: number; line: string } {
  const used = entries.filter((e) => leakLevel(e) !== 'unused').length;
  // A key already revoked or replaced has been dealt with: its row keeps its sign, and it is not
  // counted among the keys to look at.
  const on = entries.filter((e) => e.key.state !== 'revoked');
  const signs = on.filter((e) => e.score > 0).length;
  const likely = on.filter((e) => leakLevel(e) === 'likely').length;
  const line =
    entries.length === 0
      ? 'No keys to report on.'
      : used === 0
        ? 'No key has been used yet, so there is nothing to compare.'
        : signs === 0
          ? `${count(used, 'key has', 'keys have')} been used, and none shows a sign of being passed on.`
          : `${count(signs, 'key shows', 'keys show')} a sign of being passed on, of ${used} used${likely ? `; ${likely} ${likely === 1 ? 'looks' : 'look'} posted in public` : ''}.`;
  return { used, signs, likely, line };
}

// ---- the message ----

export interface MessageFacts {
  station: string;
  /** Who the key is for, as staff typed it. */
  label: string;
  kind: KeyKind;
  link: string;
  /** `KETM-7QMP-2D4X`. */
  text: string;
  /** What it opens, in words (`opensWords`). */
  opens: string;
  valid_from_ms?: number | null;
  valid_until_ms?: number | null;
  max_devices?: number | null;
  /** Say that the key is the person's own and is not to be passed on. */
  personal: boolean;
  /** Where to turn if it does not work: the station's own words or address; may be empty. */
  contact: string;
}

/** A first name to greet, when the label reads as a person's name; else nothing (a ticket
 *  number, a group's name and a machine are not greeted by name). */
export function greetingName(label: string, kind: KeyKind): string | null {
  if (kind !== 'personal') return null;
  const first = label.trim().split(/\s+/)[0] ?? '';
  if (/^(dr|mr|mrs|ms|prof|rev)\.?$/i.test(first)) return label.trim();
  return /^\p{Lu}[\p{L}’'-]+$/u.test(first) && !/\d/.test(label) ? first : null;
}

/** The message a key is sent with: the station's words, the link, the key to type, the window,
 *  and what to do if it fails. Plain text, for an email or a message; staff edit it before it
 *  goes. It never says "token" or "access code". */
export function messageFor(m: MessageFacts, now: number, zone = ''): { subject: string; body: string } {
  const name = greetingName(m.label, m.kind);
  const lines: string[] = [];
  lines.push(name ? `Hello ${name},` : 'Hello,');
  lines.push('');
  const opens = m.opens === 'Every keyed show' ? `${m.station}’s shows for key holders` : m.opens;
  lines.push(`Here is your key for ${opens}.`);
  lines.push('');
  lines.push('Open this link and press Listen:');
  lines.push(m.link);
  lines.push('');
  // `/listen/` opened without its key has the box open, headed Your key (1.3.2).
  lines.push(`Or go to ${stationAddress(m.link)} and type this into “Your key”:`);
  lines.push(m.text);
  lines.push('Capitals, spaces and dashes do not matter.');
  lines.push('');
  const from = m.valid_from_ms ?? null;
  const until = m.valid_until_ms ?? null;
  const zoneNote = zone ? ` (${zone.replaceAll('_', ' ')} time)` : '';
  if (from !== null && from > now && until !== null) lines.push(`It opens on ${timeWords(from, zone)} and ends on ${timeWords(until, zone)}${zoneNote}.`);
  else if (from !== null && from > now) lines.push(`It opens on ${timeWords(from, zone)}${zoneNote}.`);
  else if (until !== null) lines.push(`It works until ${timeWords(until, zone)}${zoneNote}.`);
  if (m.kind !== 'machine' && m.max_devices != null) lines.push(`It works on ${m.max_devices === 1 ? 'one device' : `${m.max_devices} devices`} at a time. The same link works on your phone and your computer.`);
  if (m.personal) lines.push(m.kind === 'group' ? 'This key is for your group only. Please do not pass it on to anyone else.' : 'This key is yours alone. Please do not forward it or post it anywhere.');
  lines.push('');
  lines.push(`If it does not work, the page says why.${m.contact.trim() ? ` If you are stuck: ${m.contact.trim()}` : ` If you are stuck, reply to this message.`}`);
  lines.push('');
  lines.push(m.station);
  return { subject: `Your key for ${m.station}`, body: lines.join('\n') };
}

/** The station's address without the key, from a key's link: `/listen/`, where the box for a key is. */
export function stationAddress(link: string): string {
  const i = link.indexOf('#');
  return i < 0 ? link : link.slice(0, i);
}

// ---- when the whole screen is refused ----

export type Unready =
  /** The sign-in has ended, or there is none. */
  | 'signed-out'
  /** A presenter or a screener. */
  | 'not-allowed'
  /** A station older than 1.3.2: it has no such route. */
  | 'older'
  /** Keys are not set up: no database, or no station key to seal them under. */
  | 'not-set-up'
  /** Anything else: the station's sentence, and Try again. */
  | 'problem';

export interface UnreadyWords {
  kind: Unready;
  title: string;
  body: string;
  /** The station's own sentence, shown as given under the body; empty when there is none. */
  said: string;
  retry: boolean;
}

/** What a refusal of the keys' list means, in words that say what to do about it. */
export function unreadyOf(e: unknown): UnreadyWords {
  const status = e instanceof FieldError ? e.status : 0;
  const said = e instanceof FieldError && !/^\d{3}\b/.test(e.message) ? e.message : '';
  if (status === 401) return { kind: 'signed-out', title: 'Sign in to see the keys', body: 'Your sign-in has ended, or this window has none. Sign in again and the keys come back.', said: '', retry: true };
  if (status === 403) return { kind: 'not-allowed', title: 'Keys are a producer’s or an admin’s', body: 'Keys hold listeners’ names, email addresses and the means to listen, so a presenter’s or a screener’s account cannot open them. Ask the station’s admin.', said, retry: false };
  if (status === 404 || status === 405) return { kind: 'older', title: 'This station has no keys yet', body: 'Keyed shows came with WeaveSignal 1.3.2, and this station runs an older version. Update the station and this screen fills in.', said: '', retry: true };
  if (status === 503) return { kind: 'not-set-up', title: 'Keys are not set up at this station', body: 'The station keeps keys in its database, sealed under its station key, and one of the two is missing. The line below is the station’s own; whoever runs the server can put it right.', said, retry: true };
  return { kind: 'problem', title: 'The keys could not be read', body: e instanceof TypeError || status === 0 ? 'The station did not answer. Check the connection and try again.' : 'The station refused. Its own words are below.', said, retry: true };
}

/** A refusal of one action on a key, as a sentence for its row or dialog. The station's
 *  sentence is shown as given; the two the station words for a program get a person's words. */
export function actionProblem(e: unknown): string {
  if (e instanceof FieldError) {
    if (e.status === 409 && /token key|no longer be read|cannot be read/i.test(e.message)) return 'This key’s text can no longer be read: the station’s own key changed after it was made. Replace the key and send the new one.';
    if (e.status === 404) return 'That key is no longer there. It may have been erased from another window.';
    if (e.status === 403) return e.message || 'Your account may not do this.';
    return e.message;
  }
  return e instanceof Error && e.message ? e.message : 'The station did not answer. Try again.';
}

/** A key's text as it is written, from however it was typed or found: `ketm 7qmp2d4x` to
 *  `KETM-7QMP-2D4X`. Null when it cannot be one (the station's alphabet has no 0, 1, I, L or O). */
export function shapeOfText(typed: string): string | null {
  const t = typed.toUpperCase().replace(/[\s\-–—_.]/g, '');
  if (!/^[ABCDEFGHJKMNPQRSTUVWXYZ23456789]{12}$/.test(t)) return null;
  return `${t.slice(0, 4)}-${t.slice(4, 8)}-${t.slice(8)}`;
}

/** A key's text out of whatever was pasted: its own text, or a link that carries one. */
export function textIn(pasted: string): string | null {
  const m = /[#&?]key=([^&\s]+)/i.exec(pasted);
  return shapeOfText(m ? decodeURIComponent(m[1]) : pasted.trim());
}
