// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The studio app's link to the control plane: the studio's view as server-sent events, and
// the commands. The routes need a staff member's sign-in or the station key (ADR-0127); the
// packaged app keeps either in the keychain (ADR-0121). The browser build is given one
// (`?token=` once, kept for this session) or runs behind a proxy that adds it (the loopback
// test run's Vite proxy), so the browser build need not hold it.
//
// Server-sent events are read with fetch, not EventSource, because EventSource cannot send
// the Authorization header.

import type { CartsView, Catalogue } from '../../../../packages/ui/carts.ts';
import type { StationMode } from '../../../../packages/ui/states.ts';

export type Role = 'presenter' | 'dj' | 'guest' | 'caller' | 'screener' | 'producer' | 'listener';

export interface DeskPerson {
  identity: string;
  name: string;
  role: Role;
  on_air: boolean;
  mute: boolean;
  cue: boolean;
  talk: boolean;
  talk_to: string | null;
  present: boolean;
  away_ms: number | null;
  linked: boolean;
  lufs: number | null;
  lufs_peak: number | null;
}

export interface Source extends DeskPerson {
  slot: number;
  caller: { line: string; note: string; name_on_air: string; sent: boolean; taken: boolean } | null;
}

export interface Caller {
  identity: string;
  name: string;
  ordinal: number;
  state: 'waiting' | 'screening' | 'ready';
  since_ms: number;
  arrived_ms: number;
  line: string;
  note: string;
  name_on_air: string;
  declined: { at_ms: number; reason: string } | null;
  banned_before: { date: string; reason: string } | null;
  /** What they typed on the player page. */
  topic: string;
  /** Their consent to record (M3); null means they cannot go to the desk. */
  consent: Consent | null;
  dropped_ms: number | null;
  slot: number | null;
  on_air: boolean;
  cue: boolean;
  linked: boolean;
  present: boolean;
  lufs: number | null;
}

/** A caller's consent to record: when, to which words' version, and who gave it (the caller on
 *  the player page, or the station on their behalf). */
export interface Consent {
  at_ms: number;
  version: string;
  by: 'caller' | 'station';
}

export interface RundownItem {
  at: string;
  title: string;
  detail: string;
  hard_post: boolean;
  key: string | null;
}

export interface Notice {
  id: number;
  at_ms: number;
  surface: 'desk' | 'screener';
  tone: 'good' | 'warning' | 'serious';
  kind: string;
  subject: string | null;
  title: string;
  message: string;
}

/** The profanity delay toward playout (desk B's `desk.delay`). */
export interface DelayView {
  delay_ms: number;
  /** What it builds back up to; 0 when the delay is off. */
  target_ms: number;
  /** Building back up after a dump or a start; a dump meanwhile throws away what is held so far (ADR-0166). */
  rebuilding: boolean;
  dumps: number;
  dumped_at_ms: number | null;
}

export interface View {
  station: string;
  now_ms: number;
  show: { title: string; rundown: RundownItem[] };
  desk: {
    reachable: boolean;
    error: string | null;
    playout: { connected: boolean; for_ms: number | null };
    /** Absent from a control plane too old to report it; null while the desk does not answer. */
    delay?: DelayView | null;
    /** The picture (desk B's `desk.picture`, F7, M5); null with video off. */
    picture?: PictureView | null;
  };
  seats: { presenter: string | null; screener: string | null; presenter_name: string; screener_name: string };
  sources: Source[];
  callers: Caller[];
  desk_taking_callers: boolean;
  chat: {
    slow: boolean;
    hold_new: boolean;
    links_trusted: boolean;
    accounts_only: boolean;
    min_age_24h: boolean;
    influx: boolean;
    influx_since_ms: number | null;
    /** Messages held for a moderator (ADR-0274); null while not counted, absent from a control
     *  plane too old to report it. */
    held?: number | null;
    /** Open reports, notices and reviews waiting for a moderator; null while not counted. */
    open_reports?: number | null;
  };
  notices: Notice[];
  carts: CartsView;
  /** How far behind listeners hear (ADR-0103, ADR-0272): the parts read live and the rest as
   *  the station's setting. */
  listener_delay: ListenerDelay;
  /** Listeners hear the default loop WeaveSignal ships, because the station has no fallback files
   *  of its own yet (ADR-0130). Absent from a control plane too old to report it. */
  default_fallback?: boolean;
  /** Who is listening and what carries them (ADR-0046, ADR-0017); absent from a control plane
   *  too old to report it. */
  audience?: AudienceView;
  /** The station's mode (ADR-0045, ADR-0183's AUTO lamp); null until the control plane has read
   *  the setting, absent from a control plane too old to report it. */
  mode?: StationMode | null;
  /** The station's beginner step (ADR-0271, ADR-0274): what is switched on, and whose step it is. */
  step: StepView;
}

