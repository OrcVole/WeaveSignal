// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Keyed shows (1.3.2): the staff routes for listener keys, key families, the leak report and a
// show's access rule, as the control plane answers them (docs/openapi/weavesignal-v1.json, the
// `keys` tag; keys.test.ts checks these names against it). A producer's, an admin's or the
// station key's; a presenter or a screener is refused (403). The route names are in one place
// (`ROUTES`), so a renamed route is one line.
//
// A listener key is not a show key: a show key (Settings, Own studios) lets a host's software
// send to the station; a listener key lets a listener hear a keyed show.

import { FieldError, base, headers, netFetch } from './api.ts';

const id = encodeURIComponent;

export const ROUTES = {
  keys: '/api/v1/listener-keys',
  key: (k: string) => `/api/v1/listener-keys/${id(k)}`,
  text: (k: string) => `/api/v1/listener-keys/${id(k)}/text`,
  extend: (k: string) => `/api/v1/listener-keys/${id(k)}/extend`,
  pause: (k: string) => `/api/v1/listener-keys/${id(k)}/pause`,
  resume: (k: string) => `/api/v1/listener-keys/${id(k)}/resume`,
  revoke: (k: string) => `/api/v1/listener-keys/${id(k)}/revoke`,
  replace: (k: string) => `/api/v1/listener-keys/${id(k)}/replace`,
  log: (k: string) => `/api/v1/listener-keys/${id(k)}/log`,
  devices: (k: string) => `/api/v1/listener-keys/${id(k)}/devices`,
  device: (k: string, d: string) => `/api/v1/listener-keys/${id(k)}/devices/${id(d)}`,
  find: '/api/v1/listener-keys/find',
  leaks: '/api/v1/listener-keys/leaks',
  families: '/api/v1/listener-keys/families',
  family: (f: string) => `/api/v1/listener-keys/families/${id(f)}`,
  familyExport: (f: string) => `/api/v1/listener-keys/families/${id(f)}/export`,
  access: (show: string) => `/api/v1/shows/${id(show)}/access`,
  keyedShows: '/api/v1/public/keyed-shows',
} as const;

export type KeyKind = 'personal' | 'group' | 'machine';
export type KeyMedia = 'sound' | 'video' | 'both';
/** Where a key is in its life. A key whose window has ended is still `active`: the window decides. */
export type KeyState = 'active' | 'paused' | 'revoked';

/** A key as staff see it: everything but its text and its email address. */
export interface ListenerKey {
  id: string;
  kind: KeyKind;
  /** Who it is for: a person's name, a group's, a partner's, or a ticket number. */
  label: string;
  note: string;
  /** Whether an email address is kept; the address itself comes only with the key's text. */
  has_email: boolean;
  media: KeyMedia;
  /** Opens every keyed show. */
  all_shows: boolean;
  /** The shows it opens when it is not `all_shows` (show ids). */
  shows: string[];
  valid_from_ms?: number | null;
  valid_until_ms?: number | null;
  /** Browsers or apps that may hold it at once; null: any number. */
  max_devices?: number | null;
  /** How many times it may start a listen; null: any number. */
  max_listens?: number | null;
  /** How many different devices may ever use it (a group key's people); null: any number. */
  max_people?: number | null;
  state: KeyState;
  state_reason: string;
  listens_used: number;
  first_used_ms?: number | null;
  last_used_ms?: number | null;
  family_id?: string | null;
  replaces_id?: string | null;
  replaced_by_id?: string | null;
  created_by: string;
  created_ms: number;
  updated_ms: number;
}

/** What a new key is. Left out, a key opens every keyed show, for sound and video, on two
 *  devices at a time, with no end. */
export interface KeySpec {
  label: string;
  kind?: KeyKind;
  email?: string | null;
  note?: string;
  media?: KeyMedia;
  all_shows?: boolean | null;
  /** By id or slug, at most 32. */
  shows?: string[];
  valid_from_ms?: number | null;
  valid_until_ms?: number | null;
  max_devices?: number | null;
  max_listens?: number | null;
  max_people?: number | null;
  /** A family to issue it in. */
  family?: string | null;
}

/** A change to a key: only the fields sent change; a window or a limit sent as `null` is taken away. */
export type KeyChange = Partial<Omit<KeySpec, 'kind' | 'family'>>;

/** What every key of a family is. */
export type KeyTemplate = Omit<KeySpec, 'label' | 'email' | 'note' | 'family'>;

