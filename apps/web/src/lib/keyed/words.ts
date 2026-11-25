// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What the player says about a key (keyed shows, 1.3.2), as pure rules: the welcome, the quiet
// badge, when a key opens and ends, the words for every way a key does not work, the lost-key
// answer and the "Can't hear the show?" help. The word is "key" everywhere: never "token",
// never "access code". Every refusal has the page's own words; the station's sentence is
// written for a program and is not shown.

import type { Device, KeyedShow, KeyedShows, ListenKey, Refusal } from './routes.ts';

const fmt = (o: Intl.DateTimeFormatOptions, zone?: string) => new Intl.DateTimeFormat('en-GB', { ...o, ...(zone ? { timeZone: zone } : {}) });

/** `30 November` (the year too when it is not this one), in the listener's own time zone. */
export function dayWords(ms: number, now: number, zone?: string): string {
  const year = (t: number) => fmt({ year: 'numeric' }, zone).format(new Date(t));
  return fmt({ day: 'numeric', month: 'long', ...(year(ms) === year(now) ? {} : { year: 'numeric' }) }, zone).format(new Date(ms));
}

/** `30 November at 22:00`. */
export function whenWords(ms: number, now: number, zone?: string): string {
  return `${dayWords(ms, now, zone)} at ${fmt({ hour: '2-digit', minute: '2-digit', hourCycle: 'h23' }, zone).format(new Date(ms))}`;
}

/** The greeting, to anyone whose key was accepted: "Welcome." and no name. A key has one name,
 *  the one staff gave it to find it again ("Agent test (delete me)", "Ticket 014", "Press, batch
 *  3"): it is theirs, not the holder's, and the page never shows it (the operator, 2026-11-25:
 *  the person's own label if the key has one, otherwise just "Welcome"). The station sends no
 *  name of the holder's own today; if a key gains one, it is greeted here. */
export const GREETING = 'Welcome.';

/** What a key opens, by name: one show, two, or the first and how many more. */
export function opensWords(key: Pick<ListenKey, 'all_shows' | 'shows'>, station: string): string {
  if (key.all_shows) return `every show ${station || 'the station'} keeps for key holders`;
  const names = key.shows.map((s) => s.name);
  if (names.length === 0) return 'no show at the moment';
  if (names.length === 1) return names[0];
  if (names.length === 2) return `${names[0]} and ${names[1]}`;
  return `${names[0]} and ${names.length - 1} more`;
}

/** The welcome: "Welcome. This key opens Friday Supporters' Hour and 3 more, until 30 November."
 *  A key for the sound alone says so; a key for the picture carries the show's sound with it
 *  (`video` opens what `both` does), so it has nothing to add. */
export function welcome(key: Pick<ListenKey, 'media' | 'all_shows' | 'shows' | 'valid_until_ms'>, station: string, now: number, zone?: string): { lead: string; rest: string } {
  const until = key.valid_until_ms != null ? `, until ${dayWords(key.valid_until_ms, now, zone)}` : '';
  const media = key.media === 'sound' ? ' It is for the sound, not the picture.' : '';
  return { lead: GREETING, rest: `This key opens ${opensWords(key, station)}${until}.${media}` };
}

/** The quiet badge beside the show, for a key's holder. It says a key is in use here and never
 *  whose: the key's name is staff's own (see `GREETING`). */
export const BADGE = 'Your key is on this device';

/** "Your key ends on 30 November at 22:00", once the station says it ends soon; else null. */
export function endsNotice(key: ListenKey, now: number, zone?: string): string | null {
  if (!key.ends_soon || key.valid_until_ms == null) return null;
  return `Your key ends on ${whenWords(key.valid_until_ms, now, zone)}.`;
}

/** Whether a key opens a show, as far as the page can tell before it asks. */
export function opens(key: Pick<ListenKey, 'all_shows' | 'shows'>, show: Pick<KeyedShow, 'id'>): boolean {
  return key.all_shows || key.shows.some((s) => s.id === show.id);
}

/** The keyed show on now, by its id: the player's facts say which show is on (`show_id`) and
 *  whether its keyed window is open (`keyed`); the keyed shows' list names it. Null while what is
 *  on is public. A show the list does not hold yet (made a moment ago) is named by `fallback`
 *  (the key's own token for it), else not at all. */
export function keyedNow(list: KeyedShows | null, on: { show_id?: string | null; keyed?: boolean } | null | undefined, fallback: { id: string; slug: string; name: string } | null = null): KeyedShow | null {
  const id = on?.keyed ? (on.show_id ?? list?.on_now ?? null) : null;
  if (!id) return null;
  const found = list?.shows.find((s) => s.id === id);
  if (found) return found;
  return fallback && fallback.id === id ? { ...fallback, now: 'key', opens_at_ms: null, slate: 'announcement', slate_note: '' } : null;
}

