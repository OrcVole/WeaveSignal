// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A show's rule changed, and every window is told (new-show.md section 3): the control plane
// keeps a notice of kind `rule.changed` whose title is the whole sentence, *Moira changed when
// The Folk Club airs.* The Schedule tab says it and reads the week and the shows again, but not
// for the change this window has just made itself: that one it has already said and drawn.
//
// The notice names nobody but in its sentence, so "this window's own" is this window's own
// record. A save on the rule's routes marks the show; the notice that ends with that show's
// name takes the mark away again. A mark nobody takes lapses, so it cannot hide a later change
// by someone else.

import type { Notice } from './api.ts';

/** How long after this window's save its own notice may still arrive: the station tells the
 *  windows at its next look at the schedule, and the view follows within a second or two. */
export const OWN_MS = 20_000;
/** A notice older than this when it is first seen is the past, not news. */
export const NEWS_MS = 60_000;

/** The shows this window has changed the rule of, by path: when each save was sent. */
const mine = new Map<string, number[]>();

/** This window is saving the rule of this show (or of one of its series). */
export function markOwn(slug: string, now = Date.now()): void {
  mine.set(slug, [...(mine.get(slug) ?? []), now]);
}

/** The save was refused, so no notice comes: the newest mark for the show is taken back. */
export function unmarkOwn(slug: string): void {
  const marks = mine.get(slug);
  if (!marks) return;
  marks.pop();
  if (!marks.length) mine.delete(slug);
}

/** Forget every mark (a test's). */
export function forgetOwn(): void {
  mine.clear();
}

/** Whether this notice is for a change this window made; if so, its mark is used up. */
export function takeOwn(title: string, shows: readonly { slug: string; name: string }[], now = Date.now()): boolean {
  for (const [slug, marks] of mine) {
    const fresh = marks.filter((at) => now - at < OWN_MS);
    if (!fresh.length) {
      mine.delete(slug);
      continue;
    }
    const name = shows.find((s) => s.slug === slug)?.name;
    if (name === undefined || !title.endsWith(` changed when ${name} airs.`)) {
      mine.set(slug, fresh);
      continue;
    }
    fresh.shift();
    if (fresh.length) mine.set(slug, fresh);
    else mine.delete(slug);
    return true;
  }
  return false;
}

/** The `rule.changed` notices a window has not heard yet. Every view carries the station's last
 *  few notices again, so each is heard once, by its id and its time (ids start again when the
 *  control plane does). What the first view carries is what happened before the window looked. */
export class Heard {
  #seen = new Set<string>();
  #primed = false;

  /** The notices that are new since the last view, oldest first. `now` is the station's clock. */
  fresh(notices: readonly Notice[], now: number): Notice[] {
    const rules = notices.filter((n) => n.kind === 'rule.changed');
    const keys = new Set(rules.map((n) => `${n.id}@${n.at_ms}`));
    const news = this.#primed ? rules.filter((n) => !this.#seen.has(`${n.id}@${n.at_ms}`) && now - n.at_ms < NEWS_MS) : [];
    this.#seen = keys;
    this.#primed = true;
    return news;
  }
}
