// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A busy queue (ADR-0222): you are Shona, the screener, and four callers ring at once. Send one,
// see the presenter decline the next (DESK BUSY), hold one for later (back to the queue with a
// note), drop one, and send the declined one again when the desk is free. Eilidh, at the desk,
// is simulated: she takes a ready caller after a moment, and declines one while she is busy,
// as the real desk's Esc on the ready toast does.

import { L, U } from '../lines.ts';
import type { Station } from '../station.ts';
import { ALASTAIR, DOUGAL, EILIDH, FLORA, NIALL, SHONA } from './cast.ts';
import { k, rundownAt, type Scenario, type Step } from './scenario.ts';

export const QUEUE: Scenario = {
  id: 'queue',
  title: 'A busy queue',
  blurb: 'You are the screener and four callers ring within seconds: send, see the desk decline, hold one, drop one, send again.',
  minutes: 4,
  step: 'full',
  learn: [
    'Pick a caller by number, talk to them off air, and send them to the desk.',
    'What DESK BUSY means: the presenter declined, the caller stays ready, you send again at the next gap.',
    'Hold a caller for later with a note, and drop one who should not reach air.',
  ],
  you: SHONA,
  show: 'The Morning Kettle',
  cast: [EILIDH, SHONA],
  callers: { niall: NIALL, dougal: DOUGAL, flora: FLORA, alastair: ALASTAIR },
  rundown: (start) => {
    const at = rundownAt(start);
    return [
      { at: at(0), title: 'Phones', detail: 'Shona screens; Eilidh takes them one at a time', hard_post: false, key: null },
      { at: at(8), title: 'News', detail: 'Top of the hour', hard_post: true, key: null },
    ];
  },
  start: (s, now) => {
    s.desk({ op: 'on_air', identity: 'eilidh', on: true }, now);
    s.ring('niall', now + 1_000);
    s.ring('dougal', now + 2_500);
    s.ring('flora', now + 4_000);
    s.ring('alastair', now + 5_500);
  },
  beats: [
    {
      id: 'niall-booth',
      when: (s) => s.person('shona')?.talk_to === 'niall',
      lines: [
        L('l-s1', 'shona', 2.8, "Kettle FM, you're through to Shona. Who's this?", 'booth'),
        L('l-n1', 'niall', 3.6, "Niall Anderson, from Leith. It's about the bus lanes on Leith Walk.", 'booth'),
        L('l-s3', 'shona', 3.28, "You're on next. Keep it short, and turn your radio down.", 'booth'),
        L('l-n3', 'niall', 1.76, 'Will do. Cheers, Shona.', 'booth'),
      ],
    },
    {
      // The presenter takes a ready caller after a moment.
      id: 'niall-taken',
      when: (s) => s.callerState('niall') === 'ready' && !s.callerRec('niall')?.declined,
      lines: [],
      after: (s, now) => s.at(now + 2_500, (s, t) => s.person('niall') && s.desk({ op: 'on_air', identity: 'niall', on: true }, t)),
    },
    {
      id: 'niall-air',
      when: (s) => s.open('niall'),
      lines: [
        L('l-e-n', 'eilidh', 4.13, "Niall Anderson in Leith, good morning. You've been trying to get through for a while."),
        L('l-n-air', 'niall', 8.27, 'Three weeks, Eilidh. The bus lanes on Leith Walk stop right where the buses need them most. At half past eight you can walk into town faster than the bus.'),
        L('l-e-bye', 'eilidh', 1.88, 'Thanks, Niall. Keep ringing.'),
      ],
      after: (s, now) => s.at(now + 800, (s, t) => s.person('niall') && s.desk({ op: 'on_air', identity: 'niall', on: false }, t)),
    },
    {
      id: 'dougal-booth',
      when: (s) => s.person('shona')?.talk_to === 'dougal',
      lines: [
        U('t-dougal-1', 'dougal', "Dougal Thomson, Portobello. It's the bins. Three weeks now and nobody's lifted them, and the gulls have found out.", 'booth'),
        U('t-dougal-2', 'dougal', "Aye, I'll keep it short. I've been trying since the news.", 'booth'),
      ],
    },
    {
      // Sent while Niall is on air: the presenter declines, as Esc on the ready toast does.
      id: 'dougal-declined',
      when: (s) => s.callerState('dougal') === 'ready' && !s.callerRec('dougal')?.declined && s.open('niall'),
      lines: [],
      after: (s, now) => s.at(now + 1_500, (s, t) => s.callerState('dougal') === 'ready' && s.open('niall') && s.screener({ op: 'decline', identity: 'dougal', reason: 'not now' }, t)),
    },
    {
      id: 'dougal-taken',
      when: (s) => s.callerState('dougal') === 'ready' && !s.callerRec('dougal')?.declined && s.done('niall-air') && !s.open('niall'),
      lines: [],
      after: (s, now) => s.at(now + 2_000, (s, t) => s.person('dougal') && s.desk({ op: 'on_air', identity: 'dougal', on: true }, t)),
    },
    {
      id: 'dougal-air',
      when: (s) => s.open('dougal'),
      lines: [
        U('t-e-dougal', 'eilidh', 'Dougal Thomson in Portobello. Dougal, the bins.'),
        U('t-dougal-air', 'dougal', "Three weeks, Eilidh. Three weeks of bags on the pavement, and the gulls having a festival. I've rung the council, I've rung the councillor, and now I'm ringing you."),
        U('t-e-dougal-bye', 'eilidh', "We'll ask them. Thanks, Dougal."),
      ],
      after: (s, now) => s.at(now + 800, (s, t) => s.person('dougal') && s.desk({ op: 'on_air', identity: 'dougal', on: false }, t)),
    },
    {
      id: 'flora-booth',
      when: (s) => s.person('shona')?.talk_to === 'flora',
      lines: [U('t-flora-1', 'flora', "Flora Armstrong, Musselburgh. Och, I've no question really, Shona. I just love the show and I wanted to say so.", 'booth')],
    },
    {
      id: 'alastair-booth',
      when: (s) => s.person('shona')?.talk_to === 'alastair',
      lines: [U('t-alastair-1', 'alastair', "Alastair. I've a thing or two to say about that presenter of yours, and I'll be saying them on air, so just put me through.", 'booth')],
    },
  ],
  steps: (s: Station): Step[] => {
    const dougal = s.callerRec('dougal');
    const flora = s.callerRec('flora');
    const ord = (id: string) => String(s.callers.findIndex((c) => c.identity === id) + 1 || '?');
    return [
      {
        id: 'niall',
        say: ['Four callers rang within seconds. Press ', k('1'), ' to pick Niall and hold ', k('T'), ' to answer him. Then ', k('P'), ' sends him to the desk: Eilidh takes him.'],
        done: s.callerState('niall') === 'ready' || !!s.person('niall')?.on_air || s.done('niall-air'),
      },
      {
        id: 'dougal',
        say: ['Eilidh is on with Niall. Meanwhile press ', k(ord('dougal')), ' for Dougal, hold ', k('T'), ', then ', k('P'), ' sends him too.'],
        done: !!dougal?.declined || s.done('dougal-declined') || s.done('dougal-air') || !!s.person('dougal')?.on_air,
      },
      {
        id: 'flora',
        say: ['Eilidh declined him: DESK BUSY. Dougal stays ready and you send again when the desk is free. Now press ', k(ord('flora')), ' for Flora and hold ', k('T'), '.'],
        done: s.done('flora-booth') || s.callerState('flora') === 'screening' || s.backs > 0,
      },
      {
        id: 'hold',
        say: ['Flora is just saying hello: keep her for a quiet moment. Write a note Eilidh will see, then ', k('W'), ' puts her back in the queue.'],
        done: s.backs > 0 || !!flora?.note,
      },
      {
        id: 'drop',
        say: ['Press ', k(ord('alastair')), ' for Alastair and hold ', k('T'), '. He is here to abuse the presenter: ', k('Ctrl+D'), ' twice drops him. He can ring again; a banned account would come back marked.'],
        done: (s.drops.alastair ?? 0) > 0,
      },
      {
        id: 'again',
        say: ['Niall is off air. Press ', k(ord('dougal')), ', then ', k('P'), ' sends Dougal again: Eilidh takes him.'],
        done: s.done('dougal-air') || !!s.person('dougal')?.on_air,
      },
    ];
  },
  did: (s) => [
    'You screened four callers by number, talking to each off air: listeners heard none of it.',
    'You sent Niall and Eilidh took him; you sent Dougal while she was busy and saw DESK BUSY, then sent him again at the gap.',
    `You held Flora for later${s.callerRec('flora')?.note ? ' with a note for Eilidh' : ''}, and dropped Alastair before he reached air.`,
    'The desk takes one caller at a time; the queue is yours to order.',
  ],
};