/** How much of the studio a show uses (ADR-0271): each step adds one kind of person. */
export type ShowStep = 'solo' | 'guests' | 'callers' | 'full';

/** The station's step and where it comes from. The step of the show on air; between shows, of the
 *  show last on air; `full` when no show has ever been on air. */
export interface StepView {
  step: ShowStep;
  /** The show the step comes from; null when no show has been on air. */
  show: { slug: string; name: string } | null;
  /** That show is on air now (false: it is the show last on air). */
  on_air: boolean;
  /** The next show to start by the schedule, with its step. */
  next: { slug: string; name: string; start_ms: number; step: ShowStep } | null;
  /** Whether the account asking may change the step: an admin, a producer or the show's presenter. */
  may_change: boolean;
}

/** The listener delay: `ms` is `desk_ms` + `process_ms` + `rest_ms`. */
export interface ListenerDelay {
  ms: number;
  /** True only when no part is a setting. Nothing reads the mount's own hold or a player's buffer,
   *  so a control plane of this build always says false. */
  measured: boolean;
  /** What the desk's profanity delay holds now (read live). Absent from a control plane too old. */
  desk_ms?: number;
  /** What the listener process holds of playout's feed (read live); null while it does not answer. */
  process_ms?: number | null;
  /** The rest of the path, as the `studio.listener_delay_ms` setting says (never read). */
  rest_ms?: number;
}

export interface MountCount {
  /** The mount as the listener process names it, such as `/programme.opus`. */
  mount: string;
  listeners: number;
}

export interface AudienceView {
  /** Listeners connected to the station's stream now, every mount together; null while not counted. */
  listeners: number | null;
  /** The same by mount, most listeners first; empty while not counted. Absent from a control plane too old. */
  by_mount?: MountCount[];
  /** Why there is no count, in words to show; null while counted. Absent from a control plane too old. */
  error?: string | null;
  /** The station has edges or a CDN configured. */
  edges: boolean;
}

export type DeskCommand =
  | { op: 'on_air' | 'mute' | 'cue' | 'talk'; identity: string; on: boolean }
  | { op: 'talk_to'; identity: string; to: string; on: boolean }
  | { op: 'dump' }
  | { op: 'picture'; mode: 'auto' | 'slate' | 'person'; identity?: string };

/** The desk's picture as it reports it (ADR-0009, ADR-0010). */
export interface PictureView {
  /** Who chooses the picture: the rule (`auto`), the slate held (F7), or one camera held. */
  mode: { mode: 'auto' | 'slate' | 'person'; identity?: string };
  /** What is on air. */
  shot: 'slate' | 'person';
  identity?: string | null;
  /** Whose camera has a picture at the desk. */
  cameras: string[];
  /** The encoders and the HLS writer run. */
  hls: boolean;
  /** The picture is published into the room for the people on the show. */
  room: boolean;
  /** The slate has a picture (ADR-0010); absent from a desk too old to report it. */
  still?: boolean;
}

export type ScreenerCommand =
  | { op: 'screen' | 'send' | 'back' | 'release' | 'drop'; identity: string }
  | { op: 'decline'; identity: string; reason: string }
  | { op: 'notes'; identity: string; line?: string; note?: string; name_on_air?: string };

export type CartCommand =
  | { op: 'toggle' | 'fire' | 'stop' | 'cue'; cart: string }
  | { op: 'stop_last' | 'fade_all' | 'clear_cue' };

export type ChatCommand =
  | { op: 'mode'; mode: 'slow' | 'hold_new' | 'links_trusted'; on: boolean }
  | { op: 'influx'; on: boolean };

/** End a pass by its holder's room identity (ADR-0119): off air, out of the room, no new tokens. */
export type EndPassCommand = { identity: string };

