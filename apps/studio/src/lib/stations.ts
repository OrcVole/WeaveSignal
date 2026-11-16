// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The station switcher's rules and words, apart from the screen (ADR-0285;
// docs/reference/design/station-switcher.md): what a station is called here, its badge of two
// letters, what the row says of a station on another version, and whether leaving the station
// the window is on must be asked first.
//
// The three questions the design puts to the operator are answered here as recommended, and
// each answer is marked DECIDED (ADR-0287, the operator took the recommendations) where the code takes it.

import { isOpen } from '../../../../packages/ui/states.ts';
import type { LinkState, StationRow, View } from './api.ts';

/** A station's address without its scheme, as the list shows it. */
export function hostOf(url: string): string {
  return url.replace(/^https?:\/\//, '');
}

/** What a station is called on this computer: the label given here, else the station's own
 *  name, else its address. */
export function stationName(row: Pick<StationRow, 'label' | 'name' | 'url'>): string {
  return row.label || row.name || hostOf(row.url);
}

function words(name: string): string[] {
  return name.split(/[^\p{L}\p{N}]+/u).filter(Boolean);
}

/**
 * A badge of two letters for each station, no two alike: the first letters of the first two
 * words; taken already, the first and the last word (Kettle FM test is KT beside KF); then the
 * first two letters; then a letter and a digit. In list order, so a station keeps its badge
 * when one is added after it.
 *
 * DECIDED (ADR-0287, question 2): letters, and no colour for a station, because every colour family
 * in the studio already has one job.
 */
export function badges(rows: readonly Pick<StationRow, 'id' | 'label' | 'name' | 'url'>[]): Record<string, string> {
  const taken = new Set<string>();
  const out: Record<string, string> = {};
  for (const row of rows) {
    const w = words(stationName(row));
    const first = [...(w[0] ?? 'S')];
    const tries: string[] = [];
    if (w.length > 1) {
      tries.push(first[0] + [...w[1]][0]);
      tries.push(first[0] + [...w[w.length - 1]][0]);
    }
    tries.push(first.slice(0, 2).join(''));
    for (let d = 2; d <= 9; d++) tries.push(first[0] + d);
    const pick = tries.map((t) => t.toLocaleUpperCase()).find((t) => !taken.has(t)) ?? first[0].toLocaleUpperCase();
    taken.add(pick);
    out[row.id] = pick;
  }
  return out;
}

/** Who the list says is signed in at a station. */
export function whoWords(row: Pick<StationRow, 'kept' | 'who'>): string {
  if (!row.kept) return 'signed out';
  return row.who || 'signed in';
}

/** A station's version beside the app's. Only the numbers count: `1.0.5-test.2` is 1.0.5. */
export type Versus = 'same' | 'older' | 'newer' | 'major' | 'unknown';

function core(v: string | null | undefined): number[] | null {
  const m = /^(\d+)\.(\d+)\.(\d+)/.exec((v ?? '').trim());
  return m ? m.slice(1).map(Number) : null;
}

export function versus(app: string | null | undefined, station: string | null | undefined): Versus {
  const [a, s] = [core(app), core(station)];
  if (!a || !s) return 'unknown';
  if (a[0] !== s[0]) return 'major';
  for (let i = 1; i < 3; i++) if (a[i] !== s[i]) return s[i] < a[i] ? 'older' : 'newer';
  return 'same';
}

/** What a row says of its station's version; nothing when it is the app's own, or not known. */
export function versionTag(app: string | null | undefined, station: string | null | undefined): string {
  switch (versus(app, station)) {
    case 'older':
      return `${station}, older than this app`;
    case 'newer':
      return `${station}, newer than this app`;
    case 'major':
      return `${station}, a different major version`;
    default:
      return '';
  }
}

/**
 * What the arrival note says of a station on another version; nothing when it is the app's own.
 *
 * DECIDED (ADR-0287, question 3): a station is never refused on its version alone. An older one of
 * the same major version works, with what it lacks hidden or switched off part by part
 * (`speaksRules` is the first such part); the note says which way the difference runs.
 */
export function versionNote(app: string | null | undefined, station: string | null | undefined): string {
  switch (versus(app, station)) {
    case 'older':
      return `It runs ${station}, older than this app (${app}): what it does not have yet is hidden or switched off here.`;
    case 'newer':
      return `It runs ${station}, newer than this app (${app}): update the app to use everything there.`;
    case 'major':
      return `It runs ${station}, a different major version from this app (${app}): some screens may not work until the two match.`;
    default:
      return '';
  }
}

/** The question asked before the window leaves a station (`studio.stations.leave`). */
export interface LeaveAsk {
  heading: string;
  /** What is on at the station, from the desk's own facts. */
  lead: string;
  /** What leaving does and does not do. */
  lines: string[];
  /** Whether anything was open: the arrival note then says the station carries on. */
  live: boolean;
}

const plural = (n: number, one: string, many: string) => (n === 1 ? one : many);

/**
 * Whether leaving the station must be asked first, and the words; null when the window may go
 * at once.
 *
 * DECIDED (ADR-0287, question 1): warn, then allow. A Preparing window changes nothing on air
 * (ADR-0276), so it has nothing to leave. A Live window is asked while a strip is open on the
 * desk or a caller is in screening, whoever holds which seat: the window cannot be sure whose
 * hands the show is in. When it cannot see the desk at all it says so and asks the same way,
 * as the update screen does (ADR-0140). It never refuses: an update can wait, a switch may be
 * the urgent thing, and the app can be closed without a question anyway.
 */
export function leaveAsk(view: View | null, link: LinkState, preparing: boolean, name: string): LeaveAsk | null {
  if (preparing) return null;
  const stays = (on: string) => [
    `${on} Switching changes nothing there.`,
    `This window stops controlling it. Nobody here can mute, dump or take a call on ${name} until you switch back.`,
  ];
  if (!view || link !== 'live') {
    return {
      heading: `Leave ${name}?`,
      lead: `This window cannot see ${name}’s desk, so it cannot tell whether a show is on air.`,
      lines: stays('If a show is on, it stays on.'),
      live: false,
    };
  }
  const open = view.sources.filter(isOpen).length;
  const screening = view.callers.filter((c) => c.state === 'screening').map((c) => c.name_on_air || c.name);
  if (open === 0 && screening.length === 0) return null;
  const onAir = view.desk.reachable && view.desk.playout.connected;
  const inScreening = screening.length
    ? `${screening.length === 1 ? screening[0] : `${screening.length} callers`} ${plural(screening.length, 'is', 'are')} in screening.`
    : '';
  if (open === 0) {
    return {
      heading: `Leave ${name}?`,
      lead: inScreening.replace(/\.$/, ` on ${name}.`),
      lines: [`They stay in screening. Switching changes nothing there.`, stays('')[1]],
      live: true,
    };
  }
  const strips = `${open} ${plural(open, 'strip is', 'strips are')} open on the desk`;
  return {
    heading: onAir ? `Leave ${name} while it is on air?` : `Leave ${name} while its room is open?`,
    lead: `${name} ${onAir ? 'is on air' : 'has its room open'}: ${strips}.${inScreening ? ` ${inScreening}` : ''}`,
    lines: stays(onAir ? 'It stays on air.' : 'The room stays open.'),
    live: true,
  };
}

/** What the window was told as it left a station, for the note on arrival at the next. */
export interface Arrival {
  /** The station left, by its name here; null when there was none. */
  from: string | null;
  /** Something was open there when the window left. */
  leftLive: boolean;
}

/** The arrival note: where the window is, who it is signed in as, what it left, and the
 *  station's version when it is not the app's. */
export function arrivalNote(name: string, who: string | null, arrival: Arrival, app: string | null, version: string | null): { title: string; message: string } {
  const parts = who ? [who === 'the station key' ? 'Signed in with the station key.' : `Signed in as ${who}.`] : [];
  if (arrival.from && arrival.leftLive) parts.push(`${arrival.from} carries on without this window.`);
  const v = versionNote(app, version);
  if (v) parts.push(v);
  return { title: `You are on ${name}`, message: parts.join(' ') };
}
