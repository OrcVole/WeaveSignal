// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The invite dialog's words (lib/invite.ts): the roles, the link, how long it lasts, a refusal.
import { describe, expect, it } from 'vitest';
import { FieldError } from './lib/api.ts';
import { INVITE_ROLES, inviteProblem, inviteUrl, joinsLine, lasts, lastsLine, nameProblem } from './lib/invite.ts';

describe('the invite dialog', () => {
  it('offers the DJ first, and says of each role whether it joins on air', () => {
    expect(INVITE_ROLES.map((r) => r.role)).toEqual(['dj', 'guest', 'presenter']);
    expect(INVITE_ROLES[0].what).toMatch(/off air/);
    expect(INVITE_ROLES[1].what).toMatch(/On air the moment they join/);
    expect(INVITE_ROLES[2].what).toMatch(/On air the moment they join/);
  });

  it('asks for a name of 120 characters at most', () => {
    expect(nameProblem('  ')).toMatch(/Enter their name/);
    expect(nameProblem('Finlay Grant')).toBeNull();
    expect(nameProblem('x'.repeat(121))).toMatch(/120/);
  });

  it('puts the station’s address before the link', () => {
    expect(inviteUrl('/join/#abc', 'https://kettle.example/')).toBe('https://kettle.example/join/#abc');
    expect(inviteUrl('https://other.example/join/#abc', 'https://kettle.example')).toBe('https://other.example/join/#abc');
  });

  it('says how long the link lasts', () => {
    expect(lasts(4 * 3600_000)).toBe('4 hours');
    expect(lasts(90 * 60_000)).toBe('1 hour 30 minutes');
    expect(lasts(45 * 60_000 - 2_000)).toBe('45 minutes');
    expect(lasts(10_000)).toBe('1 minute');
    const now = new Date(2026, 10, 10, 9, 50).getTime();
    expect(lastsLine({ expires_at_ms: now + 4 * 3600_000 }, now)).toBe('The link works for 4 hours, until 13:50.');
  });

  it('says where they arrive, by role', () => {
    expect(joinsLine({ role: 'dj', name: 'Finlay Grant', identity: 'dj-1' })).toMatch(/^Finlay opens it .* off air, under Not on air\.$/);
    expect(joinsLine({ role: 'guest', name: 'Gregor Stewart', identity: 'guest-1' })).toMatch(/on air the moment they join\.$/);
  });

  it('puts a refusal in words, never a bare status', () => {
    expect(inviteProblem(new FieldError(503, 'the room is not configured: set WEAVESIGNAL_LIVEKIT_API_SECRET', null))).toMatch(/^This station cannot create invitations yet\. It says: the room is not configured/);
    expect(inviteProblem(new FieldError(422, 'a name is at most 120 characters', null))).toBe('The station refused the invitation. It says: a name is at most 120 characters.');
    expect(inviteProblem(new FieldError(401, 'a valid bearer token is needed', null))).toMatch(/sign-in has ended/);
    expect(inviteProblem(new FieldError(500, '500 Internal Server Error', null))).toBe('The station could not create the invitation. Try again.');
    expect(inviteProblem(new FieldError(422, '422 Unprocessable Entity', null))).toBe('The station could not create the invitation. Try again.');
    expect(inviteProblem(new TypeError('Failed to fetch'))).toMatch(/did not answer/);
  });
});
