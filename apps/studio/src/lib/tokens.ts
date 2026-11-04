// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// API tokens in the studio app (ADR-0195), as pure rules: a token's state, the scopes grouped
// by what they touch, the words for an expiry, and the checks a form runs before it asks the
// station. The station stays the authority and its own sentence is shown as given.

import type { ApiScope, ApiTokenInfo } from './api.ts';

export const NAME_MAX = 80;

/** How long a new token lasts, as the form offers it; `null` is never. */
export const EXPIRIES: readonly { days: number | null; label: string }[] = [
  { days: 30, label: '30 days' },
  { days: 90, label: '90 days' },
  { days: 365, label: 'A year' },
  { days: null, label: 'Never' },
];

export type TokenState = 'active' | 'expired' | 'revoked';

/** Whether a token works now. */
export function stateOf(t: ApiTokenInfo, now: number): TokenState {
  if (t.revoked_ms !== null) return 'revoked';
  if (t.expires_ms !== null && t.expires_ms <= now) return 'expired';
  return 'active';
}

/** Active ones first, then by newest; the revoked and the expired sink. */
export function ordered(tokens: ApiTokenInfo[], now: number): ApiTokenInfo[] {
  const rank = { active: 0, expired: 1, revoked: 2 } as const;
  return [...tokens].sort((a, b) => rank[stateOf(a, now)] - rank[stateOf(b, now)] || b.created_ms - a.created_ms);
}

/** The scopes grouped by the thing they touch (`carts:read` and `carts:fire` under `carts`),
 *  in the order the station gave them. */
export function scopeGroups(scopes: ApiScope[]): { group: string; scopes: ApiScope[] }[] {
  const out: { group: string; scopes: ApiScope[] }[] = [];
  for (const s of scopes) {
    const group = s.name.split(':')[0];
    const last = out.find((g) => g.group === group);
    if (last) last.scopes.push(s);
    else out.push({ group, scopes: [s] });
  }
  return out;
}

/** The word after the colon, for a checkbox inside its group. */
export const scopeAction = (name: string): string => name.split(':')[1] ?? name;

/** A name for a row, or the sentence for the field. */
export function nameProblem(name: string): string | null {
  const n = name.trim();
  if (!n) return 'Name the token after what will use it, so you can tell it apart.';
  if (n.length > NAME_MAX) return `A name is up to ${NAME_MAX} characters.`;
  return null;
}

export function scopesProblem(chosen: string[]): string | null {
  return chosen.length === 0 ? 'Choose at least one scope. A token can do only what its scopes allow.' : null;
}

const day = (ms: number) => new Date(ms).toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' });

/** When a token ends, in words. */
export function expiryWords(t: ApiTokenInfo, now: number): string {
  if (t.revoked_ms !== null) return `Revoked ${day(t.revoked_ms)}`;
  if (t.expires_ms === null) return 'Never expires';
  return t.expires_ms <= now ? `Expired ${day(t.expires_ms)}` : `Expires ${day(t.expires_ms)}`;
}

/** When it was last used, in words ("used" is "presented": a refused request counts). */
export function lastUsedWords(t: ApiTokenInfo): string {
  return t.last_used_ms === null ? 'Not used yet' : `Last used ${day(t.last_used_ms)}`;
}
