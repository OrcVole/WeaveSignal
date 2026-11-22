// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The sandbox's cart wall changed from the page (1.2.1): the same routes a real station answers
// (a bank made or renamed, a slot filled, a cart changed or moved, a slot emptied, a show's own
// bank removed), worked on the catalogue the page holds. Nothing is sent anywhere: a sound that
// is added is decoded in the page and is gone when the page is reloaded. Pure but for the
// catalogue it is given, so the rules are tested without a page.

import type { Bank, Cart, CartMode, Catalogue } from '../../../../packages/ui/carts.ts';

/** What an added sound is, once the page has decoded it. */
export interface Decoded {
  /** The name the sandbox's sound keeps it under. */
  file: string;
  durationMs: number;
}

export interface Answer {
  status: number;
  body: unknown;
}

const SLOT = /^\/api\/v1\/(?:shows\/([^/]+)\/)?carts\/banks\/(\d+)\/slots\/(\d+)$/;
const BANK = /^\/api\/v1\/(?:shows\/([^/]+)\/)?carts\/banks\/(\d+)$/;
const MODES: readonly CartMode[] = ['one_shot', 'bed', 'sweeper'];

/** Whether a request is one of the cart wall's changes (and so this module's to answer). */
export function isCartChange(method: string, path: string): boolean {
  return (method === 'PUT' || method === 'PATCH' || method === 'DELETE') && (SLOT.test(path) || BANK.test(path));
}

/** Whether it is a sound sent to a slot: the page decodes it before [`cartChange`] is asked. */
export function isUpload(method: string, path: string): boolean {
  return method === 'PUT' && SLOT.test(path);
}

const no = (status: number, error: string): Answer => ({ status, body: { error } });
const owned = (x: { show?: string | null }, owner: string | null) => (x.show ?? null) === owner;
const text = (v: unknown, max: number): string | null => (typeof v === 'string' && [...v.trim()].length <= max && !/[\u0000-\u001f]/.test(v) ? v.trim() : null);

/** Answer one of the cart wall's changes on `cat`, as the station would. `decoded` is the sound
 *  for an upload; `show` is the show whose own banks the answer lists beside the station's. */
