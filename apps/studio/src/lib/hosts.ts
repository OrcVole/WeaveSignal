// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Hosts in their own studios (ADR-0280, ADR-0284) in the studio app, as pure rules: what a
// source's tile says, a key's status in words, the details a host's software asks for and
// the text block to send them, and the checks the setup runs before it asks the station.
// The station stays the authority and its own sentence is shown as given when it refuses.
//
// The monitor link (design section 4.5) is a public page of the station, `monitor/`, opened by
// a link that carries its own secret after the `#`. The studio gives that link once, with the
// key's details; Talk on a source's tile reaches it while it is open. The post, the grace, the
// late count and a lost link are the station's since 1.1's post rule, and the desk's words for
// them are in `post.ts`.

import { parts } from '../schedule/model.ts';
import { publicAddress } from '../schedule/files/links.ts';
import type { DeskSource, KeyList, KeyRow, MadeKey, Me, ShowStep, SourceDetails, SourceLogEntry, SourceNow, SourceSettings, SourceWay } from './api.ts';
import { dur, firstName, signed } from './format.ts';

export const WAY_WORD: Record<SourceWay, string> = { icecast: 'Icecast', srt: 'SRT' };

/** The setup's one line on each way in (design section 8). */
export const WAY_LINE: Record<SourceWay, string> = {
  icecast: 'Works with most radio software. About 2 s to the station: right for a show on their own.',
  srt: 'For OBS and other software that offers it. Under half a second: needed for callers or a co-host.',
};

/** The whole of the leveller's reach, in dB either way. */
export const LEVELLER_REACH_DB = 12;

/** Why Talk is off on a source's tile (design sections 4.1 and 8): it goes to the host's
 *  monitor link, and with that not open nobody would hear it. */
export const talkOffWhy = (name: string): string => `${possessive(name)} monitor link is not open, so they would not hear you.`;
/** The tile's line on what they hear (design section 4.1). */
export const MONITOR_WORD = { open: 'Monitor open', closed: 'Monitor not open' } as const;
export const MONITOR_WHY = {
  open: 'Their monitor link is open: they hear the station without themselves, your cue, and Talk.',
  closed: 'Nobody has their monitor link open. They hear nothing from the station, and Talk is off.',
} as const;
/** The tile's Disconnect, in full (design section 8). */
export const DISCONNECT_WHY =
  'Closes the connection and turns this key away for the rest of today’s show. Let them back in, or end the key for good, in Settings.';

// ---- the tile ----

/** The role line: *Own studio*, and at large sizes the way in and the show. */
export function tileRole(s: DeskSource, showName = ''): { short: string; long: string } {
  const long = [WAY_WORD[s.way] ?? s.way, showName || s.show].filter(Boolean).join(' · ');
  return { short: 'Own studio', long };
}

/** The link line: the way in and how long it has held; Icecast says how far behind it is. */
export function tileLink(s: DeskSource): string {
  const held = `${WAY_WORD[s.way] ?? s.way} · steady ${dur(s.connected_ms)}`;
  return s.way === 'icecast' ? `${held} · about 2 s behind` : held;
}

/** `−7.4`, for *leveller −7.4 dB*. */
export const levellerFigure = (db: number): string => signed(db);

/** The leveller at the end of its reach: by how much it cannot help, and which way. */
export function levellerLimit(db: number): string | null {
  if (Math.abs(db) < LEVELLER_REACH_DB - 0.05) return null;
  return db > 0
    ? 'Too quiet for the station to make up: the leveller is at its +12 dB. Ask them to raise their software’s output.'
    : 'Too loud for the station to bring down: the leveller is at its −12 dB. Ask them to lower their software’s output.';
}

/** The one note under a source's tile: an Icecast source on a show with callers (design 4.1). */
export function tileNote(s: DeskSource, name: string, step: ShowStep): string | null {
  const limit = levellerLimit(s.leveller_db);
  if (limit) return limit;
  if (s.way === 'icecast' && (step === 'callers' || step === 'full')) return `Callers will hear ${name} about 2 s late. Use SRT for a show with callers.`;
  return null;
}

