// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Times and levels as the surfaces print them.
import { instant, parts } from '../schedule/model.ts';

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

/** `15 s behind`, labelled as a setting while nothing measures it (ADR-0103). */
export function delayWords(d: { ms: number; measured: boolean } | undefined | null): string {
  const s = Math.round(listenerDelayMs(d) / 1000);
  return d?.measured ? `${s} s behind` : `${s} s behind (set, not measured)`;
}