const TOKEN_KEY = 'weavesignal.token';
/** This window has settled whether it is Live or Preparing for this sign-in (ADR-0276): the
 *  account's id, kept for the window's session so a reload does not apply the start's default
 *  again over a deliberate switch. Nothing secret. */
const PREPARE_KEY = 'weavesignal.prepare.settled';

/** The account this window has settled its Live or Preparing for, since it was opened. */
export function prepareSettledFor(): string | null {
  try {
    return globalThis.sessionStorage?.getItem(PREPARE_KEY) ?? null;
  } catch {
    return null;
  }
}

/** Remember that this window settled it for the account; null forgets (a new sign-in). */
export function settlePrepare(accountId: string | null): void {
  try {
    if (accountId) globalThis.sessionStorage?.setItem(PREPARE_KEY, accountId);
    else globalThis.sessionStorage?.removeItem(PREPARE_KEY);
  } catch {
    // A private window: the default applies again after a reload.
  }
}
/** Where builds before ADR-0121 kept the station and its key, in the page's local storage. */
const STATION_KEY = 'weavesignal.station';

/** The station a packaged app runs (ADR-0019 §3): its address and where its key is kept. The
 * key itself stays on the Rust side, in the system's keychain (ADR-0121); the page never
 * holds it. */
export interface SavedStation {
  url: string;
  kept: 'keychain' | 'file';
}

let station: SavedStation | null = null;

async function core() {
  return import('@tauri-apps/api/core');
}

/** Packaged: read the saved station from the Rust side before the window starts, and move a
 * key an older build left in local storage into the keychain. */
export async function loadStation(): Promise<void> {
  if (!packaged()) return;
  const { invoke } = await core();
  try {
    const old = JSON.parse(globalThis.localStorage?.getItem(STATION_KEY) ?? 'null');
    if (old && typeof old.url === 'string' && typeof old.token === 'string' && old.token) {
      await invoke('station_save', { url: old.url, key: old.token });
    }
  } catch {
    /* nothing to move, or it would not save: the setup asks again */
  }
  try {
    globalThis.localStorage?.removeItem(STATION_KEY);
  } catch {
    /* nothing kept */
  }
  station = (await invoke<SavedStation | null>('station_info').catch(() => null)) ?? null;
}

export function savedStation(): SavedStation | null {
  return station;
}

/** Ask a station whether it takes this key: the HTTP status it answers (setup, packaged). */
export async function checkStation(url: string, key: string): Promise<number> {
  const { invoke } = await core();
  return invoke<number>('station_check', { url, key });
}

/** Save the station and its key on the Rust side; the page keeps only the address. */
export async function saveStation(url: string, key: string): Promise<SavedStation> {
  const { invoke } = await core();
  station = await invoke<SavedStation>('station_save', { url, key });
  return station;
}

/** A staff member's role, station-wide (ADR-0127). Release one enforces two tiers: an admin
 *  alone changes retention, erases people, sees personal data and manages staff; every role
 *  runs the studio. */
export type StaffRole = 'admin' | 'producer' | 'presenter' | 'screener';

export interface StaffAccount {
  id: string;
  username: string;
  name: string;
  role: StaffRole;
  /** When the account was turned off; null while it is on. */
  disabled_ms: number | null;
  created_ms: number;
  /** Whether an address is set for the emailed code; the address itself is never kept. */
  has_email?: boolean;
}

/** Who this window runs as: a member of staff signed in, or the station key (an admin). */
export interface Me {
  kind: 'staff' | 'station';
  role: StaffRole;
  account?: StaffAccount;
  expires_ms?: number;
  /** This window is preparing a show (ADR-0276): the station refuses what changes the air from
   *  it. Always false for the station key; absent from a control plane too old to report it. */
  preparing?: boolean;
}

/** Sign in as a member of staff (packaged): the session's token goes into the keychain on the
 *  Rust side, as a station key does; the page keeps only the address. */
export async function signIn(
  url: string,
  username: string,
  password: string,
): Promise<{ station: SavedStation; account: StaffAccount; expires_ms: number }> {
  const { invoke } = await core();
  const r = await invoke<{ station: SavedStation; account: StaffAccount; expires_ms: number }>('station_sign_in', {
    url,
    username,
    password,
  });
  station = r.station;
  settlePrepare(null);
  return r;
}

