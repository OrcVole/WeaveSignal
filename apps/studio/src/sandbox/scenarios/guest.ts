// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A guest by link (ADR-0222, ADR-0271): the one scenario of the Guests step. You invite Gregor
// from the desk (Invite someone…, beside Not on air, which arrives at this step), hear him
// arrive on air (a guest is on air the moment they join), take him off and talk to him off air,
// put him back and mute him, then end his pass. The phones stay closed: call-in comes with
// Callers.

import { L, U } from '../lines.ts';
import type { Station } from '../station.ts';
import { EILIDH, GREGOR } from './cast.ts';
import { k, rundownAt, type Scenario, type Step } from './scenario.ts';

export const GUEST: Scenario = {
  id: 'guest',
  title: 'A guest by link',
  blurb: 'Invite Gregor from the desk, hear him arrive on air, talk to him off air, mute him, end his pass.',
  minutes: 2,
  step: 'guests',
  learn: [
    'Invite someone…, beside Not on air: a name, the role Guest, and a link to send. No account, nothing to install at their end.',
    'A guest is on air the moment they join. Their On key takes them off to Not on air and back; their talk key reaches them off air.',
    'Mute a guest without moving them, and end their pass when they are done: the link then no longer works.',
  ],
  you: EILIDH,
  show: 'The Morning Kettle',
  cast: [EILIDH, GREGOR],
  later: ['gregor'],
  callers: {},
  rundown: (start) => {
    const at = rundownAt(start);
    return [
      { at: at(0), title: 'Open', detail: 'Eilidh; the guest joins by link', hard_post: false, key: null },
      { at: at(1), title: 'Gregor Stewart', detail: 'Local historian: the last tram down Leith Walk, 1956', hard_post: false, key: null },
      { at: at(8), title: 'News', detail: 'Top of the hour', hard_post: true, key: null },
    ];
  },
  beats: [
    {
      id: 'open',
      when: (s) => s.open('eilidh'),
      lines: [U('t-guest-open', 'eilidh', "Good morning, this is The Morning Kettle on Kettle FM. No phones this morning: one guest, and he is on his way.")],
    },
    {
      id: 'gregor',
      when: (s) => s.open('gregor') && s.open('eilidh') && !s.has('talked'),
      lines: [
        L('l-intro-gregor', 'eilidh', 7.11, "My guest this morning is the local historian Gregor Stewart, who's joining us from home with nothing but a link. Gregor, are you there?"),
        L('l-gregor-here', 'gregor', 3.92, "I'm here, Eilidh. No account, no app. I just pressed the button in the email."),
        L('l-photo', 'eilidh', 2.44, "Now, Leith Walk. You've brought a photograph."),
        L('l-tram', 'gregor', 8.29, 'I have. Nineteen fifty-six, the last tram down Leith Walk, and a crowd of folk waving it goodbye. Fifty years later, we spent a fortune putting them back.'),
      ],
    },
    {
      id: 'aside',
      when: (s) => s.done('gregor') && s.person('eilidh')?.talk_to === 'gregor' && !s.open('gregor'),
      lines: [
        U('t-guest-aside-e', 'eilidh', "Gregor, it's Eilidh, off air. Two more minutes when we come back: the depot at Shrubhill.", 'booth'),
        U('t-guest-aside-g', 'gregor', 'Shrubhill. Right you are.', 'booth'),
      ],
      after: (s) => s.mark('talked'),
    },
    {
      id: 'back',
      when: (s) => s.has('talked') && s.open('gregor') && s.open('eilidh'),
      lines: [U('t-guest-back', 'gregor', 'The depot at Shrubhill is where they went to be broken up. There is a block of flats on it now, and not one plaque.')],
    },
    {
      id: 'muted',
      when: (s) => s.done('back') && !!s.person('gregor')?.mute,
      lines: [],
      after: (s) => s.mark('muted'),
    },
    {
      id: 'thanks',
      when: (s) => s.has('muted') && s.open('gregor') && s.open('eilidh'),
      lines: [U('t-guest-thanks', 'eilidh', 'Gregor Stewart, thank you. That was the last tram down Leith Walk.')],
    },
  ],
  steps: (s: Station): Step[] => {
    const gregor = s.person('gregor');
    const gone = s.done('thanks') && !gregor;
    return [
      { id: 'on', say: ['The show runs at Guests: the phones are closed, and a guest can join by link. Press ', k('1'), ' to go on air.'], done: !!s.person('eilidh')?.on_air || s.done('open') },
      {
        id: 'invite',
        say: ['Press Invite someone…, at the right of the Not on air line. Type Gregor Stewart, keep the role Guest, press Create invitation.'],
        done: s.has('invited:gregor') || !!gregor || gone,
      },
      {
        id: 'joined',
        say: ['That link is what you would send him; Copy link copies it. Here he opens it himself: press ', k('Esc'), ' to close. A guest is on air the moment he joins, on strip 2.'],
        done: s.done('gregor') || gone,
      },
      {
        id: 'aside',
        say: ['Press ', k('2'), ' to take Gregor off air: he moves to Not on air, with the same keys. Hold ', k('S'), ' to talk to him: he hears you, listeners do not.'],
        done: s.has('talked') || gone,
      },
      { id: 'back', say: ['Press ', k('2'), ' again: Gregor is back on air, on the stage.'], done: s.done('back') || gone },
      { id: 'mute', say: [k('X'), ' mutes him where he is: MUTED on his tile, and listeners hear only you. ', k('X'), ' again unmutes him.'], done: (s.has('muted') && !gregor?.mute) || gone },
      {
        id: 'end',
        say: ['When he has finished, press End pass on his tile, twice: he is out of the room, and his link no longer works.'],
        done: gone,
      },
    ];
  },
  did: (s) => [
    `You made Gregor an invitation at the desk with the role Guest${s.invites > 1 ? ` (${s.invites} links in all)` : ''}; he joined from it, on air at once.`,
    'You took him off air and talked to him there, then put him back: the same keys, on the stage or under Not on air.',
    'You muted him without moving him, and ended his pass when he was done: his link no longer works.',
    'The phones stayed closed throughout: call-in comes with the Callers step.',
  ],
};
