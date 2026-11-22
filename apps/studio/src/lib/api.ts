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
import type { EpisodeView, ShowInput, ShowView } from './schedule.ts';
import { markOwn, newChange, unmarkOwn } from './ruleNotice.ts';
import { onStationChange } from './reset.ts';

/** `source` is a host's own studio (ADR-0280): what their software sends, taken with a show key
 *  and never through the room. It is at the desk like a person, with a slot and its keys. */
export type Role = 'presenter' | 'dj' | 'guest' | 'caller' | 'screener' | 'producer' | 'listener' | 'source';

/** A host's own studio as the desk reports it on its tile (`sources[].source`, ADR-0280). */
export interface DeskSource {
  way: SourceVia;
  /** What the host's software sends, as the station read it: `mp3 44.1 kHz stereo`. */
  format: string;
  /** The show key's public id. */
  key: string;
  /** The show's slug. */
  show: string;
  /** How long the desk has had it, unbroken: its steady time. It moves with every view. */
  connected_ms: number;
  /** What the leveller does to bring it to the desk's level, in dB (within 12 either way). */
  leveller_db: number;
  /** Its own level before the leveller, in dBFS RMS; null until it has sounded. */
  level_db?: number | null;
  /** Its monitor link is open: someone listens, and the desk makes their monitor mix. Absent
   *  from a station that has no monitor link yet. */
  monitor?: boolean;
}

/** The ways in for a host's software that it is given details for. */
export type SourceWay = 'icecast' | 'srt' | 'whip';
/** How a source arrived: those, the studio app's bridge, or VBAN on the station's network
 *  (Phase 17g). */
export type ArrivalWay = SourceWay | 'bridge' | 'vban';
/** How a source reaches the desk: one of the ways it arrives, or `pull`, a stream the station
 *  opens itself from an address kept with the show's key (PLAN 17b). */
export type SourceVia = ArrivalWay | 'pull';

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
  /** For a host's own studio (role `source`): how it came in and what the leveller does.
   *  Absent for a person in the room, and from a station older than 1.1. */
  source?: DeskSource | null;
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
  /** A phone caller's line (17f): the last four digits at most, never the number; absent or
   *  null for a browser caller. A withheld caller cannot be banned by number. */
  phone?: { number: string; withheld: boolean; can_ban: boolean } | null;
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
  /** `not_asked`: a phone caller let in with the station's phone words off (17f). */
  by: 'caller' | 'station' | 'not_asked';
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
  /** The person it is about; for `rule.changed`, the show's path (1.0.5). */
  subject: string | null;
  title: string;
  message: string;
  /** For `rule.changed`: the token the saving window sent (1.0.5); absent from older stations. */
  change?: string | null;
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
  /** The next handover (ADR-0280): the show whose post it is, its host's source and the clocks of
   *  the post's rule, who holds it, a lost link and its place kept. Null with no handover to
   *  show; absent from a station older than 1.1. */
  post?: PostView | null;
}

/** The show whose post it is: the next to start, or the one on air whose host has not taken the air. */
export interface PostShow {
  slug: string;
  name: string;
  /** When its post is (its airing's start). */
  start_ms: number;
  /** Its post has come. */
  started: boolean;
}

/** How far the post's source has got. */
export type PostState = 'not_connected' | 'connecting' | 'standing_by' | 'on_air' | 'link_lost';

/** A lost link and the place kept for it. The source itself has left the desk, so it is not
 *  among the view's `sources` until its software reconnects. */
export interface LinkLost {
  key_id: string;
  name: string;
  /** Its show's slug. */
  show?: string | null;
  /** When the link was lost. */
  lost_ms: number;
  /** When automation covers it, if nobody else is on air. */
  covers_ms: number;
  /** Until when the place is kept. */
  place_kept_until_ms: number;
  /** Automation has covered it already. */
  covered: boolean;
}

/** What `/studio` says of the post (hosts-in-their-own-studios.md section 10). */
export interface PostView {
  show?: PostShow | null;
  /** Its source: the key's id, or for a presenter in the room their identity. */
  source?: string | null;
  source_name?: string | null;
  /** Null when the show has no host to bring on (it comes from the room). */
  state?: PostState | null;
  /** When the source became steady, while the schedule may still bring it on by itself. */
  standing_since_ms?: number | null;
  /** When the schedule puts it on air from automation: at the post, or the late count's end. */
  on_air_at_ms?: number | null;
  /** When the grace ends and the schedule hands over from whoever holds the air. */
  grace_ends_ms?: number | null;
  /** Who holds this handover; the grace does not run. */
  held_by?: string | null;
  /** When the late count ends and the source goes on air. */
  late_ends_ms?: number | null;
  /** Who presents now, on air at the desk, by name. */
  holders: string[];
  link_lost?: LinkLost | null;
  /** The handover window before this post (1.2; handover-window.md). */
  window?: HandoverWindow | null;
}

/** What the handover window is doing (1.2). */
export type HandoverWindowState = 'quiet' | 'announced' | 'cueing' | 'ready' | 'overrun' | 'held';
/** How far a host has got, for the window and the rota. */
export type HandoverReadiness = 'not_connected' | 'connecting' | 'ready' | 'on_air';
/** Where a show's host comes from. */
export type HostOrigin = 'room' | 'push';

