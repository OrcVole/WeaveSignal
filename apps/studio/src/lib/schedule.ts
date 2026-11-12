// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The Schedule tab's link to the control plane: shows, series and episodes (M8's routes),
// the scheduler's clocks, week grid and log (M4's), and the card preview. Types mirror the
// Rust structs field for field; times from the control plane are epoch seconds for episodes
// (`air_at`, `aired_at`) and epoch milliseconds for the log (`planned_at_ms`).

import { ApiError, base, headers, netFetch } from './api.ts';
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
  slot: Slot | null;
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
  slot: Slot | null;
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
}

export interface SeriesInput {
  title?: string | null;
  description?: string | null;
  hosts?: string[] | null;
  slot?: Slot | null;
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

export const weekEpisodes = (fromMs: number, toMs: number) =>
  get<{ zone: string; episodes: WeekEpisode[] }>(`/api/v1/schedule/episodes?from_ms=${fromMs}&to_ms=${toMs}`);
export const shows = () => get<Show[]>('/api/v1/shows');
/** The station's setup (ADR-0237): admins only; `pending` while the setup page is still offered. */
export const setupState = () => get<{ status: 'pending' | 'done'; missing: string[] }>('/api/v1/setup');
export const show = (slug: string) => get<ShowView>(`/api/v1/shows/${slug}`);
export const putShow = (slug: string, input: ShowInput) => call<ShowView>('PUT', `/api/v1/shows/${slug}`, input);
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