/** The foot's item (design section 4.2): `1 on air · 1 standing by`, or nothing with none. */
export function ownStudiosWords(sources: readonly { role: string; on_air: boolean }[]): string {
  const own = sources.filter((s) => s.role === 'source');
  const on = own.filter((s) => s.on_air).length;
  const by = own.length - on;
  return [on ? `${on} on air` : '', by ? `${by} standing by` : ''].filter(Boolean).join(' · ');
}

// ---- the toast when a source arrives ----

/** How long after it connects a source's arrival is said in the desk's toast. */
export const ARRIVAL_MS = 30_000;

interface Arriving {
  identity: string;
  name: string;
  role: string;
  slot: number;
  on_air: boolean;
  cue: boolean;
  source?: DeskSource | null;
}

/** The source that has just arrived standing by and has not been told of yet (design section
 *  3.2). Cueing it or putting it on air is an answer too, and the toast goes. */
export function arrival<T extends Arriving>(sources: readonly T[], told: readonly string[]): T | null {
  return sources.find((s) => s.role === 'source' && !!s.source && !s.on_air && !s.cue && s.source.connected_ms < ARRIVAL_MS && !told.includes(s.identity)) ?? null;
}

/** `Finlay is standing by` · `From their own studio, for Steeped at 14:00.` The time is given
 *  when the show is the next to start by the schedule. */
export function arrivalWords(s: Arriving, next: { slug: string; name: string; start_ms: number } | null, showName: string, zone: string): { title: string; message: string } {
  const mine = next && s.source && next.slug === s.source.show ? next : null;
  const show = mine?.name || showName || s.source?.show || 'their show';
  return {
    title: `${firstName(s.name, s.identity)} is standing by`,
    message: `From their own studio, for ${show}${mine ? ` at ${clockWords(mine.start_ms, zone)}` : ''}.`,
  };
}

// ---- who may ----

/** An admin, a producer or the station key sets up hosts for any show; the station also lets a
 *  show's own presenter manage its keys, which is Your show's to offer. */
export function mayManageHosts(me: Me | null | undefined): boolean {
  return !!me && (me.kind === 'station' || me.role === 'admin' || me.role === 'producer');
}

// ---- times in the station's zone ----

const WEEKDAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];
const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
const pad = (n: number) => String(n).padStart(2, '0');

function zoned(ms: number, zone: string) {
  try {
    return parts(ms, zone || 'UTC');
  } catch {
    return parts(ms, 'UTC');
  }
}

/** `13:41` on the station's clock. */
export function clockWords(ms: number, zone: string): string {
  const p = zoned(ms, zone);
  return `${pad(p.hh)}:${pad(p.mm)}`;
}

/** `today 13:41`, `yesterday 22:05`, or `Thursday 19 November, 15:48`, on the station's clock. */
export function whenWords(ms: number, now: number, zone: string): string {
  const p = zoned(ms, zone);
  const day = (q: { y: number; m: number; d: number }) => Date.UTC(q.y, q.m - 1, q.d);
  const apart = Math.round((day(zoned(now, zone)) - day(p)) / 86_400_000);
  const at = `${pad(p.hh)}:${pad(p.mm)}`;
  if (apart === 0) return `today ${at}`;
  if (apart === 1) return `yesterday ${at}`;
  if (apart === -1) return `tomorrow ${at}`;
  const wd = (new Date(day(p)).getUTCDay() + 6) % 7;
  return `${WEEKDAYS[wd]} ${p.d} ${MONTHS[p.m - 1]}, ${at}`;
}

// ---- a key's row ----

const REFUSAL: Record<string, string> = {
  wrong_key: 'the key is wrong',
  ended: 'the key has ended',
  not_your_slot: 'it was not the show’s slot, and tests are off',
  in_use: 'another connection was using the key',
  too_many: 'too many wrong keys came from that address',
  turned_away: 'the key was turned away from the desk',
  format: 'the station could not read what the software sent',
  desk_full: 'every place on the desk was taken',
  too_fast: 'the software sent more than 2 Mbit/s',
};

