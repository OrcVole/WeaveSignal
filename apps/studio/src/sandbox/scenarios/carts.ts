// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Carts and a bed under a link (ADR-0222): a bed ducks under your voice, shots stack on top,
// the cart wall's keys, Space to stop the last shot, Shift+Space twice to fade everything, and
// a cart cued in your headphones only.

import { L, U } from '../lines.ts';
import type { Station } from '../station.ts';
import { EILIDH } from './cast.ts';
import { k, rundownAt, type Scenario, type Step } from './scenario.ts';

export const CARTS: Scenario = {
  id: 'carts',
  title: 'Carts and a bed',
  blurb: 'A music bed under your link, shots over the top, the cart wall’s keys, and a cart cued in your headphones.',
  minutes: 2,
  learn: [
    'F1 to F6 fire the bank’s top row from any tab; a bed ducks under an open voice.',
    'The cart wall: the same four rows of keys as the desk, Space to stop the last shot, Shift+Space twice to fade everything.',
    'Cue a cart in your headphones with Shift and its key; listeners never hear a cue.',
  ],
  you: EILIDH,
  show: 'The Morning Kettle',
  cast: [EILIDH],
  callers: {},
  rundown: (start) => {
    const at = rundownAt(start);
    return [
      { at: at(0), title: 'Link over the bed', detail: 'Eilidh; bed on F2', hard_post: false, key: null },
      { at: at(8), title: 'News', detail: 'Top of the hour', hard_post: true, key: null },
    ];
  },
  beats: [
    {
      id: 'link',
      when: (s) => s.open('eilidh') && s.sounding.some((x) => x.pool === 'bed' && !x.cue && !x.stopping),
      lines: [
        L('i-eilidh', 'eilidh', 4.35, "I'm Eilidh. I present The Morning Kettle, and I run the whole desk from the keyboard."),
        U('t-carts-link', 'eilidh', 'Coming up after the news: Gregor Stewart and the last tram down Leith Walk. Stay with us.'),
      ],
    },
  ],
  steps: (s: Station): Step[] => {
    const bed = s.sounding.some((x) => x.pool === 'bed' && !x.cue && !x.stopping);
    return [
      { id: 'on', say: ['Press ', k('1'), ': you are on air.'], done: !!s.person('eilidh')?.on_air || s.done('link') || !!s.beat?.beat.id.startsWith('link') },
      { id: 'bed', say: [k('F2'), ' starts the music bed. Eilidh speaks over it: the desk ducks the bed under her voice and brings it back after.'], done: s.done('link') || !!s.beat?.beat.id.startsWith('link') },
      {
        id: 'shots',
        say: [k('F3'), ' is applause and ', k('F1'), ' the jingle: one-shots play over the bed and over each other. A new bed replaces the old; shots stack.'],
        done: s.fired.includes('applause-1') && s.fired.includes('jingle-1'),
      },
      {
        id: 'wall',
        say: [k('Ctrl+3'), ' opens the cart wall: the desk’s four rows of keys, 1 to 6 over Q to Y, are its 24 slots. ', k('Space'), ' stops the last shot; ', k('Shift+Space'), ' twice fades everything out.'],
        done: s.fades > 0,
      },
      {
        id: 'cue',
        say: ['On the wall, ', k('Shift+2'), ' cues the bed in your headphones only: listeners never hear a cue. ', k('Esc'), ' takes it out again.'],
        done: s.cartCues > 0,
      },
      { id: 'off', say: ['Press ', k('1'), ' to take yourself off when you are done.'], done: s.cartCues > 0 && !s.person('eilidh')?.on_air && !bed },
    ];
  },
  did: (s) => [
    `You fired ${s.fired.length} cart${s.fired.length === 1 ? '' : 's'} from the keys: the bed ducked under your voice and came back after.`,
    'You stacked one-shots over the bed, stopped the last with Space and faded everything with Shift+Space twice.',
    `You cued a cart in your headphones ${s.cartCues === 1 ? 'once' : `${s.cartCues} times`}; listeners never heard it.`,
  ],
};
