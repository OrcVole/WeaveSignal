// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Times and levels as the surfaces print them.
import { instant, parts } from '../schedule/model.ts';
import type { ListenerDelay } from './api.ts';

const pad = (n: number) => String(n).padStart(2, '0');

/** `13:57:12` for an epoch-ms time, in this machine's time zone. */
export function hms(ms: number): string {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

export function hm(ms: number): string {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** `2:48` for a duration in ms; hours when it has them. */
export function dur(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h ? `${h}:${pad(m)}:${pad(s % 60)}` : `${m}:${pad(s % 60)}`;
}

/** A rundown time (`HH:MM` or `HH:MM:SS`) as epoch ms today, around `now`, on the station's
 *  wall clock in `zone` (IANA); with no zone, on this machine's (issue 10: it always was). */
export function atToday(at: string, now: number, zone = ''): number {
  const [h, m, s = 0] = at.split(':').map(Number);
  if (zone) {
    try {
      const p = parts(now, zone);
      return instant({ y: p.y, m: p.m, d: p.d, hh: h, mm: m }, zone) + s * 1000;
    } catch {
      // An unknown zone: this machine's clock, as before.
    }
  }
  const d = new Date(now);
  d.setHours(h, m, s, 0);
  return d.getTime();
}

/** −22.6 with a true minus sign, one decimal. */
export function signed(x: number): string {
  const v = Math.abs(x).toFixed(1);
  return x < 0 && v !== '0.0' ? `−${v}` : x > 0 && v !== '0.0' ? `+${v}` : v;
}

/** The meter's scale: −45 to −5 LUFS across the bar. */
export function meterPct(lufs: number | null): number {
  if (lufs === null) return 0;
  return Math.max(0, Math.min(100, ((lufs + 45) / 40) * 100));
}

export function firstName(name: string, fallback: string): string {
  return (name.trim().split(/\s+/)[0] || fallback).trim();
}

/** The listener delay the surfaces show (ADR-0103): the view's figure, or spike 01's 15 s before
 *  the first view arrives. */
export function listenerDelayMs(d: { ms: number } | undefined | null): number {
  return d?.ms ?? 15_000;
}

/** The listener delay as one sentence for the stage's subline and the delay panel (ADR-0103,
 *  ADR-0272). `measured` is the whole path read, so the figure then stands alone. Otherwise it
 *  is an estimate, and the words say what is read live and what is not: the profanity delay and
 *  the listener process are read, the mount's own hold and a listener's player are not. */
export function delayWords(d: ListenerDelay | undefined | null): string {
  const s = Math.round(listenerDelayMs(d) / 1000);
  if (isMeasured(d)) return `${s} s behind`;
  return `about ${s} s behind (an estimate)`;
}

/** The whole path was measured: the station says `measured` (1.1), or, before it did, `measured`
 *  was true. An estimate otherwise. */
const isMeasured = (d: ListenerDelay | undefined | null) => (d?.basis ? d.basis === 'measured' : !!d?.measured);

/** Seconds to one decimal, a whole number without it: what a setting says (`13 s`, `14.5 s`). */
const setSecs = (ms: number) => `${Number.isInteger(ms / 1000) ? ms / 1000 : (ms / 1000).toFixed(1)} s`;

/** The delay on the clock's line (issue 17; the look's review, finding 3: each panel states the
 *  figure once). While it is an estimate, *about 23 s behind*, and the line under it names the
 *  parts (`delayParts`); once the whole path is measured, the plain figure. A control plane
 *  before 1.1 sends no parts, so nothing under the line says what the figure is: the line then
 *  keeps *(an estimate)*, as it did. */
export function delayLine(d: ListenerDelay | undefined | null): string {
  if (isMeasured(d) || !delayParts(d)) return delayWords(d);
  return `about ${Math.round(listenerDelayMs(d) / 1000)} s behind`;
}

/** What the figure is made of, in a presenter's words, for the one line under the clock's (ADR-0289
 *  item 1 keeps the estimate labelled with its parts): *8.0 s profanity delay, 0.5 s buffer, about
 *  14.5 s in listeners’ players (estimated)*. The profanity delay and the buffer (what the listener
 *  process holds of playout's feed) are the station's own readings, to 0.1 s; the rest is the
 *  setting that stands for the stream's own hold and each listener's player, a whole number
 *  without a decimal, and it alone is called estimated. Nothing here says *read* or *set*: those
 *  are the code's words (the API's `read_ms` and `set_ms`). Empty when the control plane is too
 *  old to say. */
export function delayParts(d: ListenerDelay | undefined | null): string {
  if (!d || d.desk_ms === undefined || d.rest_ms === undefined) return '';
  const s = (ms: number) => `${(ms / 1000).toFixed(1)} s`;
  const parts = [`${s(d.desk_ms)} profanity delay`];
  if (d.process_ms != null) parts.push(`${s(d.process_ms)} buffer`);
  if (isMeasured(d)) {
    // Measured end to end: what is left of the figure is in the stream and the players, and nothing is estimated.
    const rest = d.ms - d.desk_ms - (d.process_ms ?? 0);
    if (rest > 0) parts.push(`${s(rest)} in listeners’ players`);
    return parts.join(', ');
  }
  return `${parts.join(', ')}, about ${setSecs(d.rest_ms)} in listeners’ players (estimated)`;
}