/** The handover window: who is next, from where, ready or not, and their heads-up. */
export interface HandoverWindow {
  state: HandoverWindowState;
  /** The show's slug (the same as the post's). */
  show: string;
  show_name: string;
  /** The post: the show's start. */
  start_ms: number;
  /** When the window opens. */
  opens_ms: number;
  /** Who is down for it, by name. */
  host: string;
  from: HostOrigin;
  /** When the incoming host was given their heads-up (once an airing). */
  heads_up_ms?: number | null;
  /** The host the show under way still waits for, once the window has moved on to the next
   *  show (1.2.1). Null when nobody is late; absent on a station older than 1.2.1, where the
   *  desk reads the late host from the post's own source. */
  late?: HandoverLate | null;
}

/** A host who never came: the show under way is theirs, and the window is about the next. */
export interface HandoverLate {
  show: string;
  show_name: string;
  /** When their show started. */
  start_ms: number;
  /** Who is down for it, by name. */
  host: string;
  from: HostOrigin;
  /** How far they have got: never `on_air`. */
  state: HandoverReadiness;
}

/** One airing on the rota. */
export interface RotaRow {
  show: string;
  show_name: string;
  start_ms: number;
  end_ms: number;
  /** Who is down for it; empty when nobody is. */
  hosts: string[];
  from: HostOrigin;
  state: HandoverReadiness;
  ready: boolean;
  /** On air by the schedule now. */
  now: boolean;
}

/** `GET /api/v1/rota`: the coming twelve hours. */
export interface RotaView {
  now_ms: number;
  until_ms: number;
  airings: RotaRow[];
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
  /** `estimate` while part of `ms` is a setting, `measured` only when the whole path was measured
   *  (issue 17). The same fact as `measured`, as a word. Absent from a control plane before 1.1. */
  basis?: 'estimate' | 'measured';
  /** The part of `ms` that is read live now: `desk_ms` plus `process_ms`. Absent before 1.1. */
  read_ms?: number;
  /** The part of `ms` that is a setting and never read: the same as `rest_ms`. Absent before 1.1. */
  set_ms?: number;
  /** What the desk's profanity delay holds now (read live). Absent from a control plane too old. */
  desk_ms?: number;
  /** What the listener process holds of playout's feed (read live); null while it does not answer. */
  process_ms?: number | null;
  /** The rest of the path (never read): the mount's fixed hold and `studio.player_allowance_ms`,
   *  or the older single `studio.listener_delay_ms`. While `process_ms` is null it also holds the
   *  buffer's nominal 0.5 s. */
  rest_ms?: number;
  /** Of `rest_ms`, the listener mount's own hold: fixed at 12 s, not a setting (ADR-0289). Null on
   *  a station that still gives the older single figure, which cannot be split; absent before 1.1.1. */
  mount_hold_ms?: number | null;
  /** Of `rest_ms`, what a listener's player is taken to add (`studio.player_allowance_ms`), an
   *  estimate. Null and absent as `mount_hold_ms` is. */
  player_ms?: number | null;
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
  /** What the count covers (issue 17): `origin`, the station's own listener process, every mount
   *  together; listeners on an edge or a CDN and video viewers on HLS are not added. Absent
   *  from a control plane before 1.1. */
  scope?: 'origin';
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

/** The station a packaged app is on (ADR-0019 §3): this computer's id for it (ADR-0285), its
 * address and where its key is kept. The key itself stays on the Rust side, in the system's
 * keychain (ADR-0121); the page never holds it. */
export interface SavedStation {
  id: string;
  url: string;
  kept: 'keychain' | 'file';
  /** False only for a station just added while the window is on another: it has joined the
   *  list, and the window switches to it by the path every switch takes. */
  current?: boolean;
}

let station: SavedStation | null = null;

/** The window is leaving its station (ADR-0285): from here until the page loads again, no
 *  request leaves it. The Rust side refuses a request that names the wrong station as well;
 *  this is the page's own lock, so nothing is even asked. */
let sealed = false;

/** Seal the page: a switch has closed the links and is about to tell the Rust side. */
export function sealStation(): void {
  sealed = true;
}

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

/** Save the station and its key on the Rust side; the page keeps only the address. A station
 *  added while the window is on another answers `current: false`, and the page's station stays. */
export async function saveStation(url: string, key: string): Promise<SavedStation> {
  const { invoke } = await core();
  const saved = await invoke<SavedStation>('station_save', { url, key });
  if (saved.current !== false) station = saved;
  return saved;
}

/** A station as the list keeps it (ADR-0285): what this computer last saw there, and no key. */
export interface StationRow {
  id: string;
  url: string;
  /** The name the person gave it on this computer. */
  label: string | null;
  /** The station's own name, as it last said it. */
  name: string | null;
  /** Who was signed in there, as the frame says it. */
  who: string | null;
  version: string | null;
  /** Where its key is; null while signed out. */
  kept: 'keychain' | 'file' | null;
}

/** Every station kept on this computer, which one the window is on, and this app's version. */
export interface StationList {
  app_version: string;
  current: string | null;
  stations: StationRow[];
}

async function listCall(cmd: string, args: Record<string, unknown> = {}): Promise<StationList> {
  const { invoke } = await core();
  return invoke<StationList>(cmd, args);
}

/** The list of stations (packaged). */
export const listStations = () => listCall('stations_list');
/** Move the window to another station on the Rust side: it stops every request still open. */
export const switchToStation = (id: string) => listCall('stations_switch', { id });
/** Give a station a name on this computer; empty takes it away. */
export const renameStation = (id: string, label: string) => listCall('stations_rename', { id, label });
/** Sign out of a station in the list; its row stays. */
export const signOutOfStation = (id: string) => listCall('stations_sign_out', { id });
/** Remove a station from this computer: its key is deleted, its row forgotten. */
export const removeStation = (id: string) => listCall('stations_remove', { id });
/** Tell the list what the window sees at its own station, for the rows. */
export const noteStation = (id: string, seen: { name?: string | null; who?: string | null; version?: string | null }) =>
  listCall('stations_note', { id, name: seen.name ?? null, who: seen.who ?? null, version: seen.version ?? null });

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
  if (r.station.current !== false) {
    station = r.station;
    settlePrepare(null);
  }
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
  if (r.station.current !== false) {
    station = r.station;
    settlePrepare(null);
  }
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
    /** The refusal's whole body, for a reply that says more than a sentence (a clash's 409). */
    readonly data: Record<string, unknown> = {},
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
  let data: { error?: string; field?: string } & Record<string, unknown> = {};
  try {
    data = text ? JSON.parse(text) : {};
  } catch {
    data = {};
  }
  const plain = data.error === undefined && !text.trimStart().startsWith('{') ? text.trim().slice(0, 200) : '';
  if (!res.ok) throw new FieldError(res.status, data.error ?? (plain || `${res.status} ${res.statusText}`), data.field ?? null, data);
  return data as T;
}

