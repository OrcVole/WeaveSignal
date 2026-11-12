// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Your own microphone (ADR-0222): two mutes that look alike to listeners. The browser tab you
// speak through has its own microphone (the studio app does not join the room, ADR-0013): off,
// the desk says ON AIR and your meter stays flat. The desk's own mute (Z) says MUTED on your
// tile and STANDBY on the lamp. The meter is the tell.

import { L } from '../lines.ts';
import type { Station } from '../station.ts';
import { EILIDH } from './cast.ts';
import { k, rundownAt, type Scenario, type Step } from './scenario.ts';

export const MIC: Scenario = {
  id: 'mic',
  title: 'Your own microphone',
  blurb: 'ON AIR but silent: the tab’s microphone is off. Then the desk’s own mute, and how to tell the two apart.',
  minutes: 1,
  learn: [
    'Your voice reaches the desk through a browser tab: its microphone is not the desk’s mute.',
    'A flat meter under ON AIR means nothing is reaching the desk; look at the tab first.',
    'The desk’s mute (Z): MUTED on your tile, STANDBY on the lamp when nothing else is open.',
  ],
  you: EILIDH,
  show: 'The Morning Kettle',
  cast: [EILIDH],
  callers: {},
  rundown: (start) => {
    const at = rundownAt(start);
    return [
      { at: at(0), title: 'Open', detail: 'Eilidh on air', hard_post: false, key: null },
      { at: at(8), title: 'News', detail: 'Top of the hour', hard_post: true, key: null },
    ];
  },
  start: (s, now) => s.setMic(false, now),
  beats: [
    {
      id: 'hello',
      when: (s) => s.heardFromYou(),
      lines: [L('i-eilidh', 'eilidh', 4.35, "I'm Eilidh. I present The Morning Kettle, and I run the whole desk from the keyboard.")],
    },
    {
      id: 'muted',
      when: (s) => s.done('hello') && !!s.person('eilidh')?.mute,
      lines: [],
      after: (s) => s.mark('muted'),
    },
    {
      id: 'unmuted',
      when: (s) => s.has('muted') && s.heardFromYou(),
      lines: [L('l-e-back', 'eilidh', 1.16, "And we're back.")],
    },
  ],
  steps: (s: Station): Step[] => {
    const me = s.person('eilidh');
    return [
      {
        id: 'tab',
        say: ['Press ', k('1'), '. The lamp says ON AIR, but your meter stays flat: the browser tab’s microphone is off. Unmute it in the tab panel above (on a real station, the tab’s own button).'],
        done: s.mic && (!!me?.on_air || s.done('hello')),
      },
      { id: 'heard', say: ['Now you are heard. Next, the desk’s own mute: press ', k('Z'), '. Your tile says MUTED and the lamp STANDBY: nothing open reaches listeners.'], done: s.has('muted') },
      { id: 'unmute', say: [k('Z'), ' again unmutes you: ON AIR, and heard.'], done: s.done('unmuted') || (s.has('muted') && s.heardFromYou()) },
      { id: 'off', say: ['Both mutes sound the same to listeners: silence. The desk knows only its own; your meter is the tell. Press ', k('1'), ' to take yourself off.'], done: s.done('unmuted') && !me?.on_air },
    ];
  },
  did: () => [
    'You went ON AIR with the tab’s microphone off, saw the flat meter, and unmuted the tab.',
    'You muted yourself at the desk (Z): MUTED on the tile, STANDBY on the lamp; then unmuted.',
    'Two mutes, one silence: the meter tells them apart.',
  ],
};
