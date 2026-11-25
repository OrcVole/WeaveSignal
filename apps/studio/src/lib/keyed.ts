// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The way to a keyed show, in words and pure rules (keyed shows, 1.3.2; the operator's
// click-through of 2026-11-25: "the clear and obvious route to it is not visible"). One place
// for what every surface says of it, so the Schedule's cards, the show's pane, an episode, New
// show, the Keys tab and the make-a-key dialogs use the same words: the mark on a keyed show,
// the line that says whose rule an episode follows, the three steps from a show to a key in a
// listener's hands, the hints on a key's less obvious fields, the body that keeps a show for
// people with a key in one press, and what a refusal of that means.
//
// The word is "key" everywhere, and a show kept for people with a key is "keyed".

import { FieldError } from './api.ts';
import type { ShowAccess, ShowAccessSet } from './keysApi.ts';

/** The mark on a keyed show: its word, and the sentence its tooltip says. */
export const KEYED = {
  word: 'Keyed',
  tip: 'Keyed: advertised to everyone, heard and watched only with a key you give out.',
  /** The Schedule's legend, in full. */
  legend: 'Keyed: for people with a key',
} as const;

// ---- keeping a show for people with a key, in one press ----

/** The words of the one control that makes a show keyed, and of what leads to it. */
export const KEEP = {
  /** The tick's words, the same wherever a show is made keyed (KeyedTick, New show). */
  tick: 'Keyed show',
  hint: 'Kept for people with a key: advertised to everyone, heard only with a key you give out. You make the keys on the Keys tab.',
  /** Where a show is made: the choice beside New show. */
  fresh: 'New keyed show',
  /** The way to the rest of the choice (what the public hears, when it opens to everyone). */
  change: 'Who can listen…',
} as const;

/**
 * The rule sent to keep a show for people with a key in one press: what the show had before is
 * kept (what the public hears and its line), so a show made public for a while and keyed again
 * comes back as it was; a time it opens to everyone is kept only while it is still to come,
 * since a past one would leave the show open to everyone and the press would seem to do nothing.
 */
export function keepBody(stored: Pick<ShowAccess, 'slate' | 'slate_note' | 'opens_at_ms'> | null | undefined, now: number): ShowAccessSet {
  if (!stored) return { rule: 'key' };
  const opens = stored.opens_at_ms != null && stored.opens_at_ms > now ? stored.opens_at_ms : null;
  return { rule: 'key', slate: stored.slate, slate_note: stored.slate_note, opens_at_ms: opens };
}

/**
 * Why a show's access rule could not be read or saved, in a sentence that says what to do.
 * `at`: reading the rule (`read`), or saving it (`save`). A refusal to save a `key` rule is the
 * station failing closed: it keeps a show only where playout can keep it from the public (409:
 * playout has keyed shows off; 503: playout did not answer). A 503 on reading is keys not set
 * up at all, in the station's own words.
 */
export function accessProblem(e: unknown, at: 'read' | 'save', rule: 'key' | 'public' = 'key'): string {
  if (e instanceof FieldError) {
    const said = !/^\d{3}\b/.test(e.message) ? e.message : '';
    if (e.status === 401) return 'Your sign-in has ended. Sign in again, then try once more.';
    if (e.status === 403) return 'Only a producer or an admin can change who can listen. Ask the station’s admin.';
    if (e.status === 404 || e.status === 405) return at === 'read' && !/show/i.test(said) ? 'This station has no keyed shows yet: they came with WeaveSignal 1.3.2. Update the station first.' : said || 'The station no longer has that show.';
    if (at === 'save' && rule === 'key' && e.status === 409) return 'Playout has keyed shows off, so nothing would keep this show from the public. Turn them on (KEYED_SHOWS in the Compose bundle’s settings), restart playout, then try again.';
    if (at === 'save' && rule === 'key' && e.status === 503) return 'Playout is not answering, so the station cannot make sure it keeps this show from the public. Try again once playout is back.';
    if (e.status === 503) return `Keys are not set up at this station${said ? `: ${said}` : '.'}`;
    if (said) return said;
  }
  return e instanceof Error && e.message && !/^\d{3}\b/.test(e.message) && !(e instanceof TypeError) ? e.message : 'The station did not answer. Try again.';
}