/** Who this window runs as. */
export const whoAmI = () => call<Me>('GET', '/api/v1/sessions/me');
/** Set this window's own session to Preparing or back to Live (ADR-0276); answers as `whoAmI`.
 *  The station key is always live (422). */
export const setPreparing = (preparing: boolean) => call<Me>('PUT', '/api/v1/sessions/me/mode', { preparing });
/** Every staff account, on or off (an admin's). */
export const listStaff = () => call<{ accounts: StaffAccount[] }>('GET', '/api/v1/staff').then((r) => r.accounts);
/** Who is on the team, for a producer or an admin (1.1, ADR-0289; a presenter or a screener gets
 *  403): each account that is on, with only its id, name and role. The show editor names a show's
 *  presenter from it. */
export const listTeam = () =>
  call<{ accounts: { id: string; name: string; role: StaffRole }[] }>('GET', '/api/v1/staff/directory').then((r) => r.accounts);
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
    // The station this page is on: the Rust side refuses a request that names another (ADR-0285).
    'x-station-id': station?.id ?? '',
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

/** Whether this station's rule saves carry a token (1.0.5), as its version was last read. */
let sendChange = false;

// A sign-in that ended at one station says nothing about another (ADR-0285). The handler
// itself is the frame's wiring, and stays.
onStationChange('api', () => {
  sessionOver = false;
  sendChange = false;
});

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
  if (sealed) throw new Error('This window is changing station.');
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
  /** The next host's name when they were standing by and the air passed to them (ADR-0280);
   *  nothing went to automation then. Absent otherwise, and from a station older than 1.1. */
  passed_to?: string | null;
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

/** What the station says of a restart of playout. */
export interface PlayoutRestarting {
  restarting: boolean;
  words: string;
}

/** Restart playout (⌃R twice): the station asks it to shut down, and its supervisor starts it
 *  again. Listeners hear the fallback for a few seconds. */
export async function restartPlayout(): Promise<PlayoutRestarting> {
  const res = await netFetch(`${base()}/api/v1/playout/restart`, { method: 'POST', headers: headers(true), body: '{}' });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, data.error ?? `${res.status} ${res.statusText}`);
  return data as PlayoutRestarting;
}

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
  /** `youtube`, `owncast`, `x`, `rumble`, `odysee`, `peertube` or `custom`. */
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

// ---- When a show airs: the recurrence rule (ADR-0278; new-show.md section 9) ------------------
//
// The control plane stores the rule, works out its dates, says it as a sentence and finds its
// clashes; the studio sends a rule and draws what comes back. The shapes below are the design's
// where it gives them and this build's choice where it leaves them open; the API reference
// (docs/…/reference/show-rules.md and api.md) is the contract. Each reply goes through one `as…` function
// here, so a difference found when the two halves meet is put right in one place. Those
// functions read the control plane's shapes as branch `newshow-api` built them (instants as
// `start_ms` and `end_ms`, a clash's `short` and `hosts`, the preview's `words`, `notes`,
// `more` and `change`), and the shapes this half first chose as well.

/** A date on the station's wall clock, `2026-12-01`. */
export type IsoDate = string;

/** How a show comes back. Weekdays are 1 (Monday) to 7; `nth` is 1 to 4, or -1 for the last;
 *  `day` is 1 to 31, or -1 for the last day; `month` is 1 to 12. */
export type Repeat =
  | { kind: 'once' }
  | { kind: 'weekly'; every: number; weekdays: number[] }
  | { kind: 'monthly_nth'; every: number; nth: number; weekday: number }
  | { kind: 'monthly_day'; every: number; day: number }
  | { kind: 'yearly'; month: number; day: number }
  | { kind: 'dates'; dates: IsoDate[] };

export type RuleEnd = { kind: 'never' } | { kind: 'until'; until: IsoDate } | { kind: 'count'; count: number };

/** One date aired at another time: `from` is the rule's date, `to` a wall-clock date and time
 *  (`2027-04-13T20:00:00`). */
export interface RuleMove {
  from: IsoDate;
  to: string;
  minutes: number | null;
}

/** When a show airs. All dates and times are the station's wall clock. */
export interface Rule {
  /** The first date the rule may give; null only for an old weekly slot. */
  starts_on: IsoDate | null;
  /** Minutes after midnight. */
  start: number;
  /** The length in minutes; null is the studio's 60. */
  minutes: number | null;
  repeat: Repeat;
  end: RuleEnd;
  skip: IsoDate[];
  moves: RuleMove[];
}

