// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A show's presenter as a staff account (ADR-0218, `presenter_account`), picked in the show
// editor's Presenter group (`calendar.show-presenter`). The station's rule decides who may
// pick whom: an admin (or the station key) any account; any other member of staff only
// themselves, and clearing is anyone's. So a producer is offered themselves and the account
// the show already has, never the whole team, and a save that leaves the account as it was
// sends nothing about it (the station refuses a body that names someone else, even unchanged).
// The directory the picker reads is a producer's and an admin's (ADR-0289): a presenter's or a
// screener's window never asks for it, and has no picker.

import type { Me, StaffRole } from '../lib/api.ts';

/** One member of the team as `GET /api/v1/staff/directory` gives them. */
export interface TeamMember {
  id: string;
  name: string;
  role: StaffRole;
}

export interface PresenterOption {
  /** The account's id, or '' for no account. */
  value: string;
  label: string;
  /** Not for picking: it stays only because the show has it now. */
  disabled?: boolean;
}

export interface PresenterChoice {
  options: PresenterOption[];
  /** This window may name any account, not only its own. */
  any: boolean;
}

const WORD: Record<StaffRole, string> = { admin: 'admin', producer: 'producer', presenter: 'presenter', screener: 'screener' };

/** Whether the station gives this window the team's directory (`GET /api/v1/staff/directory`,
 *  ADR-0289): a producer, an admin or the station key. A presenter or a screener is refused, so
 *  the window does not ask: the picker is left out and the show keeps its account. */
export const mayListTeam = (me: Me | null | undefined): boolean => !!me && (me.kind === 'station' || me.role === 'admin' || me.role === 'producer');

/** The team a window that may not ask for the directory can still offer: only itself, as the
 *  station's rule allows (any member of staff may name themselves), and only while the show has
 *  no account or has this one (another's would need its name, which this window is not given).
 *  Null leaves the picker out and the show's account as it is. */
export function ownTeam(me: Me | null | undefined, current: string | null): TeamMember[] | null {
  const mine = me?.kind === 'staff' ? me.account : null;
  if (!mine || (current && current !== mine.id)) return null;
  return [{ id: mine.id, name: mine.name, role: mine.role }];
}

/** An admin, or the station key. */
export const namesAnyone = (me: Me | null | undefined): boolean => !!me && (me.kind === 'station' || me.role === 'admin');

/** The accounts this window may pick, in the order the directory gave, after "No account".
 *  The account the show has now stays in the list, named, when the window could not pick it;
 *  when it is not on the team any more (turned off) it is named as that. */
export function presenterChoice(team: readonly TeamMember[], me: Me | null | undefined, current: string | null): PresenterChoice {
  const any = namesAnyone(me);
  const mine = me?.account?.id ?? null;
  const options: PresenterOption[] = [{ value: '', label: 'No account: just a name' }];
  for (const m of team) {
    const pickable = any || m.id === mine;
    if (!pickable && m.id !== current) continue;
    options.push({ value: m.id, label: `${m.name} · ${WORD[m.role]}${m.id === mine ? ' (you)' : ''}`, disabled: !pickable });
  }
  if (current && !team.some((m) => m.id === current)) options.push({ value: current, label: 'An account that is turned off', disabled: true });
  return { options, any };
}

/** What to send for `presenter_account`: nothing when it is as stored, else the id or null. */
export function accountPatch(draft: string, stored: string | null): { presenter_account?: string | null } {
  const now = draft || null;
  return now === (stored || null) ? {} : { presenter_account: now };
}

/** The on-air name a pick offers when there is none yet: the account's own name. */
export function offeredName(team: readonly TeamMember[], id: string, typed: string): string {
  return typed.trim() ? typed : (team.find((m) => m.id === id)?.name ?? typed);
}

/** The sentence under the picker. */
export function presenterHint(choice: PresenterChoice): string {
  return choice.any
    ? 'The member of staff who presents this show. The schedule brings them on air at its time when they are in the room signed in as themselves, and they may change the show’s step, its presenter details and its keys.'
    : 'Only an admin names another member of staff. You can name yourself, or leave the show with a name only.';
}
