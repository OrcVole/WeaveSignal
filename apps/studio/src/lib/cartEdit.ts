// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Adding and changing carts from the wall (1.2.1), as pure rules and the station's routes: whose
// bank a tab is and who may change it, what a file must be before it is sent, the words for a
// refusal, and the requests themselves. The station stays the authority on every one of them;
// what is checked here is checked so a person hears it before a 64 MB file has travelled.

import { cartKey } from '../../../../packages/ui/keymap.ts';
import { bankAt, cartsAt, type Bank, type Cart, type CartMode, type Catalogue } from '../../../../packages/ui/carts.ts';
import { ApiError, base, headers, netFetch, type Me, type StepView } from './api.ts';
import { dur, signed } from './format.ts';

/** What a station answers when its catalogue does not say (one older than 1.2.1): the defaults. */
export const DEFAULT_MAX_MB = 64;
export const DEFAULT_MAX_SECONDS = 600;

/** The file chooser's filter: what the media worker reads, by type and by ending. */
export const ACCEPT = 'audio/*,.wav,.wave,.flac,.mp3,.ogg,.oga,.opus,.m4a,.mp4,.aac';
/** The endings a dropped file may have (a drop has no filter of its own). */
export const ENDINGS = ['wav', 'wave', 'flac', 'mp3', 'ogg', 'oga', 'opus', 'm4a', 'mp4', 'aac'] as const;
/** The formats in words, as the dialog prints them. */
export const FORMATS = 'WAV, FLAC, MP3, Ogg (Vorbis or Opus) or M4A';

export interface Limits {
  maxMb: number;
  maxSeconds: number;
  loudnessLufs: number | null;
}

/** The limits a station holds an upload to, from its catalogue. */
export function limitsOf(cat: Catalogue | null): Limits {
  return {
    maxMb: cat?.max_upload_mb ?? DEFAULT_MAX_MB,
    maxSeconds: cat?.max_seconds ?? DEFAULT_MAX_SECONDS,
    loudnessLufs: cat?.loudness_lufs ?? null,
  };
}

/** `10 minutes`, `90 seconds`, `1 minute`: a length limit in words. */
export function lengthWords(seconds: number): string {
  if (seconds >= 120 && seconds % 60 === 0) return `${seconds / 60} minutes`;
  if (seconds === 60) return '1 minute';
  return `${Math.round(seconds)} seconds`;
}

/** `WAV, FLAC, … · up to 64 MB and 10 minutes`: what a slot takes, under the drop zone. */
export function takesLine(l: Limits): string {
  return `${FORMATS} · up to ${l.maxMb} MB and ${lengthWords(l.maxSeconds)}`;
}

