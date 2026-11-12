// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The desk's invite (ADR-0074): the words of its dialog, kept out of the component so the
// tests can hold them. An invite is a link to the station's join page with a pass in its
// fragment; the role decides whether its person joins on air or off.

import { FieldError, type Invite, type InviteRole } from './api.ts';
import { firstName, hm } from './format.ts';

/** The roles an invite may carry, the DJ first (the default), each with what joining means. */
export const INVITE_ROLES: readonly { role: InviteRole; name: string; what: string }[] = [
  { role: 'dj', name: 'DJ', what: 'Joins off air, under Not on air. Listeners hear nothing of them until you put them on. For the next show’s DJ.' },
  { role: 'guest', name: 'Guest', what: 'On air the moment they join.' },
  { role: 'presenter', name: 'Presenter', what: 'On air the moment they join, as a presenter of this show.' },
];

/** What is wrong with the name as typed, or null: the desk shows it on their tile. */
export function nameProblem(name: string): string | null {
  const n = name.trim();
  if (!n) return 'Enter their name: the desk shows it on their tile.';
  if ([...n].length > 120) return 'Use a shorter name: 120 characters at most.';
  return null;
}

/** The whole link, to send: the join page lives at the station's own address. `origin` is the
 *  station's address as this window knows it (`base()`), or the page's own. */
export function inviteUrl(link: string, origin: string): string {
  return /^https?:\/\//.test(link) ? link : `${origin.replace(/\/$/, '')}${link}`;
}

/** `4 hours`, `1 hour 30 minutes`, `45 minutes`: how long a link lasts, to the minute. */
export function lasts(ms: number): string {
  const min = Math.max(1, Math.round(ms / 60_000));
  const h = Math.floor(min / 60);
  const m = min % 60;
  const hours = h ? `${h} ${h === 1 ? 'hour' : 'hours'}` : '';
  const minutes = m ? `${m} ${m === 1 ? 'minute' : 'minutes'}` : '';
  return [hours, minutes].filter(Boolean).join(' ');
}

/** How long the link lasts and when it ends, by this machine's clock. */
export function lastsLine(invite: Pick<Invite, 'expires_at_ms'>, now: number): string {
  return `The link works for ${lasts(invite.expires_at_ms - now)}, until ${hm(invite.expires_at_ms)}.`;
}

/** What happens when they open it, by role. */
export function joinsLine(invite: Pick<Invite, 'role' | 'name' | 'identity'>): string {
  const who = firstName(invite.name, invite.identity);
  return invite.role === 'dj'
    ? `${who} opens it in a browser and presses Join the studio. They arrive off air, under Not on air.`
    : `${who} opens it in a browser and presses Join the studio. They are on air the moment they join.`;
}

/** A refusal in words: the station's own sentence where it has one worth reading, ours where
 *  it is a status or the network. */
export function inviteProblem(e: unknown): string {
  if (e instanceof FieldError) {
    if (e.status === 401) return 'Your sign-in has ended. Sign in again, then create the invitation.';
    if (e.status === 403) return 'Your account may not create invitations. Ask a station admin.';
    if (e.status === 503) return `This station cannot create invitations yet. It says: ${e.message}.`;
    if ((e.status === 422 || e.status === 400) && e.message && !/^\d{3}\b/.test(e.message)) return `The station refused the invitation. It says: ${e.message}.`;
    return 'The station could not create the invitation. Try again.';
  }
  return 'The station did not answer. Check the link to the station, then try again.';
}