/** `refused today 13:41: the key is wrong`. A word this studio does not know is shown as given. */
export function refusalWords(e: SourceLogEntry, now: number, zone: string): string {
  const why = (e.reason && REFUSAL[e.reason]) || e.detail || e.reason || 'the station gave no reason';
  return `refused ${whenWords(e.at_ms, now, zone)}: ${why}`;
}

/** The chip on a key's row: the vocabulary's own words, or a plain *Ended*; none for a key that
 *  is neither connected nor due. */
export type KeyChip = 'on_air' | 'standby' | 'test' | 'not_connected' | 'ended' | null;

export interface KeyStatus {
  chip: KeyChip;
  /** The lines under the chip, the first the one that matters most. */
  lines: string[];
  refusal: string | null;
  /** *turned away by Shona until 16:00*, with Let back in. */
  turnedAway: string | null;
  /** A connection with this key is open now (at the desk or as a test). */
  connected: boolean;
  onAir: boolean;
  ended: boolean;
}

/** What `live.level` reads: LUFS at the desk, the station's own dBFS reading in a test. */
function levelWords(live: SourceNow): string | null {
  if (live.state === 'test') return live.level === null ? 'no sound yet' : `level ${signed(live.level)} dBFS`;
  if (live.leveller_db === null) return null;
  if (Math.abs(live.leveller_db) >= LEVELLER_REACH_DB - 0.05) return live.leveller_db > 0 ? 'too quiet for the leveller' : 'too loud for the leveller';
  return 'level in range';
}

export function keyStatus(key: KeyRow, live: SourceNow | undefined, now: number, zone: string): KeyStatus {
  const ended = key.revoked_ms !== null;
  const refusal = key.last_refusal ? refusalWords(key.last_refusal, now, zone) : null;
  const away = key.turned_away_until_ms !== null && key.turned_away_until_ms > now;
  const turnedAway = !ended && away ? `turned away${key.turned_away_by ? ` by ${key.turned_away_by}` : ''} until ${whenWords(key.turned_away_until_ms!, now, zone).replace(/^today /, '')}` : null;
  if (ended) {
    return { chip: 'ended', lines: [`ended ${whenWords(key.revoked_ms!, now, zone)}`], refusal, turnedAway: null, connected: false, onAir: false, ended };
  }
  const lines: string[] = [];
  const connected = !!live && live.state !== 'not_connected';
  if (live && connected) {
    const since = live.steady_ms !== null ? clockWords(now - live.steady_ms, zone) : null;
    // `steady_ms` is how long the connection has held, not how long it has been on air.
    const head = live.state === 'on_air' ? (since ? `on air · connected since ${since}` : 'on air') : `${live.state === 'test' ? 'a private test' : 'standing by'}${since ? ` since ${since}` : ''}`;
    lines.push([head, levelWords(live), live.monitor_open ? 'monitor open' : null].filter(Boolean).join(' · '));
    lines.push([live.way ? WAY_WORD[live.way] : null, live.format, live.slot !== null ? `strip ${live.slot}` : null].filter(Boolean).join(' · '));
    if (live.title) lines.push(`playing: ${live.title}`);
  } else if (live) {
    lines.push(live.door_closes_ms !== null ? `its slot is open until ${whenWords(live.door_closes_ms, now, zone).replace(/^today /, '')}` : 'its slot is open');
  }
  if (!connected) lines.push(key.last_used_ms !== null ? `last connected ${whenWords(key.last_used_ms, now, zone)}` : 'never connected yet');
  if (key.ends_on_disconnect) lines.push('ends when this connection closes: a new key was made');
  return { chip: live ? live.state : null, lines: lines.filter(Boolean), refusal, turnedAway, connected, onAir: live?.state === 'on_air', ended };
}