/** `3.2 MB`, `640 KB`. */
export function sizeWords(bytes: number): string {
  return bytes >= 1024 * 1024 ? `${(bytes / (1024 * 1024)).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

const ending = (name: string): string => /\.([a-z0-9]+)$/i.exec(name)?.[1]?.toLowerCase() ?? '';

/** Why a file cannot be sent, or null: empty, too large for this station, or not a sound by its
 *  name and type. The station reads the file itself and has the last word on what it is. */
export function fileProblem(file: { name: string; size: number; type?: string }, l: Limits): string | null {
  if (file.size === 0) return `“${file.name}” is empty.`;
  if (file.size > l.maxMb * 1024 * 1024) return `“${file.name}” is ${sizeWords(file.size)}. This station takes sounds up to ${l.maxMb} MB.`;
  const known = (ENDINGS as readonly string[]).includes(ending(file.name));
  if (!known && !(file.type ?? '').startsWith('audio/')) return `“${file.name}” does not look like a sound. A cart is ${FORMATS}.`;
  return null;
}

/** A cart's name from its file's: the ending off, underscores and hyphens as spaces, at most the
 *  120 characters a label holds. `01_Top-of-hour.wav` is `01 Top of hour`. */
export function labelFromFile(name: string): string {
  const stem = name.replace(/^.*[\\/]/, '').replace(/\.[a-z0-9]{1,5}$/i, '');
  const words = stem
    .replace(/[_-]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
  return [...(words || 'New cart')].slice(0, 120).join('');
}

/** Why a name cannot be kept, or null: the station's own rule (1 to 120 characters). */
export function labelProblem(label: string): string | null {
  const n = [...label.trim()].length;
  if (n === 0) return 'Give the cart a name: it is what the pad shows.';
  if (n > 120) return 'A name is at most 120 characters.';
  return null;
}

/** A ramp typed in seconds, as the station keeps it: `null` for none, `'bad'` for what is not a
 *  length, and never longer than the cart when its length is known. */
export function rampMs(text: string, durationMs: number | null): number | null | 'bad' | 'long' {
  const t = text.trim().replace(',', '.');
  if (t === '') return null;
  if (!/^\d+(\.\d+)?$/.test(t)) return 'bad';
  const ms = Math.round(Number(t) * 1000);
  if (ms === 0) return null;
  if (durationMs && ms > durationMs) return 'long';
  return ms;
}

/** A ramp as the field shows it: seconds, with a decimal only when it has one. */
export function rampText(ms: number | null | undefined): string {
  if (!ms) return '';
  const s = ms / 1000;
  return Number.isInteger(s) ? String(s) : s.toFixed(1);
}

/** What each mode does, for the dialog's choice. */
export const MODE_CHOICES: readonly { mode: CartMode; name: string; what: string }[] = [
  { mode: 'one_shot', name: 'One-shot', what: 'Plays once: a jingle, a sting, applause.' },
  { mode: 'bed', name: 'Bed', what: 'Loops until you stop it, and dips while a microphone is open.' },
  { mode: 'sweeper', name: 'Sweeper', what: 'Plays once, and any bed dips while it sounds.' },
];

// ---- whose bank, and who may change it ----

/** Who owns the bank a change goes to: the station (`null`) or a show, by its slug. */
export type OwnerSlug = string | null;

/** Whose tab `n` is in this window: its bank's owner, or for a bank not made, the station. */
export function ownerOfTab(cat: Catalogue | null, n: number): OwnerSlug {
  return bankAt(cat, n)?.show ?? null;
}

/** Whether this window may change `owner`'s banks, as far as the window can tell. The station's
 *  banks: any member of staff (the station's own rule). A show's own: an admin, a producer, the
 *  station key, or the show's presenter. A preparing window works with its own show, so the
 *  show is this account's; a live window's show is the step's, which says whether this account
 *  may change it. The station refuses whatever this gets wrong. */
export function mayChange(owner: OwnerSlug, me: Me | null | undefined, step: StepView | null | undefined, preparing: boolean): boolean {
  if (owner === null) return true;
  // A station too old to say who the window is behaves as the station key did.
  if (!me || me.kind === 'station' || me.role === 'admin' || me.role === 'producer') return true;
  if (preparing) return me.role === 'presenter';
  return step?.show?.slug === owner && step.may_change;
}

/** Why this window cannot change a show's own bank, for a tooltip. */
export const NOT_YOURS = (show: string): string => `Only an admin, a producer or ${show}’s presenter changes its own bank.`;

/** `the station’s bank 2, Idents` or `Steeped’s own bank 1, Dog woofs`: a bank named in full, so
 *  a dialog always says whose bank it writes to. */
export function bankWords(bank: Bank | null, n: number, showName: string | null): string {
  const whose = bank?.show ? `${showName ?? 'The show'}’s own` : 'the station’s';
  return `${whose} bank ${n}${bank?.name ? `, ${bank.name}` : ''}`;
}

/** The slots of tab `bank` that hold no cart, for the key a cart can move to. */
export function emptySlots(cat: Catalogue | null, bank: number): number[] {
  const taken = new Set(cartsAt(cat, bank).map((c) => c.slot));
  return Array.from({ length: 24 }, (_, i) => i + 1).filter((s) => !taken.has(s));
}

// ---- the words ----

/** What the worker measured and what the desk will do about it: `0:04 · measured −9.3 LUFS, plays
 *  at −23.0 LUFS (gain −13.7 dB)`. A cart with nothing measured says its length alone. */
export function soundLine(cart: Pick<Cart, 'duration_ms' | 'loudness_lufs' | 'gain_db'>, targetLufs: number | null): string {
  const length = dur(cart.duration_ms + 999);
  if (cart.loudness_lufs === null || cart.loudness_lufs === undefined) return length;
  const plays = targetLufs === null ? '' : `, plays at ${signed(targetLufs)} LUFS`;
  return `${length} · measured ${signed(cart.loudness_lufs)} LUFS${plays} (gain ${signed(cart.gain_db)} dB)`;
}

/** The toast after a cart is added or its sound replaced. */
export function addedWords(cart: Cart, replaced: boolean, targetLufs: number | null): { title: string; message: string } {
  return {
    title: replaced ? 'Sound replaced' : 'Cart added',
    message: `“${cart.label}” is on ${cartKey(cart.slot)}: ${soundLine(cart, targetLufs)}.`,
  };
}

/** What to tell a person when a cart's change is refused. The station's sentence, except where
 *  it has none: a file over the limit comes back as a bare 413 (and behind a proxy the proxy's
 *  own), and a station older than 1.2.1 has no route to change a cart without its sound. */
export function cartProblem(err: unknown, l: Limits, what: 'send' | 'change' | 'remove' | 'bank' = 'send'): string {
  if (err instanceof ApiError) {
    if (err.status === 413) return `The file is too large: this station takes sounds up to ${l.maxMb} MB.`;
    if (what === 'change' && (err.status === 405 || (err.status === 404 && !/slot|bank|cart/i.test(err.message)))) {
      return 'This station is older than 1.2.1: it cannot change a cart’s name or key without its sound. Replace the sound to change it, or update the station.';
    }
    return err.message;
  }
  if (err instanceof Error && err.message) return what === 'send' ? `The file did not reach the station: ${err.message}` : err.message;
  return 'The station did not answer. Try again.';
}

// ---- the station's routes ----

const enc = encodeURIComponent;

/** A bank's route: the station's, or a show's own. */
export function bankRoute(owner: OwnerSlug, n: number): string {
  return owner === null ? `/api/v1/carts/banks/${n}` : `/api/v1/shows/${enc(owner)}/carts/banks/${n}`;
}

/** A slot's route in the station's bank, or a show's own. */
export function slotRoute(owner: OwnerSlug, bank: number, slot: number): string {
  return `${bankRoute(owner, bank)}/slots/${slot}`;
}

/** The cart's words, as an upload carries them in its query. */
export interface CartWords {
  label: string;
  detail: string;
  mode: CartMode;
  ramp_ms: number | null;
  licence: string;
  /** Where it came from: the file's name. */
  origin: string;
}

/** The upload's query: every word the station takes, the empty ones left out. */
export function uploadQuery(w: CartWords): string {
  const q = new URLSearchParams({ label: w.label.trim(), mode: w.mode });
  if (w.detail.trim()) q.set('detail', w.detail.trim());
  if (w.ramp_ms) q.set('ramp_ms', String(w.ramp_ms));
  if (w.licence.trim()) q.set('licence', w.licence.trim());
  if (w.origin.trim()) q.set('origin', w.origin.trim());
  return q.toString();
}

/** What a `PATCH` may change. */
export interface CartChange {
  label?: string;
  detail?: string;
  mode?: CartMode;
  ramp_ms?: number | null;
  licence?: string;
  slot?: number;
}

/** Only what differs from the cart as it is: a change that names nothing is not sent. */
export function changeOf(cart: Cart, w: Omit<CartWords, 'origin'>, slot: number): CartChange {
  const c: CartChange = {};
  if (w.label.trim() !== cart.label) c.label = w.label.trim();
  if (w.detail.trim() !== cart.detail) c.detail = w.detail.trim();
  if (w.mode !== cart.mode) c.mode = w.mode;
  if ((w.ramp_ms ?? null) !== (cart.ramp_ms ?? null)) c.ramp_ms = w.ramp_ms ?? null;
  if (w.licence.trim() !== cart.licence) c.licence = w.licence.trim();
  if (slot !== cart.slot) c.slot = slot;
  return c;
}

async function answer<T>(res: Response): Promise<T> {
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, typeof data.error === 'string' && data.error ? data.error : `${res.status} ${res.statusText}`.trim());
  return data as T;
}

/** Send a sound to a slot: the station's media worker reads it, measures it and makes it the
 *  slot's cart, in place of whatever was there. */
export async function putCart(owner: OwnerSlug, bank: number, slot: number, words: CartWords, file: Blob, signal?: AbortSignal): Promise<Cart> {
  const h = headers(false) as Record<string, string>;
  h['Content-Type'] = 'application/octet-stream';
  const sent = netFetch(`${base()}${slotRoute(owner, bank, slot)}?${uploadQuery(words)}`, { method: 'PUT', headers: h, body: file, signal });
  // The packaged app's request goes out from Rust, which hears a stop only once the answer has
  // begun: the wait itself ends here, at once, in either build.
  const res = await (signal ? Promise.race([sent, stopped(signal)]) : sent);
  return answer<Cart>(res);
}

/** Rejects as a stopped request does, when `signal` says stop. */
function stopped(signal: AbortSignal): Promise<never> {
  return new Promise((_, reject) => {
    const no = () => reject(new DOMException('The request was stopped.', 'AbortError'));
    if (signal.aborted) no();
    else signal.addEventListener('abort', no, { once: true });
  });
}

/** Change the slot's cart and keep its sound. */
export async function patchCart(owner: OwnerSlug, bank: number, slot: number, change: CartChange): Promise<Cart> {
  const res = await netFetch(`${base()}${slotRoute(owner, bank, slot)}`, { method: 'PATCH', headers: headers(true), body: JSON.stringify(change) });
  return answer<Cart>(res);
}

/** Empty a slot: the cart and its sound go. */
export async function removeCart(owner: OwnerSlug, bank: number, slot: number): Promise<Catalogue> {
  const res = await netFetch(`${base()}${slotRoute(owner, bank, slot)}`, { method: 'DELETE', headers: headers(false) });
  return answer<Catalogue>(res);
}

/** Make a bank, or change its name and colour. */
export async function putBank(owner: OwnerSlug, n: number, bank: { name: string; colour_slot: number }): Promise<Catalogue> {
  const res = await netFetch(`${base()}${bankRoute(owner, n)}`, { method: 'PUT', headers: headers(true), body: JSON.stringify({ name: bank.name.trim(), colour_slot: bank.colour_slot }) });
  return answer<Catalogue>(res);
}

/** Remove a show's own bank with its carts. The station's banks are not removed. */
export async function removeShowBank(show: string, n: number): Promise<Catalogue> {
  const res = await netFetch(`${base()}${bankRoute(show, n)}`, { method: 'DELETE', headers: headers(false) });
  return answer<Catalogue>(res);
}

/** Why a bank's name cannot be kept, or null: the station's own rule (1 to 80 characters). */
export function bankNameProblem(name: string): string | null {
  const n = [...name.trim()].length;
  if (n === 0) return 'Give the bank a name: its tab shows it.';
  if (n > 80) return 'A bank’s name is at most 80 characters.';
  return null;
}

// ---- the default pack, loaded again (1.2.1) ----

/** One of the default pack's banks: its number in the pack, its name, and its sounds. */
export interface PackBank {
  number: number;
  name: string;
  carts: { slot: number; label: string; mode: CartMode }[];
}

/** `GET /api/v1/carts/pack`: the station's default pack, and which of the station's carts are
 *  still a default pack's cart as it was loaded. */
export interface PackView {
  available: boolean;
  problem?: string | null;
  /** A pack is loading now. */
  loading: boolean;
  banks: PackBank[];
  /** The ids of the carts a load may replace or remove; every other cart stays. */
  untouched: string[];
}

/** What loading one of the pack's banks into a station bank does. */
export interface PackPlan {
  /** The pack's bank and the station's it goes into. */
  bank: number;
  into: number;
  /** The station's bank has been made. */
  made: boolean;
  /** The pack's sounds that are loaded. */
  load: number;
  /** Default carts nobody changed that a pack sound takes the place of, and those that go
   *  because the pack has no sound for their key. */
  replace: number;
  remove: number;
  /** The slots of the carts that are a person's own and stay. */
  keep: number[];
  /** The pack's sounds left out because a cart that stays has their key. */
  leftOut: number;
}

/** The station's rule, worked out in the page so the dialog can say what will happen before
 *  anything is asked: only a cart in `untouched` is replaced or removed. */
export function packPlan(view: PackView, cat: Catalogue | null, bank: number, into: number): PackPlan | null {
  const pack = view.banks.find((b) => b.number === bank);
  if (!pack) return null;
  const there = (cat?.carts ?? []).filter((c) => (c.show ?? null) === null && c.bank === into);
  const defaults = there.filter((c) => view.untouched.includes(c.id));
  const keep = there.filter((c) => !view.untouched.includes(c.id)).map((c) => c.slot);
  const loaded = pack.carts.filter((c) => !keep.includes(c.slot)).map((c) => c.slot);
  return {
    bank,
    into,
    made: !!cat?.banks.some((b) => (b.show ?? null) === null && b.number === into),
    load: loaded.length,
    replace: defaults.filter((c) => loaded.includes(c.slot)).length,
    remove: defaults.filter((c) => !loaded.includes(c.slot)).length,
    keep: keep.sort((a, b) => a - b),
    leftOut: pack.carts.length - loaded.length,
  };
}

/** Whether a plan touches a bank that holds carts, so the load says `replace`. */
export const planAsks = (p: PackPlan): boolean => p.replace + p.remove + p.keep.length > 0;

const count = (n: number, one: string, many: string): string => `${n} ${n === 1 ? one : many}`;

/** What a load will do to one station bank, in sentences, for the dialog. `name` is the station
 *  bank's own name, when it has been made. */
export function planWords(p: PackPlan, name: string | null): string[] {
  const bank = `The station’s bank ${p.into}${name ? `, ${name},` : ''}`;
  if (!planAsks(p)) {
    return [`${bank} ${p.made ? 'is empty' : 'has not been made: it is made under the pack’s name'}. ${count(p.load, 'sound is', 'sounds are')} loaded into it.`];
  }
  const out = [`${bank} holds ${count(p.replace + p.remove + p.keep.length, 'cart', 'carts')}.`];
  const gone = p.replace + p.remove;
  if (gone) {
    const removed = p.remove ? ` (${count(p.remove, 'goes', 'go')} with nothing in its place: the pack has no sound for ${p.remove === 1 ? 'that key' : 'those keys'})` : '';
    out.push(`${count(gone, 'is a default sound nobody changed and is', 'are default sounds nobody changed and are')} replaced${removed}.`);
  } else out.push('None is a default sound nobody changed, so nothing is replaced.');
  if (p.keep.length) {
    const keys = p.keep.map((s) => cartKey(s)).join(', ');
    out.push(`${count(p.keep.length, 'is yours and stays', 'are yours and stay')}, on ${keys}${p.leftOut ? `; ${p.keep.length === 1 ? 'the pack’s sound for that key is' : `the pack’s ${count(p.leftOut, 'sound for those keys is', 'sounds for those keys are')}`} left out` : ''}.`);
  }
  out.push(`${count(p.load, 'sound is', 'sounds are')} loaded.`);
  return out;
}

/** The dialog's button: what pressing it does. */
export function loadWords(plans: PackPlan[]): string {
  const load = plans.reduce((n, p) => n + p.load, 0);
  const gone = plans.reduce((n, p) => n + p.replace + p.remove, 0);
  return gone ? `Replace ${gone} and load ${load}` : `Load ${count(load, 'sound', 'sounds')}`;
}

/** The station's default pack. */
export async function packView(): Promise<PackView> {
  const res = await netFetch(`${base()}/api/v1/carts/pack`, { headers: headers(false) });
  return answer<PackView>(res);
}

/** Start loading banks of the pack into station banks. `replace` goes on where a bank holds
 *  carts; the station replaces only default carts nobody changed. */
export async function loadPack(loads: { bank: number; into: number }[], replace: boolean): Promise<{ loading: number }> {
  const res = await netFetch(`${base()}/api/v1/carts/pack`, { method: 'POST', headers: headers(true), body: JSON.stringify({ loads, replace }) });
  return answer<{ loading: number }>(res);
}