/** Ask a station to email a sign-in code (packaged, ADR-0127). The station answers alike
 *  whether or not the username and address name an account: success means only "asked". */
export async function askSignInCode(url: string, username: string, email: string): Promise<void> {
  const { invoke } = await core();
  await invoke<void>('station_code_ask', { url, username, email });
}

/** Sign in with the emailed code (packaged): as `signIn`, the token goes into the keychain. */
export async function finishSignInCode(
  url: string,
  username: string,
  email: string,
  code: string,
): Promise<{ station: SavedStation; account: StaffAccount; expires_ms: number }> {
  const { invoke } = await core();
  const r = await invoke<{ station: SavedStation; account: StaffAccount; expires_ms: number }>('station_code_finish', {
    url,
    username,
    email,
    code,
  });
  station = r.station;
  settlePrepare(null);
  return r;
}

/** Sign out (packaged): the station ends the session and this computer forgets it; the
 *  address is kept for the next sign-in. With a station key, the key is forgotten. */
export async function signOut(): Promise<void> {
  const { invoke } = await core();
  await invoke('station_sign_out');
  station = null;
  settlePrepare(null);
}

/** The last station's address, to fill in the sign-in screen after a sign-out (packaged). */
export async function lastStation(): Promise<string | null> {
  if (!packaged()) return null;
  const { invoke } = await core();
  return invoke<string | null>('station_last').catch(() => null);
}

