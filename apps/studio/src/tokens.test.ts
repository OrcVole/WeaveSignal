// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// API tokens in the studio app (ADR-0195) without a station: the rules a form checks before
// asking, how a token's state and ends are put in words, and the Settings panel rendered on the
// server. Sample people come from the Scottish names list (ADR-0071).
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import { PANELS } from '../../../packages/ui/panels.ts';
import type { ApiScope, ApiTokenInfo, Me } from './lib/api.ts';
import { expiryWords, lastUsedWords, nameProblem, ordered, scopeAction, scopeGroups, scopesProblem, stateOf } from './lib/tokens.ts';
import Settings from './settings/Settings.svelte';
import Tokens from './settings/Tokens.svelte';

const NOW = Date.UTC(2026, 10, 2, 12);
const DAY = 86_400_000;

const scopes: ApiScope[] = [
  { name: 'carts:read', description: 'Read the cart wall.' },
  { name: 'carts:fire', description: 'Fire, stop and cue carts.' },
  { name: 'schedule:read', description: 'Read the clocks and the week.' },
];

const deck: ApiTokenInfo = {
  id: 't1',
  name: 'Stream deck',
  scopes: ['carts:fire', 'carts:read'],
  prefix: 'rsa_1a2b',
  created_by: 'Morag Campbell',
  created_ms: NOW - 3 * DAY,
  expires_ms: NOW + 30 * DAY,
  last_used_ms: NOW - DAY,
  revoked_ms: null,
};
const website: ApiTokenInfo = { ...deck, id: 't2', name: 'Station website', scopes: ['schedule:read'], prefix: 'rsa_9f8e', created_ms: NOW - 10 * DAY, expires_ms: null, last_used_ms: null };
const old: ApiTokenInfo = { ...deck, id: 't3', name: 'Old script', expires_ms: NOW - DAY };
const gone: ApiTokenInfo = { ...deck, id: 't4', name: 'Fraser’s test', revoked_ms: NOW - 2 * DAY };

const asAdmin: Me = { kind: 'station', role: 'admin' };
const asPresenter: Me = { kind: 'staff', role: 'presenter', account: { id: 'a2', username: 'fraser', name: 'Fraser Macleod', role: 'presenter', disabled_ms: null, created_ms: 1 } };

describe('what a token is, in words', () => {
  it('works until it is revoked or its end passes', () => {
    expect(stateOf(deck, NOW)).toBe('active');
    expect(stateOf(website, NOW)).toBe('active');
    expect(stateOf(old, NOW)).toBe('expired');
    expect(stateOf(gone, NOW)).toBe('revoked');
  });

  it('lists the ones that work first, newest first, and the dead last', () => {
    expect(ordered([gone, old, website, deck], NOW).map((t) => t.id)).toEqual(['t1', 't2', 't3', 't4']);
  });

  it('says when it ends and when it was last used', () => {
    expect(expiryWords(deck, NOW)).toBe('Expires 2 Dec 2026');
    expect(expiryWords(website, NOW)).toBe('Never expires');
    expect(expiryWords(old, NOW)).toBe('Expired 1 Nov 2026');
    expect(expiryWords(gone, NOW)).toBe('Revoked 31 Oct 2026');
    expect(lastUsedWords(deck)).toBe('Last used 1 Nov 2026');
    expect(lastUsedWords(website)).toBe('Not used yet');
  });

  it('groups the scopes by what they touch, in the station’s order', () => {
    const g = scopeGroups(scopes);
    expect(g.map((x) => [x.group, x.scopes.map((s) => scopeAction(s.name))])).toEqual([
      ['carts', ['read', 'fire']],
      ['schedule', ['read']],
    ]);
  });
});

describe('what the form checks before it asks', () => {
  it('wants a name, and at least one scope', () => {
    expect(nameProblem('')).toContain('Name the token');
    expect(nameProblem('  ')).toContain('Name the token');
    expect(nameProblem('x'.repeat(81))).toContain('up to 80');
    expect(nameProblem('Station website')).toBeNull();
    expect(scopesProblem([])).toContain('at least one scope');
    expect(scopesProblem(['carts:read'])).toBeNull();
  });
});

describe('Settings > API tokens', () => {
  const props = { initial: { tokens: [gone, website, deck, old], scopes }, now: NOW };

  it('lists each token with what it may do, how it is doing and no secret', () => {
    const { body } = render(Tokens, { props });
    const text = body.replace(/<!--[^>]*-->/g, '');
    expect(body).toContain('data-panel="settings.tokens"');
    expect(PANELS['settings.tokens']).toBeTruthy();
    for (const t of [deck, website, old, gone]) {
      expect(text).toContain(t.name);
      expect(text).toContain(`${t.prefix}…`);
    }
    expect(text).toContain('2 tokens work');
    expect(text).toContain('Made by Morag Campbell');
    expect(body).toContain('>carts:fire</li>');
    expect(text).toContain('Working');
    expect(text).toContain('Expired');
    expect(text).toContain('Revoked');
    expect(text).toContain('Never expires');
    expect(text).toContain('Not used yet');
    // The order: working ones first.
    const order = [deck, website, old, gone].map((t) => body.indexOf(t.name));
    expect(order).toEqual([...order].sort((a, b) => a - b));
    // A working token can be revoked; a revoked one cannot.
    expect(body).toContain('Revoke…<span class="sr-only"> Stream deck</span>');
    expect(body).not.toContain('Revoke…<span class="sr-only"> Fraser’s test</span>');
    expect(body).not.toMatch(/rsa_[0-9a-f]{20}/);
  });

  it('says so when there are none, and what a token can never do', () => {
    const { body } = render(Tokens, { props: { initial: { tokens: [], scopes }, now: NOW } });
    const text = body.replace(/<!--[^>]*-->/g, '');
    expect(text).toContain('No tokens yet.');
    expect(text).toContain('never manage staff, retention, erasure, settings or other tokens');
  });

  it('the make form asks for a name, the scopes in groups and how long it lasts', () => {
    const { body } = render(Tokens, { props });
    expect(body).toMatch(/<label for="tk-name"[^>]*>Name<\/label>/);
    expect(body).toMatch(/<legend[^>]*>May<\/legend>/);
    expect(body.match(/type="checkbox"/g)?.length).toBe(scopes.length);
    expect(body).toContain('Fire, stop and cue carts.');
    for (const w of ['30 days', '90 days', 'A year', 'Never']) expect(body).toContain(w);
    // The secret box is there for the dialog, empty until a token is made.
    expect(body).toContain('This is the only time it is shown.');
    expect(body).toMatch(/id="tk-secret"[^>]*readonly/);
  });

  it('is an admin’s panel: Settings draws it for an admin and not for a presenter', () => {
    const admin = render(Settings, { props: { me: asAdmin, initial: null, initialHolds: null } });
    const other = render(Settings, { props: { me: asPresenter, initial: null, initialHolds: null } });
    expect(admin.body).toContain('data-panel="settings.tokens"');
    expect(other.body).not.toContain('data-panel="settings.tokens"');
  });
});
