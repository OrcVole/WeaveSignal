// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A caller who must be dumped (ADR-0222): Jessie is already on your strip (the show runs at the
// Callers step, ADR-0271: you took her call from the desk; Dump comes on at this step). Open her, hear
// the swear, dump the profanity delay, and hear as a listener that it never aired (ADR-0166).

import { L } from '../lines.ts';
import type { Station } from '../station.ts';
import { EILIDH, JESSIE } from './cast.ts';
import { k, rundownAt, type Scenario, type Step } from './scenario.ts';

export const DUMP: Scenario = {
  id: 'dump',
  title: 'The dump',
  blurb: 'A caller swears on air: dump the profanity delay in time, and hear as a listener that it never went out.',
  minutes: 2,
  step: 'callers',
  learn: [
    'Where the profanity delay sits and how many seconds it holds.',
    'Dump with Delete pressed twice, and what the DUMPED chip means while the delay rebuilds.',
    'Hear the difference between the studio and the listeners.',
  ],
  you: EILIDH,
  show: 'The Morning Kettle',
  cast: [EILIDH],
  callers: { jessie: JESSIE },
  rundown: (start) => {
    const at = rundownAt(start);
    return [
      { at: at(0), title: 'Phones', detail: 'Jessie Brown, the night bus', hard_post: false, key: null },
      { at: at(8), title: 'News', detail: 'Top of the hour', hard_post: true, key: null },
    ];
  },
  start: (s, now) => {
    s.arrive('jessie', now - 40_000);
    s.screener({ op: 'notes', identity: 'jessie', line: 'A nurse on nights: no bus home after ten past eleven.' }, now - 30_000);
    s.screener({ op: 'send', identity: 'jessie' }, now - 20_000);
  },
  beats: [
    {
      id: 'jessie-air',
      when: (s) => s.open('jessie') && s.open('eilidh'),
      lines: [
        L('l-e-j', 'eilidh', 3.47, 'Jessie Brown is on the line from the Infirmary. Jessie, go ahead.'),
        L('l-j', 'jessie', 7.53, "Eilidh, I'm a nurse on nights, and there's no bus home after ten past eleven. It's a bloody disgrace, and whoever planned it is a total arse."),
      ],
      after: (s, now) => {
        s.swore = now;
      },
    },
    {
      id: 'sorry',
      when: (s) => s.swore !== null && (s.delay.dumps > 0 || s.clock - s.swore > 4_000),
      lines: [L('l-e-sorry', 'eilidh', 5.36, "Jessie, I'll stop you there. Sorry about the language, folks. You never heard it, and that's the point.")],
    },
  ],
  steps: (s: Station, g): Step[] => [
    { id: 'on', say: ['Press ', k('1'), ': you are on air. Jessie is READY on strip 4: you took her call, and her line is on her tile. The delay beside the lamp holds 8 s.'], done: !!s.person('eilidh')?.on_air || s.done('jessie-air') },
    { id: 'air', say: [k('4'), ' puts Jessie on air. She is about to swear: the moment you hear it, press ', k('Delete'), ' twice.'], done: s.delay.dumps > 0 },
    {
      id: 'listen',
      say: ['Switch Hear to Listeners. They were 8 s behind, and the dump threw those seconds away. The chip says DUMPED while the delay rebuilds; it holds a little again at once (ADR-0166).'],
      done: g.heardListenersAfterDump,
    },
    { id: 'off', say: ['Apologise, then ', k('4'), ' takes Jessie off.'], done: g.heardListenersAfterDump && !s.open('jessie') && s.done('sorry') },
  ],
  did: (s) => [
    `You dumped the delay ${s.delay.dumps === 1 ? 'once' : `${s.delay.dumps} times`}: Delete twice, within three seconds.`,
    'The studio heard the swear; the listeners never did, because they were 8 s behind and those seconds were thrown away.',
    'The delay rebuilt on its own, and Dump worked again as soon as it held anything.',
  ],
};
