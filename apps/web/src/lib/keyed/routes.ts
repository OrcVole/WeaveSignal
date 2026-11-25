// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Keyed shows on the public pages (1.3.2): every call a listener's key makes, and every address
// a key's holder plays, in this one module (the routes are the control plane's
// `/api/v1/keyed/*`, in docs/openapi). The keyed addresses themselves come from the station, in
// the redeem's `urls`, with the token in their path: this module only picks among them and
// carries a fresh token into an address made with an older one. Nothing outside this file
// writes a keyed path.
//
// No bearer anywhere: the key is the credential for redeeming and for its devices, and the
// token it earns is the credential for renewing and for the stream. The token is opaque to the
// page: it is kept in memory, never in storage, and never read.

import type { StreamChoice } from '../streams.ts';

/** The control plane's routes for a listener with a key. */
export const ROUTES = {
  redeem: '/api/v1/keyed/redeem',
  renew: '/api/v1/keyed/renew',
  devices: '/api/v1/keyed/devices',
  endDevice: '/api/v1/keyed/devices/end',
  lost: '/api/v1/keyed/lost',
  keyedShows: '/api/v1/public/keyed-shows',
} as const;

/** Where a key's holder hears a keyed show, from the station's own `urls`: the keyed MP3 stream
 *  when the listener chose an MP3 stream and the station has one, else the keyed Opus stream.
 *  Keyed sound is served by the station itself, never an edge's relay, so a relative address is
 *  the page's own origin. */
export function keyedStream(urls: KeyedUrls, publicUrl: string, page = globalThis.location?.href ?? 'http://localhost/'): string {
  const mp3 = /\.mp3$/i.test(new URL(publicUrl || '/', page).pathname);
  return new URL(mp3 && urls.mp3 ? urls.mp3 : urls.opus, page).href;
}

/** The keyed HLS, picture and sound (`live`) and its sound alone (`audio`, beside it), from the
 *  station's `urls.hls`. Every playlist and segment under it carries the token in its path, since
 *  they are relative to it. */
export function keyedHls(urls: KeyedUrls, page = globalThis.location?.href ?? 'http://localhost/'): { live: string; audio: string } {
  const live = new URL(urls.hls, page);
  const audio = new URL(live.href);
  audio.pathname = live.pathname.replace(/[^/]*$/, 'audio.m3u8');
  return { live: live.href, audio: audio.href };
}

/** The streams a station offers, as a key's holder plays them: the same choices, each on its
 *  keyed address. */
export function keyedStreams(streams: StreamChoice[], urls: KeyedUrls, page?: string): StreamChoice[] {
  return streams.map((s) => ({ ...s, url: keyedStream(urls, s.url, page) }));
}

/** A keyed address with the token the page holds now in the place of the one it was made
 *  with: a playlist that was loaded a minute ago names its segments under the old one. */