/** Live keys first (on air, standing by, a test, due, then the rest by show), the ended last. */
export function orderedKeys<T extends { key: KeyRow; status: KeyStatus; showName: string }>(rows: T[]): T[] {
  const RANK = { on_air: 0, standby: 1, test: 2, not_connected: 3 } as const;
  const r = (x: T) => (x.status.ended || x.status.chip === 'ended' ? 9 : x.status.chip ? RANK[x.status.chip] : 4);
  return [...rows].sort((a, b) => r(a) - r(b) || a.showName.localeCompare(b.showName) || b.key.created_ms - a.key.created_ms);
}

/** The foot of the panel's heading: `2 keys work · 1 on air · 1 standing by`. */
export function keysSummary(statuses: KeyStatus[]): string {
  const live = statuses.filter((s) => !s.ended);
  const count = (n: number, one: string, many = one) => (n ? `${n} ${n === 1 ? one : many}` : '');
  return [
    `${live.length} ${live.length === 1 ? 'key works' : 'keys work'}`,
    count(live.filter((s) => s.chip === 'on_air').length, 'on air'),
    count(live.filter((s) => s.chip === 'standby').length, 'standing by'),
    count(live.filter((s) => s.chip === 'test').length, 'testing'),
  ]
    .filter(Boolean)
    .join(' · ');
}

// ---- the setup ----

export const LABEL_MAX = 120;

/** Who the key is for: the sentence for the field, or null when fine (the station's own rule). */
export function labelProblem(label: string): string | null {
  const n = label.trim();
  if (!n) return 'Say who the key is for: their name, as it will read on the desk.';
  if ([...n].length > LABEL_MAX) return `A name is up to ${LABEL_MAX} characters.`;
  if (/[\u0000-\u001f\u007f]/.test(n)) return 'A name is plain text, on one line.';
  return null;
}

/** The first name, for a sentence: `Finlay` of `Finlay Grant`. */
export const hostName = (label: string): string => firstName(label, label);

/** `Finlay’s`, `Angus’s`. */
export const possessive = (name: string): string => `${name}’s`;

/** A show a key can be made for, as the setup lists it. */
export interface ShowChoice {
  slug: string;
  name: string;
  /** Who holds a key for it that has not ended. */
  holders: string[];
}

/** How the Show list names a show that already has a host: `Steeped · Finlay Grant has a key`. */
export function showOption(name: string, holders: string[]): string {
  if (!holders.length) return name;
  if (holders.length === 1) return `${name} · ${holders[0]} has a key`;
  return `${name} · ${holders.length} hosts have keys`;
}

/** A second key for a show whose host sends over Icecast (design section 12). */
export const SECOND_KEY_NOTE = 'This show already has a host. Each key is one person’s, ended on its own. Two hosts over Icecast cannot talk to each other: each hears the other seconds late.';

// ---- the details ----

export interface DetailField {
  label: string;
  value: string;
  /** Whether Copy is offered: never on a placeholder for a secret that cannot be shown again. */
  copy: boolean;
  /** The value is, or holds, the secret. */
  secret?: boolean;
  /** A value that is words, not something to type: drawn as text, not in a box. */
  words?: boolean;
}

/** The address a host's software is given: the station's own, else the one this studio uses. */
export function stationHost(details: SourceDetails, studioAddress: string): string {
  if (details.host) return details.host;
  try {
    return new URL(studioAddress).hostname;
  } catch {
    return '';
  }
}

const NO_HOST = 'the station’s address';
const SECRET_GONE = 'shown once, when the key was made';

/** The fields a host's software asks for, for each way in, in the words that software uses.
 *  With no secret (a key made earlier) the secret fields say so and offer no Copy. */
