// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The cart wall's states as code (docs/reference/design/cart-wall.md section 3): what a cart
// in a slot is now, from the catalogue and what the control plane says is sounding. Pure, so
// the rules are tested without a page.

export type CartMode = 'one_shot' | 'bed' | 'sweeper';

export interface Cart {
  id: string;
  bank: number;
  slot: number;
  label: string;
  detail: string;
  mode: CartMode;
  file: string | null;
  duration_ms: number;
  gain_db: number;
  loudness_lufs: number | null;
  true_peak_dbtp: number | null;
  ramp_ms: number | null;
  origin: string;
  licence: string;
  missing: boolean;
  /** The show whose own bank holds it, by its slug; null (or absent, from a station older
   *  than 1.0.2) for the station's. */
  show?: string | null;
}

export interface Bank {
  number: number;
  name: string;
  /** 1 to 12: the bank's (and its show's) slot in the cart palette (ADR-0066). */
  colour_slot: number;
  /** The show whose own bank this is, by its slug; null (or absent) for the station's, which
   *  every show shares. */
  show?: string | null;
}

export interface Sounding {
  cart: string;
  /** The cart's label, so a window whose banks do not hold it can name it (1.0.2). */
  label?: string;
  pool: 'shot' | 'bed';
  player: number;
  mode: CartMode;
  /** Cued into the presenter's headphones (⇧ and its key), not on air. */
  cue: boolean;
  /** The desk's number for this play. */
  play: number;
  started_ms: number;
  ends_ms: number | null;
  stopping: boolean;
  seq: number;
}

/** The studio view's `carts`. */
export interface CartsView {
  version: number;
  /** The desk, which plays the carts (ADR-0117), answers; else why not. */
  desk: boolean;
  desk_error: string | null;
  sounding: Sounding[];
  ducked: boolean;
  mic_open: boolean;
  missing: string[];
  importing: [number, number] | null;
}

export interface Catalogue {
  version: number;
  /** The show whose own banks are here beside the station's; null: the station's alone. */
  show?: string | null;
  banks: Bank[];
  carts: Cart[];
  loudness_lufs: number;
}

/** Under this much left, a playing cart says ENDING in amber (design: under 5 s). */
export const ENDING_MS = 5_000;

export type CartState = 'empty' | 'missing' | 'idle' | 'cued' | 'playing' | 'ending' | 'stopping';

export interface CartNow {
  state: CartState;
  /** Time left of this pass, for a cart that plays once; null for a bed or a cart not sounding. */
  leftMs: number | null;
  /** The ring: 1 full, 0 empty. A bed's ring stays full and shows the loop. */
  progress: number;
  /** The sounding record, if it sounds. */
  sounding: Sounding | null;
}

/** What a slot shows now. `missing` comes from the live view as well as the catalogue, so a
 *  file that goes is FILE MISSING at once, without fetching the catalogue again. */
export function cartNow(cart: Cart | null, view: CartsView | null, now: number): CartNow {
  if (!cart) return { state: 'empty', leftMs: null, progress: 0, sounding: null };
  const missing = cart.missing || !cart.file || (view?.missing.includes(cart.id) ?? false);
  const s = view?.sounding.filter((x) => x.cart === cart.id).at(-1) ?? null;
  if (s) {
    // Cued: in the presenter's headphones only; the design's violet CUE, never ENDING.
    const sounding: CartState = s.cue ? 'cued' : 'playing';
    if (s.ends_ms === null) return { state: s.stopping ? 'stopping' : sounding, leftMs: null, progress: 1, sounding: s };
    const left = Math.max(0, s.ends_ms - now);
    const total = Math.max(1, s.ends_ms - s.started_ms);
    const state: CartState = s.stopping ? 'stopping' : s.cue ? 'cued' : left < ENDING_MS ? 'ending' : 'playing';
    return { state, leftMs: left, progress: left / total, sounding: s };
  }
  if (missing) return { state: 'missing', leftMs: null, progress: 0, sounding: null };
  return { state: 'idle', leftMs: null, progress: 0, sounding: null };
}

/** The mode's word and glyph (report 08's three modes). */
export const MODE: Record<CartMode, { word: string; glyph: string }> = {
  one_shot: { word: 'one-shot', glyph: 'g-once' },
  bed: { word: 'bed', glyph: 'g-loop' },
  sweeper: { word: 'sweeper', glyph: 'g-sweep' },
};

// ---- a cart wall per show (1.0.2) ----
//
// A window works with the station's banks and its show's own, each numbered 1 to 5 apart. The
// wall has five tabs, ⌃F1 to ⌃F5 (cart-wall.md: "banks are per show, with a station bank"):
// tab N is the show's bank N where the show has made one, and the station's bank N otherwise.
// F1 to F6 fire the top row of the tab this window shows, as before.

const ownerOf = (x: { show?: string | null }): string | null => x.show ?? null;

/** Tab `n`'s bank in this window: the show's own bank `n`, else the station's, else null. */
export function bankAt(cat: Catalogue | null, n: number): Bank | null {
  const banks = cat?.banks.filter((b) => b.number === n) ?? [];
  return banks.find((b) => ownerOf(b) !== null) ?? banks.find((b) => ownerOf(b) === null) ?? null;
}

/** Whose a bank is, in the strip's word: the show's own, or the station's (shared by every
 *  show). */
export function bankOwner(b: Bank): 'show' | 'station' {
  return ownerOf(b) === null ? 'station' : 'show';
}

/** The station's bank `n` when the show's own bank `n` takes its tab in this window. */
export function stationBankUnder(cat: Catalogue | null, n: number): Bank | null {
  const tab = bankAt(cat, n);
  if (!tab || ownerOf(tab) === null) return null;
  return cat?.banks.find((b) => b.number === n && ownerOf(b) === null) ?? null;
}

/** The carts of tab `n`'s bank. */
export function cartsAt(cat: Catalogue | null, n: number): Cart[] {
  const bank = bankAt(cat, n);
  if (!bank) return [];
  return cat?.carts.filter((c) => c.bank === n && ownerOf(c) === ownerOf(bank)) ?? [];
}

/** The slot's cart in tab `bank`, or null. */
export function cartAt(cat: Catalogue | null, bank: number, slot: number): Cart | null {
  return cartsAt(cat, bank).find((c) => c.slot === slot) ?? null;
}

/** The tab a cart sits on in this window, or null when its bank is not one of the five (another
 *  show's, or a station bank the show's own takes the place of). */
export function tabOf(cat: Catalogue | null, cart: Cart): number | null {
  const bank = bankAt(cat, cart.bank);
  return bank && ownerOf(bank) === ownerOf(cart) ? cart.bank : null;
}

/** Move a focus on the six-by-four grid, clamped at the edges. */
export function moveSlot(slot: number, dx: number, dy: number): number {
  const col = Math.min(5, Math.max(0, ((slot - 1) % 6) + dx));
  const row = Math.min(3, Math.max(0, Math.floor((slot - 1) / 6) + dy));
  return row * 6 + col + 1;
}
