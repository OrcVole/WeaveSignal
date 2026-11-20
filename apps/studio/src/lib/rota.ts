// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The rota (1.2; handover-window.md): the coming twelve hours as the station lists them
// (`GET /api/v1/rota`), put into rows a person reads at a glance: when, the show, who is down for
// it, where they come from, and how far they have got. The station works every fact out; this
// only chooses the words, and says nothing the station did not.

import type { RotaRow, RotaView } from './api.ts';
import { clockWords } from './hosts.ts';
import { fromWords, readinessChip } from './window.ts';
import { CHIP, type Chip } from '../../../../packages/ui/states.ts';

/** How soon a host is looked for: inside this, *not connected* is said as a state, with its
 *  chip; further off it is only where things stand, in quiet words. */
export const SOON_MS = 3600_000;

export interface RotaLine {
  key: string;
  /** *14:00 – 16:00* on the station's clock. */
  time: string;
  /** *on air now*, *in 8 min*, *in 2 h 08 min*. */
  when: string;
  show: string;
  /** Who is down for it; empty when nobody is. */
  who: string;
  nobody: boolean;
  /** *in the room*, *own studio*. */
  from: string | null;
  /** Their state as a chip of the vocabulary, where it is news; else `quiet` says it. */
  chip: { chip: Chip; word: string } | null;
  quiet: string | null;
  now: boolean;
  /** The row as one sentence, for a screen reader. */
  said: string;
}

/** *in 8 min*, *in 2 h 08 min*, *in 3 h*. */
export function inWords(ms: number): string {
  const min = Math.max(1, Math.ceil(ms / 60_000));
  if (min < 60) return `in ${min} min`;
  const h = Math.floor(min / 60);
  const m = min % 60;
  return m ? `in ${h} h ${String(m).padStart(2, '0')} min` : `in ${h} h`;
}

function stateOf(r: RotaRow, now: number): Pick<RotaLine, 'chip' | 'quiet'> {
  if (!r.hosts.length) return { chip: null, quiet: r.now ? 'Nobody is down for it' : null };
  if (r.state === 'on_air') return { chip: { chip: CHIP.onAir, word: 'On air' }, quiet: null };
  if (r.state === 'ready') return { chip: readinessChip('ready'), quiet: null };
  if (r.state === 'connecting') return { chip: readinessChip('connecting'), quiet: null };
  // Not connected: news once the show is under way or within the hour; before that, only so.
  if (r.now || r.start_ms - now <= SOON_MS) return { chip: readinessChip('not_here'), quiet: null };
  return { chip: null, quiet: 'Not due yet' };
}

/** The rota's rows, in the station's order: the airing on air now first. */
export function rotaLines(view: RotaView | null | undefined, zone: string): RotaLine[] {
  if (!view) return [];
  return view.airings.map((r) => {
    const time = `${clockWords(r.start_ms, zone)} – ${clockWords(r.end_ms, zone)}`;
    const when = r.now ? 'on air now' : inWords(r.start_ms - view.now_ms);
    const who = r.hosts.join(', ');
    const from = r.hosts.length ? fromWords(r.from) : null;
    const st = stateOf(r, view.now_ms);
    const stateWords = st.chip?.word.toLowerCase() ?? st.quiet?.toLowerCase() ?? '';
    return {
      key: `${r.show}@${r.start_ms}`,
      time,
      when,
      show: r.show_name,
      who,
      nobody: r.hosts.length === 0,
      from,
      ...st,
      now: r.now,
      said: `${r.show_name}, ${time.replace(' – ', ' to ')}, ${when}. ${who ? `${who}, ${from ?? ''}` : 'Nobody is down for it'}${stateWords && who ? `: ${stateWords}` : ''}.`,
    };
  });
}

/** The head's line: *13:52 to 01:52 · 4 shows · 1 with nobody down*. */
export function rotaHead(view: RotaView | null | undefined, zone: string): string {
  if (!view) return '';
  const n = view.airings.length;
  const nobody = view.airings.filter((r) => !r.hosts.length).length;
  return [`${clockWords(view.now_ms, zone)} to ${clockWords(view.until_ms, zone)}`, `${n} ${n === 1 ? 'show' : 'shows'}`, nobody ? `${nobody} with nobody down` : null].filter(Boolean).join(' · ');
}