/** The answer to making a key: the key, its text and its link, this once. */
export interface MadeListenerKey {
  key: ListenerKey;
  /** Written `KETM-7QMP-2D4X`. */
  text: string;
  /** Opens the player with the key entered (the key is after the `#`). */
  link: string;
}

export interface KeyText {
  text: string;
  link: string;
  /** The email address kept with the key, if any. */
  email?: string | null;
}

export interface KeyFamily {
  id: string;
  label: string;
  note: string;
  /** How many keys it has issued (replaced ones included). */
  keys: number;
  created_by: string;
  created_ms: number;
}

export interface KeyRecipient {
  label: string;
  email?: string | null;
  note?: string;
}

export interface FamilySpec {
  label: string;
  note?: string;
  template: KeyTemplate;
  /** One key per recipient, named… */
  recipients?: KeyRecipient[];
  /** …or this many anonymous keys, `<prefix> 001`, `<prefix> 002`. */
  count?: number | null;
  prefix?: string | null;
}

export interface MadeFamily {
  family: KeyFamily;
  keys: MadeListenerKey[];
}

export interface KeyDevice {
  id: string;
  /** The browser or app, as it named itself. */
  label: string;
  first_seen_ms: number;
  last_seen_ms: number;
  ended_ms?: number | null;
  last_show_id?: string | null;
}

export interface KeyLogEntry {
  /** `created`, `changed`, `extended`, `paused`, `resumed`, `revoked`, `replaced`, `text_shown`,
   *  `lost_key_asked`, `device_ended`. */
  action: string;
  actor: string;
  at_ms: number;
  detail: Record<string, unknown>;
}

/** One key's row in the leak report. */
export interface KeyLeak {
  key: ListenerKey;
  /** The family's label, when it has one. */
  family?: string | null;
  first_seen_ms?: number | null;
  last_seen_ms?: number | null;
  listens: number;
  devices: number;
  addresses: number;
  addresses_24h: number;
  addresses_7d: number;
  peak_new_addresses_in_an_hour: number;
  /** A burst of new addresses: what a key posted in public looks like. */
  spike: boolean;
  /** "Most likely passed on": a hint, a sum of signs, with the signs in `reasons`. */
  score: number;
  reasons: string[];
}

export interface LeakReport {
  generated_ms: number;
  /** The most likely passed on first; keys never used are last. */
  entries: KeyLeak[];
}

export type AccessRule = 'public' | 'key';
export type Slate = 'announcement' | 'fallback' | 'teaser';

/** A show's access rule. A show with none set is public. */
export interface ShowAccess {
  show_id: string;
  rule: AccessRule;
  /** From this moment the show is public whatever the rule says. */
  opens_at_ms?: number | null;
  /** What the public hears while the show is keyed. */
  slate: Slate;
  slate_note: string;
  updated_by: string;
  updated_ms: number;
}

export interface ShowAccessSet {
  rule: AccessRule;
  opens_at_ms?: number | null;
  slate?: Slate;
  slate_note?: string;
}

/** A keyed show as the public route lists it. */
export interface KeyedShow {
  id: string;
  slug: string;
  name: string;
  /** `key`: only a key opens it now; `public`: it has opened to everyone. */
  now: AccessRule;
  opens_at_ms?: number | null;
  slate: Slate;
  slate_note: string;
}