/** The keyed show an invitation is for (the join page's line, `join.keyed`), or null when its
 *  show is public. From 1.3.2 the pass's own facts say it (`show_id`, `keyed`): the line then
 *  follows the invitation's show, not whatever is on when the page is opened, so a guest of
 *  tonight's keyed show is told in the afternoon, and a guest of a public show is told nothing
 *  while another show's keyed window is open. The list names the show and when it opens to
 *  everyone; a show it does not hold yet is named by the pass. A station that does not say
 *  (`keyed` absent) is read as before: a live link's show is the keyed show on now, if one is. */
export function invitedKeyed(pass: { show?: string; show_id?: string | null; keyed?: boolean; studio?: 'live' | 'rehearsal' } | null, list: KeyedShows | null): KeyedShow | null {
  if (!pass) return null;
  if (pass.keyed === undefined) return (pass.studio ?? 'live') === 'live' && list?.on_now ? keyedNow(list, { show_id: list.on_now, keyed: true }) : null;
  if (!pass.keyed) return null;
  const found = pass.show_id ? list?.shows.find((s) => s.id === pass.show_id) : undefined;
  return found ?? { id: pass.show_id ?? '', slug: '', name: pass.show || 'this show', now: 'key', opens_at_ms: null, slate: 'announcement', slate_note: '' };
}

/** The join page's keyed line, after its bold lead. In the rehearsal studio nobody hears
 *  anything, so the line speaks of when the show airs. */
export function invitedWords(show: Pick<KeyedShow, 'name' | 'opens_at_ms' | 'now'>, o: { rehearsal?: boolean; now: number; zone?: string }): string {
  const opens = opensToAll(show, o.now, o.zone);
  const who = o.rehearsal
    ? `When ${show.name} airs, only listeners who were given a key hear it; everyone else hears what the station plays meanwhile. You need no key to rehearse, or to join the show.`
    : `Only listeners who were given a key hear ${show.name}; everyone else hears what the station plays meanwhile. You are joining the studio, so you need no key.`;
  return opens ? `${who} ${opens}` : who;
}

/** `/listen/` opened with no key in its address (the page a key's link leads to): the line for
 *  whoever has the link, and the line over the box for whoever has the key itself. The box is
 *  the player's own, so a key typed here is checked, refused and welcomed in the same words. */
export function entryWords(station: string): { title: string; link: string; box: string } {
  return {
    title: 'Listen with a key',
    link: `Have the link ${station || 'the station'} sent you? Open that link, all of it, on this device, and your key comes with it.`,
    box: 'Or type or paste the key itself here; the whole link works too.',
  };
}

/** After a key typed on `/listen/` is accepted and the browser will not start the sound by
 *  itself (the press was too long ago for it): what to press, said calmly. */
export function pressToHear(button: string, show: string | null): string {
  return `Your key is accepted. Press ${button} to hear ${show || 'the show'}.`;
}

/** What someone without a key hears while a keyed show is on, in a sentence. In 1.3.2 a station
 *  plays its one slate (its slate file, else its fallback music) whatever the show's rule names,
 *  so the page does not say which. */
export function publicHears(station: string): string {
  return `Without a key you hear what ${station || 'the station'} plays for everyone else until the public programme returns.`;
}

/** "Opens to everyone on 30 November at 19:00", for a show with early access; else null. */
export function opensToAll(show: Pick<KeyedShow, 'opens_at_ms' | 'now'>, now: number, zone?: string): string | null {
  if (show.now !== 'key' || show.opens_at_ms == null || show.opens_at_ms <= now) return null;
  return `It opens to everyone on ${whenWords(show.opens_at_ms, now, zone)}.`;
}

export interface Problem {
  /** One short line: what is wrong. */
  title: string;
  /** What to do about it. */
  body: string;
  /** What the page offers: type it again, ask the station, sign a device out, or wait. */
  act: 'retype' | 'ask' | 'devices' | 'wait' | 'again' | 'none';
  /** Whether the key stays kept on this device (it may work later). */
  keep: boolean;
}

/** The words for a key that does not work. `station` is the station's name; `show` the show
 *  the listener wanted, when there was one. */