export function detailBlocks(d: SourceDetails, host: string, secret: string | null): Record<SourceWay, DetailField[]> {
  const server: DetailField = host ? { label: 'Server', value: host, copy: true } : { label: 'Server', value: NO_HOST, copy: false, words: true };
  const keyId = d.srt.streamid;
  const srtUrl = host && secret ? `srt://${host}:${d.srt.port}?streamid=${keyId}&passphrase=${secret}&pbkeylen=${d.srt.pbkeylen}` : null;
  return {
    icecast: [
      server,
      { label: 'Port', value: String(d.icecast.port), copy: true },
      { label: 'Mount', value: d.icecast.mount, copy: true },
      { label: 'User', value: d.icecast.user, copy: true },
      secret
        ? { label: 'Password', value: `${keyId}-${secret}`, copy: true, secret: true }
        : { label: 'Password', value: `${keyId}- then the secret (${SECRET_GONE})`, copy: false, words: true },
    ],
    srt: [
      server,
      { label: 'Port', value: String(d.srt.port), copy: true },
      { label: 'Stream ID', value: keyId, copy: true },
      secret ? { label: 'Passphrase', value: secret, copy: true, secret: true } : { label: 'Passphrase', value: `the secret (${SECRET_GONE})`, copy: false, words: true },
      { label: 'Key length', value: `${d.srt.pbkeylen} (AES-128)`, copy: false, words: true },
      ...(srtUrl ? [{ label: 'As one address', value: srtUrl, copy: true, secret: true }] : []),
    ],
  };
}

const WAY_HEAD: Record<SourceWay, string> = {
  icecast: 'Icecast (most radio software; about 2 s to the station)',
  srt: 'SRT (OBS and other software that offers it; needed for callers or a co-host)',
};

/** What to tell the host (design section 8): the station hands over by the schedule, so they
 *  go on air at their show's time, not when they connect. */
export const TELL_HOST =
  'Put these into your software, open the monitor link, and press connect. You can test at any time: outside your show’s slot nobody hears you. You go on air at your show’s time, not when you connect.';

/** The host's monitor link (design section 4.5): the station's public address, then `/monitor/#`,
 *  then the link's own secret. Null when the station's settings give no public address: the
 *  address this window talks to may sit behind a login (on Cloudron it does), and a host sent
 *  there would get a login page, so no link is made from it. */
export function monitorLink(publicUrl: string | null | undefined, secret: string): string | null {
  const p = publicAddress(publicUrl);
  return p && secret ? `${p}/monitor/#${secret}` : null;
}

/** How the link is formed, for a station with no public address to make it from. */
export const MONITOR_BY_HAND = 'The monitor link is the station’s public address, then /monitor/# and this secret.';

/** *Copy everything as text*: a block to paste into a message, the way they said they would send
 *  first. The studio writes its own, because the station's `as_text` gives the monitor link's
 *  secret without the link, and may not know the station's address. `link` is the whole monitor
 *  link, or null on a station with no public address. */
export function detailsText(made: MadeKey, showName: string, host: string, link: string | null = null): string {
  const blocks = detailBlocks(made.details, host, made.secret);
  const order: SourceWay[] = made.key.hint === 'srt' ? ['srt', 'icecast'] : ['icecast', 'srt'];
  const block = (w: SourceWay) => [WAY_HEAD[w], ...blocks[w].map((f) => `  ${f.label}: ${f.value}`)].join('\n');
  return [
    `${possessive(made.key.label)} details for ${showName}`,
    '',
    block(order[0]),
    '',
    block(order[1]),
    '',
    ...(link
      ? ['Monitor link (open it in a browser, and keep it open beside your software)', `  ${link}`]
      : ['Monitor link', `  ${MONITOR_BY_HAND}`, `  Secret: ${made.monitor_secret}`]),
    '',
    TELL_HOST,
    '',
  ].join('\n');
}

// ---- the rules ----

/** The grace's choices (design section 4.3): the station takes these four and no other. */
export const GRACES: readonly { s: number; label: string }[] = [
  { s: 0, label: 'Off' },
  { s: 60, label: '1 minute' },
  { s: 120, label: '2 minutes' },
  { s: 300, label: '5 minutes' },
];

export type RuleField = 'door_before_min' | 'late_count_s' | 'test_max_min' | 'port_icecast' | 'port_srt';

