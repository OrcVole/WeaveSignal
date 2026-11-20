// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The telephone line (1.2, PLAN 17f): what the station says of it (`GET /api/v1/phone`), the
// station's own recording of the phone words, number bans, and the takedown search; and the
// words the screener and Settings put them in. The station never keeps a caller's number: a
// caller is the last four digits at most (`···0123`) or *Withheld*, and a ban is a key made
// from the number, never the number.
//
// What is an install setting has no route and is not written from here: whether the line is on
// (it needs a trunk: the provider's connection, its number and its addresses). Whether the words
// are played and which shipped voice plays are installation settings (`PHONE_CONSENT_WORDS`,
// `PHONE_DEFAULT_VOICE`) that an admin may change here (`PUT /api/v1/phone/consent`, 1.2): what is
// changed here is kept by the station and wins over the settings file.

import { ApiError, FieldError, base, headers, netFetch, type Caller } from './api.ts';
import { firstName } from './format.ts';

export type WordsProblem = 'recording_out_of_date' | 'default_missing' | 'no_recording';
/** Why the line takes no calls: `off` (not turned on, or no trunk set), `no_key`, or the words. */
export type Closed = 'off' | 'no_key' | { words: WordsProblem };

export interface PhoneRecording {
  duration_ms: number;
  recorded_by: string;
  recorded_at_ms: number;
  /** Made from other words than today's: the line is closed until it is recorded again. */
  out_of_date: boolean;
}

/** `GET /api/v1/phone`. */
export interface PhoneStatus {
  open: boolean;
  closed: Closed | null;
  lines: number;
  at_gate: number;
  in_queue: number;
  /** `off`, `default` (the recording that comes with WeaveSignal) or `station` (its own). */
  words: { kind: 'off' | 'default' | 'station'; version: string; text: string } | null;
  words_to_record: string | null;
  recording: PhoneRecording | null;
  consent_words_help: string;
  /** Whether the consent words play (the studio's setting, else `phone.consent_words`). Absent
   *  from a station older than 1.2's last build. */
  consent_words?: boolean;
  /** Which shipped recording plays until the station records its own (the operator,
   *  2026-11-20): `alba` (the default) or `edinburgh`, or `own`, none: only the station's own
   *  recording, which once made plays instead of either (`phone.default_voice`,
   *  `PHONE_DEFAULT_VOICE`, changed with `PUT /api/v1/phone/consent`). */
  default_voice?: ConsentVoice | null;
  default_voice_help?: string;
}

/** `phone.default_voice`: `alba` (the default) or `edinburgh`, which come with WeaveSignal, or
 *  `own`, none of them: the station's own recording only. */
export type ConsentVoice = 'alba' | 'edinburgh' | 'own';
export const DEFAULT_VOICES: readonly { voice: Exclude<ConsentVoice, 'own'>; name: string; who: string }[] = [
  { voice: 'alba', name: 'Alba', who: 'a Scottish woman' },
  { voice: 'edinburgh', name: 'Edinburgh', who: 'a man' },
];

/** Whether this station lets the screen change the consent settings: it says what is in force
 *  (`consent_words`), so it has `PUT /api/v1/phone/consent`, and the line is on. */
export const canSetConsent = (s: PhoneStatus | null | undefined): boolean => !!s && typeof s.consent_words === 'boolean' && s.default_voice != null && s.closed !== 'off';

/** Which recording a caller hears, as the station says: its own whenever it has one (it plays
 *  instead of either voice), else the shipped voice chosen (`own` with none: nothing, and the
 *  line is closed while the words are on). Null when it cannot be told. */
export function voiceInForce(s: PhoneStatus): ConsentVoice | null {
  if (s.words?.kind === 'station') return 'own';
  return s.default_voice ?? null;
}

/** Where a shipped voice can be heard (staff; the line must be on). */
export const previewPath = (voice: Exclude<ConsentVoice, 'own'>) => `/api/v1/phone/consent/default/${voice}.opus`;

/** One of the default voices, as sound to play; rejects when the station has no preview. */
export async function previewVoice(voice: Exclude<ConsentVoice, 'own'>): Promise<Blob> {
  const res = await netFetch(`${base()}${previewPath(voice)}`, { headers: headers(false) });
  if (!res.ok) throw new ApiError(res.status, `${res.status} ${res.statusText}`);
  return res.blob();
}

/** Change whether the words play, or which shipped voice plays (an admin's): kept by the
 *  station, over the settings file, from the next call. */