export function withToken(url: string, token: string): string {
  return url.replace(/\/k\/([^/]+)\/[^/]+\//, (_m, show: string) => `/k/${show}/${token}/`);
}

/** What a keyed address says when it refuses, which an audio element cannot read: `null` when
 *  it plays (the answer's body is not read) or did not answer. The codes the page waits on are
 *  `WAIT_CODES`: a change of keyed show, a window not yet open. */
export async function streamRefusal(url: string): Promise<string | null> {
  if (!url) return null;
  const stop = new AbortController();
  try {
    const res = await fetch(url, { cache: 'no-store', signal: stop.signal });
    if (res.ok) {
      stop.abort();
      return null;
    }
    const data = (await res.json().catch(() => ({}))) as { code?: unknown };
    return typeof data.code === 'string' ? data.code : `status_${res.status}`;
  } catch {
    return null;
  }
}

/** A keyed stream refused for a moment: the next keyed show waits for the last one's holders
 *  (`changing_show`), or the window has not opened yet (`not_on_now`, `not_yet`). */
export const WAIT_CODES: readonly string[] = ['changing_show', 'not_on_now', 'not_yet'];

/** A key out of the page's address: the link staff send is `…/listen/#key=KETM-7QMP-2D4X`, the
 *  key after the `#`, which a browser never sends to a server. Null when there is none. */
export function keyInAddress(hash: string): string | null {
  const m = /(?:^#|&)key=([^&]+)/.exec(hash);
  if (!m) return null;
  try {
    return decodeURIComponent(m[1]).trim() || null;
  } catch {
    return m[1].trim() || null;
  }
}

/** Whether a page's path is `/listen/`, where a key's link leads (`…/listen/#key=…`). The same
 *  player is at `/`, which offers the key box only when the station has a show that needs one;
 *  at `/listen/` the box is always there, for a link that arrived without its key. */
export function isKeyPage(pathname: string): boolean {
  return /^\/listen(\/(index\.html)?)?$/.test(pathname);
}

/** The address without its key, so a copied address does not carry the key onward. */
export function withoutKey(href: string): string {
  const u = new URL(href);
  const rest = u.hash
    .replace(/^#/, '')
    .split('&')
    .filter((p) => p && !p.startsWith('key='));
  u.hash = rest.length ? rest.join('&') : '';
  return u.href.replace(/#$/, '');
}

export type KeyKind = 'personal' | 'group' | 'machine';
export type KeyMedia = 'sound' | 'video' | 'both';

/** What the holder is told about their key: enough for the welcome. */
export interface ListenKey {
  id: string;
  kind: KeyKind;
  /** The name staff gave it, to find it again. The station sends it; the page never shows it
   *  and does not keep it (words.ts, `GREETING`). */
  label: string;
  /** `video` opens what `both` does: a key for the picture carries the show's sound. */
  media: KeyMedia;
  all_shows: boolean;
  /** The shows it opens when it is not `all_shows`. */
  shows: { id: string; slug: string; name: string }[];
  valid_from_ms?: number | null;
  valid_until_ms?: number | null;
  /** The key ends within the station's `keys.ends_soon_days`: say so. */
  ends_soon: boolean;
}

/** A show, as the station names it to a key's holder. */
export interface ListenShow {
  id: string;
  slug: string;
  name: string;
}

/** The keyed addresses for one show's window, the token in their path. */
export interface KeyedUrls {
  /** The keyed HLS the key opens: `audio.m3u8` (the sound alone) for a sound key, `live.m3u8`
   *  (picture and sound, `audio.m3u8` beside it) for a key with the picture (`video` or `both`). */
  hls: string;
  /** The keyed Opus stream, at the station. */
  opus: string;
  /** The keyed MP3 stream, when the station's MP3 stream is on. */
  mp3?: string | null;
}

/** A token for one show's window: the show, when it opens, when it ends, and its addresses. */
export interface ListenToken {
  token: string;
  /** This browser's id on the key: kept, and sent with the next redeem. */
  device: string;
  /** The window's lead: a token for a show later today waits until then. */
  starts_ms: number;
  expires_ms: number;
  /** The one show the token opens. */
  show: ListenShow;
  urls: KeyedUrls;
  /** Renew about this often, in seconds. */
  renew_in_s: number;
  key: ListenKey;
}

export interface Device {
  id: string;
  /** The browser or app, as it named itself: `Firefox on Linux`. */
  label: string;
  first_seen_ms: number;
  last_seen_ms: number;
  ended_ms?: number | null;
  last_show_id?: string | null;
}

export interface KeyedShow {
  id: string;
  slug: string;
  name: string;
  /** `key`: only a key opens it now; `public`: it has opened to everyone. */
  now: 'key' | 'public';
  opens_at_ms?: number | null;
  /** What the public hears while it is keyed. */
  slate: 'announcement' | 'fallback' | 'teaser';
  slate_note: string;
}

export interface KeyedShows {
  shows: KeyedShow[];
  /** The keyed show whose window is open now, if one is. */
  on_now?: string | null;
  /** Where to get a key, in the station's words and as a link; either may be empty. */
  get_a_key_text: string;
  get_a_key_url: string;
  /** The station sends email (1.3.2), so *Lost your key?* can say the key is emailed again;
   *  false: its staff answer the request themselves. Absent from a station that does not say. */
  mail?: boolean;
}

/** The station's codes for a key that does not work, and the page's own for the rest. */
export type RefusalCode =
  | 'not_recognised'
  | 'not_yet_valid'
  | 'ended'
  | 'paused'
  | 'revoked'
  | 'does_not_open_show'
  | 'too_many_devices'
  | 'people_limit'
  | 'listens_used_up'
  /** The key works, but none of its shows (or not the one asked for) is on the schedule now or
   *  in the next fortnight. */
  | 'not_scheduled'
  | 'device_signed_out'
  /** The token has ended or is not one: the page redeems the key again, and says nothing. */
  | 'token_ended'
  | 'token_invalid'
  /** Too many tries from this connection (429). */
  | 'slow_down'
  /** Keys are not set up at this station (503), or it is older than 1.3.2 (404 on the route). */
  | 'not_set_up'
  /** The station did not answer. */
  | 'no_answer';

/** A key that did not work: the code, and what the station sent with it. The station's own
 *  sentence is not shown (it is written for a program, and two of them say "token"): the page
 *  has words of its own for every code (words.ts). */
export class Refusal extends Error {
  constructor(
    readonly code: RefusalCode,
    readonly status: number,
    /** When the key starts (`not_yet_valid`) or ended (`ended`). */
    readonly at_ms: number | null = null,
    /** The devices that hold the key (`too_many_devices`). */
    readonly devices: Device[] = [],
    /** How long to wait, in seconds (`slow_down`). */
    readonly retry_s: number | null = null,
    /** How long a device holds its place on the key, in seconds (`too_many_devices`). */
    readonly hold_s: number | null = null,
  ) {
    super(code);
  }
}

const CODES: readonly string[] = ['not_recognised', 'not_yet_valid', 'ended', 'paused', 'revoked', 'does_not_open_show', 'not_scheduled', 'too_many_devices', 'people_limit', 'listens_used_up', 'device_signed_out', 'token_ended', 'token_invalid'];

/** A refusal read from the station's answer. */
export function refusalOf(status: number, data: Record<string, unknown>, retryAfter: string | null = null): Refusal {
  const n = (v: unknown) => (typeof v === 'number' ? v : null);
  if (status === 429) return new Refusal('slow_down', status, null, [], retryAfter && /^\d+$/.test(retryAfter) ? Number(retryAfter) : null);
  const code = typeof data.code === 'string' && CODES.includes(data.code) ? (data.code as RefusalCode) : null;
  if (code) return new Refusal(code, status, n(data.from_ms) ?? n(data.until_ms), Array.isArray(data.devices) ? (data.devices as Device[]) : [], null, n(data.device_hold_s));
  if (status === 503 || status === 405) return new Refusal('not_set_up', status);
  // A 404 with no code is a station that has no such route: older than 1.3.2.
  if (status === 404) return new Refusal('not_set_up', status);
  return new Refusal('no_answer', status);
}

async function post<T>(path: string, body: unknown, expectBody: boolean | 'if-any' = true): Promise<T> {
  let res: Response;
  try {
    res = await fetch(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body), cache: 'no-store' });
  } catch {
    throw new Refusal('no_answer', 0);
  }
  // `if-any`: a body an older station does not send (its 202 was empty) is read where there is one.
  if (res.ok && expectBody === 'if-any') return (await res.json().catch(() => undefined)) as T;
  if (res.ok) return (expectBody && res.status !== 204 && res.status !== 202 ? await res.json() : undefined) as T;
  const data = (await res.json().catch(() => ({}))) as Record<string, unknown>;
  throw refusalOf(res.status, data, res.headers.get('Retry-After'));
}

/** Enter a key: trade its text for a token. `show` narrows the token to the show wanted (id or
 *  slug); `device` is this browser's id from last time. */
export const redeem = (key: string, o: { show?: string | null; device?: string | null } = {}) =>
  post<ListenToken>(ROUTES.redeem, { key, ...(o.show ? { show: o.show } : {}), ...(o.device ? { device: o.device } : {}) });

/** Trade a token that has not ended for a fresh one. A key revoked, paused, ended, reduced or
 *  signed out on this device is refused here. */
export const renew = (token: string) => post<ListenToken>(ROUTES.renew, { token });

/** The browsers and apps that hold the key, the live ones first, and how long a device holds
 *  its place (seconds): one seen within it is listening now. */
export const devices = (key: string) => post<{ devices: Device[]; device_hold_s?: number }>(ROUTES.devices, { key }).then((r) => ({ devices: r.devices, hold_s: r.device_hold_s ?? null }));

/** Sign a device out of the key: its place is free at once. */
export const endDevice = (key: string, device: string) => post<void>(ROUTES.endDevice, { key, device }, false);

/** Ask for a lost key by the email address it was made with. The station answers the same
 *  whether one matched or not. From 1.3.2 its answer says whether the station sends email at
 *  all (`mail`: a fact about the station, never the address): with false nothing is emailed, the
 *  request is noted for the station's staff, and the page says so. Null from a station that does
 *  not say (an empty answer), which the page reads as the list of keyed shows has it, else as
 *  it always did. */
export async function lost(email: string): Promise<{ mail: boolean | null }> {
  const said = await post<{ mail?: unknown } | undefined>(ROUTES.lost, { email }, 'if-any');
  return { mail: typeof said?.mail === 'boolean' ? said.mail : null };
}

/** The keyed shows, or null from a station that has none of this (older than 1.3.2) or does
 *  not answer: the page then shows nothing about keys. */
export async function keyedShows(): Promise<KeyedShows | null> {
  try {
    const res = await fetch(ROUTES.keyedShows, { cache: 'no-store' });
    if (!res.ok) return null;
    const data = (await res.json()) as KeyedShows;
    return Array.isArray(data.shows) ? data : null;
  } catch {
    return null;
  }
}
