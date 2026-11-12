// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Running everything alone (ADR-0222): you are the DJ and the call screener. The show runs at
// the Callers step (ADR-0271), so there is no Screener tab and no screener's seat: the callers
// wait on the desk, in `desk.callers`, and you screen from the desk: take the caller who has
// waited longest to strip 4 (P, ADR-0275), cue them, talk to them on the strip, then open them.
// No DJ follows (the DJ handover, ADR-0074, is a Full part: `handover.ts`), so handing over to
// playout is taking yourself off: STANDBY.

import { L, U } from '../lines.ts';
import type { Station } from '../station.ts';
import { EILIDH, JESSIE, NIALL } from './cast.ts';
import { k, rundownAt, type Scenario, type Step } from './scenario.ts';

export const ALONE: Scenario = {
  id: 'alone',
  title: 'Running everything alone',
  blurb: 'You are the DJ and the screener: answer a caller from the desk, cue and talk to them, take them to air, play a cart, hand back to playout.',
  minutes: 4,
  step: 'callers',
  learn: [
    'How a DJ alone answers the phones: take the caller who has waited longest to strip 4, cue them, talk to them on the strip, then open them.',
    'At the Callers step the queue is on the desk, in the Callers panel: there is no Screener tab to go to.',
    'Play a cart from any tab, and hand the air back to playout by taking yourself off (STANDBY).',
  ],
  you: EILIDH,
  show: 'The Morning Kettle',
  cast: [EILIDH],
  callers: { niall: NIALL, jessie: JESSIE },
  rundown: (start) => {
    const at = rundownAt(start);
    return [
      { at: at(0), title: 'Open and jingle', detail: 'Eilidh alone on the desk', hard_post: false, key: null },
      { at: at(1), title: 'Phones', detail: 'No screener today: Eilidh answers from the desk', hard_post: false, key: null },
      { at: at(5), title: 'Hand over to playout', detail: 'STANDBY; the log takes the air', hard_post: false, key: null },
      { at: at(8), title: 'News', detail: 'Top of the hour', hard_post: true, key: null },
    ];
  },
  beats: [
    {
      id: 'open',
      when: (s) => s.open('eilidh') && !s.shotOnAir(),
      lines: [
        L('l-open', 'eilidh', 8.74, "Good morning, and welcome to The Morning Kettle on Kettle FM. I'm Eilidh Fraser-MacLeod. The kettle's on, the phones are open, and this morning it's all about Leith Walk."),
        U('t-alone-phones', 'eilidh', "It's just me on the desk this morning, so bear with me while I answer the phones myself."),
      ],
      after: (s, now) => s.ring('niall', now + 800),
    },
    {
      id: 'niall-cue',
      when: (s) => s.person('niall')?.cue === true && !s.open('niall'),
      lines: [
        L('l-n1', 'niall', 3.6, "Niall Anderson, from Leith. It's about the bus lanes on Leith Walk.", 'booth'),
        L('l-n2', 'niall', 2.52, "Three weeks running. Nobody's let me on yet.", 'booth'),
      ],
    },
    {
      id: 'niall-talk',
      when: (s) => s.person('eilidh')?.talk_to === 'niall' && !s.open('niall'),
      lines: [
        L('l-e-cue', 'eilidh', 2.12, "Niall, it's Eilidh. You're on in a moment.", 'booth'),
        L('l-n-cue', 'niall', 1.02, 'Right you are.', 'booth'),
      ],
    },
    {
      id: 'niall-air',
      when: (s) => s.open('niall') && s.open('eilidh'),
      lines: [
        L('l-e-n', 'eilidh', 4.13, "Niall Anderson in Leith, good morning. You've been trying to get through for a while."),
        L('l-n-air', 'niall', 8.27, 'Three weeks, Eilidh. The bus lanes on Leith Walk stop right where the buses need them most. At half past eight you can walk into town faster than the bus.'),
        L('l-e-bye', 'eilidh', 1.88, 'Thanks, Niall. Keep ringing.'),
      ],
      after: (s, now) => s.ring('jessie', now + 3_000),
    },
    {
      id: 'standby',
      when: (s) => s.done('niall-air') && !s.open('niall') && !s.open('eilidh') && s.fired.includes('applause-1'),
      lines: [],
      after: (s) => s.mark('handed-over'),
    },
  ],
  steps: (s: Station): Step[] => {
    const niall = s.callerState('niall');
    const n = s.person('niall');
    return [
      { id: 'on', say: ['You are alone on the desk. Press ', k('1'), ' to go on air, then ', k('F1'), ' for the jingle: Eilidh opens the show.'], done: s.done('open') || !!s.beat?.beat.id.startsWith('open') },
      {
        id: 'take',
        say: ['Niall is ringing: he waits in Callers, on the right, with what he typed. Press ', k('P'), ', or Take Niall to strip 4: he joins strip 4, off air.'],
        done: niall === 'ready' || !!n?.on_air || !!n?.cue || s.done('niall-cue') || s.done('niall-air'),
      },
      { id: 'cue', say: ['Niall is READY on strip 4, under Not on air. Press ', k('R'), ' to cue him: you hear him, listeners do not.'], done: !!n?.cue || !!n?.on_air || s.done('niall-cue') },
      { id: 'talk', say: ['Hold ', k('F'), ' to talk to Niall on his strip: he hears you, listeners do not. Tell him he is on next.'], done: s.done('niall-talk') || !!n?.on_air },
      { id: 'air', say: ['Press ', k('4'), ': Niall is on air.'], done: s.done('niall-air') || !!n?.on_air },
      { id: 'off', say: ['When he has finished, ', k('4'), ' takes him off. ', k('Ctrl+D'), ' twice would drop him from the desk; his strip is freed either way.'], done: s.done('niall-air') && !s.open('niall') },
      { id: 'cart', say: ['A cart from any tab: ', k('F3'), ' is applause, over the top of whatever is open.'], done: s.fired.includes('applause-1') },
      {
        id: 'handover',
        say: ['Hand the air back to playout: press ', k('1'), ' to take yourself off. The lamp says STANDBY and listeners hear playout’s log; nobody follows you today, so this is how the show ends. (Handing over to a next DJ comes with the Full step.)'],
        done: s.has('handed-over'),
      },
    ];
  },
  did: (s) => [
    'You ran the desk and the phones alone: you took Niall from the Callers panel to strip 4, with no screener and no Screener tab.',
    'You cued him and talked to him on his strip before opening him, so listeners heard him only once you chose.',
    `You fired ${s.fired.length} cart${s.fired.length === 1 ? '' : 's'} from the keys, from whatever tab you were on.`,
    'You handed the air back to playout by taking yourself off: STANDBY, and the log carried on.',
    s.callerState('jessie') ? 'Jessie rang while you were busy; she is still waiting in Callers, as a real caller would be.' : 'Nobody was left waiting.',
  ],
};