export const setConsent = (c: { consent_words?: boolean; default_voice?: ConsentVoice }) => ask<PhoneStatus>('PUT', '/api/v1/phone/consent', c);

export interface PhoneBan {
  ban_key: string;
  /** The last digits as the screener saw them (at most four). */
  tail: string;
  date: string;
  reason: string;
  expires_ms: number | null;
}

async function ask<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await netFetch(`${base()}${path}`, { method, headers: headers(body !== undefined), body: body === undefined ? undefined : JSON.stringify(body) });
  if (res.status === 204) return undefined as T;
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new FieldError(res.status, data.error ?? `${res.status} ${res.statusText}`, data.field ?? null, data);
  return data as T;
}

export const phoneStatus = () => ask<PhoneStatus>('GET', '/api/v1/phone');
export const phoneBans = () => ask<PhoneBan[]>('GET', '/api/v1/phone/bans');
/** Ban a phone caller in the queue (staff). `days` absent: until it is lifted. */
export const banCaller = (identity: string, reason: string, days: number | null) => ask<PhoneBan[]>('POST', '/api/v1/phone/bans', { identity, reason, ...(days ? { days } : {}) });
/** Ban a number as typed (an admin's). The number is used for the request only. */
export const banNumber = (number: string, reason: string, days: number | null) => ask<PhoneBan[]>('POST', '/api/v1/phone/bans', { number, reason, ...(days ? { days } : {}) });
export const liftBan = (banKey: string) => ask<void>('DELETE', `/api/v1/phone/bans/${encodeURIComponent(banKey)}`);
/** The consent records of the calls a number may have made (an admin's). */
export const findCalls = (number: string) => ask<{ consents: number[] }>('POST', '/api/v1/phone/takedown', { number });
export const removeWords = () => ask<PhoneStatus>('DELETE', '/api/v1/phone/words');

/** Keep a recording of the phone words (an admin's): any sound file the station reads. */
export async function putWords(file: Blob): Promise<PhoneStatus> {
  const h = headers(false) as Record<string, string>;
  h['Content-Type'] = 'application/octet-stream';
  const res = await netFetch(`${base()}/api/v1/phone/words`, { method: 'PUT', headers: h, body: file });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, data.error ?? `${res.status} ${res.statusText}`);
  return data as PhoneStatus;
}

// ---- the station's own takedown contact (the setup page's routes, ADR-0237) ----

interface Answer {
  value: string;
  source: 'page' | 'settings';
  settings_value: string;
}
export interface ContactView {
  contact: Answer;
  name: Answer;
}
export const stationContact = () => ask<ContactView>('GET', '/api/v1/setup');
/** Save the station's takedown contact. The route takes the name and the contact together and
 *  hands back to the settings file whichever is left out, so a name set on the setup page is
 *  sent again as it is, and one that is the settings file's is left out. */
export const saveStationContact = (view: ContactView, contact: string) =>
  ask<ContactView>('PUT', '/api/v1/setup', { ...(view.name.source === 'page' ? { name: view.name.value } : {}), contact: contact.trim() });

export const CONTACT_MAX = 200;
export function contactProblem(contact: string): string | null {
  const c = contact.trim();
  if ([...c].length > CONTACT_MAX) return 'At most 200 characters: an email address or a link.';
  if (/[\u0000-\u001f\u007f]/.test(c)) return 'Keep it on one line.';
  return null;
}

// ---- words ----

/** The tooltip beside the consent-words setting: whether a station needs them. */
export const WORDS_TIP = 'An internet-only station may not need it. Stations that broadcast over radio frequencies, or that record calls, often should; check your own requirements.';

/** Where the two install settings are set: the same sentence wherever one is shown. */
export const SET_AT_INSTALL = 'Set where the station is installed: in its settings file with weavesignal.sh, or in weavesignal.env on Cloudron.';

