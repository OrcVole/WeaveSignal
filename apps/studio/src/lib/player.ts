// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Open the player (1.3.1; the first-sound run's K1 and K3). The studio app makes no sound: a
// listener, and a presenter checking the programme, hears the station in a browser, from the
// player page at the station's address. This is the rule for that address and the words that
// go with it, shared by the Desk's Listeners panel and by Settings.
//
// Which address: the station's public address (`publishing.public_url`, sent with the week) when
// it has one, because that is the address the world reaches, and on Cloudron the studio's own
// domain is behind Cloudron's login. A station on a home network (the Compose install that set
// no public address) has none, and then the player is on the address this window already talks
// to, which is where the control plane serves it. Cloudron always sets a public address, so
// that fallback never lands on the login page.

import { publicAddress } from '../schedule/files/links.ts';

/** What the buttons need to know. */
export type PlayerLink =
  /** The station has not been asked yet. */
  | { state: 'asking'; url: null; why: string; own: false }
  /** A link to open and copy. `own`: it is this window's own address, not a public one. */
  | { state: 'ready'; url: string; why: ''; own: boolean }
  /** No link can be written; `why` says so. */
  | { state: 'none'; url: null; why: string; own: false };

export const PLAYER = {
  /** The one sentence for the Desk (K1). */
  sound: 'Sound plays in your browser, not in this app.',
  /** The Open the player button's tooltip on the Desk: the sentence the panel stops drawing once the player has been used. */
  openTip: 'Opens the station’s player in your browser. Sound plays there, not in this app.',
  open: 'Open the player',
  copy: 'Copy link',
  asking: 'Asking the station for its address…',
  /** Where the studio has no address of any kind to write. */
  none: 'This window has no address for the station, so there is no link to open. Sign in to a station first.',
  /** Shown beside a link made from the window's own address. */
  own: 'The station’s settings give no public address (publishing.public_url), so this is the address this window uses. It opens the player here; people elsewhere need the station’s public address.',
  opened: 'The player is open in your browser.',
  notOpened: 'Could not open a browser from here. Copy the link and paste it into your browser.',
  copied: 'Copied. Paste it into a browser.',
  notCopied: 'Could not copy from here. The link is selected in its box: copy it from there.',
};

/** The player's address from a web address: its origin and a slash, nothing else. */
const playerAt = (origin: string): string => `${origin.replace(/\/+$/, '')}/`;

/** The address the player is at.
 *
 *  - `publicUrl`: what the week said: a string, null when the station gave none, undefined when
 *    it could not be asked or an older control plane did not send it.
 *  - `asked`: the asking has finished (`undefined` only means "unknown" once it has).
 *  - `own`: the address this window talks to (`base()`, or the page's origin in a browser). */
export function playerLink(o: { publicUrl: string | null | undefined; asked: boolean; own: string }): PlayerLink {
  const pub = publicAddress(o.publicUrl);
  if (pub) return { state: 'ready', url: playerAt(pub), why: '', own: false };
  if (!o.asked) return { state: 'asking', url: null, why: PLAYER.asking, own: false };
  const own = publicAddress(o.own);
  if (own) return { state: 'ready', url: playerAt(own), why: '', own: true };
  return { state: 'none', url: null, why: PLAYER.none, own: false };
}

/** What to try first on an empty Desk, in a first minute (G12): a cart plays with nobody in the room.
 *  The player first, because it takes a moment to start and is behind the desk by `behind` (the
 *  desk's own figure, such as *about 21 s behind*): a pad pressed first is heard late. */
export const firstMinute = (behind: string): string =>
  `To hear something first: open the player and press Listen live, then press a pad on the Carts tab (⌃3). Listeners are ${behind}, so wait for it. Sound plays in your browser, not in this app.`;
