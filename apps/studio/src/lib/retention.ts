// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Settings > Retention: the control plane's retention routes (crates/control/src/retention.rs)
// and the words the screen prints for a period. The rules live on the server
// (weavesignal_model::retention); this file only reads and words them.

import { ApiError, base, headers, netFetch } from './api.ts';

export type Kind =
  | 'programme_recordings'
  | 'per_play_log'
  | 'chat_messages'
  | 'moderation_records'
  | 'consent'
  | 'call_recordings'
  | 'listener_statistics'
  | 'access_logs';

export type Period =
  | { keep: 'days'; days: number }
  | { keep: 'with_archive' }
  | { keep: 'after_recordings'; days: number }
  | { keep: 'with_call' }
  | { keep: 'no_limit' };

export type Periods = Record<Kind, Period>;

export interface Floor {
  days: number;
  rule: string;
  /** When the floor applies, where it applies only in some cases (ADR-0155). */
  condition?: string | null;
}

export interface Row {
  kind: Kind;
  label: string;
  period: Period;
  source: string;
  floor: Floor | null;
  /** Whether this release keeps records of the kind, and so deletes them. */
  swept: boolean;
}

export interface ComplaintHold {
  days: number | null;
  rule: string;
}

export interface PresetView {
  key: string;
  name: string;
  summary: string;
  licensed: boolean;
  complaint_hold: ComplaintHold | null;
  rows: Row[];
}

export interface Warning {
  kind: Kind | null;
  level: 'warning' | 'serious';
  title: string;
  message: string;
  source: string;
}

export interface SweepReport {
  dry_run: boolean;
  preset: string;
  recordings_deleted: number;
  files_missing: number;
  consents_deleted: number;
  /** Released holds whose own record went (ADR-0157). */
  holds_deleted?: number;
  held: number;
  errors: string[];
}

export interface LastSweep {
  finished_at_ms: number;
  preset: string;
  recordings_deleted: number;
  files_missing: number;
  consents_deleted: number;
  held: number;
  errors: string;
}

export interface RetentionView {
  setting: {
    preset: string;
    based_on: string | null;
    broadcast_licence: boolean | null;
    us_statutory_licence: boolean | null;
    updated_by: string | null;
    updated_at_ms: number | null;
  };
  rows: Row[];
  warnings: Warning[];
  complaint_hold: ComplaintHold | null;
  presets: PresetView[];
  presets_as_of: string;
  last_sweep: LastSweep | null;
  sweep_every_s: number | null;
}

export interface Hold {
  id: number;
  reason: string;
  placed_by: string;
  placed_at_ms: number;
  from_ms: number | null;
  to_ms: number | null;
  records: { kind: 'recording' | 'consent'; id: number }[];
  released_by: string | null;
  released_at_ms: number | null;
  release_reason: string | null;
  /** The date its latest extension fixed (ADR-0156), which replaces the complaint rule's. */
  extended_until_ms: number | null;
  extended_by: string | null;
  extended_at_ms: number | null;
  extend_reason: string | null;
  recordings: number;
  consents: number;
}

/** What the screen sends: a preset, or Custom with every period. */
export interface Choice {
  preset: string;
  based_on: string | null;
  periods: Periods | null;
  broadcast_licence: boolean | null;
  us_statutory_licence: boolean | null;
}

export const NO_RETENTION = 'none';
export const CUSTOM = 'custom';

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await netFetch(`${base()}${path}`, {
    method,
    headers: headers(body !== undefined),
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, (data as { error?: string }).error ?? `${res.status} ${res.statusText}`);
  return data as T;
}

export const load = () => call<RetentionView>('GET', '/api/v1/retention');
export const save = (c: Choice, by: string) => call<RetentionView>('PUT', '/api/v1/retention', { ...c, by });
export const preview = (c: Choice) =>
  call<{ warnings: Warning[]; sweep: SweepReport }>('POST', '/api/v1/retention/preview', c);
export const holds = () => call<Hold[]>('GET', '/api/v1/retention/holds');
export const placeHold = (h: { reason: string; by: string; from_ms: number; to_ms: number }) =>
  call<Hold>('POST', '/api/v1/retention/holds', h);
export const extendHold = (id: number, until_ms: number, by: string, reason: string) =>
  call<Hold>('POST', `/api/v1/retention/holds/${id}/extend`, { until_ms, by, reason });
export const releaseHold = (id: number, by: string, reason: string) =>
  call<Hold>('POST', `/api/v1/retention/holds/${id}/release`, { by, reason });

/** The periods in a list of rows. */
export function periodsOf(rows: Row[]): Periods {
  return Object.fromEntries(rows.map((r) => [r.kind, r.period])) as Periods;
}