export function problemWords(r: Pick<Refusal, 'code' | 'at_ms' | 'devices' | 'retry_s'>, o: { station: string; show?: string | null; now: number; zone?: string }): Problem {
  const station = o.station || 'the station';
  switch (r.code) {
    case 'not_recognised':
      return { title: 'That key is not recognised.', body: 'Check it against the one you were sent. Capitals, spaces and dashes do not matter, and a key has no 0, 1, I, L or O in it.', act: 'retype', keep: false };
    case 'not_yet_valid':
      return { title: r.at_ms != null ? `This key opens on ${whenWords(r.at_ms, o.now, o.zone)}.` : 'This key has not started yet.', body: 'It is kept on this device, so come back then and press Listen.', act: 'wait', keep: true };
    case 'ended':
      return { title: r.at_ms != null ? `Your key ended on ${whenWords(r.at_ms, o.now, o.zone)}.` : 'Your key has ended.', body: `Ask ${station} for a new one.`, act: 'ask', keep: false };
    case 'paused':
      return { title: 'This key is switched off for now.', body: `${cap(station)} has paused it and can switch it back on. Ask them; the key itself stays the same.`, act: 'ask', keep: true };
    case 'revoked':
      return { title: `This key was ended by ${station}.`, body: 'It will not work again. Ask them for a new one.', act: 'ask', keep: false };
    case 'does_not_open_show':
      return { title: o.show ? `This key does not open ${o.show}.` : 'This key does not open this show.', body: `It is for other shows. Ask ${station} if it should open this one too.`, act: 'ask', keep: true };
    case 'too_many_devices': {
      const n = r.devices.filter((d) => d.ended_ms == null).length;
      return { title: n > 0 ? `This key is already in use on ${n === 1 ? 'one device' : `${n} devices`}.` : 'This key is already in use on as many devices as it allows.', body: 'Sign one out below and this one takes its place.', act: 'devices', keep: true };
    }
    case 'people_limit':
      return { title: 'This key has reached the number of people it is for.', body: `It is shared by a group, and the group is full. Ask ${station}.`, act: 'ask', keep: true };
    case 'listens_used_up':
      return { title: 'This key’s listens are used up.', body: `It was for a set number of listens, and they have been used. Ask ${station} for more.`, act: 'ask', keep: true };
    case 'not_scheduled':
      return { title: 'This key works, but none of its shows is on the schedule yet.', body: `Your key is kept on this device. Come back when ${station} says the show is on, and press Listen live.`, act: 'wait', keep: true };
    case 'device_signed_out':
      return { title: 'This device was signed out of the key.', body: 'Someone with the key, or the station, signed it out. Use the key here again to carry on.', act: 'again', keep: true };
    case 'slow_down':
      return { title: 'Too many tries from this connection.', body: r.retry_s ? `Wait ${waitWords(r.retry_s)} and try again.` : 'Wait a few minutes and try again.', act: 'wait', keep: true };
    case 'not_set_up':
      return { title: 'Keys are not in use at this station.', body: `${cap(station)} cannot check a key at the moment. Try again later, or ask them.`, act: 'ask', keep: true };
    default:
      return { title: 'The station did not answer.', body: 'Check your connection and try again. Your key is kept on this device.', act: 'again', keep: true };
  }
}

const cap = (s: string): string => (s ? s[0].toUpperCase() + s.slice(1) : s);

export function waitWords(seconds: number): string {
  if (seconds < 90) return `${Math.max(1, Math.round(seconds))} seconds`;
  const m = Math.round(seconds / 60);
  return m < 90 ? `${m} minutes` : `${Math.round(m / 60)} hours`;
}

/** A device's line: its name, whether it is this one, and when it was last listening. A device
 *  holds its place while it has renewed within the station's `holdS` (`keys.device_hold_s`, six
 *  minutes unless the station set another): within it, it is listening now. */
export function deviceLine(d: Device, mine: string | null, now: number, holdS: number | null = null): { name: string; when: string; here: boolean; over: boolean } {
  const here = !!mine && d.id === mine;
  const ago = now - d.last_seen_ms;
  const when = d.ended_ms != null ? 'signed out' : ago < (holdS ?? 360) * 1000 ? 'listening now' : ago < 3_600_000 ? `last listened ${Math.round(ago / 60_000)} minutes ago` : ago < 48 * 3_600_000 ? `last listened ${Math.round(ago / 3_600_000)} hours ago` : `last listened ${Math.round(ago / 86_400_000)} days ago`;
  return { name: d.label || 'A browser', when, here, over: d.ended_ms != null };
}

/** *Lost your key?* in words (keyed-shows.md, section 4.2 and "Mail"). They follow whether the
 *  station sends email, which is a fact about the station and never about the address typed: the
 *  page says the same thing whether an address has a key or not, so it cannot be used to find
 *  out who has one. With mail the key's link is emailed again; without it nothing is sent by
 *  the station itself, the request is noted for its staff, and they send the key themselves.
 *  `mail` unknown (a station that does not say) reads as mail, as the page always did. */
