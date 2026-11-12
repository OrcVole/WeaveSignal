// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Playout takes over and hands back (ADR-0222, ADR-0045, ADR-0183): the station starts in auto
// mode, automation holding the air; the lamp says AUTO and anything you open says OPEN, the
// white lamp. The mode is a playout setting on a real station (Settings, or the on-air API),
// so the guide's buttons stand in for it. A Solo scenario: nobody else is in the room (the next
// show's DJ is a Full seat, and has the two handover scenarios).

import { L, U } from '../lines.ts';
import type { Station } from '../station.ts';
import { EILIDH } from './cast.ts';
import { k, rundownAt, type Scenario, type Step } from './scenario.ts';

export const AUTO: Scenario = {
  id: 'auto',
  title: 'Playout takes over',
  blurb: 'The station starts in auto mode: see AUTO and OPEN, take it live, hand the air back to automation.',
  minutes: 2,
  step: 'solo',
  learn: [
    'AUTO: automation holds the air, listeners hear the log, and your On key opens you to the room only (OPEN, the white lamp).',
    'Taking the station live is a playout setting, not a desk key; then open faders say ON AIR.',
    'Hand back: take yourself off (STANDBY), then put the station back in auto.',
  ],
  you: EILIDH,
  show: 'The Morning Kettle',
  cast: [EILIDH],
  callers: {},
  mode: 'auto',
  rundown: (start) => {
    const at = rundownAt(start);
    return [
      { at: at(0), title: 'Overnight log', detail: 'Automation holds the air', hard_post: false, key: null },
      { at: at(2), title: 'Live', detail: 'Eilidh takes the station live', hard_post: false, key: null },
      { at: at(5), title: 'Back to automation', detail: 'The log takes the air again', hard_post: false, key: null },
    ];
  },
  beats: [
    {
      id: 'room',
      when: (s) => s.open('eilidh') && s.mode === 'auto',
      lines: [U('t-auto-room', 'eilidh', "Morning. If you can hear this, you're in the room with me: listeners are still on the overnight log.")],
    },
    {
      id: 'live',
      when: (s) => s.open('eilidh') && s.mode === 'live',
      lines: [
        L('l-e-back', 'eilidh', 1.16, "And we're back."),
        U('t-auto-live', 'eilidh', "That's the overnight done. Kettle FM is live, the kettle's on, and Steeped follows after the news."),
      ],
    },
    {
      id: 'handed-back',
      when: (s) => s.done('live') && !s.open('eilidh') && s.mode === 'auto',
      lines: [],
      after: (s) => s.mark('handed-back'),
    },
  ],
  steps: (s: Station, g): Step[] => [
    { id: 'open', say: ['The lamp says AUTO: automation holds the air and listeners hear the log. Press ', k('1'), ': your tile says OPEN, the white lamp, not ON AIR.'], done: !!s.person('eilidh')?.on_air || s.done('room') || s.mode === 'live' },
    { id: 'listen', say: ['Switch Hear to Listeners: you are not there, the log is. Then back to Studio.'], done: g.listened },
    {
      id: 'live',
      say: ['Take the station live. On a real station this is the playout setting (Settings, or the on-air API), not a desk key; here, the button. The lamp turns ON AIR.'],
      done: s.mode === 'live' || s.done('live') || s.has('handed-back'),
      action: { label: 'Take the station live', run: 'go-live' },
    },
    {
      id: 'back',
      say: ['Hand back: press ', k('1'), ' to take yourself off (STANDBY, playout holds the air), then put the station back in auto for the night.'],
      done: s.has('handed-back'),
      action: { label: 'Back to auto', run: 'go-auto' },
    },
  ],
  did: () => [
    'You opened a fader in auto mode and saw OPEN, the white lamp: the room heard you, listeners heard the log.',
    'You took the station live (a playout setting, stood in for by the button) and the lamp turned ON AIR.',
    'You took yourself off and handed the air back to automation.',
  ],
};
