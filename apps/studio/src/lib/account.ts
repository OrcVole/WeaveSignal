// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Who this window runs as (ADR-0127), as pure rules: what a role may do, the words for a
// role, the checks a form runs before it asks the station, and how a refusal lands under the
// field it names. The station stays the authority: these mirror its rules so a slip is caught
// before the round trip, and its own sentence is shown as given when it still refuses.

import { FieldError, type Me, type StaffRole } from './api.ts';

export const ROLES: readonly StaffRole[] = ['admin', 'producer', 'presenter', 'screener'];

/** The role's word for a label or a row. */
export const roleWord = (r: StaffRole): string => ({ admin: 'Admin', producer: 'Producer', presenter: 'Presenter', screener: 'Screener' })[r];

/** One line on what a role may do, for the role picker. */
export const roleLine = (r: StaffRole): string =>
  ({
    admin: 'Runs the studio and manages staff, retention and erasure.',
    producer: 'Runs the studio: the desk, the screener, the carts and the schedule.',
    presenter: 'Runs the studio from the desk.',
    screener: 'Runs the studio from the screener.',
  })[r];

/** An admin, or the station key, manages staff, retention and erasure. */
export function isAdmin(me: Me | null | undefined): boolean {
  return !!me && (me.kind === 'station' || me.role === 'admin');
}

/** The signed-in person's line for the frame: a name and a role, or the station key. */
export function whoLine(me: Me | null | undefined): { name: string; role: string } {
  if (!me || me.kind === 'station') return { name: 'Station key', role: 'admin' };
  return { name: me.account?.name || me.account?.username || 'Signed in', role: roleWord(me.role).toLowerCase() };
}

/** Whether a change to an account is one the station refuses from the account itself. */
export function selfLockout(me: Me | null | undefined, id: string, change: { role?: StaffRole; disabled?: boolean }): boolean {
  if (!me || me.kind !== 'staff' || me.account?.id !== id) return false;
  return change.disabled === true || (change.role !== undefined && change.role !== 'admin');
}

export const USERNAME_MAX = 40;
export const NAME_MAX = 80;
export const PASSWORD_MIN = 10;
export const PASSWORD_MAX = 256;

/** The station's rule for a username: lower-case letters, digits, `.`, `_` and `-`, up to 40,
 *  starting with a letter or digit. Returns the sentence for the field, or null when fine. */
export function usernameProblem(username: string): string | null {
  const u = username.trim();
  if (!u) return 'Enter a username.';
  if (u.length > USERNAME_MAX) return `A username is up to ${USERNAME_MAX} characters.`;
  if (!/^[a-z0-9][a-z0-9._-]*$/.test(u))
    return 'A username is lower-case letters, digits, dots, underscores and hyphens, and starts with a letter or digit.';
  return null;
}

export function nameProblem(name: string): string | null {
  const n = name.trim();
  if (!n) return 'Enter their name, as it should appear on air and in the log.';
  if (n.length > NAME_MAX) return `A name is up to ${NAME_MAX} characters.`;
  return null;
}

/** The station's rule for a password: 10 to 256 characters, not the username, nothing else. */
export function passwordProblem(password: string, username = ''): string | null {
  const n = [...password].length;
  if (n === 0) return 'Enter a password.';
  if (n < PASSWORD_MIN) return `A password is at least ${PASSWORD_MIN} characters. Any characters count; a few words work well.`;
  if (n > PASSWORD_MAX) return `A password is up to ${PASSWORD_MAX} characters.`;
  if (username && password.toLowerCase() === username.trim().toLowerCase()) return 'The password cannot be the username.';
  return null;
}

/** The shape of an email address, as the station checks it before it sends anything: a local
 *  part, an `@` and a dotted domain, no spaces. The code that comes back is the real check.
 *  Returns the sentence for the field, or null when it looks like one. */
export function emailProblem(email: string): string | null {
  const e = email.trim();
  if (!e) return 'Enter your email address.';
  const at = e.lastIndexOf('@');
  const domain = at < 0 ? '' : e.slice(at + 1);
  const ok =
    at > 0 && e.length <= 254 && domain.includes('.') && !domain.startsWith('.') && !domain.endsWith('.') && !/[\s<>,\u0000-\u001f]/.test(e);
  return ok ? null : 'Enter an email address, like name@example.com.';
}

/** A code as typed: six digits, spaces ignored (an email client may break it up). */
export function cleanCode(code: string): string {
  return code.replace(/\s+/g, '');
}

/** The sentence for a code field, or null when it is six digits. */
export function codeProblem(code: string): string | null {
  const c = cleanCode(code);
  if (!c) return 'Enter the code from the email.';
  return /^\d{6}$/.test(c) ? null : 'The code is six digits.';
}

/** A refusal placed under the field it names, or on the form when it names none, or when it
 *  names a field the form does not have. The station's sentence is shown as given. */
export function placeError<F extends string>(e: unknown, fields: readonly F[]): { field: F | null; message: string } {
  if (e instanceof FieldError) {
    const f = e.field && (fields as readonly string[]).includes(e.field) ? (e.field as F) : null;
    return { field: f, message: e.message };
  }
  if (typeof e === 'string') return { field: null, message: e };
  return { field: null, message: e instanceof Error ? e.message : 'Something went wrong. Try again.' };
}


/** A password to pass on: four groups of four from an alphabet with no look-alikes (no 0, O,
 *  1, l, I), 19 characters, well past the floor. `random` gives a number in [0, 1). */
export function suggestPassword(random: () => number = secureRandom): string {
  const alphabet = 'abcdefghjkmnpqrstuvwxyz23456789';
  const group = () => Array.from({ length: 4 }, () => alphabet[Math.floor(random() * alphabet.length)]).join('');
  return [group(), group(), group(), group()].join('-');
}

function secureRandom(): number {
  const a = new Uint32Array(1);
  globalThis.crypto.getRandomValues(a);
  return a[0] / 2 ** 32;
}