/** The rule said by the control plane: the sentence, the short form for a field or a list, and
 *  the second line when the rule ends (`The last is Tuesday 5 October 2027.`). */
export interface RuleWords {
  sentence: string;
  short: string;
  last: string | null;
  /** The last date, when the rule ends. */
  last_on: IsoDate | null;
  /** What the calendar cannot give, said rather than hidden: `Months without a 31st are skipped.` */
  notes: string[];
}

/** Why a show has nothing coming up. */
export type NoDateReason = 'none' | 'once_passed' | 'ended' | 'complete';

/** A show's next date: the next its rule gives or the next drafted episode still to air,
 *  whichever is sooner; or none, with the reason and the last date there was. */
export interface NextOn {
  on: IsoDate | null;
  /** Epoch seconds. */
  start: number | null;
  minutes: number | null;
  reason: NoDateReason | null;
  /** The last date the show had, for `was once, on 30 November` and `ended 29 January`. */
  last_on: IsoDate | null;
}

/** Another show in a date's way. */
export interface Clash {
  /** The other show's name and path. */
  show: string;
  slug: string;
  /** Its start, epoch seconds, and length on that date. */
  start: number;
  minutes: number;
  overlap_minutes: number;
  /** When the other show airs, in the control plane's words: `Tuesdays, 21:00 to 22:00`. */
  when: string | null;
  /** Who presents it, for the refusal's `with Shona Campbell and …`. */
  with: string[];
  /** This session may not draft over it (a presenter's). */
  refused: boolean;
}

/** One of a rule's next dates, as the preview draws it. */
export interface PreviewDate {
  /** The rule's own date: an episode's `occurrence`. */
  on: IsoDate;
  /** The episode number it would take, or has. */
  number: number | null;
  /** The real start, epoch seconds, and the real length. */
  start: number;
  minutes: number;
  /** It airs, it is skipped, or an episode already fills it. */
  state: 'airs' | 'skipped' | 'filled';
  /** It is under way now. */
  now: boolean;
  /** The rule's own wall-clock time, when the date was moved. */
  moved_from: string | null;
  /** The clocks change over its start. */
  clock: 'forward' | 'back' | null;
  /** The episode that fills it. */
  episode: { series: number; number: number; aired: boolean } | null;
  clashes: Clash[];
}

/** A line under a field: a problem stops the save, a hint does not. */
export interface FieldNote {
  field: string;
  message: string;
}

/** What a changed rule does to a show's episodes (`calendar.rule.change`). Times are epoch seconds. */
export interface RuleChangePlan {
  /** How many have aired, and the date of the first. */
  aired: number;
  since: IsoDate | null;
  /** Drafted, still on the old rule's dates: where each is and where it would go. `to` null:
   *  the new rule has no date for it, so it stays. */
  drafted: { series: number; number: number; from: number; to: number | null; clash: Clash | null }[];
  /** Drafted, moved or made by hand: each stays. */
  by_hand: { series: number; number: number; at: number }[];
}

/** What `POST /api/v1/schedule/rule/preview` takes. It writes nothing. */
export interface RulePreviewRequest {
  rule: Rule;
  /** The show and series, when they exist: their own episodes are not clashes. */
  show?: string;
  series?: number;
  /** How many dates that air to give: 10, then ten more at a time, 60 at most. */
  limit?: number;
}

export interface RulePreview {
  sentence: string;
  short: string;
  /** The second line, when the rule ends. */
  last: string | null;
  dates: PreviewDate[];
  /** The rule gives more dates than these; null when the station did not say. */
  more: boolean | null;
  /** How many dates the rule still gives in all, when that is known: the station's own
   *  count, or the dates given when there are no more. */
  total: number | null;
  /** A start within three hours that is clear on every date shown, minutes after midnight. */
  clear: { start: number; minutes: number } | null;
  problems: FieldNote[];
  hints: FieldNote[];
  /** Skipped and moved dates the pattern no longer gives; the studio drops them and says so once. */
  dropped: { skip: IsoDate[]; moves: IsoDate[] };
  /** With a show: what saving would do to its episodes. */
  change: RuleChangePlan | null;
}

/** A date a rule gives that no episode fills yet: a dotted block in the week. */
export interface ScheduleDate {
  /** The show's path, and the series the episode would join. */
  show: string;
  series: number | null;
  on: IsoDate;
  number: number | null;
  /** Epoch seconds. */
  start: number;
  minutes: number;
  moved_from: string | null;
  clock: 'forward' | 'back' | null;
}

/** What a saved rule did: the rule as stored, and the drafted episodes that moved. */
export interface RuleChanged {
  rule: Rule | null;
  rule_words: RuleWords | null;
  moved: { series: number; number: number; from: number; to: number }[];
}

type Raw = Record<string, unknown>;
const obj = (v: unknown): Raw => (v && typeof v === 'object' && !Array.isArray(v) ? (v as Raw) : {});
const list = (v: unknown): unknown[] => (Array.isArray(v) ? v : []);
const str = (v: unknown): string | null => (typeof v === 'string' && v ? v : null);
const num = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null);

/** Epoch seconds from a field in seconds, or from its `_ms` twin. */
const secs = (o: Raw, key: string): number | null => {
  const ms = num(o[`${key}_ms`]);
  return ms !== null ? Math.round(ms / 1000) : num(o[key]);
};
const strings = (v: unknown): string[] => list(v).filter((x): x is string => typeof x === 'string' && x !== '');
const ISO = /\d{4}-\d{2}-\d{2}/;