export interface KeyedShows {
  shows: KeyedShow[];
  /** Where to get a key, in the station's words; may be empty. */
  get_a_key_text: string;
  get_a_key_url: string;
  /** The station sends email (1.3.2): *Lost your key?* on the player emails a key's link again.
   *  False: nothing is sent, the request is logged on the key, and staff send it themselves.
   *  Absent from a station that does not say. */
  mail?: boolean;
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await netFetch(`${base()}${path}`, {
    method,
    headers: headers(body !== undefined),
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (res.status === 204) return undefined as T;
  const text = await res.text().catch(() => '');
  let data: { error?: string; field?: string } & Record<string, unknown> = {};
  try {
    data = text ? JSON.parse(text) : {};
  } catch {
    data = {};
  }
  if (!res.ok) throw new FieldError(res.status, typeof data.error === 'string' && data.error ? data.error : `${res.status} ${res.statusText}`.trim(), data.field ?? null, data);
  return data as T;
}

export interface KeyFilter {
  family?: string;
  state?: KeyState;
  kind?: KeyKind;
  show?: string;
  q?: string;
  limit?: number;
  offset?: number;
}

function query(f: object): string {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(f)) if (v !== undefined && v !== null && v !== '') q.set(k, String(v));
  const s = q.toString();
  return s ? `?${s}` : '';
}

/** Keys, newest first, narrowed; never a key's text. `total` is how many match before `limit`. */
export const listKeys = (f: KeyFilter = {}) => call<{ keys: ListenerKey[]; total: number }>('GET', `${ROUTES.keys}${query(f)}`);
export const getKey = (k: string) => call<ListenerKey>('GET', ROUTES.key(k));
export const makeKey = (spec: KeySpec) => call<MadeListenerKey>('POST', ROUTES.keys, spec);
export const changeKey = (k: string, change: KeyChange) => call<ListenerKey>('PATCH', ROUTES.key(k), change);
/** Erase a key and everything kept about its use (an admin's; ADR-0122). */
export const eraseKey = (k: string) => call<void>('DELETE', ROUTES.key(k));
/** A later end: days added to the end (or to now, if it has ended), or the new end. */
export const extendKey = (k: string, by: { by_days: number } | { until_ms: number }) => call<ListenerKey>('POST', ROUTES.extend(k), by);
export const pauseKey = (k: string, reason = '') => call<ListenerKey>('POST', ROUTES.pause(k), reason ? { reason } : {});
export const resumeKey = (k: string) => call<ListenerKey>('POST', ROUTES.resume(k), {});
export const revokeKey = (k: string, reason = '') => call<ListenerKey>('POST', ROUTES.revoke(k), reason ? { reason } : {});
/** Revoke it and issue a new key to the same recipient, in one step. */
export const replaceKey = (k: string) => call<{ old: ListenerKey; new: ListenerKey; text: string; link: string }>('POST', ROUTES.replace(k), {});
/** The key's text, link and email again. The station logs who asked. */
export const keyText = (k: string) => call<KeyText>('POST', ROUTES.text(k), {});
export const keyLog = (k: string) => call<{ entries: KeyLogEntry[] }>('GET', ROUTES.log(k)).then((r) => r.entries);
export const keyDevices = (k: string) => call<{ devices: KeyDevice[] }>('GET', ROUTES.devices(k)).then((r) => r.devices);
export const endDevice = (k: string, device: string) => call<void>('DELETE', ROUTES.device(k, device));
/** Whose key a text found somewhere is; 404 when it is no key of this station. */
export const findKey = (text: string) => call<ListenerKey>('POST', ROUTES.find, { text });
export const leakReport = (family?: string) => call<LeakReport>('GET', `${ROUTES.leaks}${query({ family })}`);
export const listFamilies = () => call<{ families: KeyFamily[] }>('GET', ROUTES.families).then((r) => r.families);
export const makeFamily = (spec: FamilySpec) => call<MadeFamily>('POST', ROUTES.families, spec);

/** A family's keys as CSV (`name,email,note,key,link,state`), as the station writes it. It
 *  holds every key's text, so the station logs it on each key. */
export async function familyCsv(family: string): Promise<string> {
  const res = await netFetch(`${base()}${ROUTES.familyExport(family)}`, { headers: headers(false) });
  if (!res.ok) {
    const data = (await res.json().catch(() => ({}))) as { error?: unknown };
    throw new FieldError(res.status, typeof data.error === 'string' && data.error ? data.error : 'The station did not give the family’s keys. Try again.', null);
  }
  return res.text();
}

/** How tokens are signed and how long one lives: a change to a key reaches someone who is
 *  listening within a token's life. */
export interface KeySigning {
  alg: string;
  prefix: string;
  public_key?: string | null;
  token_ttl_s: number;
  /** A player renews about this often: a change other than a pause, a revoke or a replace reaches a listener within it. */
  renew_every_s: number;
  clock_skew_s: number;
  /** A key ends "soon" this many days before its end (absent from a station before 1.3.2's integration). */
  ends_soon_days?: number;
  /** A device holds its place on a key while it has renewed within this many seconds. */
  device_hold_s?: number;
}
export const keySigning = () => call<KeySigning>('GET', '/api/v1/listener-keys/signing');

export const showAccess = (show: string) => call<ShowAccess>('GET', ROUTES.access(show));
export const setShowAccess = (show: string, set: ShowAccessSet) => call<ShowAccess>('PUT', ROUTES.access(show), set);
/** The keyed shows, as anyone may read them: for the lock on a show and what the public hears. */
export const keyedShows = () => call<KeyedShows>('GET', ROUTES.keyedShows);