/** A failed request: the status, the station's sentence, and the field it names, if any. */
export class FieldError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly field: string | null,
  ) {
    super(message);
  }
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await netFetch(`${base()}${path}`, {
    method,
    headers: headers(body !== undefined),
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (res.status === 204) return undefined as T;
  // A refusal from the framework (a body it could not read) is plain text: keep its words.
  const text = await res.text().catch(() => '');
  let data: { error?: string; field?: string } = {};
  try {
    data = text ? JSON.parse(text) : {};
  } catch {
    data = {};
  }
  const plain = data.error === undefined && !text.trimStart().startsWith('{') ? text.trim().slice(0, 200) : '';
  if (!res.ok) throw new FieldError(res.status, data.error ?? (plain || `${res.status} ${res.statusText}`), data.field ?? null);
  return data as T;
}

/** Who this window runs as. */
export const whoAmI = () => call<Me>('GET', '/api/v1/sessions/me');
/** Set this window's own session to Preparing or back to Live (ADR-0276); answers as `whoAmI`.
 *  The station key is always live (422). */
export const setPreparing = (preparing: boolean) => call<Me>('PUT', '/api/v1/sessions/me/mode', { preparing });
/** Every staff account, on or off (an admin's). */
export const listStaff = () => call<{ accounts: StaffAccount[] }>('GET', '/api/v1/staff').then((r) => r.accounts);
/** A new account (an admin's). Errors name `username`, `name` or `password`. */
export const createStaff = (a: { username: string; name: string; role: StaffRole; password: string }) =>
  call<StaffAccount>('POST', '/api/v1/staff', a);
/** Change a name or role, or turn an account off (ends its sessions at once) or on. */
export const changeStaff = (id: string, c: { name?: string; role?: StaffRole; disabled?: boolean }) =>
  call<StaffAccount>('PUT', `/api/v1/staff/${encodeURIComponent(id)}`, c);
/** Set someone's password (an admin's); ends their sessions. */
export const setStaffPassword = (id: string, password: string) =>
  call<void>('POST', `/api/v1/staff/${encodeURIComponent(id)}/password`, { password });
/** Set the address someone's sign-in codes go to, or take it away with null (an admin's). The
 *  station keeps only a keyed hash of it. Errors name `email`. */
export const setStaffEmail = (id: string, email: string | null) =>
  call<StaffAccount>('PUT', `/api/v1/staff/${encodeURIComponent(id)}/email`, { email });
/** An API token as the station lists it (ADR-0195): never its secret. */
export interface ApiTokenInfo {
  id: string;
  name: string;
  scopes: string[];
  /** The secret's first characters, to tell tokens apart. */
  prefix: string;
  created_by: string;
  created_ms: number;
  expires_ms: number | null;
  last_used_ms: number | null;
  revoked_ms: number | null;
}

/** A scope an API token can hold, with what it lets an integration do. */
export interface ApiScope {
  name: string;
  description: string;
}

/** Every API token, revoked and expired included, and the scopes one can hold (an admin's). */
export const listTokens = () => call<{ tokens: ApiTokenInfo[]; scopes: ApiScope[] }>('GET', '/api/v1/tokens');
/** Make a token: its secret is in the answer, and nowhere else, ever. Errors name `name`,
 *  `scopes` or `expires_in_days`. */
export const makeToken = (t: { name: string; scopes: string[]; expires_in_days?: number }) =>
  call<{ token: ApiTokenInfo; secret: string }>('POST', '/api/v1/tokens', t);
/** Revoke a token: it stops working at once. */
export const revokeToken = (id: string) => call<void>('DELETE', `/api/v1/tokens/${encodeURIComponent(id)}`);

/** Change your own password; your other sessions end, this one stays. Errors name `current` or `new`. */
export const changeOwnPassword = (current: string, next: string) =>
  call<void>('POST', '/api/v1/sessions/me/password', { current, new: next });

export async function forgetStation(): Promise<void> {
  const { invoke } = await core();
  await invoke('station_forget');
  station = null;
}

/** True in the Tauri window (its page origin is `tauri://localhost`, or `tauri.localhost` on
 * Windows), where there is no control plane behind the page's own origin. */
export function packaged(): boolean {
  const l = globalThis.location;
  return !!l && (l.protocol === 'tauri:' || l.hostname === 'tauri.localhost');
}

/** The sandbox build for the website's "Try the desk" (ADR-0212): the station is simulated in
 *  the page and nothing leaves it. Fixed at build time (`vite build --mode sandbox`), so the
 *  other builds carry none of it. */
export function sandboxed(): boolean {
  return import.meta.env.MODE === 'sandbox';
}

/** The bearer a browser build sends (`?token=` once, kept for this session). A packaged app
 * sends none from the page: its requests go out from Rust, which adds the key. */
function token(): string | null {
  if (packaged()) return null;
  try {
    const q = new URLSearchParams(globalThis.location?.search ?? '');
    const given = q.get('token');
    if (given) globalThis.sessionStorage?.setItem(TOKEN_KEY, given);
    return given ?? globalThis.sessionStorage?.getItem(TOKEN_KEY) ?? null;
  } catch {
    return null;
  }
}

/** The control plane's address: `?control=` if given, else the station saved by a packaged
 * app, else the page's own origin. */
export function base(): string {
  const q = new URLSearchParams(globalThis.location?.search ?? '');
  const given = q.get('control');
  if (given) return given.replace(/\/$/, '');
  return packaged() ? (station?.url ?? '') : '';
}

/** A request body as bytes, for the Rust side. */
async function bodyBytes(body: BodyInit | null | undefined): Promise<Uint8Array | null> {
  if (body == null) return null;
  if (typeof body === 'string') return new TextEncoder().encode(body);
  if (body instanceof Uint8Array) return body;
  if (body instanceof ArrayBuffer) return new Uint8Array(body);
  if (ArrayBuffer.isView(body)) return new Uint8Array(body.buffer, body.byteOffset, body.byteLength);
  return new Uint8Array(await new Response(body).arrayBuffer());
}

function headerOf(h: HeadersInit | undefined, name: string): string | null {
  return h ? new Headers(h).get(name) : null;
}

/** What `station_fetch` answers before the body. */
interface Head {
  id: number;
  status: number;
  status_text: string;
  content_type: string | null;
}

/** A body chunk from the Rust side as bytes: an ArrayBuffer, or numbers where the platform
 * sends them so. */
function chunkBytes(m: unknown): Uint8Array | null {
  if (m instanceof ArrayBuffer) return new Uint8Array(m);
  if (m instanceof Uint8Array) return m;
  if (Array.isArray(m)) return Uint8Array.from(m as number[]);
  return null;
}

/** `fetch` for the packaged app: the request goes out from Rust, which adds the station's key
 * (ADR-0121); the answer's body comes back over a channel as it arrives, so an event stream
 * reads as it would from the browser's fetch. Only paths on the saved station. */
async function stationFetch(input: RequestInfo | URL, init: RequestInit = {}): Promise<Response> {
  const { invoke, Channel } = await core();
  const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
  const root = station?.url ?? '';
  const path = root && url.startsWith(root) ? url.slice(root.length) : url;
  let ctrl!: ReadableStreamDefaultController<Uint8Array>;
  let id: number | null = null;
  let done = false;
  const finish = (err?: Error) => {
    if (done) return;
    done = true;
    try {
      if (err) ctrl.error(err);
      else ctrl.close();
    } catch {
      /* already closed */
    }
  };
  const stream = new ReadableStream<Uint8Array>({
    start(c) {
      ctrl = c;
    },
    cancel() {
      done = true;
      if (id !== null) void invoke('station_stop', { id });
    },
  });
  const channel = new Channel<unknown>();
  channel.onmessage = (m) => {
    if (done) return;
    const bytes = chunkBytes(m);
    if (bytes) return ctrl.enqueue(bytes);
    const o = (typeof m === 'string' ? JSON.parse(m) : m) as { end?: boolean; error?: string };
    finish(o?.error ? new Error(o.error) : undefined);
  };
  const headers: Record<string, string> = {
    'x-station-method': (init.method ?? 'GET').toUpperCase(),
    'x-station-path': path,
    'x-station-channel': `__CHANNEL__:${channel.id}`,
  };
  const type = headerOf(init.headers, 'content-type');
  if (type) headers['x-station-content-type'] = type;
  const body = await bodyBytes(init.body);
  const head = await invoke<Head>('station_fetch', body ?? new Uint8Array(), { headers });
  id = head.id;
  init.signal?.addEventListener('abort', () => {
    void invoke('station_stop', { id: head.id });
    finish(new DOMException('The request was stopped.', 'AbortError'));
  });
  const nullBody = head.status === 204 || head.status === 205 || head.status === 304;
  return new Response(nullBody ? null : stream, {
    status: head.status,
    statusText: head.status_text,
    headers: head.content_type ? { 'content-type': head.content_type } : {},
  });
}

/** Told when a control-plane request answers 401 (the sign-in has ended, or the key is
 *  refused), with the station's sentence; answers true when requests should stop retrying,
 *  and the frame then offers the way back (ADR-0127). The chat's own refusals are its link's. */
let onUnauthorized: ((why: string) => boolean) | null = null;
let sessionOver = false;

export function setUnauthorizedHandler(fn: ((why: string) => boolean) | null): void {
  onUnauthorized = fn;
  sessionOver = false;
}

async function noticeRefusal(url: string, res: Response): Promise<void> {
  if (res.status !== 401 || !onUnauthorized || url.includes('/api/v1/chat/')) return;
  const data = await res
    .clone()
    .json()
    .catch(() => ({}));
  if (onUnauthorized(data.error ?? 'Your sign-in has ended. Sign in again.')) sessionOver = true;
}

/** `fetch`, sent from the Tauri app's Rust side when packaged (no CORS; a login proxy's
 * redirect of the preflight would otherwise block every request; and the key stays out of the
 * page), the browser's otherwise. A 401 is noticed on the way back. */
export const netFetch: typeof fetch = async (input, init) => {
  if (import.meta.env.MODE === 'sandbox') return (await import('../sandbox/sandbox.svelte.ts')).sandboxFetch(input, init);
  const res = await (packaged() ? stationFetch(input, init) : fetch(input, init));
  const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
  await noticeRefusal(url, res);
  return res;
};

export function headers(json: boolean): HeadersInit {
  const h: Record<string, string> = {};
  const t = token();
  if (t) h.Authorization = `Bearer ${t}`;
  if (json) h['Content-Type'] = 'application/json';
  return h;
}

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

async function post(path: string, body: unknown): Promise<View> {
  const res = await netFetch(`${base()}${path}`, { method: 'POST', headers: headers(true), body: JSON.stringify(body) });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, data.error ?? `${res.status} ${res.statusText}`);
  return data as View;
}

