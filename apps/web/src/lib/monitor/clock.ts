// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The station's clock, as this page keeps it (ADR-0280; the design's section 12: "the page
// counts from the station's clock, measured over its own connection"). Every time and every
// count on the monitor link comes from here and never from the device's own clock, which may be
// minutes wrong or in another zone.
//
// Each view carries the station's time when it was made (`station.now_ms`). It reaches the page
// a little later, so the station's time less the page's own steady time at arrival is never more
// than the true difference, and is closest for the view that travelled fastest. The clock keeps
// the largest of the last readings: one slow view cannot pull it back, and a count does not
// stutter from event to event. A station whose own clock is set back (a correction, a restart)
// is followed at once. The page's steady time is `performance.now()`, which a change to
// the device's clock does not move.

/** How many readings are kept: about half a minute of views. */
const KEPT = 30;
/** A reading this far behind the best means the station's clock stepped back. */
const STEP_BACK_MS = 3000;

export class StationClock {
  #readings: number[] = [];
  #best: number | null = null;

  /** A view arrived: the station's time in it, and the page's steady time at that moment (ms). */
  read(stationMs: number, steadyMs: number): void {
    if (!Number.isFinite(stationMs) || !Number.isFinite(steadyMs)) return;
    const reading = stationMs - steadyMs;
    // A reading seconds behind the best is not a slow view: the station's clock was set back.
    // The old readings would hold the page ahead of it for half a minute, so they go.
    if (this.#best !== null && reading < this.#best - STEP_BACK_MS) this.#readings = [];
    this.#readings.push(reading);
    if (this.#readings.length > KEPT) this.#readings.shift();
    this.#best = Math.max(...this.#readings);
  }

  /** Whether the station has told the page its time yet. */
  get known(): boolean {
    return this.#best !== null;
  }

  /** The station's time at this steady time, in ms since the Unix epoch; null until known. */
  at(steadyMs: number): number | null {
    return this.#best === null ? null : Math.round(steadyMs + this.#best);
  }
}

/** `14:00:00` in a zone; `UTC` when the zone's name is not one the browser knows. */
export function hms(ms: number, zone: string): string {
  return fmt(zone, { hour: '2-digit', minute: '2-digit', second: '2-digit', hourCycle: 'h23' }).format(ms);
}

/** `14:00` in a zone. */
export function hm(ms: number, zone: string): string {
  return fmt(zone, { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' }).format(ms);
}

/** `Friday 27 November` in a zone. */
export function day(ms: number, zone: string): string {
  return fmt(zone, { weekday: 'long', day: 'numeric', month: 'long' }).format(ms);
}

/** The calendar day in a zone, as `2026-11-20`, to tell today from another day. */
export function dayKey(ms: number, zone: string): string {
  const p = fmt(zone, { year: 'numeric', month: '2-digit', day: '2-digit' }).formatToParts(ms);
  const part = (t: string) => p.find((x) => x.type === t)?.value ?? '';
  return `${part('year')}-${part('month')}-${part('day')}`;
}

const made = new Map<string, Intl.DateTimeFormat>();
function fmt(zone: string, o: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  const key = `${zone}|${JSON.stringify(o)}`;
  let f = made.get(key);
  if (!f) {
    try {
      f = new Intl.DateTimeFormat('en-GB', { ...o, timeZone: zone });
    } catch {
      f = new Intl.DateTimeFormat('en-GB', { ...o, timeZone: 'UTC' });
    }
    made.set(key, f);
  }
  return f;
}

/** The device's own zone, or null where the browser does not say. */
export function deviceZone(): string | null {
  try {
    return new Intl.DateTimeFormat().resolvedOptions().timeZone || null;
  } catch {
    return null;
  }
}

/**
 * The line under the station's time (ADR-0189): its zone, and the host's own time beside it
 * when their device keeps a different one at this moment. Two zones with the same time now
 * (Europe/London and Europe/Lisbon) read as the same.
 */
export function zoneLine(ms: number, station: string, own: string | null): string {
  if (!own || own === station) return own ? `${station} · the same as yours` : station;
  const same = dayKey(ms, station) === dayKey(ms, own) && hms(ms, station) === hms(ms, own);
  return same ? `${station} · the same as yours` : `${hm(ms, own)} your time · ${own}`;
}