/** The checks the rules run before asking the station, as its own rule has them. */
export function rulesProblems(s: SourceSettings): Partial<Record<RuleField, string>> {
  const out: Partial<Record<RuleField, string>> = {};
  const whole = (v: number, lo: number, hi: number) => Number.isInteger(v) && v >= lo && v <= hi;
  if (!whole(s.door_before_min, 0, 240)) out.door_before_min = 'Whole minutes, 0 to 240.';
  if (!whole(s.late_count_s, 0, 120)) out.late_count_s = 'Whole seconds, 0 (off) to 120.';
  if (!whole(s.test_max_min, 1, 60)) out.test_max_min = 'Whole minutes, 1 to 60.';
  if (!whole(s.port_icecast, 1, 65_535)) out.port_icecast = 'A port is a number from 1 to 65535.';
  if (!whole(s.port_srt, 1, 65_535)) out.port_srt = 'A port is a number from 1 to 65535.';
  else if (s.port_srt === s.port_icecast) out.port_srt = 'Use a different number from the Icecast port.';
  return out;
}

/** The station names one field for two (`steady_s` for either time, `port_icecast` for either
 *  port): put its sentence under a field this panel draws, or leave it for the form. */
export function ruleFieldOf(field: string | null): RuleField | null {
  return field === 'door_before_min' || field === 'late_count_s' || field === 'test_max_min' || field === 'port_icecast' || field === 'port_srt' ? field : null;
}

// ---- the panel's rows ----

export interface HostShow {
  slug: string;
  name: string;
  colour_slot: number;
  next_on?: { start: number | null } | null;
}

export interface HostRow {
  key: KeyRow;
  show: HostShow;
  showName: string;
  status: KeyStatus;
  /** This is the show's only key that has not ended: ending it sends the show back to the room. */
  last: boolean;
}

/** Every key of every show as a row, the live ones in order and the ended ones apart. */
export function hostRows(shows: readonly HostShow[], lists: readonly KeyList[], live: readonly SourceNow[], now: number, zone: string): { live: HostRow[]; ended: HostRow[] } {
  const bySlug = new Map(shows.map((s) => [s.slug, s]));
  const liveByKey = new Map(live.map((s) => [s.key_id, s]));
  const rows: HostRow[] = [];
  for (const list of lists) {
    const show = bySlug.get(list.show) ?? { slug: list.show, name: list.show, colour_slot: 13 };
    const open = list.keys.filter((k) => k.revoked_ms === null).length;
    for (const key of list.keys) {
      rows.push({ key, show, showName: show.name, status: keyStatus(key, liveByKey.get(key.key_id), now, zone), last: key.revoked_ms === null && open === 1 });
    }
  }
  return {
    live: orderedKeys(rows.filter((r) => !r.status.ended)),
    ended: rows.filter((r) => r.status.ended).sort((a, b) => (b.key.revoked_ms ?? 0) - (a.key.revoked_ms ?? 0)),
  };
}

/** Who holds a live key for each show, for the setup's Show list. */
export function showChoices(shows: readonly HostShow[], lists: readonly KeyList[]): ShowChoice[] {
  const holders = new Map(lists.map((l) => [l.show, l.keys.filter((k) => k.revoked_ms === null).map((k) => k.label)]));
  return shows.map((s) => ({ slug: s.slug, name: s.name, holders: holders.get(s.slug) ?? [] }));
}

/** When a show is next on, for its row: `next today 14:00`, or that it has no date to come. */
export function nextWords(show: HostShow, now: number, zone: string): string {
  const start = show.next_on?.start ?? null;
  return start === null ? 'no date to come: a key works only around the show’s times' : `next ${whenWords(start * 1000, now, zone)}`;
}

/** A station that has none of these routes (older than 1.1): the screens leave themselves out. */
export function routeAbsent(e: unknown): boolean {
  const status = (e as { status?: unknown } | null)?.status;
  return status === 404 || status === 405;
}

/** The station is there and cannot keep keys (no database, or no token key): its own sentence. */
export function unavailableWords(e: unknown): string | null {
  const x = e as { status?: unknown; message?: unknown } | null;
  return x?.status === 503 ? String(x.message ?? 'The station cannot keep show keys.') : null;
}
