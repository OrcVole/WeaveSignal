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
  hint: 'Kept for people with a key: advertised to everyone, heard only with a key you give out. Its podcast feed and episode files are held back while it is keyed. You make the keys on the Keys tab.',
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

/**
 * Whether the tick Keyed show is ticked. It means one thing: a key is needed to listen now. A
 * show that was keyed and has opened to everyone by itself is not ticked (1.3.3; it stayed
 * ticked in 1.3.2, and staff could take an open show for a kept one). Its rule is still there:
 * the words beside the tick say when it opened, and ticking it keeps it again from that moment
 * (`keepBody` leaves the past time out).
 */
export function ticked(state: ListenState): boolean {
  return state === 'key';
}

/**
 * How long until the next keyed show opens to everyone by itself, in ms from `now` (the
 * station's time); null when none is to. The list of keyed shows says how each stands when it
 * was read, so it is read again at that moment: a tick that was true a minute ago must not
 * stand on a show everyone can now hear.
 */
export function nextOpening(shows: readonly { now: 'public' | 'key'; opens_at_ms?: number | null }[], now: number): number | null {
  const at = shows.filter((s) => s.now === 'key' && s.opens_at_ms != null).map((s) => s.opens_at_ms as number);
  return at.length ? Math.max(0, Math.min(...at) - now) : null;
}

/** The short words beside a show's name on its pane. */
export const LISTEN_SHORT: Record<ListenState, string> = {
  public: 'Everyone can listen.',
  key: 'Only people with a key can listen.',
  opened: 'Everyone can listen now.',
};

/** What stands beside an unticked tick on a show that opened to everyone by itself: when, and
 *  the way back. `when` is the moment in the station's time, when the list gives it. */
export function openedWords(when?: string | null): string {
  return `Opened to everyone${when ? ` ${when}` : ''}: no key is needed now. Tick to keep it for people with a key again.`;
}

// ---- a keyed show's podcast feed and episode files (1.3.3) ----

/**
 * Whether the station holds back a keyed show's episode files. From 1.3.3 it does: while a show
 * is kept for people with a key its podcast feed lists no episode and its episode files cannot
 * be fetched, earlier episodes included; they return when the show is public or has opened to
 * everyone. 1.3.2, the one release with keyed shows before that, left both open to anyone, and
 * the app says so there instead of promising what that station does not do. A version not read
 * yet is taken as the app's own release.
 *
 * `app` is this app's own version. The station says the version it was built as, and a build
 * made before the release's version is set (a candidate, a test image, a development stack)
 * still says 1.3.2 while it holds the files back; this app is then such a build too, and
 * cannot tell its own station from the released 1.3.2. So an app that is not yet 1.3.3 by its
 * own version says what 1.3.3 does, and the released app, which is, goes by the station's.
 */
export function holdsFiles(station: string | null | undefined, app?: string | null): boolean {
  if (app != null && !from133(app)) return true;
  return from133(station);
}
/** 1.3.3 or later by its numbers; a version that cannot be read counts as one. */
function from133(v: string | null | undefined): boolean {
  const m = /^(\d+)\.(\d+)\.(\d+)/.exec((v ?? '').trim());
  if (!m) return true;
  const [a, b, c] = m.slice(1).map(Number);
  return a > 1 || (a === 1 && (b > 3 || (b === 3 && c >= 3)));
}

/** What becomes of a keyed show's podcast feed and episode files, in the same words wherever a
 *  show is made keyed or stands keyed. */
export const FILES = {
  /** On a show that is keyed. */
  held: 'Its podcast feed lists no episode and its episode files are held back while it is keyed, earlier episodes included.',
  /** Where a show is about to be made keyed. */
  tick: 'Its podcast feed and episode files are held back too, earlier episodes included, until it is open to everyone again.',
  /** Said after a tick. */
  ticked: 'Its podcast feed and episode files are held back meanwhile, earlier episodes included.',
  /** Said when a show is opened to everyone again. The feed is cached for a few minutes. */
  back: 'Its episodes return to the podcast feed within a few minutes.',
  /** What no tick can do. */
  gone: 'Copies already downloaded cannot be called back.',
  /** At a station of 1.3.2, which holds nothing back. */
  older: 'This station runs WeaveSignal 1.3.2, which leaves a keyed show’s podcast feed and episode files open to anyone. Update the station to hold them back.',
} as const;