export interface LostWords {
  /** Under the field, before asking. */
  hint: string;
  button: string;
  busy: string;
  /** After asking: what happens now. */
  answer: string;
  /** And what to do if nothing comes. */
  after: string;
}

export function lostWords(station: string, mail: boolean | null | undefined = true): LostWords {
  const s = station || 'the station';
  if (mail === false) {
    return {
      hint: `The one ${s} has for you. ${cap(s)} does not send email by itself: if a key was made with that address, its staff see your request and send the key to you again.`,
      button: 'Ask for my key again',
      busy: 'Asking…',
      answer: `Asked. If ${s} has a key for that address, its staff can see your request and will send the key to you again.`,
      after: `Nothing is emailed by itself here, so it may take a while: a person at ${s} sends it. Not heard from them? Ask ${s}; the key may have been made without an email address.`,
    };
  }
  return {
    hint: `The one ${s} has for you. If a key was made with it, the key’s link is sent there again.`,
    button: 'Send my key again',
    busy: 'Asking…',
    answer: lostAnswer(station),
    after: `The link in that email works on any device. Nothing arrived? The key may have been made without an email address: ask ${s}.`,
  };
}

/** What the lost-key request says at a station that sends email, whichever way it went. */
export function lostAnswer(station: string): string {
  return `If ${station || 'the station'} has a key for that address, the key is on its way there. Look in your inbox, and your spam folder, in the next few minutes.`;
}

/** A typed email address that cannot be one, or null. */
export function emailProblem(email: string): string | null {
  const e = email.trim();
  if (!e) return 'Enter the email address the station has for you.';
  const at = e.lastIndexOf('@');
  const domain = at < 0 ? '' : e.slice(at + 1);
  return at > 0 && e.length <= 254 && domain.includes('.') && !domain.startsWith('.') && !domain.endsWith('.') && !/\s/.test(e) ? null : 'That does not look like an email address.';
}

export interface HelpFacts {
  /** A key is held and the station accepted it. */
  keyed: boolean;
  /** The key's own problem, if it has one. */
  problem: Problem | null;
  /** The player's facts: something is on air at the station. */
  onAir: boolean;
  /** The show on now is a keyed one, by name; null when what is on is public. */
  keyedShow: string | null;
  /** The key held opens the keyed show that is on. */
  opensIt: boolean;
  /** The stream is playing, as far as the page knows. */
  playing: boolean;
  /** The stream was started and failed. */
  streamFailed: boolean;
  station: string;
}

/** "Can't hear the show?": the likeliest reason first, from what the page knows, then the
 *  others. Each is a heading and what to do. */
export function helpItems(f: HelpFacts): { lead: string; body: string }[] {
  const station = f.station || 'the station';
  const out: { lead: string; body: string }[] = [];
  if (f.problem) out.push({ lead: 'Your key needs attention.', body: `${f.problem.title} ${f.problem.body}` });
  if (!f.keyedShow && f.onAir) out.push({ lead: 'The keyed show is not on now.', body: `What is on at the moment is ${station}’s public programme, which needs no key. Your key is kept on this device and works when its show starts.` });
  if (!f.onAir) out.push({ lead: `${cap(station)} is not on air yet.`, body: 'Nothing is live at the moment. You may hear the station’s music meanwhile. Come back at the show’s time and press Listen.' });
  if (f.keyedShow && f.keyed && !f.opensIt) out.push({ lead: `Your key is for another show.`, body: `It does not open ${f.keyedShow}. Ask ${station} whether it should.` });
  if (f.keyedShow && !f.keyed && !f.problem) out.push({ lead: `${f.keyedShow} needs a key.`, body: 'Enter your key above, or open the link you were sent. Without one you hear what the station plays for everyone else.' });
  out.push({
    lead: f.streamFailed ? 'The sound did not start.' : 'Your browser may be holding the sound back.',
    body: 'Press Listen again: a browser plays sound only after you press something on the page. Check that this tab is not muted and that the volume is up, on the page and on the device. On a phone, check the silent switch.',
  });
  out.push({ lead: 'Still nothing?', body: `Reload the page, or try another browser. Your key is kept on this device, and the link you were sent works again on any device. If it still fails, tell ${station} what this page says.` });
  return out;
}

/** What the station keeps about a key's holder, in a sentence for the listener (the design's
 *  privacy line). */
export function keptWords(station: string): string {
  const s = station || 'The station';
  return `${cap(s)} keeps the name on your key, and when and on how many devices and connections it was used, to notice a key that has been passed on. It does not keep your address in a form that can be read back. Ask ${station || 'the station'} to remove what it holds.`;
}
