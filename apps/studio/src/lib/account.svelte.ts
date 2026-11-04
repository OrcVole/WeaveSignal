// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The window's own account (ADR-0127): who it runs as, from `/api/v1/sessions/me`, and the
// moment a sign-in ends. A request that answers 401 under a staff session or a station key
// marks the session ended; the frame then shows one way back (Sign in again) rather than
// reconnecting in silence. The browser build is left as it was: its bearer comes from the
// proxy or `?token=`, and there is no keychain to sign out of.

import { FieldError, packaged, setUnauthorizedHandler, signOut, whoAmI, type Me } from './api.ts';

export const account = $state({
  /** Who this window runs as; null until the station has answered, or when it cannot say. */
  me: null as Me | null,
  /** The station has answered (or refused) `/sessions/me` once. */
  loaded: false,
  /** The sign-in has ended: the station's sentence. */
  ended: null as string | null,
});

/** Ask the station who this window is. An older control plane without the route (404)
 *  leaves `me` null and `loaded` true: the window then behaves as the station key did. */
export async function loadMe(): Promise<void> {
  try {
    const me = await whoAmI();
    // A station too old for staff sign-in, or anything that is not this answer, is as the key was.
    account.me = me && typeof me.role === 'string' && typeof me.kind === 'string' ? me : null;
  } catch (e) {
    account.me = null;
    if (e instanceof FieldError && e.status === 401) endSession(e.message);
  } finally {
    account.loaded = true;
  }
}

/** The sign-in has ended (a 401): say so once, with the station's sentence. */
export function endSession(why: string): void {
  if (!packaged()) return;
  account.ended ??= why || 'Your sign-in has ended. Sign in again.';
}

/** Sign in again: the station ends the session, this computer forgets it, the window reloads
 *  on the sign-in screen with the address kept. */
export async function signInAgain(): Promise<void> {
  try {
    await signOut();
  } catch {
    // The keychain refused or the station was away: the reload still lands on sign-in.
  }
  location.reload();
}

/** Wire the 401 hook once, from the app's frame. */
export function installSessionWatch(): void {
  setUnauthorizedHandler((why) => {
    endSession(why);
    return packaged();
  });
}
