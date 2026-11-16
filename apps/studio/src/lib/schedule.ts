// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The Schedule tab's link to the control plane: shows, series and episodes (M8's routes),
// the scheduler's clocks, week grid and log (M4's), and the card preview. Types mirror the
// Rust structs field for field; times from the control plane are epoch seconds for episodes
// (`air_at`, `aired_at`) and epoch milliseconds for the log (`planned_at_ms`).

import {
  ApiError,
  asNextOn,
  asRuleWords,
  asScheduleDates,
  base,
  headers,
  netFetch,
  type IsoDate,
  type NextOn,
  type Rule,
  type RuleWords,
  type ScheduleDate,
  type ShowStep,
} from './api.ts';
import { dataUrl } from './video.ts';

/** A weekly slot: weekday 1 (Monday) to 7, the start in minutes after midnight, the length. */
export interface Slot {
  weekday: number;
  start: number;
  minutes: number | null;
}

export interface Show {
  id: string;
  slug: string;
  name: string;
  description: string;
  colour_slot: number;
  layout: 'poster' | 'slate' | string;
  /** The weekly slot, when the rule is one an older studio can draw; else null. */
  slot: Slot | null;
  /** When the show airs (ADR-0278); absent from a station older than 1.0.4. */
  rule?: Rule | null;
  /** The rule in the control plane's words; null with no rule. */
  rule_words?: RuleWords | null;
  /** The next date, or why there is none. */
  next_on?: NextOn;
  hosts: string[];
  tags: string[];
  explicit: boolean;
  feed_guid: string | null;
  cards: Cards | null;
  /** The presenter's on-air name (ADR-0214); empty when the show has none of its own. */
  presenter: string;
  /** The show's own takedown contact (ADR-0214); empty when the station's is the only one. */
  contact: string;
  /** Links to the presenter's channels (ADR-0219). */
  links: string[];
  /** Whether the show plays commercial music (ADR-0219): a reminder, it blocks nothing. */
  commercial_music: boolean;
  /** The staff account that presents the show (ADR-0218), or null. */
  presenter_account: string | null;
  /** How much of the studio the show uses (ADR-0271); absent from a control plane too old to
   *  report it. Shows that existed before steps are `full`; a new show starts at `solo`. */
  step?: ShowStep;
}

export interface Cards {
  link?: string;
  post?: string;
  square?: string;
  alt?: string;
}

export interface Series {
  id: string;
  show_id: string;
  number: number;
  title: string | null;
  description: string | null;
  hosts: string[] | null;
  slot: Slot | null;
  /** The series' own rule; null follows the show's (ADR-0278). */
  rule?: Rule | null;
  /** The rule in force for the series (its own, else its show's), in the station's words. */
  rule_words?: RuleWords | null;
  starts_on: string | null;
  planned_episodes: number | null;
  tags: string[] | null;
  cards: Cards | null;
}

/** Live, or played from a recording (ADR-0188). */
export type EpisodeKind = 'live' | 'prerecorded';

export interface Episode {
  id: string;
  series_id: string;
  number: number;
  title: string;
  description: string;
  kind: EpisodeKind;
  /** Epoch seconds. */
  air_at: number | null;
  minutes: number | null;
  hosts: string[] | null;
  guests: string[];
  tags: string[] | null;
  aired_at: number | null;
  /** The rule date the episode fills (ADR-0278); null when made or left by hand, absent from
   *  an older station. */
  occurrence?: IsoDate | null;
  frozen: Resolved | null;
  files: unknown;
  cards: Cards | null;
}

export interface Resolved {
  show: string;
  slug: string;
  description: string;
  colour_slot: number;
  layout: string;
  slot: Slot | null;
  /** The rule in force at this level: the series' own, else the show's. */
  rule?: Rule | null;
  rule_words?: RuleWords | null;
  hosts: string[];
  guests: string[];
  tags: string[];
  explicit: boolean;
  series: { number: number; title: string | null; first: string | null; last: string | null; episodes: number | null } | null;
  episode: { id: string; number: number; title: string; description: string; kind: EpisodeKind; air_at: number | null; minutes: number | null } | null;
  /** The fields that came from the level above: `hosts`, `slot`, `tags`, `description`, `minutes`. */
  inherited: string[];
  frozen: boolean;
}