// ---- who can listen, said on a show's pane and on an episode ----

/** How a show stands: open to everyone as ever, kept for people with a key now, or kept until
 *  it opened to everyone by itself. */
export type ListenState = 'public' | 'key' | 'opened';

/** From the public list's row for the show (`ruledShow`): none is a public show. */
export function listenState(row: { now: 'public' | 'key' } | null | undefined): ListenState {
  return !row ? 'public' : row.now === 'key' ? 'key' : 'opened';
}

/** The short words beside a show's name on its pane. */
export const LISTEN_SHORT: Record<ListenState, string> = {
  public: 'Everyone can listen.',
  key: 'Only people with a key can listen.',
  opened: 'Kept for people with a key until it opened to everyone.',
};

/**
 * The line on an episode, or on a date not drafted yet, that says whose rule it follows: who
 * can listen is the show's, never one episode's. `aired`: the episode is in the past, where the
 * operator saw nothing and gave up; it says plainly that a key keeps the whole show.
 */
export function listenLine(state: ListenState, o: { show: string; aired?: boolean; opens?: string | null }): { lead: string; rest: string } {
  const whole = 'A key keeps the whole show, not one episode';
  if (o.aired) {
    const now = state === 'key' ? `${o.show} is kept for people with a key now` : state === 'opened' ? `${o.show} was kept for people with a key and has opened to everyone` : `${o.show} is open to everyone`;
    return { lead: `${whole}.`, rest: `This one has aired, so nothing here changes who heard it. ${now}; the tick changes the episodes to come.` };
  }
  if (state === 'key') return { lead: 'This show is kept for people with a key.', rest: `Every episode of ${o.show}, this one too: it is advertised to everyone and heard only with a key.${o.opens ? ` It opens to everyone ${o.opens}.` : ''}` };
  if (state === 'opened') return { lead: 'This show has opened to everyone.', rest: `${o.show} was kept for people with a key until then. ${whole}.` };
  return { lead: 'This show is open to everyone.', rest: `${whole}: the tick keeps every episode of ${o.show} for people with a key, this one too.` };
}

// ---- the three steps, from a show to a key in a listener's hands ----

export type KeyStep = 1 | 2 | 3;

/** The numbered line at the top of the dialogs that keep a show, make a key and send it. */
export const KEY_STEPS: readonly { n: KeyStep; label: string; where: string }[] = [
  { n: 1, label: 'Keep a show for people with a key', where: 'Schedule › show › Who can listen' },
  { n: 2, label: 'Make a key for it', where: 'Keys › Make a key…' },
  { n: 3, label: 'Send it', where: 'opens when the key is made' },
];

/** Why a step cannot be gone to from where the line stands; empty when it can. */
export const STEP_WAITS = {
  /** Send it, before there is a key to send. */
  send: 'Send opens by itself the moment the key is made, with its link, its text and a message ready to copy.',
  /** Make a key, while a key has only just been made. */
  made: 'This key is made. Done closes this, and Make a key… makes another.',
} as const;

/** The hints on a key's less obvious fields, a sentence or two each (the operator, 2026-11-25:
 *  "inline tool tips … would help"). */
export const KEY_TIPS = {
  kind: 'Personal: one listener, named, on a couple of their own devices. Group: one key for a room, a club or a class, with a count of people. Machine: for a player that is not a person, such as a relay or a screen in a hall; it has no devices to count.',
  opens: 'Every keyed show: the key also opens shows you key later. Only these shows: it opens the ones ticked, and you can change them after the key is in someone’s hands.',
  works: 'The key’s window, in the station’s time. Before From its holder is told when it opens; after Until it stops, and Extend on its page starts it again. Both empty: it works at once, with no end.',
  limits: 'Limits make passing a key on harder. Devices at once: how many browsers or apps may listen at the same moment. Listens in all: how many times it may start, for a ticket to one show. People: for a group key, how many different devices may ever use it.',
  family: 'A family is keys issued together, one a person, all alike. If one turns up where it should not be, Find a key says whose it was, the leak report ranks them, and that one key is replaced while the others go on working.',
  media: 'Sound and picture: the holder hears the show, and watches it where the station sends a picture. Sound only keeps the picture back. Picture, with its sound opens the same as Sound and picture: a key for the picture always carries the show’s sound.',
} as const;
