// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A caller's line goes bad (ADR-0222, ADR-0069): Iona is on air from Derry when her link
// degrades (LINK DEGRADED on her tile and in the status line, her voice in pieces), then drops
// (the desk keeps her 30 s, the queue holds her place 5 min), then comes back ready on the same
// strip. The timings are the scenario's; the rules are the control plane's.

import { U } from '../lines.ts';
import type { Station } from '../station.ts';
import { EILIDH, IONA } from './cast.ts';
import { k, rundownAt, type Scenario, type Step } from './scenario.ts';

export const LINK: Scenario = {
  id: 'link',
  title: 'A line goes bad',
  blurb: 'A caller’s link degrades on air, then drops: read LINK DEGRADED, take her off, and put her back when she returns.',
  minutes: 2,
  learn: [
    'LINK DEGRADED on a tile and in the status line: the person is in the room, but their audio is breaking up.',
    'A dropped link: the desk keeps the person 30 s, the queue holds their place 5 min, and they come back where they were.',
    'Take a rough line off air, and put it back when it is good.',
  ],
  you: EILIDH,
  show: 'The Morning Kettle',
  cast: [EILIDH],
  callers: { iona: IONA },
  rundown: (start) => {
    const at = rundownAt(start);
    return [
      { at: at(0), title: 'Phones', detail: 'Iona MacIntosh, from Derry', hard_post: false, key: null },
      { at: at(8), title: 'News', detail: 'Top of the hour', hard_post: true, key: null },
    ];
  },
  start: (s, now) => {
    s.arrive('iona', now - 50_000);
    s.screener({ op: 'notes', identity: 'iona', line: 'From Derry, on the road: the ferry fares.' }, now - 40_000);
    s.screener({ op: 'send', identity: 'iona' }, now - 20_000);
  },
  beats: [
    {
      id: 'iona-air',
      when: (s) => s.open('iona') && s.open('eilidh') && !s.has('degraded'),
      lines: [
        U('t-e-iona', 'eilidh', 'Iona MacIntosh is on the line from Derry. Iona, go ahead.'),
        U('t-iona-1', 'iona', "Morning, Eilidh. It's the ferry fares. They've gone up twice this year, the timetable's got worse, and if you miss the last one you're sleeping in the car."),
      ],
      after: (s, now) => {
        s.at(now + 500, (s, t) => {
          s.degrade('iona', true, t);
          s.mark('degraded');
        });
        s.at(now + 9_000, (s, t) => {
          s.dropLink('iona', t);
          s.mark('dropped');
        });
        s.at(now + 24_000, (s, t) => {
          s.returnLink('iona', t);
          s.mark('returned');
        });
      },
    },
    {
      id: 'iona-bad',
      when: (s) => s.degraded('iona') && s.open('iona'),
      lines: [U('t-iona-2', 'iona', "…can you hear me? Hello? The signal's awful on this stretch of road, I'll… the fares, I was saying…")],
    },
    {
      id: 'iona-back',
      when: (s) => s.has('returned') && s.open('iona'),
      lines: [
        U('t-iona-3', 'iona', "Sorry about that, Eilidh, I'm back. Where was I: the fares. Twice this year."),
        U('t-e-iona-bye', 'eilidh', 'Thanks, Iona. Safe home.'),
      ],
    },
  ],
  steps: (s: Station): Step[] => {
    const iona = s.person('iona');
    return [
      { id: 'on', say: ['Press ', k('1'), ', then ', k('4'), ': Iona is READY on strip 4, from Derry.'], done: s.done('iona-air') || s.has('degraded') },
      {
        id: 'degraded',
        say: ['Her line is breaking up: the tile says LINK DEGRADED and the status line names her. Tell listeners, then ', k('4'), ' takes her off.'],
        done: s.has('degraded') && (!iona?.on_air || s.has('returned')),
      },
      {
        id: 'dropped',
        say: ['Her link dropped: on the Screener her row says DROPPED and her place is held 5 min (on air, her tile would have said she left the room and is kept 30 s). Wait for her.'],
        done: s.has('returned'),
      },
      { id: 'back', say: ['Iona is back on strip 4, where she was: press ', k('4'), ' to put her back on.'], done: s.done('iona-back') || (s.has('returned') && !!iona?.on_air) },
      { id: 'off', say: [k('4'), ' takes her off when she is done. Her place was held; nothing was lost.'], done: s.done('iona-back') && !iona?.on_air },
    ];
  },
  did: () => [
    'You read LINK DEGRADED on Iona’s tile and in the status line, and took her off before listeners heard worse.',
    'Her link dropped: the desk kept her 30 s and the queue held her place 5 min, with a notice to the screener.',
    'She came back ready on the same strip, and you put her back on air.',
  ],
};
