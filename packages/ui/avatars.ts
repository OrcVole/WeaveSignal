// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The chat's shipped avatars (ADR-0235): the list (avatars/manifest.json, which the chat service
// reads too, so it accepts exactly these ids) and each one's file as the page's build serves it.
// The pictures are Fluent Emoji's Flat drawings (MIT, Microsoft Corporation; avatars/README.md).
// Both the player page and the studio app's screener draw from here.

import manifest from './avatars/manifest.json';

export interface StockAvatar {
  /** What the chat keeps for a chatter who chose it, and the file's name. */
  id: string;
  /** What a screen reader says: "Fox". */
  label: string;
  group: 'animal' | 'monster';
}

export const STOCK: readonly StockAvatar[] = manifest.avatars.map((a) => ({ id: a.id, label: a.label, group: a.group as StockAvatar['group'] }));

const files = import.meta.glob('./avatars/*.webp', { eager: true, query: '?url', import: 'default' }) as Record<string, string>;

/** The address of a shipped avatar's picture, or null for an id this build does not know (a
 *  newer chat service than the page). */
export function stockUrl(id: string): string | null {
  return files[`./avatars/${id}.webp`] ?? null;
}

export function stockLabel(id: string | null | undefined): string | null {
  return STOCK.find((a) => a.id === id)?.label ?? null;
}

/** The letter the plain default shows: a name's first letter or digit, upper case; "?" for none. */
export function initial(name: string): string {
  const first = [...name.trim()].find((c) => /[\p{L}\p{N}]/u.test(c));
  return first ? first.toLocaleUpperCase() : '?';
}

/** The plain default's ground, one of eight by the name, so two chatters without an avatar are
 *  still told apart at a glance (never the only cue: the name is beside it). */
export function ground(name: string): number {
  let h = 0;
  for (const c of name.trim().toLocaleLowerCase()) h = (h * 31 + (c.codePointAt(0) ?? 0)) >>> 0;
  return h % 8;
}