/** A clash, whichever way the control plane sent it (`start_ms` and `end_ms`, `short`, `hosts`). */
export function asClash(v: unknown, refused = false): Clash {
  const c = obj(v);
  const start = secs(c, 'start') ?? 0;
  const end = secs(c, 'end');
  const hosts = strings(c.with).length ? strings(c.with) : strings(c.hosts);
  return {
    show: str(c.show) ?? str(c.name) ?? 'Another show',
    slug: str(c.slug) ?? '',
    start,
    minutes: num(c.minutes) ?? (end !== null ? Math.max(1, Math.round((end - start) / 60)) : 60),
    overlap_minutes: num(c.overlap_minutes) ?? num(c.overlap) ?? 0,
    when: str(c.when) ?? str(c.short),
    with: hosts.length ? hosts : str(c.presenter) ? [str(c.presenter)!] : [],
    refused: refused || c.refused === true,
  };
}

/** Problems and hints by field. A field's path is cut to its first part (`repeat.every` is
 *  Repeats'), and a bare sentence takes the field given. */
const notes = (v: unknown, field = ''): FieldNote[] =>
  list(v).flatMap((x) => {
    if (typeof x === 'string') return x ? [{ field, message: x }] : [];
    const n = obj(x);
    const message = str(n.message) ?? str(n.error);
    return message ? [{ field: (str(n.field) ?? field).split('.')[0], message }] : [];
  });

/** The second line as words: the control plane's own, or its date left for the studio to say. */
function lastWords(v: unknown): string | null {
  if (typeof v === 'string') return v || null;
  const o = obj(v);
  return str(o.words) ?? str(o.on);
}

/** What saving would do to the episodes, from the control plane's lists (`moved`, `no_date`,
 *  `left`, `by_hand`) or from this half's first shape (`drafted`, `by_hand`). */
function asChange(raw: unknown): RuleChangePlan | null {
  if (!raw) return null;
  const c = obj(raw);
  const ep = (x: unknown) => {
    const e = obj(x);
    return { series: num(e.series) ?? 1, number: num(e.number) ?? 0, e };
  };
  const drafted = [
    ...list(c.drafted).map((x) => ({ ...ep(x), from: secs(obj(x), 'from') ?? 0, to: secs(obj(x), 'to'), clash: obj(x).clash ? asClash(obj(x).clash) : null })),
    ...list(c.moved).map((x) => ({ ...ep(x), from: secs(obj(x), 'from') ?? 0, to: secs(obj(x), 'to'), clash: null })),
    // Left on their old dates, by choice or because the new rule has no date for them: they stay.
    ...[...list(c.no_date), ...list(c.left)].map((x) => ({ ...ep(x), from: secs(obj(x), 'start') ?? 0, to: null, clash: null })),
  ]
    .map(({ e: _e, ...rest }) => rest)
    .sort((a, b) => a.series - b.series || a.number - b.number);
  return {
    aired: num(c.aired) ?? 0,
    since: str(c.since),
    drafted,
    by_hand: list(c.by_hand).map((x) => ({ series: ep(x).series, number: ep(x).number, at: secs(obj(x), 'at') ?? secs(obj(x), 'start') ?? 0 })),
  };
}

/** The preview's reply, made whole: a field the control plane left out is empty, never undefined. */
export function asPreview(raw: unknown): RulePreview {
  const r = obj(raw);
  const words = obj(r.words);
  const clear = obj(r.clear ?? r.offered);
  const given = obj(r.dropped);
  const dates = list(r.dates).map((x) => {
    const d = obj(x);
    const ep = typeof d.episode === 'number' ? { series: 1, number: d.episode } : d.episode ? obj(d.episode) : null;
    const state = d.state === 'skipped' || d.state === 'filled' ? d.state : 'airs';
    const start = secs(d, 'start') ?? 0;
    const end = secs(d, 'end');
    return {
      on: str(d.on) ?? '',
      number: num(d.number),
      start,
      minutes: num(d.minutes) ?? (end !== null ? Math.max(1, Math.round((end - start) / 60)) : 60),
      state,
      now: d.now === true || d.under_way === true,
      moved_from: str(d.moved_from),
      clock: d.clock === 'forward' || d.clock === 'back' ? d.clock : null,
      episode: ep ? { series: num(ep.series) ?? 1, number: num(ep.number) ?? num(d.number) ?? 0, aired: ep.aired === true } : null,
      // A date the station refuses this session marks its clashes refused.
      clashes: list(d.clashes).map((c) => asClash(c, d.refused === true)),
    } satisfies PreviewDate;
  });
  // A skipped or moved date the pattern no longer gives comes back as a refusal that names
  // the date (`the rule gives no date on 2027-01-05`); the studio drops it and asks again.
  const all = notes(r.problems);
  const gone = (field: string) => all.filter((p) => p.field === field && /gives no date/.test(p.message)).flatMap((p) => ISO.exec(p.message)?.[0] ?? []);
  const dropped = { skip: [...strings(given.skip), ...gone('skip')], moves: [...strings(given.moves), ...gone('moves')] };
  const problems = all.filter((p) => !((p.field === 'skip' || p.field === 'moves') && /gives no date/.test(p.message) && ISO.test(p.message)));
  const more = typeof r.more === 'boolean' ? r.more : null;
  const airing = dates.filter((d) => d.state !== 'skipped').length;
  return {
    sentence: str(r.sentence) ?? str(words.sentence) ?? '',
    short: str(r.short) ?? str(words.short) ?? '',
    last: lastWords(r.last) ?? lastWords(words.last),
    dates,
    more,
    total: num(r.total) ?? (more === false ? airing : null),
    clear: num(clear.start) !== null ? { start: num(clear.start)!, minutes: num(clear.minutes) ?? 60 } : null,
    problems,
    // The station's notes on the dates sit under First on; what the calendar cannot give, under Repeats.
    hints: [...notes(r.hints), ...notes(r.notes, 'starts_on'), ...notes(words.notes, 'repeat')],
    dropped,
    change: asChange(r.change),
  };
}