export interface WeekEpisode {
  show: Show;
  series: Series;
  episode: Episode;
  resolved: Resolved;
}

export interface ShowView {
  show: Show;
  resolved: Resolved;
  series: Series[];
}

export interface EpisodeView {
  episode: Episode;
  resolved: Resolved;
}

export interface SeriesView {
  series: Series;
  resolved: Resolved;
  episodes: EpisodeView[];
}

export interface ShowInput {
  name: string;
  description: string;
  colour_slot: number;
  layout: string;
  /** The weekly slot: what a station older than 1.0.4 keeps. A newer one ignores it when
   *  `rule` is sent. */
  slot?: Slot | null;
  /** When the show airs (ADR-0278): left out keeps, null clears. Never sent to a station
   *  older than 1.0.4, which refuses a field it does not know. */
  rule?: Rule | null;
  /** Make series 1 if there is none and draft the first episode, in the same transaction. */
  draft_first?: boolean;
  hosts: string[];
  tags: string[];
  explicit: boolean;
  /** The presenter's fields (ADR-0214, ADR-0219): left out, the station keeps what it has. */
  presenter?: string;
  contact?: string;
  links?: string[];
  commercial_music?: boolean;
  /** Left out keeps; null clears; a staff account's id ties the show to it (ADR-0218). */
  presenter_account?: string | null;
  /** The show's step (ADR-0271): left out keeps what is stored, a new show starts at `solo`.
   *  Changing it takes an admin, a producer or the show's presenter. */
  step?: ShowStep;
}

export interface SeriesInput {
  title?: string | null;
  description?: string | null;
  hosts?: string[] | null;
  slot?: Slot | null;
  /** The series' own rule; null follows the show again. Not sent to an older station. */
  rule?: Rule | null;
  starts_on?: string | null;
  planned_episodes?: number | null;
  tags?: string[] | null;
}

/** A change to an episode: a field left out is untouched; an inheriting field set to `null` goes
 *  back to inheriting (the control plane's `EpisodePatch`). */
export interface EpisodePatch {
  title?: string;
  description?: string;
  /** Live or pre-recorded; it has no level above to inherit from. */
  kind?: EpisodeKind;
  /** An RFC 9557 zoned time, `2026-10-24T20:00:00+01:00[Europe/London]`, or null. */
  air_at?: string | null;
  minutes?: number | null;
  hosts?: string[] | null;
  guests?: string[];
  tags?: string[] | null;
}

/** A correction to the public text of an aired episode (ADR-0177): a field left out is not
 *  changed, and a reason is needed. */
export interface CorrectionInput {
  title?: string;
  description?: string;
  hosts?: string[];
  guests?: string[];
  tags?: string[];
  reason: string;
}

/** One line of an episode's corrections log: who, why, and the fields as they were and became. */
export interface Correction {
  id: string;
  episode_id: string;
  /** Epoch seconds. */
  made_at: number;
  /** A username, or "station key". */
  made_by: string;
  reason: string;
  old_values: Record<string, unknown>;
  new_values: Record<string, unknown>;
  /** An erasure took a person's name out of this line. */
  redacted: boolean;
}

export interface Corrected extends EpisodeView {
  correction: Correction;
  /** The published files carry the corrected tags and card. */
  files_retagged: boolean;
  files_problem: string | null;
}

export type ClockSlot = { category: string } | { cart: string };

export interface Clock {
  id: string;
  name: string;
  slots: ClockSlot[];
}

/** Which clock runs a local hour: weekday 0 (Monday) to 6. */
export interface WeekHour {
  weekday: number;
  hour: number;
  clock: string;
}

export interface Track {
  id: string;
  title: string;
  artist: string;
  album: string;
  label: string;
  isrc: string | null;
  category: string;
  file: string;
  duration_ms: number;
  enabled: boolean;
}

export interface LogItem {
  id: number;
  planned_at_ms: number;
  hour_ms: number;
  clock: string | null;
  slot: number;
  kind: 'track' | 'cart' | string;
  track: string | null;
  cart: string | null;
  category: string;
  duration_ms: number;
  status: 'planned' | 'queued' | 'played' | 'skipped' | string;
  note: string;
  title: string;
  artist: string;
  /** The rules it breaks, in the scheduler's words; `(US)` marks the statutory licence. */
  breaches: string[];
}