export const desk = (c: DeskCommand) => post('/api/v1/desk/commands', c);
export const screener = (c: ScreenerCommand) => post('/api/v1/screener/commands', c);
export const chat = (c: ChatCommand) => post('/api/v1/chat/modes', c);
export const carts = (c: CartCommand) => post('/api/v1/carts/commands', c);
export const endPass = (c: EndPassCommand) => post('/api/v1/passes/end', c);

/** What End show did (ADR-0272). */
export interface EndShowResult {
  view: View;
  /** How many were taken off air. */
  off_air: number;
  /** When the air goes to automation: 0 it was there already, milliseconds the profanity delay
   *  still has to play out, null the station has no playout layer to hand it to. */
  automation_in_ms: number | null;
}

/** End the show: everyone off air, the carts faded, the air to automation once the delay has played out. */
export async function endShow(): Promise<EndShowResult> {
  const res = await netFetch(`${base()}/api/v1/show/end`, { method: 'POST', headers: headers(true), body: '{}' });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, data.error ?? `${res.status} ${res.statusText}`);
  return data as EndShowResult;
}

/** Hand the air to automation (`auto`) or take it back (`live`), at once, taking nobody off air. */
export const setMode = (mode: StationMode) => post('/api/v1/playout/mode', { mode });

/** The roles an invitation may carry; a caller joins from the player page instead. */
export type InviteRole = 'dj' | 'guest' | 'presenter';

