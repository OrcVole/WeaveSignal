// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What the join page says before a person joins: who the link is for, where it leads, and what
// happens when they press join. Every invitation arrives standing by (1.3): nobody is on air
// the moment they join, in the live studio or after a move from the rehearsal. The rehearsal
// studio reaches no air, and its words say who hears it. Pure, so the words are tested.

import type { Role, Studio } from './api.ts';
import { clock } from './words.ts';
import type { Moved } from './move.ts';

/** "as a guest", "to present": what follows the show's name in the heading. */
export const AS: Record<string, string> = { guest: 'as a guest', dj: 'as a remote DJ', presenter: 'to present' };

/** The page's heading: "Join The Morning Kettle as a guest", "Rehearse Steeped to present". */
export function heading(show: string, role: Role, studio: Studio): string {
  const verb = studio === 'rehearsal' ? 'Rehearse' : 'Join';
  return `${verb} ${show} ${AS[role] ?? ''}`.trim();
}

/** The small label over the heading. */
export const caps = (studio: Studio): string => (studio === 'rehearsal' ? 'Rehearsal invitation' : 'Invitation');

/** The button that joins. */
export const joinWord = (studio: Studio): string => (studio === 'rehearsal' ? 'Join the rehearsal studio' : 'Join the studio');

/** What joining does, said once the seat is open. */
export function what(role: Role, studio: Studio): string {
  if (studio === 'rehearsal') {
    const only = 'This link leads to the rehearsal studio. Only the people in it hear you; nobody listening to the station can.';
    if (role === 'guest') return `${only} When the presenter moves everyone to the live studio, this page moves you with it, standing by.`;
    return `${only} When you move to the live studio, this page moves you with it, standing by: you are not on air until the handover.`;
  }
  switch (role) {
    case 'guest':
      return 'You join standing by: you hear the show, and you are not on air until the studio puts you on. Join when you are ready.';
    case 'presenter':
      return 'You join standing by: you are not on air until the handover. Your page says so in red when you are.';
    case 'dj':
      return 'You join standing by and hear the show. You go on air when the presenter hands over to you; your page says when.';
    default:
      return '';
  }
}

/** The line about how long the link lasts. A rehearsal link also ends with its rehearsal. */
export function lasts(untilMs: number, studio: Studio): string {
  const when = `This link works until ${clock(untilMs)}`;
  return studio === 'rehearsal' ? `${when}, while the rehearsal is open, and only for you: don't share it.` : `${when}, and only for you: don't share it.`;
}

/** A page opened from a rehearsal link after the move: the live pass it was handed, said once. */
export function movedHere(m: Moved, now: number): string {
  const due = m.airing_start_ms > now ? 'is due on air' : 'was due on air';
  return `The rehearsal for ${m.show} has moved to the live studio, which ${due} at ${clock(m.airing_start_ms)}. This page took your place there: you join standing by, and you are not on air until the handover.`;
}