export interface PlayoutSettings {
  time_zone: string;
  artist_separation_min: number;
  title_separation_min: number;
  album_separation_min: number;
  mode: string;
  default_clock: string | null;
}

/** The card preview's fields (the cards crate's `CardData`); `station` is filled in by the
 *  control plane when left out. */
export interface CardFields {
  layout: 'poster' | 'slate';
  colour_slot: number;
  station?: { name: string; place: string; address: string };
  show: { name: string; slug: string; description?: string; slot?: { weekday: string; start: string; end?: string } | null };
  series?: { number: number; title?: string | null; first?: string | null; last?: string | null; episodes?: number | null } | null;
  episode?: { number: number; title: string; air?: string | null } | null;
  hosts: string[];
  guests: string[];
}

export interface CardWords {
  alt: string;
  path: string;
  level: 'show' | 'series' | 'episode';
  station: { name: string; place: string; address: string };
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await netFetch(`${base()}${path}`, {
    method,
    headers: headers(body !== undefined),
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (res.status === 204) return undefined as T;
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, data.error ?? `${res.status} ${res.statusText}`);
  return data as T;
}

const get = <T>(path: string) => call<T>('GET', path);

/** A show as the station sent it, with its rule's words and next date made whole. A station
 *  older than 1.0.4 sends neither, and the show is returned as it came. */
export function asShow(s: Show): Show {
  const raw = s as Show & { next_on_reason?: unknown };
  if (raw.rule === undefined && raw.next_on === undefined && raw.rule_words === undefined) return s;
  const words = asRuleWords(s.rule_words);
  // The last date a finished show had: the station's, or a one-off's one date.
  const last = words?.last_on ?? (s.rule?.repeat.kind === 'once' ? s.rule.starts_on : null);
  return { ...s, rule: s.rule ?? null, rule_words: words, next_on: asNextOn(raw.next_on, raw.next_on_reason, !!s.rule, last) };
}
const asShowView = (v: ShowView): ShowView => ({ ...v, show: asShow(v.show) });

/** A show as a save sends it back, less when it airs: every other field the station holds,
 *  so a save that changes one thing keeps the rest. When it airs is added by the caller
 *  (`whenFields`), because what is sent depends on the station's version. */
export function inputOf(v: ShowView): ShowInput {
  const s = v.show;
  return {
    name: s.name,
    description: s.description,
    colour_slot: s.colour_slot,
    layout: s.layout,
    hosts: v.resolved.hosts,
    tags: s.tags,
    explicit: s.explicit,
    presenter: s.presenter ?? '',
    contact: s.contact ?? '',
    links: s.links ?? [],
    commercial_music: s.commercial_music ?? false,
  };
}

/** The week's episodes and, from a station that stores rules, every rule's unfilled dates in
 *  the window (`dates`; null from an older station). */
export const weekEpisodes = async (fromMs: number, toMs: number): Promise<{ zone: string; episodes: WeekEpisode[]; dates: ScheduleDate[] | null; publicUrl: string | null | undefined }> => {
  const w = await get<{ zone: string; episodes: WeekEpisode[]; dates?: unknown; public_url?: unknown }>(`/api/v1/schedule/episodes?from_ms=${fromMs}&to_ms=${toMs}`);
  return { zone: w.zone, episodes: w.episodes.map((e) => ({ ...e, show: asShow(e.show) })), dates: asScheduleDates(w.dates), publicUrl: asPublicUrl(w) };
};

/** The station's public address from the week's reply (`public_url`, 1.0.5): the address, null
 *  when the station has none, and undefined from a station too old to say (it has no calendar
 *  files either). */
export function asPublicUrl(week: { public_url?: unknown }): string | null | undefined {
  if (!('public_url' in week)) return undefined;
  return typeof week.public_url === 'string' && week.public_url ? week.public_url : null;
}
export const shows = async () => (await get<Show[]>('/api/v1/shows')).map(asShow);
/** The station's setup (ADR-0237): admins only; `pending` while the setup page is still offered. */
export const setupState = () => get<{ status: 'pending' | 'done'; missing: string[] }>('/api/v1/setup');
export const show = async (slug: string) => asShowView(await get<ShowView>(`/api/v1/shows/${slug}`));
export const putShow = async (slug: string, input: ShowInput) => asShowView(await call<ShowView>('PUT', `/api/v1/shows/${slug}`, input));
/** What a delete answers (ADR-0279): what went with the show, and how many files. */
export interface ShowDeleted {
  slug: string;
  name: string;
  series: number;
  episodes: number;
  cart_banks: number;
  carts: number;
  files_deleted: number;
}
/** Delete a show none of whose episodes has aired (`DELETE /api/v1/shows/{show}`, 1.0.4). An admin
 *  or a producer may. A refusal is an `ApiError` carrying the station's sentence: 409 for a show
 *  that has aired or is on air, 403 for anyone else. */
export const deleteShow = (slug: string) => call<ShowDeleted>('DELETE', `/api/v1/shows/${encodeURIComponent(slug)}`);
export const series = (slug: string, n: number) => get<SeriesView>(`/api/v1/shows/${slug}/series/${n}`);
export const putSeries = (slug: string, n: number, input: SeriesInput) =>
  call<SeriesView>('PUT', `/api/v1/shows/${slug}/series/${n}`, input);
export const createEpisodes = (slug: string, n: number, body: { count: number; title?: string; guests?: string[] }) =>
  call<EpisodeView[]>('POST', `/api/v1/shows/${slug}/series/${n}/episodes`, body);
export const patchEpisode = (slug: string, n: number, e: number, patch: EpisodePatch) =>
  call<EpisodeView>('PATCH', `/api/v1/shows/${slug}/series/${n}/episodes/${e}`, patch);
export const correctEpisode = (slug: string, n: number, e: number, input: CorrectionInput) =>
  call<Corrected>('POST', `/api/v1/shows/${slug}/series/${n}/episodes/${e}/corrections`, input);
export const corrections = (slug: string, n: number, e: number) =>
  get<Correction[]>(`/api/v1/shows/${slug}/series/${n}/episodes/${e}/corrections`);
export const airEpisode = (slug: string, n: number, e: number) =>
  call<EpisodeView>('POST', `/api/v1/shows/${slug}/series/${n}/episodes/${e}/air`, {});

export const clocks = async () => (await get<{ clocks: Clock[] }>('/api/v1/schedule/clocks')).clocks;
export const weekHours = async () => (await get<{ hours: WeekHour[] }>('/api/v1/schedule/week')).hours;
export const playoutSettings = () => get<PlayoutSettings>('/api/v1/playout/settings');

/** The station's zone and whether anyone has chosen it (ADR-0189). */
export interface StationZone {
  zone: string;
  set: boolean;
}
export const stationZone = () => get<StationZone>('/api/v1/playout/zone');
/** Saves the settings whole (an admin's); choosing the zone is `time_zone` in them. */
export const putPlayoutSettings = (s: PlayoutSettings) => call<PlayoutSettings>('PUT', '/api/v1/playout/settings', s);

export const tracks = async () => (await get<{ tracks: Track[] }>('/api/v1/library/tracks')).tracks;
export const log = async (fromMs: number, toMs: number) =>
  (await get<{ items: LogItem[] }>(`/api/v1/schedule/log?from_ms=${fromMs}&to_ms=${toMs}`)).items;
export const generate = (fromMs?: number, toMs?: number) =>
  call<{ items: unknown[]; gaps: { at_ms: number; slot: number; reason: string }[] }>('POST', '/api/v1/schedule/generate', {
    ...(fromMs !== undefined ? { from_ms: fromMs } : {}),
    ...(toMs !== undefined ? { to_ms: toMs } : {}),
  });
export const editItem = (id: number, track: string, force = false) =>
  call<{ id: number; track: string; note: string }>('PUT', `/api/v1/schedule/log/${id}`, { track, force });
export const deleteItem = (id: number) => call<void>('DELETE', `/api/v1/schedule/log/${id}`);

export const cardWords = (fields: CardFields) => call<CardWords>('POST', '/api/v1/cards/words', fields);

/** The card drawn from its fields, as a data: URL (the Tauri window's CSP allows `data:` images). */
export async function cardPreview(fields: CardFields, output: 'link' | 'post' | 'square' = 'link'): Promise<string> {
  const res = await netFetch(`${base()}/api/v1/cards/preview?output=${output}`, {
    method: 'POST',
    headers: headers(true),
    body: JSON.stringify(fields),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new ApiError(res.status, data.error ?? `${res.status} ${res.statusText}`);
  }
  return dataUrl('image/png', new Uint8Array(await res.arrayBuffer()));
}
