// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Banned before (ADR-0222, ADR-0069): you are Shona. A caller rings from an account the station
// banned; the queue marks him, with the date and reason, beside his state. Hear him out, send
// him, see the presenter decline (DESK BUSY beside BANNED BEFORE), drop him; he rings again,
// still marked, because the ban is on the account.

import { U } from '../lines.ts';
import type { Station } from '../station.ts';
import { EILIDH, SHONA, TAVISH } from './cast.ts';
import { k, rundownAt, type Scenario, type Step } from './scenario.ts';

export const BANNED: Scenario = {
  id: 'banned',
  title: 'Banned before',
  blurb: 'A caller rings from an account the station banned: read the mark, hear him out, see the desk decline, drop him, and see him come back marked.',
  minutes: 2,
  learn: [
    'What BANNED BEFORE means, where the date and reason show, and that nothing is on air yet.',
    'The mark goes with the caller to the desk, and comes back with him when he rings again.',
    'Drop with Ctrl+D twice.',
  ],
  you: SHONA,
  show: 'The Morning Kettle',
  cast: [EILIDH, SHONA],
  callers: { tavish: TAVISH },
  bans: { tavish: { date: '2026-09-12', reason: 'Abuse on air' } },
  rundown: (start) => {
    const at = rundownAt(start);
    return [
      { at: at(0), title: 'Phones', detail: 'Shona screens', hard_post: false, key: null },
      { at: at(8), title: 'News', detail: 'Top of the hour', hard_post: true, key: null },
    ];
  },
  start: (s, now) => {
    s.desk({ op: 'on_air', identity: 'eilidh', on: true }, now);
    s.ring('tavish', now + 1_000);
  },
  beats: [
    {
      id: 'tavish-booth',
      when: (s) => s.person('shona')?.talk_to === 'tavish',
      lines: [U('t-tavish-1', 'tavish', "It's Tavish. Aye, I know what it says on your screen. That was months ago. I just want a word about the ferries.", 'booth')],
    },
    {
      id: 'tavish-declined',
      when: (s) => s.callerState('tavish') === 'ready' && !s.callerRec('tavish')?.declined,
      lines: [],
      after: (s, now) => s.at(now + 1_500, (s, t) => s.callerState('tavish') === 'ready' && s.screener({ op: 'decline', identity: 'tavish', reason: 'banned before' }, t)),
    },
    {
      id: 'tavish-again',
      when: (s) => (s.drops.tavish ?? 0) >= 1 && !s.person('tavish'),
      lines: [],
      after: (s, now) => {
        s.at(now + 6_000, (s, t) => s.arrive('tavish', t));
        s.mark('rings-again');
      },
    },
    {
      id: 'tavish-booth-2',
      when: (s) => s.has('rings-again') && s.person('shona')?.talk_to === 'tavish',
      lines: [U('t-tavish-2', 'tavish', "Me again. Come on, Shona, it's one word about the ferries.", 'booth')],
    },
  ],
  steps: (s: Station): Step[] => {
    const drops = s.drops.tavish ?? 0;
    return [
      {
        id: 'read',
        say: ['A caller rang, marked BANNED BEFORE with the date and reason: the account, not this call. Press ', k('1'), ' to pick him and hold ', k('T'), ' to hear him out. Nothing is on air.'],
        done: s.done('tavish-booth') || s.callerState('tavish') === 'ready' || drops > 0,
      },
      { id: 'send', say: ['Send him anyway: ', k('P'), '. The presenter sees the mark too.'], done: !!s.callerRec('tavish')?.declined || drops > 0 },
      { id: 'drop', say: ['Eilidh declined him: DESK BUSY beside BANNED BEFORE. ', k('Ctrl+D'), ' twice drops him.'], done: drops > 0 },
      {
        id: 'again',
        say: ['He rang again, still marked: the ban is on the account and comes back with him. Pick him and drop him again, or leave him waiting; a marked caller reaches air only if you send him.'],
        done: drops > 1,
      },
    ];
  },
  did: (s) => [
    'You read BANNED BEFORE in the queue: the date and reason of the ban, beside the caller’s state.',
    'You sent him and the presenter declined: DESK BUSY sat beside the mark.',
    `You dropped him ${(s.drops.tavish ?? 0) === 1 ? 'once' : `${s.drops.tavish ?? 0} times`}; the mark came back with him when he rang again.`,
  ],
};