/** An invitation as the station made it: `link` is the join page's path with the pass in its
 *  fragment, to be put after the station's address. */
export interface Invite {
  pass: string;
  identity: string;
  role: InviteRole;
  name: string;
  expires_at_ms: number;
  link: string;
}

/** The body of an invitation for the signed-in account itself (ADR-0274): a presenter's, allowed
 *  at every step for the show's presenter; the Solo DJ's "Join with my microphone…". Send it to
 *  `POST /api/v1/invites`. */
export interface OwnInviteRequest {
  for_me: true;
  /** Left out, the account's own name. */
  name?: string;
}

/** Make an invitation link for a remote DJ, a guest or a presenter (any member of staff). It lasts
 *  the station's `join.invite_ttl_s`. A refusal throws a `FieldError` with the station's sentence. */
export const makeInvite = (role: InviteRole, name: string) => call<Invite>('POST', '/api/v1/invites', { role, name });

/** The signed-in presenter's own microphone link (ADR-0274): allowed at every step for the
 *  show's presenter. A refusal throws a `FieldError` with the station's sentence. */
export const makeOwnInvite = () => call<Invite>('POST', '/api/v1/invites', { for_me: true } satisfies OwnInviteRequest);

/** What changing a show's step answers: the show's step now and before, and the station's. */
export interface StepChanged {
  show: string;
  name: string;
  step: ShowStep;
  previous: ShowStep;
  /** The station's step after the change: it moves only when this show is the one it comes from. */
  station: StepView;
}

/** Change only a show's step (the frame's dialog): an admin, a producer or the show's presenter.
 *  A refusal throws a `FieldError` with the station's sentence. */
export const setShowStep = (slug: string, step: ShowStep) =>
  call<StepChanged>('PUT', `/api/v1/shows/${encodeURIComponent(slug)}/step`, { step });

/** Open a link in the person's own browser. The packaged app asks its Rust side (a webview
 *  opens nothing outside itself); a browser build opens a tab. False when nothing opened. */
export async function openInBrowser(url: string): Promise<boolean> {
  if (!/^https?:\/\//.test(url)) return false;
  if (packaged()) {
    try {
      const { invoke } = await core();
      await invoke('open_link', { url });
      return true;
    } catch {
      return false;
    }
  }
  // With `noopener` the browser answers null whether or not a tab opened.
  globalThis.open?.(url, '_blank', 'noopener,noreferrer');
  return true;
}

/** The station's banks and carts, and `show`'s own beside them (a cart wall per show, 1.0.2);
 *  fetched again when the view's `carts.version` moves, or the window's show changes. */
export async function catalogue(show: string | null = null): Promise<Catalogue> {
  const q = show ? `?show=${encodeURIComponent(show)}` : '';
  const res = await netFetch(`${base()}/api/v1/carts${q}`, { headers: headers(false) });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, data.error ?? `${res.status} ${res.statusText}`);
  return data as Catalogue;
}

export type LinkState = 'connecting' | 'live' | 'lost';

/** Follow the studio's view: `onView` on each event, `onLink` as the stream comes and goes.
 *  Reconnects after a second, for as long as the page lives; returns a stop function. A 401
 *  goes to the unauthorized handler, and the loop ends when it says so: the sign-in has
 *  ended, and the frame offers the way back rather than reconnecting in silence. */
