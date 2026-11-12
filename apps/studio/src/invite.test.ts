// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The invitation dialog's words (lib/invite.ts): the roles and which the show's step offers,
// the link, how long it lasts, a refusal, and the Solo DJ's own link.
import { describe, expect, it } from 'vitest';
import { FieldError } from './lib/api.ts';
import { DJ_COMES_WITH_FULL, INVITE_ROLES, OWN_LINK, inviteProblem, inviteUrl, joinsLine, lasts, lastsLine, nameProblem, rolesAt } from './lib/invite.ts';

describe('the invitation dialog', () => {
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
    expect(inviteProblem(new FieldError(401, 'a valid bearer token is needed', null))).toMatch(/sign-in has ended.*then create the invitation\.$/);
    expect(inviteProblem(new FieldError(403, 'this route is for staff', null))).toBe('Your account may not create invitations. Ask a station admin.');
    expect(inviteProblem(new FieldError(500, '500 Internal Server Error', null))).toBe('The station could not create the invitation. Try again.');
    expect(inviteProblem(new FieldError(422, '422 Unprocessable Entity', null))).toBe('The station could not create the invitation. Try again.');
    expect(inviteProblem(new TypeError('Failed to fetch'))).toMatch(/did not answer/);
  });

  it('shows the station’s own sentence when a step refuses the seat, and never another', () => {
    const guests = 'This show runs at Solo, so it has no seat for a guest or a second presenter. They come with Guests.';
    expect(inviteProblem(new FieldError(422, guests, 'role'))).toBe(guests);
    const dj = "An invitation for the next show's DJ comes with Full.";
    expect(inviteProblem(new FieldError(422, dj, 'role'))).toBe(dj);
    // The Solo DJ's own link, refused for an account that is not the show's presenter (403).
    const own = 'While this show runs at Solo, only its presenter makes their own microphone link. Ask an admin to name you its presenter, or move the show up a step.';
    expect(inviteProblem(new FieldError(403, own, null))).toBe(own);
  });

  it('offers the roles the show’s step has: no DJ below Full, and the line that says where the DJ comes', () => {
    expect(rolesAt('full').map((r) => r.role)).toEqual(['dj', 'guest', 'presenter']);
    expect(rolesAt('callers').map((r) => r.role)).toEqual(['guest', 'presenter']);
    expect(rolesAt('guests').map((r) => r.role)).toEqual(['guest', 'presenter']);
    expect(DJ_COMES_WITH_FULL).toBe('An invitation for the next show’s DJ comes with Full.');
  });

  it('words the Solo DJ’s own link as the design does', () => {
    expect(OWN_LINK.button).toBe('Join with my microphone…');
    expect(OWN_LINK.title).toBe('Your microphone link');
    expect(OWN_LINK.lede).toBe('Open it in a browser on this computer and press Join the studio. You are on air the moment you join.');
    expect(OWN_LINK.open).toBe('Open in browser');
  });
});