export interface LineWords {
  /** `open`, `off`, or `closed` (on, and taking no calls until something is put right). */
  state: 'open' | 'off' | 'closed';
  /** The chip's word. */
  word: string;
  /** One line on where the line stands. */
  line: string;
  /** What puts it right, when it is closed or off. */
  fix: string | null;
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** Where the line stands, for the screener and Settings. */
export function lineWords(s: PhoneStatus): LineWords {
  if (s.closed === 'off')
    return {
      state: 'off',
      word: 'Off',
      line: 'The phone line is off until a trunk is set.',
      fix: 'A trunk is the phone provider’s connection to this station: its number and the addresses it rings from. It is set where the station is installed (see “Take phone calls” in the documentation).',
    };
  if (s.closed === 'no_key') return { state: 'closed', word: 'Closed', line: 'The line is on and takes no calls: the station has no key to make ban keys with.', fix: 'Set the station key or the token key where the station is installed, then restart it.' };
  if (s.closed && typeof s.closed === 'object') {
    if (s.closed.words === 'recording_out_of_date') return { state: 'closed', word: 'Closed', line: 'The line takes no calls: the station’s recording says older words than today’s.', fix: 'Record the words again below. The line opens as soon as you do.' };
    if (s.closed.words === 'no_recording') return { state: 'closed', word: 'Closed', line: 'The line takes no calls: it is set to play only the station’s own recording, and there is none yet.', fix: 'Record the station’s own words below, or choose Alba or Edinburgh.' };
    return { state: 'closed', word: 'Closed', line: 'The line takes no calls: the words are on, and the recordings that come with WeaveSignal are missing from this installation.', fix: 'Record the station’s own words below, or install the station again from its image, which carries them.' };
  }
  if (!s.open) return { state: 'closed', word: 'Closed', line: 'The line is on, and call-in is closed just now.', fix: 'It opens with call-in: the show’s step, and the station’s takedown contact.' };
  const busy = [s.at_gate ? `${s.at_gate} hearing the words` : null, s.in_queue ? `${s.in_queue} in the queue` : null].filter(Boolean).join(' · ');
  return { state: 'open', word: 'Taking calls', line: `Taking calls on ${plural(s.lines, 'line', 'lines')}${busy ? ` · ${busy}` : ''}.`, fix: null };
}

/** Which words a caller hears before the queue, in one line. */
export function wordsLine(s: PhoneStatus): string {
  if (!s.words) return 'The line is off, so nothing is played.';
  if (s.words.kind === 'off') return 'Nothing is played: callers go straight into the queue, and the station’s records say the words were not played.';
  if (s.words.kind === 'station') return 'The station’s own recording.';
  return 'The short recording that comes with WeaveSignal.';
}

/** *7 s*, *1 min 05 s*. */
export function lengthWords(ms: number): string {
  const s = Math.round(ms / 1000);
  return s < 60 ? `${s} s` : `${Math.floor(s / 60)} min ${String(s % 60).padStart(2, '0')} s`;
}

/** A phone caller's line as their card says it: *Phone ···0123*, or *Withheld number*. */
export function phoneLine(c: Pick<Caller, 'phone'>): string | null {
  if (!c.phone) return null;
  return c.phone.withheld ? 'Withheld number' : `Phone ${c.phone.number}`;
}

/** What a caller is called before the screener types a name: a browser caller's name or
 *  identity; a phone caller's line, never their internal key. */
export function callerName(c: Pick<Caller, 'phone' | 'name' | 'identity'>): string {
  return c.name || phoneLine(c) || c.identity;
}

/** A phone caller at the desk: the station's key for them begins `phone-` (crates/proto). */
export const isPhonePerson = (identity: string): boolean => identity.startsWith('phone-') && identity.length > 6;

/** A person at the desk by a short name for a sentence or a button. A phone caller keeps the
 *  whole of the name the desk has for them (*Phone 4471*: not its first word), and is *the
 *  phone caller* where it has none: never their key. */
export function deskName(p: { name: string; identity: string }, nameOnAir?: string | null): string {
  if (nameOnAir) return nameOnAir;
  if (!isPhonePerson(p.identity)) return firstName(p.name, p.identity);
  return p.name.trim() || 'the phone caller';
}

export const WITHHELD_BAN = 'Cannot ban a withheld number';
export const WITHHELD_WHY = 'Every caller who withholds their number looks the same to the station, so a ban cannot single this one out. Drop the call instead.';

/** How long a ban lasts: the choices offered. */
export const BAN_LENGTHS: readonly { days: number | null; label: string }[] = [
  { days: 7, label: '7 days' },
  { days: 30, label: '30 days' },
  { days: 90, label: '90 days' },
  { days: null, label: 'Until it is lifted' },
];

export function banProblem(reason: string): string | null {
  return reason.trim() ? null : 'Say why, in a few words: the next screener reads it when this number rings again.';
}

/** A ban's row: *···0123 · banned 2026-11-20 · until 20 December*. */
export function banUntil(b: PhoneBan): string {
  if (b.expires_ms == null) return 'until it is lifted';
  return `until ${new Date(b.expires_ms).toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric', timeZone: 'UTC' })}`;
}