export function follow(onView: (v: View) => void, onLink: (s: LinkState, why?: string) => void): () => void {
  let stopped = false;
  let ctrl: AbortController | null = null;
  const run = async () => {
    while (!stopped) {
      ctrl = new AbortController();
      onLink('connecting');
      try {
        const res = await netFetch(`${base()}/api/v1/studio/events`, { headers: headers(false), signal: ctrl.signal });
        if (!res.ok || !res.body) {
          const data = await res.json().catch(() => ({}));
          const why = data.error ?? `${res.status} ${res.statusText}`;
          if (sessionOver) {
            stopped = true;
            onLink('lost', why);
            return;
          }
          throw new Error(why);
        }
        onLink('live');
        const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
        let buf = '';
        for (;;) {
          const { value, done } = await reader.read();
          if (done) break;
          buf += value;
          let cut: number;
          while ((cut = buf.indexOf('\n\n')) >= 0) {
            const frame = buf.slice(0, cut);
            buf = buf.slice(cut + 2);
            const data = frame
              .split('\n')
              .filter((l) => l.startsWith('data:'))
              .map((l) => l.slice(5).trimStart())
              .join('\n');
            if (data) onView(JSON.parse(data) as View);
          }
        }
        onLink('lost', 'the control plane closed the stream');
      } catch (e) {
        if (stopped) return;
        onLink('lost', e instanceof Error ? e.message : String(e));
      }
      await new Promise((r) => setTimeout(r, 1000));
    }
  };
  void run();
  return () => {
    stopped = true;
    ctrl?.abort();
  };
}

// --- rtmp-out (ADR-0230) -------------------------------------------------------------------

/** The desk's view of one RTMP destination (never its key). */
export interface RtmpDeskState {
  name: string;
  profile: string;
  enabled: boolean;
  /** `off`, `waiting`, `connecting`, `live` or `retrying`. */
  state: string;
  connected: boolean;
  connected_ms: number | null;
  last_error: string | null;
  last_error_ms_ago: number | null;
  bytes: number;
  bytes_total: number;
  connects: number;
  retry_in_ms: number | null;
}

export interface RtmpDestinationInfo {
  id: string;
  name: string;
  /** `rumble`, `odysee`, `peertube` or `custom`. */
  platform: string;
  url: string;
  /** `compatible` (H.264 and AAC) or `open` (AV1 and Opus over Enhanced RTMP). */
  profile: string;
  enabled: boolean;
  created_by: string;
  created_ms: number;
  updated_ms: number;
  /** False after the station's token key changed: enter the key again. */
  key_readable: boolean;
  desk: RtmpDeskState | null;
}

export interface RtmpEncoderInfo {
  profile: string;
  encoder: string;
  width: number;
  height: number;
  fps: number;
  video_kbps: number;
  ready: boolean;
}

export interface RtmpPlatform {
  platform: string;
  name: string;
  url: string;
  profile: string;
  where_to_find: string;
}

export interface RtmpList {
  destinations: RtmpDestinationInfo[];
  video: boolean;
  encoders: RtmpEncoderInfo[];
  platforms: RtmpPlatform[];
}

/** RTMP out's destinations, with the desk's view of each (admin only). */
export const listRtmp = () => call<RtmpList>('GET', '/api/v1/rtmp');
/** Add one; errors name `name`, `platform`, `url`, `key` or `profile`. The key is never shown again. */
export const addRtmp = (d: { name: string; platform: string; url: string; key: string; profile: string; enabled: boolean }) =>
  call<RtmpDestinationInfo>('POST', '/api/v1/rtmp/destinations', d);
/** Change one; a field left out (the key too) is kept. */
export const changeRtmp = (id: string, d: Partial<{ name: string; platform: string; url: string; key: string; profile: string; enabled: boolean }>) =>
  call<RtmpDestinationInfo>('PUT', `/api/v1/rtmp/destinations/${encodeURIComponent(id)}`, d);
/** Turn one on or off. */
export const turnRtmp = (id: string, enabled: boolean) =>
  call<RtmpDestinationInfo>('PUT', `/api/v1/rtmp/destinations/${encodeURIComponent(id)}/enabled`, { enabled });
export const removeRtmp = (id: string) => call<void>('DELETE', `/api/v1/rtmp/destinations/${encodeURIComponent(id)}`);