/** The choice as stored: a preset sends no periods. */
export function choiceOf(v: RetentionView): Choice {
  const s = v.setting;
  return {
    preset: s.preset,
    based_on: s.preset === CUSTOM ? s.based_on : null,
    periods: s.preset === CUSTOM ? periodsOf(v.rows) : null,
    broadcast_licence: s.broadcast_licence,
    us_statutory_licence: s.us_statutory_licence,
  };
}

export function sameChoice(a: Choice, b: Choice): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

const YEARS: Record<number, string> = { 365: '1 year', 730: '2 years', 1096: '3 years', 1461: '4 years', 2192: '6 years' };

/** `42 days`, `1,096 days (3 years)`, `not kept after the show`. */
export function daysWords(days: number): string {
  if (days === 0) return 'Not kept after the show';
  const n = `${days.toLocaleString('en-GB')} ${days === 1 ? 'day' : 'days'}`;
  const y = YEARS[days] ?? (days >= 365 ? `about ${Math.round(days / 365.25)} years` : null);
  return y ? `${n} (${y})` : n;
}

/** A period in words, as the "Kept" column prints it. */
export function periodWords(p: Period): string {
  switch (p.keep) {
    case 'days':
      return daysWords(p.days);
    case 'with_archive':
      return 'With the programme’s archive';
    case 'after_recordings':
      return `While a recording of the call exists, then ${daysWords(p.days).toLowerCase()}`;
    case 'with_call':
      return 'Until the call ends';
    case 'no_limit':
      return 'Kept, as anonymous totals only';
  }
}

/** The period as days, for comparing with a floor (null: it has no end to compare). */
export function floorDays(p: Period, logger: Period): number | null {
  switch (p.keep) {
    case 'days':
      return p.days;
    case 'with_archive':
      return logger.keep === 'days' ? logger.days : null;
    case 'with_call':
      return 0;
    default:
      return null;
  }
}

/** Whether a period is under its floor. */
export function underFloor(row: Row, p: Period, logger: Period): boolean {
  if (!row.floor) return false;
  const d = floorDays(p, logger);
  return d !== null && d < row.floor.days;
}

/** The drop-down's groups: No retention first, then by country, Custom last. */
export const GROUPS: { label: string | null; keys: string[] }[] = [
  { label: null, keys: ['none'] },
  { label: 'United Kingdom', keys: ['uk_ofcom', 'uk_internet'] },
  { label: 'United States', keys: ['us_webcaster', 'us_nce'] },
  { label: 'European Union', keys: ['eu_gdpr', 'ie_licensed', 'ie_internet', 'de'] },
  { label: 'Canada', keys: ['ca_licensed', 'ca_internet'] },
  { label: 'Australia', keys: ['au_licensed', 'au_internet'] },
  { label: null, keys: ['custom'] },
];

/** `26 Oct 2026, 14:05` in this machine's time zone. */
export function when(ms: number): string {
  return new Date(ms).toLocaleString('en-GB', {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

/** `datetime-local`'s value for an epoch-ms time, in this machine's time zone. */
export function localInput(ms: number): string {
  const d = new Date(ms - new Date(ms).getTimezoneOffset() * 60_000);
  return d.toISOString().slice(0, 16);
}

/** The date a hold is kept until: the one its latest extension fixed, else the one a complaint
 * rule gives, from the start of its broadcast window (null where the rule fixes no period or the
 * hold has no window). The control plane computes the same (ADR-0156). */
export function holdKeepUntil(h: Pick<Hold, 'from_ms' | 'extended_until_ms'>, rule: ComplaintHold | null): number | null {
  if (h.extended_until_ms !== null) return h.extended_until_ms;
  if (!rule?.days || h.from_ms === null) return null;
  return h.from_ms + rule.days * 86_400_000;
}

/** Whether an open hold is past that date (ADR-0156): flagged, never released automatically;
 * extending it clears the flag until the new date. */
export function holdPastDate(
  h: Pick<Hold, 'from_ms' | 'extended_until_ms' | 'released_at_ms'>,
  rule: ComplaintHold | null,
  now: number,
): boolean {
  const until = holdKeepUntil(h, rule);
  return h.released_at_ms === null && until !== null && until < now;
}

/** "12 recordings and 3 callers’ consent records". */
export function goneWords(r: { recordings_deleted: number; consents_deleted: number }): string {
  const parts: string[] = [];
  if (r.recordings_deleted) parts.push(`${r.recordings_deleted} ${r.recordings_deleted === 1 ? 'recording' : 'recordings'}`);
  if (r.consents_deleted)
    parts.push(`${r.consents_deleted} ${r.consents_deleted === 1 ? 'caller’s consent record' : 'callers’ consent records'}`);
  return parts.length ? parts.join(' and ') : 'nothing';
}