export function cartChange(cat: Catalogue, method: string, path: string, query: URLSearchParams, body: Record<string, unknown>, decoded: Decoded | null, list: (owner: string | null) => Catalogue): Answer {
  const b = BANK.exec(path);
  if (b) {
    const owner = b[1] ? decodeURIComponent(b[1]) : null;
    const number = Number(b[2]);
    const at = cat.banks.findIndex((x) => x.number === number && owned(x, owner));
    if (method === 'DELETE') {
      if (owner === null) return no(422, 'the station’s banks are not removed; empty their slots instead');
      if (at < 0) return no(404, `bank ${number} has not been made`);
      cat.banks.splice(at, 1);
      cat.carts = cat.carts.filter((c) => !(c.bank === number && owned(c, owner)));
      cat.version++;
      return { status: 200, body: list(owner) };
    }
    const name = text(body.name, 80);
    const colour = Number(body.colour_slot);
    if (number < 1 || number > 5) return no(422, `bank ${number} does not exist; banks are 1 to 5`);
    if (!name) return no(422, 'a bank name is 1 to 80 characters, without control characters');
    if (!Number.isInteger(colour) || colour < 1 || colour > 12) return no(422, `colour slot ${body.colour_slot} does not exist; the palette has 12`);
    const bank: Bank = { number, name, colour_slot: colour, show: owner };
    if (at < 0) cat.banks.push(bank);
    else cat.banks[at] = bank;
    cat.version++;
    return { status: 200, body: list(owner) };
  }
  const s = SLOT.exec(path);
  if (!s) return no(404, 'There is no such thing.');
  const owner = s[1] ? decodeURIComponent(s[1]) : null;
  const bank = Number(s[2]);
  const slot = Number(s[3]);
  const here = (c: Cart, bk = bank, sl = slot) => c.bank === bk && c.slot === sl && owned(c, owner);
  const at = cat.carts.findIndex((c) => here(c));
  if (method === 'DELETE') {
    if (at < 0) return no(422, `bank ${bank} slot ${slot} is empty`);
    cat.carts.splice(at, 1);
    cat.version++;
    return { status: 200, body: list(owner) };
  }
  if (method === 'PATCH') {
    if (at < 0) return no(404, `bank ${bank} slot ${slot} is empty: there is no cart to change`);
    const old = cat.carts[at];
    const next: Cart = { ...old };
    if (body.label !== undefined) {
      const label = text(body.label, 120);
      if (!label) return no(422, 'a label is 1 to 120 characters, without control characters');
      next.label = label;
    }
    if (body.detail !== undefined) next.detail = text(body.detail, 120) ?? old.detail;
    if (body.mode !== undefined) {
      if (!MODES.includes(body.mode as CartMode)) return no(422, 'mode is `one_shot`, `bed` or `sweeper`');
      next.mode = body.mode as CartMode;
    }
    if (body.ramp_ms !== undefined) next.ramp_ms = body.ramp_ms === null ? null : Number(body.ramp_ms);
    if (next.ramp_ms && next.duration_ms && next.ramp_ms > next.duration_ms) return no(422, `the ramp is ${next.ramp_ms} ms and the cart ${next.duration_ms} ms long; a ramp ends before its cart does`);
    if (body.licence !== undefined) next.licence = [...String(body.licence).trim()].slice(0, 80).join('');
    if (body.bank !== undefined) next.bank = Number(body.bank);
    if (body.slot !== undefined) next.slot = Number(body.slot);
    if (next.slot < 1 || next.slot > 24) return no(422, `slot ${next.slot} does not exist; slots are 1 to 24`);
    if (next.bank !== bank || next.slot !== slot) {
      if (!cat.banks.some((x) => x.number === next.bank && owned(x, owner))) return no(404, `bank ${next.bank} has not been made`);
      const there = cat.carts.find((c) => here(c, next.bank, next.slot));
      if (there) return no(409, `bank ${next.bank} slot ${next.slot} already holds “${there.label}”; move or remove that cart first`);
    }
    cat.carts[at] = next;
    cat.version++;
    return { status: 200, body: next };
  }
  // PUT: a sound into the slot, in place of whatever was there.
  if (slot < 1 || slot > 24) return no(422, `slot ${slot} does not exist; slots are 1 to 24`);
  if (!cat.banks.some((x) => x.number === bank && owned(x, owner))) return no(422, `bank ${bank} has not been made; make it first`);
  const label = text(query.get('label'), 120);
  const mode = query.get('mode') as CartMode;
  if (!label) return no(422, 'a label is 1 to 120 characters, without control characters');
  if (!MODES.includes(mode)) return no(422, 'mode is `one_shot`, `bed` or `sweeper`');
  if (!decoded) return no(422, 'the file was refused: not an audio file this browser reads. The sandbox decodes a sound in the page; a real station reads WAV, FLAC, MP3, Ogg and M4A itself.');
  const max = cat.max_seconds ?? 600;
  if (decoded.durationMs > max * 1000) return no(422, `the file was refused: the file is longer than ${max} s`);
  const ramp = Number(query.get('ramp_ms') ?? 0) || null;
  if (ramp && ramp > decoded.durationMs) return no(422, `the ramp is ${ramp} ms and the cart ${decoded.durationMs} ms long; a ramp ends before its cart does`);
  const cart: Cart = {
    id: `yours-${cat.version + 1}-${bank}-${slot}`,
    bank,
    slot,
    label,
    detail: text(query.get('detail') ?? '', 120) ?? '',
    mode,
    file: decoded.file,
    duration_ms: decoded.durationMs,
    // The sandbox measures nothing: the sound plays as it came.
    gain_db: 0,
    loudness_lufs: null,
    true_peak_dbtp: null,
    ramp_ms: ramp,
    origin: [...(query.get('origin') ?? '')].slice(0, 300).join(''),
    licence: [...(query.get('licence') ?? '')].slice(0, 80).join(''),
    missing: false,
    show: owner,
  };
  if (at < 0) cat.carts.push(cart);
  else cat.carts[at] = cart;
  cat.version++;
  return { status: 201, body: cart };
}
