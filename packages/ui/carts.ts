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
}

export interface Bank {
  number: number;
  name: string;
  /** 1 to 12: the bank's (and its show's) slot in the cart palette (ADR-0066). */
  colour_slot: number;
}

export interface Sounding {
  cart: string;
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

/** The slot's cart in a bank, or null. */
export function cartAt(cat: Catalogue | null, bank: number, slot: number): Cart | null {
  return cat?.carts.find((c) => c.bank === bank && c.slot === slot) ?? null;
}

/** Move a focus on the six-by-four grid, clamped at the edges. */
export function moveSlot(slot: number, dx: number, dy: number): number {
  const col = Math.min(5, Math.max(0, ((slot - 1) % 6) + dx));
  const row = Math.min(3, Math.max(0, Math.floor((slot - 1) / 6) + dy));
  return row * 6 + col + 1;
}