/** A show's `next_on`, whichever way the control plane sent it: an object with the date and
 *  `start_ms`, a bare date, or null with `next_on_reason` beside it. `hasRule` settles the
 *  reason when none is given; `lastOn` is the last date the show had, when the caller knows it. */
export function asNextOn(raw: unknown, reason: unknown, hasRule: boolean, lastOn: IsoDate | null = null): NextOn {
  const why = (v: unknown): NoDateReason | null => (v === 'none' || v === 'once_passed' || v === 'ended' || v === 'complete' ? v : null);
  if (typeof raw === 'string' && raw) return { on: raw, start: null, minutes: null, reason: null, last_on: null };
  const o = obj(raw);
  const on = str(o.on);
  return {
    on,
    start: secs(o, 'start'),
    minutes: num(o.minutes),
    reason: on ? null : (why(o.reason) ?? why(reason) ?? (hasRule ? 'ended' : 'none')),
    last_on: str(o.last_on) ?? lastOn,
  };
}

/** The rule's words, or null from a station that sent none. */
export function asRuleWords(raw: unknown): RuleWords | null {
  const w = obj(raw);
  const sentence = str(w.sentence);
  const short = str(w.short);
  if (!sentence && !short) return null;
  return { sentence: sentence ?? short ?? '', short: short ?? sentence ?? '', last: lastWords(w.last), last_on: str(w.last_on), notes: strings(w.notes) };
}

/** The week's dotted blocks, from `dates` in the week's reply; `null` from a station too old to send them. */
export function asScheduleDates(raw: unknown): ScheduleDate[] | null {
  if (!Array.isArray(raw)) return null;
  return raw.flatMap((x) => {
    const d = obj(x);
    const show = str(d.show) ?? str(d.slug) ?? str(obj(d.show).slug);
    const start = secs(d, 'start');
    const end = secs(d, 'end');
    if (!show || start === null) return [];
    return [
      {
        show,
        series: num(d.series),
        on: str(d.on) ?? '',
        number: num(d.number),
        start,
        minutes: num(d.minutes) ?? (end !== null ? Math.max(1, Math.round((end - start) / 60)) : 60),
        moved_from: str(d.moved_from),
        clock: d.clock === 'forward' || d.clock === 'back' ? d.clock : null,
      } satisfies ScheduleDate,
    ];
  });
}

/** The first release whose control plane stores a rule (ADR-0278). */
export const RULES_SINCE = '1.0.4';

/** The first release whose rule saves take a window's `change` token and give it back on the
 *  notice (1.0.5): an older control plane refuses a field it does not know. */
export const CHANGE_SINCE = '1.0.5';

/** Whether a station of this version is at least `floor`. Only the numbers count, so
 *  `1.0.4-test.2` is 1.0.4. */
function since(version: string | null | undefined, floor: string): boolean {
  const core = (v: string) => (/^(\d+)\.(\d+)\.(\d+)/.exec(v.trim()) ?? []).slice(1).map(Number);
  const [a, b] = [core(version ?? ''), core(floor)];
  if (a.length < 3) return false;
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] > b[i];
  return true;
}

/** Whether a station of this version takes `rule` and `draft_first`: an older control plane
 *  refuses a field it does not know. */
export const speaksRules = (version: string | null | undefined): boolean => since(version, RULES_SINCE);

/** Whether a station of this version takes a rule save's `change` token (1.0.5). */
export const speaksChange = (version: string | null | undefined): boolean => since(version, CHANGE_SINCE);

/** The station's version, as the Schedule tab read it: it decides whether a save sends its token. */
export function noteStationVersion(version: string | null): void {
  if (version !== null) sendChange = speaksChange(version);
}

/** The station's version, from `/health`; null when it cannot be read. */
export async function stationVersion(): Promise<string | null> {
  try {
    const res = await netFetch(`${base()}/health`, { headers: headers(false) });
    const data = (await res.json().catch(() => ({}))) as { version?: unknown };
    return typeof data.version === 'string' ? data.version : null;
  } catch {
    return null;
  }
}

/** The next dates a rule makes, its sentence and its clashes. It writes nothing. */
export const previewRule = async (req: RulePreviewRequest) => asPreview(await call<unknown>('POST', '/api/v1/schedule/rule/preview', req));

/** A show saved, with the first episode drafted when `draft_first` was asked. */
export interface ShowSaved extends ShowView {
  drafted: EpisodeView | null;
}

/** Save a show (`PUT /api/v1/shows/{show}`). With `draft_first` the station makes series 1 and
 *  drafts the first episode in the same transaction, and a refusal saves nothing. A refusal
 *  names its field (`name`, `rule`), and a clash's 409 carries the clash in `data.clash`. */
export async function saveShow(slug: string, input: ShowInput): Promise<ShowSaved> {
  const v = await call<ShowView & { drafted?: unknown }>('PUT', `/api/v1/shows/${encodeURIComponent(slug)}`, input);
  const d = Array.isArray(v.drafted) ? v.drafted[0] : v.drafted;
  return { ...v, drafted: d && typeof d === 'object' && 'episode' in d ? (d as EpisodeView) : null };
}