/** The tooltip on the tick itself: what its state means now, and what a press does. */
export function tickTip(state: ListenState, o: { opened?: string | null; held?: boolean } = {}): string {
  const held = o.held !== false;
  if (state === 'key') return `Ticked: only people with a key can listen now. ${held ? FILES.held : FILES.older} Untick to open the show to everyone again.`;
  if (state === 'opened') return `Not ticked: the show opened to everyone${o.opened ? ` ${o.opened}` : ''}, so no key is needed now. Tick to keep it for people with a key again.`;
  return `Not ticked: everyone can listen. Tick and the show is kept for people with a key from that moment. ${held ? `${FILES.tick} ${FILES.gone}` : FILES.older}`;
}

/** What the tick says it did. */
export function keptWords(name: string, held = true): string {
  return `${name} is kept for people with a key, from now. ${held ? FILES.ticked : FILES.older} Make keys for its listeners on the Keys tab.`;
}
/** What opening a show to everyone again says it did. */
export function openedAgainWords(name: string, held = true): string {
  return `${name} is open to everyone again. Its keys are kept, and open nothing until it is keyed again.${held ? ` ${FILES.back}` : ''}`;
}

/**
 * The line on an episode, or on a date not drafted yet, that says whose rule it follows: who
 * can listen is the show's, never one episode's. `aired`: the episode is in the past, where the
 * operator saw nothing and gave up; it says plainly that a key keeps the whole show. A tick
 * cannot change who heard an aired episode on air, but it does decide who can fetch its file
 * (1.3.3: a keyed show's podcast feed and episode files are held back, earlier episodes
 * included), and the line says that too. `opened`: when a show that was kept opened to everyone.
 * `held` false: a station of 1.3.2, which holds no file back.
 */
export function listenLine(state: ListenState, o: { show: string; aired?: boolean; opens?: string | null; opened?: string | null; held?: boolean }): { lead: string; rest: string } {
  const whole = 'A key keeps the whole show, not one episode';
  const held = o.held !== false;
  if (o.aired) {
    const air = 'This one has aired, so nothing here changes who heard it on air.';
    if (state === 'key')
      return { lead: `${whole}.`, rest: `${air} ${o.show} is kept for people with a key now${held ? `, so its podcast feed lists no episode and its episode files are held back, this episode’s too, until the show is open to everyone. ${FILES.gone}` : `. ${FILES.older}`}` };
    if (state === 'opened') return { lead: `${whole}.`, rest: `${air} ${o.show} was kept for people with a key and has opened to everyone${held ? ', so its episodes are in the podcast feed and its episode files can be fetched again' : ''}.` };
    return { lead: `${whole}.`, rest: `${air} ${o.show} is open to everyone. The tick keeps the episodes to come for people with a key${held ? `, and holds back the show’s podcast feed and episode files, this episode’s too, until it is open to everyone again. ${FILES.gone}` : `. ${FILES.older}`}` };
  }
  if (state === 'key') return { lead: 'This show is kept for people with a key.', rest: `Every episode of ${o.show}, this one too: it is advertised to everyone and heard only with a key.${o.opens ? ` It opens to everyone ${o.opens}.` : ''} ${held ? FILES.held : FILES.older}` };
  if (state === 'opened') return { lead: 'This show has opened to everyone.', rest: `${o.show} was kept for people with a key until ${o.opened ?? 'then'}; the tick keeps it again. ${whole}.` };
  return { lead: 'This show is open to everyone.', rest: `${whole}: the tick keeps every episode of ${o.show} for people with a key, this one too. ${held ? FILES.tick : FILES.older}` };
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
  media: 'Sound and picture: the holder hears the show, and watches it where the station sends a picture. Sound only keeps the picture back.',
} as const;

/** Beside For on a key made for the picture, a choice the studio no longer offers (1.3.3): such
 *  a key carries the show's sound and opens what Sound and picture does, so it goes on working
 *  as it is, and its form says what it is. */
export const MEDIA_OLD = 'This key was made for the picture, with its sound. It opens the same as Sound and picture, and goes on working as it is. The choice is no longer offered for new keys.';
