// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// "New sounds are available" (1.3.1; item 17 of the 1.3.1 questions). A station loads the sounds
// WeaveSignal ships once, at its first start; a later release's sounds arrive only when someone
// presses Default pack… on the Carts tab, and nobody is told. This is the rule for telling an
// admin or a producer, once, in words, with the way to do it.
//
// The studio app cannot know which pack a station last loaded: the station keeps no record of
// it, and the pack view carries no version. So the notice counts what it can see: the pack's
// sounds that would load now and that the station does not already have, and remembers a dismissal
// against a fingerprint of the pack, so a different pack asks again. The station's own field for
// this (a pack version, and the version it last loaded) is the proposal in the 1.3.1 report;
// until then a dismissal is kept on this computer, not on the station.

import type { Catalogue } from '../../../../packages/ui/carts.ts';
import type { Me } from './api.ts';
import type { PackView } from './cartEdit.ts';

/** An admin, a producer or the station key loads the default pack; the notice is theirs. */
export const mayLoadPack = (me: Me | null | undefined): boolean => !!me && (me.kind === 'station' || me.role === 'admin' || me.role === 'producer');

/** The key a dismissal is kept under (one per station: `lib/scoped.ts` adds the station). */
export const DISMISSED_KEY = 'weavesignal.packNotice';

const norm = (s: string): string => s.trim().toLowerCase();

/** A short fingerprint of the pack: its banks, and each sound's key, label and mode. A pack of
 *  another release has another one, so a dismissed notice asks again for it and not before. */
export function packFingerprint(view: PackView): string {
  let h = 5381;
  const text = view.banks.map((b) => `${b.number}:${b.name}:${b.carts.map((c) => `${c.slot}/${c.label}/${c.mode}`).join(',')}`).join('|');
  for (let i = 0; i < text.length; i++) h = ((h * 33) ^ text.charCodeAt(i)) >>> 0;
  return `${view.banks.length}.${view.banks.reduce((n, b) => n + b.carts.length, 0)}.${h.toString(36)}`;
}

/** How many of the pack's sounds a load would add now: a sound counts unless a default cart of
 *  the same name is already on the station's carts, or its key holds a cart that is a person's
 *  own (a load leaves that one alone and leaves the pack's sound out). A key that holds a
 *  default nobody changed counts: the load puts the pack's sound in its place. Only the station's
 *  own banks count, not a show's. */
export function packNewSounds(view: PackView, cat: Catalogue | null): number {
  if (!view.available || !cat) return 0;
  const mine = cat.carts.filter((c) => (c.show ?? null) === null);
  const have = new Set(mine.filter((c) => c.origin.startsWith('demo:')).map((c) => norm(c.label)));
  const theirs = new Set(mine.filter((c) => !view.untouched.includes(c.id)).map((c) => `${c.bank}/${c.slot}`));
  let n = 0;
  for (const b of view.banks) {
    if (b.number < 1 || b.number > 5) continue;
    for (const c of b.carts) if (!have.has(norm(c.label)) && !theirs.has(`${b.number}/${c.slot}`)) n++;
  }
  return n;
}

export const PACK_NOTICE = {
  /** The notice cannot know the sounds are new: a station may have deleted some it did not want. It says what it counted. */
  lead: (n: number): string => `The default pack has ${n.toLocaleString('en-GB')} ${n === 1 ? 'sound' : 'sounds'} this station does not have.`,
  rest: 'Default pack… adds them and keeps every cart of your own.',
  action: 'Default pack…',
  dismiss: 'Not now',
  dismissWhy: 'Hides this until WeaveSignal ships a different pack. It is remembered on this computer.',
};