function asRuleChanged(raw: unknown): RuleChanged {
  const r = obj(raw);
  const show = obj(obj(r.show).show ?? r.show);
  return {
    rule: (r.rule ?? show.rule ?? null) as Rule | null,
    rule_words: asRuleWords(r.rule_words ?? show.rule_words),
    moved: list(r.moved).map((x) => {
      const e = obj(x);
      return { series: num(e.series) ?? 1, number: num(e.number) ?? 0, from: secs(e, 'from') ?? 0, to: secs(e, 'to') ?? 0 };
    }),
  };
}

/** What happens to the episodes drafted on the old rule's dates. */
export type DraftedChoice = 'move' | 'leave';

/** Save a show's rule alone; `null` leaves it with no dates. */
export const putShowRule = (slug: string, rule: Rule | null, drafted: DraftedChoice) =>
  ownRuleChange(slug, async (change) => asRuleChanged(await call<unknown>('PUT', `/api/v1/shows/${encodeURIComponent(slug)}/rule`, { rule, drafted, change })));

/** Save a series' rule alone; `null` follows the show again. */
export const putSeriesRule = (slug: string, series: number, rule: Rule | null, drafted: DraftedChoice) =>
  ownRuleChange(slug, async (change) => asRuleChanged(await call<unknown>('PUT', `/api/v1/shows/${encodeURIComponent(slug)}/series/${series}/rule`, { rule, drafted, change })));

/** The station tells every window of a changed rule, this one too: the save is marked as this
 *  window's own, with a token the station gives back on its notice (1.0.5; an older station
 *  ignores it), so the Schedule tab does not say it back to the person who made it. */
async function ownRuleChange<T>(slug: string, save: (change: string | undefined) => Promise<T>): Promise<T> {
  const change = sendChange ? newChange() : null;
  markOwn(slug, Date.now(), change);
  try {
    return await save(change ?? undefined);
  } catch (e) {
    unmarkOwn(slug, change);
    throw e;
  }
}

/** The clash a 409 carries (`clashes`, the first of them), when a presenter's date is another show's. */
export function refusedClash(e: unknown): Clash | null {
  if (!(e instanceof FieldError) || e.status !== 409) return null;
  const c = list(e.data.clashes)[0] ?? e.data.clash;
  return c ? asClash(c, true) : null;
}

// ---- hosts in their own studios (ADR-0280, ADR-0284; 1.1) ----

/** A show key as the station lists it: never a secret. */
export interface ShowKey {
  id: string;
  show_id: string;
  /** Who it is for: their name on the desk. */
  label: string;
  /** The member of staff it is for, if one. */
  staff_id: string | null;
  /** The public part: the SRT stream id, and the first part of the Icecast password. */
  key_id: string;
  /** How the host means to send: a hint, not a lock. `pull`: the station opens `pull_url` itself. */
  hint: SourceVia;
  /** For a pull: the address the station opens while the slot's door is open. */
  pull_url?: string | null;
  created_by: string;
  created_ms: number;
  last_used_ms: number | null;
  /** When it ended, if it has. */
  revoked_ms: number | null;
  /** It ends when its connection closes (a new key was made with *when they disconnect*). */
  ends_on_disconnect: boolean;
  /** Turned away from the desk until then (the tile's Disconnect). */
  turned_away_until_ms: number | null;
  turned_away_by: string | null;
}

/** A line of a key's log, as a row shows its last refusal. */
export interface SourceLogEntry {
  at_ms: number;
  key_id: string;
  event: string;
  /** For a refusal: its word (`wrong_key`, `ended`, `not_your_slot`, `in_use`, `too_many`,
   *  `turned_away`, `format`, `desk_full`, `too_fast`). */
  reason: string | null;
  detail?: string;
  way: SourceVia | null;
}

/** How a host's software reaches the station. Outside the reply that makes a key, the two
 *  secret fields hold the station's placeholder sentence, never a secret. */
export interface SourceDetails {
  /** The station's address as listeners reach it; null when the station's settings give none. */
  host: string | null;
  icecast: { port: number; mount: string; user: string; password: string };
  srt: { port: number; streamid: string; passphrase: string; pbkeylen: number; url: string | null };
  /** For a pull: what the station opens. Absent from a control plane older than 1.2. */
  pull?: { url: string; kind: 'http' | 'hls' | 'srt' } | null;
  /** WHIP (17d), as OBS asks for it: port 0 when the station has it off; `url` null when off or
   *  the station has no public address. */
  whip: { port: number; url: string | null; bearer_token: string };
}

/** A key with what its row shows. */
export interface KeyRow extends ShowKey {
  last_refusal: SourceLogEntry | null;
  details: SourceDetails;
}

/** A show's keys, live and ended, newest first, and where the show comes from. */
export interface KeyList {
  show: string;
  source: 'room' | 'push' | 'pull';
  keys: KeyRow[];
}

/** A key just made: its two secrets and the details, shown this once. */
export interface MadeKey {
  key: ShowKey;
  secret: string;
  /** The monitor link's secret, made and ended with the key. */
  monitor_secret: string;
  details: SourceDetails;
  /** The station's own text block for a message. */
  as_text: string;
}

