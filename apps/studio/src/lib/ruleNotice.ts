// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A show's rule changed, and every window is told (new-show.md section 3): the control plane
// keeps a notice of kind `rule.changed` whose title is the whole sentence, *Moira changed when
// The Folk Club airs.* The Schedule tab says it and reads the week and the shows again, but not
// for the change this window has just made itself: that one it has already said and drawn.
//
// Since 1.0.5 the notice names its show (`subject`, the show's path) and the save that made it
// (`change`, a token this window made and sent with the save), so "this window's own" is exact:
// the notice carrying one of this window's tokens. A notice from an older station has neither,
// and then the sentence is all there is: the notice that ends with a marked show's name takes
// the mark away again. A mark nobody takes lapses, so it cannot hide a later change by someone
// else.

import { onStationChange } from './reset.ts';
import type { Notice } from './api.ts';

/** How long after this window's save its own notice may still arrive: the station tells the
 *  windows at its next look at the schedule, and the view follows within a second or two. */
export const OWN_MS = 20_000;
/** A notice older than this when it is first seen is the past, not news. */
export const NEWS_MS = 60_000;

/** One of this window's saves: when it was sent, and the token sent with it. */
interface Mark {
  at: number;
  change: string | null;
}

/** The shows this window has changed the rule of, by path: its saves, oldest first. */
const mine = new Map<string, Mark[]>();

// A change of station (ADR-0285): a save sent to one station is no notice at another, and two
// stations can each have a show of the same name.
onStationChange('rule notice', () => {
  mine.clear();
});

/** The shows this window has a save marked for (a test's). */
export function ownMarks(): string[] {
  return [...mine.keys()];
}

/** A token for one save, sent with it and carried back on its notice. */
export function newChange(): string {
  const c = globalThis.crypto;
  if (c && typeof c.randomUUID === 'function') return c.randomUUID();
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 12)}`;
}

/** This window is saving the rule of this show (or of one of its series), with this token. */
export function markOwn(slug: string, now = Date.now(), change: string | null = null): void {
  mine.set(slug, [...(mine.get(slug) ?? []), { at: now, change }]);
}

/** The save was refused, so no notice comes: its mark is taken back (by its token, or else the
 *  show's newest). */
export function unmarkOwn(slug: string, change: string | null = null): void {
  const marks = mine.get(slug);
  if (!marks) return;
  const i = change ? marks.findIndex((m) => m.change === change) : marks.length - 1;
  if (i >= 0) marks.splice(i, 1);
  if (!marks.length) mine.delete(slug);
}

/** Forget every mark (a test's). */
export function forgetOwn(): void {
  mine.clear();
}

/** What `takeOwn` reads of a notice: the sentence, and since 1.0.5 the show and the save. */
export type RuleNoticeLike = { title: string; subject?: string | null; change?: string | null };

/** Whether this notice is for a change this window made; if so, its mark is used up. A bare
 *  sentence is read as an older station's notice. */
export function takeOwn(notice: RuleNoticeLike | string, shows: readonly { slug: string; name: string }[], now = Date.now()): boolean {
  const n: RuleNoticeLike = typeof notice === 'string' ? { title: notice } : notice;
  // Lapsed marks go first, whatever the notice.
  for (const [slug, marks] of mine) {
    const fresh = marks.filter((m) => now - m.at < OWN_MS);
    if (fresh.length) mine.set(slug, fresh);
    else mine.delete(slug);
  }
  // The station gave the save's token back: it is this window's exactly when the token is.
  if (n.change) {
    for (const [slug, marks] of mine) {
      const i = marks.findIndex((m) => m.change === n.change);
      if (i < 0) continue;
      marks.splice(i, 1);
      if (!marks.length) mine.delete(slug);
      return true;
    }
    return false;
  }
  // An older station, or a save sent without a token: the show, by its path where the notice
  // names it, or else by the end of the sentence. Only a save of this window's that went
  // without a token can be such a notice: one that carried a token waits for its own.
  for (const [slug, marks] of mine) {
    if (n.subject) {
      if (n.subject !== slug) continue;
    } else {
      const name = shows.find((s) => s.slug === slug)?.name;
      if (name === undefined || !n.title.endsWith(` changed when ${name} airs.`)) continue;
    }
    const i = marks.findIndex((m) => m.change === null);
    if (i < 0) continue;
    marks.splice(i, 1);
    if (!marks.length) mine.delete(slug);
    return true;
  }
  return false;
}

/** Whether a notice from another window is about this show: by its path where the notice
 *  names it (1.0.5), or else by the end of its sentence. */
export function isAbout(notice: RuleNoticeLike, slug: string, name: string): boolean {
  if (notice.subject) return notice.subject === slug;
  return notice.title.endsWith(` changed when ${name} airs.`);
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
