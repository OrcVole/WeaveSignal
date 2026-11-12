// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The Morning Kettle (ADR-0212): the sandbox's first scenario, the demo video's show as the
// website's "Try the desk" has run it since 2026-11-04. You are Eilidh, the presenter; Gregor
// is the guest; Shona screens; Niall, Calum and Jessie ring. Its lines are the demo video's
// stems, so every one is recorded.

import { slotKeys } from '../../../../../packages/ui/keymap.ts';
import { L } from '../lines.ts';
import type { Station } from '../station.ts';
import { rundownAt, type Scenario, type Step } from './scenario.ts';

const k = (key: string) => ({ key });

export const SHOW: Scenario = {
  id: 'show',
  title: 'The Morning Kettle',
  blurb: 'The whole show, as the demo video runs it: a guest, two screened callers, a dump and a playout fault.',
  minutes: 4,
  learn: [
    'Put yourself and a guest on air, and fire a jingle and a bed from any tab.',
    'Screen a caller as Shona, off air, and send them to the desk.',
    'Cue a caller in your headphones, take them to air, and dump a swear before listeners hear it.',
    'What listeners hear when playout fails, and what the desk says.',
  ],
  you: { identity: 'eilidh', name: 'Eilidh Fraser-MacLeod', role: 'presenter' },
  show: 'The Morning Kettle',
  cast: [
    { identity: 'eilidh', name: 'Eilidh Fraser-MacLeod', role: 'presenter' },
    { identity: 'gregor', name: 'Gregor Stewart', role: 'guest' },
    { identity: 'shona', name: 'Shona Campbell', role: 'screener' },
  ],
  callers: {
    niall: { name: 'Niall Anderson', topic: 'The bus lanes on Leith Walk' },
    jessie: { name: 'Jessie Brown', topic: 'No night bus from the Infirmary' },
    calum: { name: 'Calum Robertson', topic: 'Parking at Easter Road on match days' },
  },
  rundown: (start) => {
    const at = rundownAt(start);
    return [
      { at: at(0), title: 'Open and jingle', detail: 'Eilidh on air, the steel jingle', hard_post: false, key: null },
      { at: at(1), title: 'Gregor Stewart', detail: 'Local historian: the last tram down Leith Walk, 1956', hard_post: false, key: null },
      { at: at(3), title: 'Phones', detail: 'Shona screens; Leith Walk callers', hard_post: false, key: null },
      { at: at(8), title: 'News', detail: 'Top of the hour', hard_post: true, key: null },
      { at: at(10), title: 'Steeped with Finlay Grant', detail: 'Next show', hard_post: false, key: null },
    ];
  },
  beats: [
    {
      id: 'open',
      // After the jingle the visitor fires, never before it: with only "no jingle sounding" she
      // opened the moment she was on air, and the guide skipped its jingle step.
      when: (s) => s.open('eilidh') && s.fired.length > 0 && !s.shotOnAir(),
      lines: [
        L('l-open', 'eilidh', 8.74, "Good morning, and welcome to The Morning Kettle on Kettle FM. I'm Eilidh Fraser-MacLeod. The kettle's on, the phones are open, and this morning it's all about Leith Walk."),
        L('l-intro-gregor', 'eilidh', 7.11, "My guest this morning is the local historian Gregor Stewart, who's joining us from home with nothing but a link. Gregor, are you there?"),
      ],
    },
    {
      id: 'gregor',
      when: (s) => s.done('open') && s.open('gregor'),
      lines: [
        L('l-gregor-here', 'gregor', 3.92, "I'm here, Eilidh. No account, no app. I just pressed the button in the email."),
        L('l-photo', 'eilidh', 2.44, "Now, Leith Walk. You've brought a photograph."),
        L('l-tram', 'gregor', 8.29, 'I have. Nineteen fifty-six, the last tram down Leith Walk, and a crowd of folk waving it goodbye. Fifty years later, we spent a fortune putting them back.'),
        L('l-phones', 'eilidh', 4.55, "And the phones have lit up already. Here's a wee bit of music while Shona answers them."),
      ],
      after: (s, now) => {
        s.ring('niall', now);
        s.ring('calum', now + 9_000);
      },
    },
    {
      id: 'niall-booth',
      when: (s) => s.callerState('niall') === 'screening',
      lines: [
        L('l-s1', 'shona', 2.8, "Kettle FM, you're through to Shona. Who's this?", 'booth'),
        L('l-n1', 'niall', 3.6, "Niall Anderson, from Leith. It's about the bus lanes on Leith Walk.", 'booth'),
        L('l-s2', 'shona', 1.98, "Have you rung us before, Niall?", 'booth'),
        L('l-n2', 'niall', 2.52, "Three weeks running. Nobody's let me on yet.", 'booth'),
        L('l-s3', 'shona', 3.28, "You're on next. Keep it short, and turn your radio down.", 'booth'),
        L('l-n3', 'niall', 1.76, 'Will do. Cheers, Shona.', 'booth'),
      ],
      after: (s, now) => s.ring('jessie', now + 4_000),
    },
    {
      id: 'niall-cue',
      when: (s) => s.person('niall')?.cue === true && !s.open('niall'),
      lines: [
        L('l-e-cue', 'eilidh', 2.12, "Niall, it's Eilidh. You're on in a moment.", 'booth'),
        L('l-n-cue', 'niall', 1.02, 'Right you are.', 'booth'),
      ],
    },
    {
      id: 'niall-air',
      when: (s) => s.open('niall'),
      lines: [
        L('l-e-n', 'eilidh', 4.13, "Niall Anderson in Leith, good morning. You've been trying to get through for a while."),
        L('l-n-air', 'niall', 8.27, 'Three weeks, Eilidh. The bus lanes on Leith Walk stop right where the buses need them most. At half past eight you can walk into town faster than the bus.'),
        L('l-g-n', 'gregor', 3.7, 'Funny. They said the very same about the trams in nineteen fifty-six.', 'air', true),
        L('l-e-bye', 'eilidh', 1.88, 'Thanks, Niall. Keep ringing.'),
      ],
    },
    {
      id: 'jessie-booth',
      when: (s) => s.callerState('jessie') === 'screening',
      lines: [L('i-jessie', 'jessie', 4.04, "I'm Jessie. I'm ringing in too, and I'm about to say something I shouldn't.", 'booth')],
    },
    {
      id: 'jessie-air',
      when: (s) => s.open('jessie') && !s.open('niall'),
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
      // After a dump, or a few seconds after she swore when nobody dumped.
      when: (s) => s.swore !== null && (s.delay.dumps > 0 || s.clock - s.swore > 4_000),
      lines: [L('l-e-sorry', 'eilidh', 5.36, "Jessie, I'll stop you there. Sorry about the language, folks. You never heard it, and that's the point.")],
    },
    {
      id: 'down',
      when: (s) => !s.playout && s.done('open'),
      lines: [
        L('l-e-down', 'eilidh', 5.05, "Gregor, the desk says playout's down. Listeners can't hear us, but we can still hear each other."),
        L('l-g-down', 'gregor', 3.41, "Just like the trams. Gone, but we're still talking about them.", 'air', true),
      ],
    },
    {
      id: 'back',
      when: (s) => s.done('down') && s.playout,
      lines: [L('l-e-back', 'eilidh', 1.16, "And we're back.")],
    },
    {
      id: 'end',
      when: (s) => s.done('sorry') && s.ended,
      lines: [L('l-e-end', 'eilidh', 6.81, "That's The Morning Kettle. Thanks to Gregor, to Niall and Jessie, and to Shona on the phones. Finlay Grant is on next with Steeped.")],
    },
  ],
  steps: (s: Station, g): Step[] => {
    const slotOf = (id: string) => s.slots.indexOf(id) + 1;
    const niall = s.callerState('niall');
    const jessie = s.callerState('jessie');
    const nSlot = slotOf('niall') || 4;
    const jSlot = slotOf('jessie') || 3;
    const jessieQueued = s.callers.findIndex((c) => c.identity === 'jessie') + 1 || 2;
    return [
      { id: 'on', say: ['Put Eilidh on air: press ', k('1'), '. You are Eilidh, the presenter, at the desk.'], done: !!s.person('eilidh')?.on_air || s.done('open') },
      { id: 'open', say: ['Press ', k('F1'), ' for the station jingle (on a laptop, ', k('Fn+F1'), ', or click the first cart); Eilidh opens the show when it ends.'], done: s.done('open') || !!s.beat?.beat.id.startsWith('open') },
      { id: 'gregor', say: ['Gregor is waiting on strip 2. Press ', k('2'), ' to bring him on air.'], done: s.done('gregor') || (s.open('gregor') && s.done('open')) },
      {
        id: 'screen',
        say: ['A caller is ringing. ', k('Ctrl+2'), ' opens the Screener; press ', k('1'), ' to pick Niall, then ', k('T'), ' to answer as Shona. ', k('F2'), ' plays a music bed meanwhile.'],
        done: niall !== null && niall !== 'waiting',
      },
      { id: 'send', say: ['Listen to Shona screen him. Write a line for the presenter if you like, then ', k('P'), ' sends Niall to the desk.'], done: niall === 'ready' || !!s.person('niall')?.on_air },
      {
        id: 'air',
        say: [k('Ctrl+1'), ` back to the desk: Niall is on strip ${nSlot}. `, k(slotKeys(nSlot).cue), ' cues him (only you hear), ', k(slotKeys(nSlot).on), ' puts him on air.'],
        done: s.done('niall-air') || !!s.person('niall')?.on_air,
      },
      {
        id: 'jessie',
        say: [`When Niall has finished, ${slotKeys(nSlot).on} takes him off. Then screen Jessie the same way: `, k('Ctrl+2'), ', ', k(String(jessieQueued)), ', ', k('T'), ', ', k('P'), '.'],
        done: jessie === 'ready' || !!s.person('jessie')?.on_air,
      },
      {
        id: 'swear',
        say: [k('Ctrl+1'), `, then `, k(slotKeys(jSlot).on), ` puts Jessie on air. She is about to swear: press `, k('Delete'), ' twice to dump the profanity delay.'],
        done: s.delay.dumps > 0,
      },
      {
        id: 'listen',
        say: [`Switch Hear to Listeners. They never heard her: they were ${s.delay.target_ms / 1000} s behind, and the dump threw those seconds away. The delay builds back on its own.`],
        done: g.heardListenersAfterDump,
      },
      {
        id: 'down',
        say: ['The safety net: pull playout’s plug for 15 seconds. The lamp says PLAYOUT DOWN, open faders say OPEN, and listeners hear the fallback loop.'],
        done: s.done('down') || !s.playout,
        action: { label: 'Pull the plug', run: 'pull-playout' },
      },
      { id: 'end', say: ['End the show when you are ready. Eilidh says goodbye.'], done: s.done('end') || s.ended, action: { label: 'End the show', run: 'end-show' } },
    ];
  },
  did: (s) => [
    'You put Eilidh and Gregor on air and fired the station jingle from F1, with a bed under the voices.',
    `You screened Niall and Jessie as Shona, off air, and sent them to the desk${s.callers.some((c) => c.line) ? ' with a line for the presenter' : ''}.`,
    'You cued Niall in your headphones before taking him to air, so listeners heard him only when you chose.',
    `You dumped the profanity delay ${s.delay.dumps === 1 ? 'once' : `${s.delay.dumps} times`}: the studio heard Jessie swear, the listeners never did.`,
    'Playout fell over and came back: the lamp said PLAYOUT DOWN, your open faders said OPEN, and listeners heard the fallback loop meanwhile.',
  ],
};
