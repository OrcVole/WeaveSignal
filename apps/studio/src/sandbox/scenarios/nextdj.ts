// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Set up the next DJ's handover (ADR-0222, ADR-0074): Eilidh is on air and Finlay, who has the
// next show, is not in the room yet. She makes his invite from the desk (Invite someone…, the
// role DJ), he joins off air on strip 5, she cues him for a level and agrees the plan on
// talkback, and he stands by for the
// post. The schedule's half (two shows back to back, each with its presenter and takedown
// contact, ADR-0214) is the Schedule tab of a real station; the sandbox has no schedule, so the
// cards say it. Doing the handover itself is `handover.ts`.

import { L, U } from '../lines.ts';
import type { Station } from '../station.ts';
import { HANDOVER_KEY } from '../../lib/handover.ts';
import { EILIDH, FINLAY } from './cast.ts';
import { k, rundownAt, type Scenario, type Step } from './scenario.ts';

export const NEXT_DJ: Scenario = {
  id: 'next-dj',
  title: 'Set up the next DJ',
  blurb: 'The next show is another DJ’s: make them an invitation from the desk, hear their level in your headphones, agree the handover off air.',
  minutes: 3,
  step: 'full',
  learn: [
    'Before the day: both shows are in the schedule back to back, each with its DJ’s name and takedown contact, so the call page’s words change at the post by themselves. That is the Schedule tab of a real station; the sandbox has none.',
    'The next DJ joins early, from an invitation you create at the desk: Invite someone…, beside Not on air, with the role DJ. A DJ arrives off air, on strip 5; a guest or a presenter would be on air the moment they joined.',
    'Cue them to hear their level, and talk to them off air to agree when you bring them on. Then they stand by.',
  ],
  you: EILIDH,
  show: 'The Morning Kettle',
  cast: [EILIDH, FINLAY],
  later: ['finlay'],
  callers: {},
  rundown: (start) => {
    const at = rundownAt(start);
    return [
      { at: at(0), title: 'Last link', detail: 'Eilidh, the end of The Morning Kettle', hard_post: false, key: null },
      { at: at(6), title: 'Handover', detail: 'Finlay on air beside Eilidh', hard_post: false, key: HANDOVER_KEY },
      { at: at(8), title: 'Steeped with Finlay Grant', detail: 'The next show', hard_post: true, key: null },
    ];
  },
  beats: [
    {
      id: 'on',
      when: (s) => s.open('eilidh'),
      lines: [U('t-nd-on', 'eilidh', "It's nearly ten on Kettle FM. Finlay Grant has Steeped after this, and he'll be joining me in a wee while.")],
    },
    {
      id: 'level',
      when: (s) => s.person('finlay')?.cue === true,
      lines: [
        L('i-finlay', 'finlay', 2.71, "I'm Finlay. I'm on after Eilidh, standing by from home."),
        U('t-nd-level', 'finlay', 'One, two. One, two. Is that loud enough, or am I too close to it?', 'air', true),
      ],
    },
    {
      id: 'plan',
      when: (s) => s.person('eilidh')?.talk_to === 'finlay',
      lines: [
        U('t-nd-plan-e', 'eilidh', "Finlay, it's Eilidh. You sound good. I'll bring you on at five to, we'll have a wee chat, then it's yours.", 'booth'),
        U('t-nd-plan-f', 'finlay', "Five to. Right you are. I'll have my first record ready.", 'booth'),
      ],
    },
    {
      id: 'standing-by',
      when: (s) => s.done('level') && s.done('plan') && !!s.person('finlay') && !s.person('finlay')?.cue && !s.person('finlay')?.on_air,
      lines: [],
      after: (s) => s.mark('standing-by'),
    },
  ],
  steps: (s: Station): Step[] => {
    const finlay = s.person('finlay');
    return [
      { id: 'open', say: ['You are on air in The Morning Kettle; Steeped, Finlay’s show, follows at the post. Press ', k('1'), ' to open your fader.'], done: !!s.person('eilidh')?.on_air || s.done('on') },
      {
        id: 'invite',
        say: ['Press Invite someone…, at the right of the Not on air line. Type Finlay Grant, keep the role DJ, press Create invitation. A DJ joins off air: listeners hear nothing of him until you put him on.'],
        done: s.has('invited:finlay') || !!finlay,
      },
      {
        id: 'joined',
        say: ['That link is what you would send him; Copy link copies it. Here he opens it himself: press ', k('Esc'), ' to close, and wait for him under Not on air, on strip 5: STANDBY.'],
        done: !!finlay,
      },
      { id: 'cue', say: ['Press ', k('T'), ' to cue him: he is in your headphones, not on air. Is his level like yours?'], done: s.done('level') },
      { id: 'plan', say: ['Hold ', k('G'), ' to talk to him off air and agree the plan: only he hears you while it is held.'], done: s.done('plan') },
      { id: 'standby', say: ['Press ', k('T'), ' again to take him out of your cue. He stands by on strip 5 until the handover. The button now on his tile, Bring Finlay on air, is its first press: that is the next scenario.'], done: s.has('standing-by') },
    ];
  },
  did: () => [
    'You made Finlay an invitation at the desk with the role DJ; he joined off air on strip 5, and listeners heard nothing of him.',
    'You cued him to hear his level, and agreed the handover with him off air.',
    'He stands by for the post. Hand over to the next DJ is the scenario that follows: bringing him on and leaving him the air.',
  ],
};