/** A source connected or due, as `GET /sources` lists it. */
export interface SourceNow {
  key_id: string;
  identity: string;
  label: string;
  show: string;
  show_name: string;
  /** `test` is a connection outside its slot, never on the desk; `not_connected` is a key whose
   *  slot's door is open and whose host has not connected. */
  state: 'on_air' | 'standby' | 'test' | 'not_connected';
  slot: number | null;
  way: SourceVia | null;
  /** The station opens this source itself (a pull). Absent from a control plane older than 1.2. */
  pull?: boolean;
  format: string | null;
  /** At the desk: short-term LUFS after the leveller. For a test: the station's reading in dBFS RMS. */
  level: number | null;
  leveller_db: number | null;
  steady_ms: number | null;
  /** The host's monitor link is open: someone listens, and the desk makes their monitor mix. */
  monitor_open: boolean;
  door_closes_ms: number | null;
  /** What the host's software says is playing (Icecast's metadata). */
  title: string | null;
  turned_away_until_ms: number | null;
}

export interface SourcesNow {
  /** Whether the part of the station that takes hosts' software is running and reporting. */
  listening: boolean;
  sources: SourceNow[];
}

/** The station's rules for sources. `PUT` takes every field at once. */
export interface SourceSettings {
  door_before_min: number;
  /** 0 (off), 60, 120 or 300. */
  grace_s: number;
  /** 0 is off. */
  late_count_s: number;
  test_outside: boolean;
  test_max_min: number;
  steady_s: number;
  silence_s: number;
  place_kept_s: number;
  fade_ms: number;
  port_icecast: number;
  port_srt: number;
  /** The handover window, in seconds: 0 (off) to 1,800 (1.2). Left out, the stored one is kept. */
  window_s?: number;
  /** WHIP's media port (UDP, 17d); 0 is off. */
  port_whip: number;
}

const showKeys = (slug: string) => `/api/v1/shows/${encodeURIComponent(slug)}/keys`;

/** A show's keys and where it comes from. For an admin, a producer or the show's presenter. */
export const listShowKeys = (slug: string) => call<KeyList>('GET', showKeys(slug));
/** Make a key: the reply holds its secrets, once. Errors name `label`, `hint` or `staff_id`. */
export function makeShowKey(slug: string, k: { label: string; hint: SourceWay; staff_id?: string | null }): Promise<MadeKey>;
/** A pull (1.2): the station opens `url` itself while the show's door is open. */
export function makeShowKey(slug: string, k: { label: string; hint: 'pull'; url: string }): Promise<MadeKey>;
export function makeShowKey(slug: string, k: { label: string; hint: SourceWay | 'pull'; staff_id?: string | null; url?: string }): Promise<MadeKey> {
  return call<MadeKey>('POST', showKeys(slug), { label: k.label, hint: k.hint, ...(k.staff_id ? { staff_id: k.staff_id } : {}), ...(k.hint === 'pull' ? { url: k.url } : {}) });
}
/** A new key in place of one, for the same person: the old one ends now, or when its connection closes. */
export const renewShowKey = (slug: string, keyId: string, old: 'now' | 'on_disconnect') =>
  call<MadeKey>('POST', `${showKeys(slug)}/${encodeURIComponent(keyId)}/renew`, { old });
/** End a key and its monitor link for good; a connection with it is closed. */
export const endShowKey = (slug: string, keyId: string) => call<{ ended: boolean }>('DELETE', `${showKeys(slug)}/${encodeURIComponent(keyId)}`);
/** Every source connected or due, and whether the station is listening for them. */
export const listSources = () => call<SourcesNow>('GET', '/api/v1/sources');
/** The tile's Disconnect: closes the connection and turns the key away until its slot's door
 *  closes. Refused from a window that is preparing (409). */
export const disconnectSource = (keyId: string) =>
  call<{ disconnected: boolean; until_ms: number }>('POST', `/api/v1/sources/${encodeURIComponent(keyId)}/disconnect`);
/** Hold this handover (an admin's or a producer's): the grace stops, and the air passes by a
 *  press only. The view follows within a second. */
export const holdPost = () => call<{ held: boolean }>('POST', '/api/v1/air/post/hold');
/** Let the held handover go: the grace runs again. */
export const releasePost = () => call<{ held: boolean }>('DELETE', '/api/v1/air/post/hold');
/** Go on air now: a source standing by takes the air from automation at once, skipping the late count. */
export const sourceGo = (keyId: string) => call<{ on_air: boolean }>('POST', `/api/v1/sources/${encodeURIComponent(keyId)}/go`);
/** Not yet: the late count stops, and the source goes on air by a press. */
export const sourceNotYet = (keyId: string) => call<unknown>('POST', `/api/v1/sources/${encodeURIComponent(keyId)}/not-yet`);
/** Off air on a kept place: a source whose link is lost has its place let go at once. `covered`
 *  says automation took the air with it. Refused (409) unless the key's place is kept now. */
export const sourceOffAir = (keyId: string) => call<{ let_go: boolean; covered: boolean }>('POST', `/api/v1/sources/${encodeURIComponent(keyId)}/off-air`);
/** Let back in: the key is taken again at once. */
export const letSourceBack = (keyId: string) => call<{ let_back: boolean }>('DELETE', `/api/v1/sources/${encodeURIComponent(keyId)}/disconnect`);
/** The rota of the coming twelve hours (staff). */
export const getRota = () => call<RotaView>('GET', '/api/v1/rota');
/** The rules for sources (an admin's). */
export const getSourceSettings = () => call<SourceSettings>('GET', '/api/v1/settings/sources');
/** Every rule at once (an admin's). Errors name the field. */
export const putSourceSettings = (s: SourceSettings) => call<SourceSettings>('PUT', '/api/v1/settings/sources', s);
