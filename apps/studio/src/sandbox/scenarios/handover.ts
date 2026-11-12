// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Hand over to the next DJ (ADR-0222, ADR-0074): Finlay is already in the room, standing by on
// strip 5 (as `nextdj.ts` leaves him). Eilidh does her last link, counts him in on talkback,
// brings him on air beside her for the overlap with `⇧⏎` (or the button on his tile), says
// goodbye and presses it again: she is off, the air is his and the desk names his show. The
// way by hand (`5`, then `1`) does the same, and the steps accept either.

import { U } from '../lines.ts';
import type { Station } from '../station.ts';
import { HANDOVER_KEY } from '../../lib/handover.ts';
import { EILIDH, FINLAY } from './cast.ts';
import { k, rundownAt, type Scenario, type Step } from './scenario.ts';

/** The next show, as the desk names it once the handover is done. */
export const NEXT_SHOW = 'Steeped';

export const HANDOVER: Scenario = {
  id: 'handover',
  title: 'Hand over to the next DJ',
  blurb: 'The next DJ stands by: count them in, bring them on beside you with one key, press it again and leave them the air with no gap.',
  minutes: 3,
  learn: [
    'The overlap: the next DJ is on air beside you before the post, so listeners hear a handover, never a gap.',
    'One key does it, pressed twice: Shift+Enter brings the next DJ on air beside you, and again takes you off. The button on their tile is the same key and says which press is next.',
    'Then the air is theirs, and so are the desk’s talk keys. By hand, their digit and then yours do the same.',
  ],
  you: EILIDH,
  show: 'The Morning Kettle',
  cast: [EILIDH, FINLAY],
  callers: {},
  rundown: (start) => {
    const at = rundownAt(start);
    return [
      { at: at(0), title: 'Last link', detail: 'Eilidh, the end of The Morning Kettle', hard_post: false, key: null },
      { at: at(2), title: 'Handover', detail: 'Finlay on air beside Eilidh', hard_post: false, key: HANDOVER_KEY },
      { at: at(4), title: 'Steeped with Finlay Grant', detail: 'The next show', hard_post: true, key: null },
    ];
  },
  beats: [
    {
      id: 'last-link',
      when: (s) => s.open('eilidh') && !s.open('finlay'),
      lines: [U('t-ho-last', 'eilidh', "That's nearly it from me this morning. Thanks for all your calls about Leith Walk, and to Gregor for the trams.")],
    },
    {
      id: 'count',
      when: (s) => s.person('eilidh')?.talk_to === 'finlay' && !s.open('finlay'),
      lines: [
        U('t-ho-count-e', 'eilidh', "Finlay, you're on after this. In three, two, one.", 'booth'),
        U('t-ho-count-f', 'finlay', 'Ready when you are.', 'booth'),
      ],
    },
    {
      id: 'overlap',
      when: (s) => s.open('eilidh') && s.open('finlay'),
      lines: [
        U('t-ho-hello', 'eilidh', "And here he is. Finlay Grant, good morning. What's in the pot for Steeped today?", 'air', true),
        U('t-ho-f-hello', 'finlay', "Morning, Eilidh. Morning, everyone. Loose leaf and long records: an hour of Scottish folk you'll not have heard on the radio."),
        U('t-ho-bye', 'eilidh', "Lovely. That's me away, then. Finlay, she's all yours.", 'air', true),
      ],
    },
    {
      // The simulated schedule moves the show on once Eilidh is off and Finlay holds the air
      // (a real station's schedule does it at the post).
      id: 'handed-over',
      when: (s) => s.done('overlap') && !s.open('eilidh') && s.open('finlay'),
      lines: [],
      after: (s) => {
        s.show = NEXT_SHOW;
        s.mark('handed-over');
      },
    },
    {
      id: 'steeped',
      when: (s) => s.has('handed-over') && s.open('finlay'),
      lines: [U('t-ho-steeped', 'finlay', "This is Steeped on Kettle FM, with me, Finlay Grant. The kettle's still on. Here's the first one.")],
      after: (s) => s.mark('his'),
    },
  ],
  steps: (s: Station): Step[] => {
    const finlay = s.person('finlay');
    const both = s.open('eilidh') && s.open('finlay');
    return [
      { id: 'open', say: ['Finlay stands by on strip 5, and Steeped is at the post. Press ', k('1'), ' to open your fader for your last link.'], done: !!s.person('eilidh')?.on_air || s.done('last-link') },
      { id: 'cue', say: ['Press ', k('T'), ' to cue Finlay: check he is there, in your headphones.'], done: !!finlay?.cue || !!finlay?.on_air || s.done('count') },
      {
        id: 'count',
        say: ['Hold ', k('G'), ' to count him in: only he hears you, and you are off the programme while it is held.'],
        done: s.done('count') || both || s.done('overlap') || s.has('handed-over'),
      },
      {
        id: 'on',
        say: ['Press ', k('Shift+Enter'), ', or Bring Finlay on air on his tile: he is on air beside you, ON AIR on both tiles. This is the overlap: say hello, then goodbye. (', k('5'), ' does it by hand.)'],
        done: both || s.done('overlap') || s.has('handed-over'),
      },
      {
        id: 'off',
        say: ['Said goodbye? Press ', k('Shift+Enter'), ' again (the button now says Hand over to Finlay): you come off, Finlay holds the air with no gap, and the desk names his show. (', k('1'), ' does it by hand.)'],
        done: s.has('handed-over'),
      },
    ];
  },
  did: (s) => [
    s.done('count') ? 'You counted Finlay in on talkback, off the programme, and brought him on air beside you: the overlap.' : 'You brought Finlay on air beside you: the overlap.',
    s.has('handed-over')
      ? 'You handed over once you had said goodbye: listeners heard the two of you, then Finlay, never a gap, and the desk now names Steeped.'
      : 'You handed over to Finlay.',
    'The desk’s talk keys went to Finlay with the air. On a real station you then leave the room (close your tab), Finlay drives the desk from his own studio app, and the schedule moves the show and the call page’s takedown contact to his at the post.',
  ],
};
