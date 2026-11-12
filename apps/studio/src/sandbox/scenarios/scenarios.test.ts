// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The training scenarios (ADR-0222) as data: every line a scenario says is either one of the
// demo video's recorded stems or is listed in lines-to-record.tsv for the recording session,
// word for word; the people are the design system's (ADR-0071); each scenario runs from its
// start card to its end card on the simulated station with the presses its guide names.
import { describe, expect, it } from 'vitest';
import { catalogueFrom, Station, type PackCart } from '../station.ts';
import { RUNG_HEAD, SCENARIOS, rungFor, rungs, scenarioFor } from './index.ts';
import { hiddenSeat } from '../station.ts';
import { STEPS } from '../../lib/steps.ts';
import { SHOW } from './show.ts';
import { HANDOVER_KEY, handoverButton, handoverStep } from '../../lib/handover.ts';
import pack from '../../../../../assets/carts-demo/carts.json';
import tsv from '../lines-to-record.tsv?raw';

const cat = catalogueFrom(pack as { banks: Record<string, string>; carts: PackCart[] });
const T0 = Date.UTC(2026, 10, 6, 7, 0);

/** The demo video's stems (`check.tsv`), as `media/sandbox/voices/` holds them. */
const RECORDED = new Set([
  'i-gregor', 'i-shona', 'i-niall', 'i-jessie', 'i-finlay', 'i-eilidh', 'o-eilidh',
  'l-open', 'l-intro-gregor', 'l-gregor-here', 'l-photo', 'l-tram', 'l-phones',
  'l-s1', 'l-n1', 'l-s2', 'l-n2', 'l-s3', 'l-n3', 'l-e-cue', 'l-n-cue',
  'l-e-n', 'l-n-air', 'l-g-n', 'l-e-bye', 'l-g-down', 'l-e-back', 'l-j', 'l-e-j', 'l-e-sorry', 'l-e-down', 'l-e-end',
]);

const TSV = tsv
  .trim()
  .split('\n')
  .slice(1)
  .map((row: string) => {
    const [id, who, speaker, heard, scenario, text] = row.split('\t');
    return { id, who, speaker, heard, scenario, text };
  });

function run(s: Station, from: number, to: number): number {
  for (let t = from; t <= to; t += 100) s.tick(t);
  return to;
}

describe('the scenarios as data', () => {
  it('answers an unknown hash with the show, and names each scenario once', () => {
    expect(scenarioFor('nothing')).toBe(SHOW);
    expect(scenarioFor('')).toBe(SHOW);
    expect(new Set(SCENARIOS.map((s) => s.id)).size).toBe(SCENARIOS.length);
  });

  it('groups the scenarios by step, Solo at the top, as the design lists them', () => {
    const ladder = rungs();
    expect(ladder.map((r) => r.step)).toEqual(['solo', 'guests', 'callers', 'full']);
    expect(ladder.map((r) => r.scenarios.map((x) => x.scenario.id))).toEqual([
      ['mic', 'carts', 'auto'],
      ['guest'],
      ['alone', 'dump', 'link'],
      ['show', 'queue', 'banned', 'next-dj', 'handover'],
    ]);
    // No rung is empty, the numbers run on down the ladder, and the list is in the ladder's order.
    expect(ladder.every((r) => r.scenarios.length > 0)).toBe(true);
    expect(ladder.flatMap((r) => r.scenarios.map((x) => x.n))).toEqual(SCENARIOS.map((_, i) => i + 1));
    expect(ladder.flatMap((r) => r.scenarios.map((x) => x.scenario))).toEqual([...SCENARIOS]);
    expect(ladder.map((r) => r.head)).toEqual(['Just you', 'You and a guest', 'Listeners call in', 'A screener, the picture, the next DJ']);
    expect(Object.keys(RUNG_HEAD)).toEqual([...STEPS]);
  });

  it('opens the chooser at a rung for a step’s hash, and a scenario for its own', () => {
    expect(STEPS.map((s) => rungFor(s))).toEqual(['solo', 'guests', 'callers', 'full']);
    expect(rungFor('alone')).toBeNull();
    expect(rungFor('')).toBeNull();
    // No scenario is named as a step is, so a link means one thing.
    expect(SCENARIOS.some((s) => rungFor(s.id) !== null)).toBe(false);
  });

  it('seats nobody its step hides: every scenario’s cast and callers are allowed at its step', () => {
    for (const sc of SCENARIOS) {
      for (const c of sc.cast) {
        const own = c.identity === sc.you.identity;
        expect(hiddenSeat(sc.step, c.role, own), `${sc.id}: ${c.name}, a ${c.role}, at ${sc.step}`).toBeNull();
      }
      if (Object.keys(sc.callers).length) expect(hiddenSeat(sc.step, 'caller'), `${sc.id}: callers at ${sc.step}`).toBeNull();
      // And the view says so: the station starts at the scenario's step, with its show on air.
      const v = new Station(T0, cat, sc).view(T0);
      expect(v.step).toMatchObject({ step: sc.step, on_air: true, may_change: true, show: { name: sc.show } });
    }
  });

  it('says only recorded lines or lines listed for recording, word for word', () => {
    for (const sc of SCENARIOS) {
      for (const line of sc.beats.flatMap((b) => b.lines)) {
        if (line.clip === false) {
          const row = TSV.find((r) => r.id === line.id);
          expect(row, `${sc.id}: ${line.id} is not in lines-to-record.tsv`).toBeDefined();
          expect(row).toMatchObject({ who: line.who, heard: line.heard, scenario: sc.id, text: line.text });
        } else {
          expect(RECORDED.has(line.id), `${sc.id}: ${line.id} is not a recorded stem`).toBe(true);
        }
      }
    }
  });

  it('names every speaker from the cast or the callers, with the registry name', () => {
    for (const sc of SCENARIOS) {
      const names = new Map<string, string>([...sc.cast.map((c) => [c.identity, c.name] as const), ...Object.entries(sc.callers).map(([id, c]) => [id, c.name] as const)]);
      for (const line of sc.beats.flatMap((b) => b.lines)) {
        expect(names.has(line.who), `${sc.id}: ${line.id} by \`${line.who}\`, who is not in the room`).toBe(true);
      }
      expect(names.get(sc.you.identity)).toBe(sc.you.name);
    }
    for (const r of TSV) {
      const sc = scenarioFor(r.scenario);
      const name = sc.cast.find((c) => c.identity === r.who)?.name ?? sc.callers[r.who]?.name;
      expect(name, `${r.id}: speaker \`${r.who}\``).toBe(r.speaker);
    }
  });

  it('every scenario has a start card, steps and an end card', () => {
    for (const sc of SCENARIOS) {
      const s = new Station(T0, cat, sc);
      expect(sc.learn.length).toBeGreaterThan(1);
      expect(sc.minutes).toBeGreaterThan(0);
      const steps = sc.steps(s, { hear: 'studio', listened: false, heardListenersAfterDump: false });
      expect(steps.length).toBeGreaterThan(2);
      expect(steps.every((st) => st.say.length > 0 && !st.done), `${sc.id}: a step is done before the start`).toBe(true);
      expect(sc.did(s, { hear: 'studio', listened: true, heardListenersAfterDump: true }).length).toBeGreaterThan(1);
    }
  });
});

describe('running everything alone', () => {
  it('takes a caller from the queue to the strip, cues and talks on the strip, then hands over', () => {
    const s = new Station(T0, cat, scenarioFor('alone'));
    s.begin(T0);
    expect(s.view(T0).seats.screener).toBeNull();
    // The Callers step: the queue is the desk's, and the send below is `desk.callers`' own (P).
    expect(s.view(T0).step.step).toBe('callers');
    s.desk({ op: 'on_air', identity: 'eilidh', on: true }, T0);
    let t = run(s, T0, T0 + 17_000);
    expect(s.done('open')).toBe(true);
    t = run(s, t, t + 1_000);
    expect(s.callerState('niall')).toBe('waiting');
    s.screener({ op: 'send', identity: 'niall' }, t);
    expect(s.view(t).sources.find((x) => x.identity === 'niall')?.slot).toBe(4);
    s.desk({ op: 'cue', identity: 'niall', on: true }, t);
    t = run(s, t, t + 300);
    expect(s.speaking?.line.id).toBe('l-n1');
    t = run(s, t, t + 8_000);
    s.desk({ op: 'talk_to', identity: 'eilidh', to: 'niall', on: true }, t);
    t = run(s, t, t + 300);
    expect(s.speaking?.line.id).toBe('l-e-cue');
    s.desk({ op: 'talk_to', identity: 'eilidh', to: 'niall', on: false }, t);
    t = run(s, t, t + 5_000);
    s.desk({ op: 'on_air', identity: 'niall', on: true }, t);
    t = run(s, t, t + 17_000);
    expect(s.done('niall-air')).toBe(true);
    s.desk({ op: 'on_air', identity: 'niall', on: false }, t);
    s.carts({ op: 'fire', cart: 'applause-1' }, t);
    s.desk({ op: 'on_air', identity: 'eilidh', on: false }, t);
    t = run(s, t, t + 500);
    expect(s.has('handed-over')).toBe(true);
    run(s, t, t + 5_000);
    expect(s.callerState('jessie')).toBe('waiting');
    const steps = scenarioFor('alone').steps(s, { hear: 'studio', listened: false, heardListenersAfterDump: false });
    expect(steps.every((x) => x.done)).toBe(true);
  });
});

describe('a guest by link', () => {
  it('invites Gregor at the desk, hears him arrive on air, talks to him off air, mutes him and ends his pass', () => {
    const sc = scenarioFor('guest');
    const s = new Station(T0, cat, sc);
    const g = { hear: 'studio' as const, listened: false, heardListenersAfterDump: false };
    const stepNow = () => sc.steps(s, g).find((x) => !x.done)?.id ?? 'done';
    s.begin(T0);
    // The Guests step: the phones are closed, and nobody but Eilidh is in the room yet.
    expect(s.view(T0).step.step).toBe('guests');
    expect(s.view(T0).sources.map((x) => x.identity)).toEqual(['eilidh']);
    expect(Object.keys(sc.callers)).toEqual([]);
    expect(stepNow()).toBe('on');
    s.desk({ op: 'on_air', identity: 'eilidh', on: true }, T0);
    let t = run(s, T0, T0 + 300);
    expect(s.speaking?.line.id).toBe('t-guest-open');
    expect(stepNow()).toBe('invite');
    // A DJ's invitation is refused at this step, in the station's words; a guest's is made.
    expect(() => s.makeInvite({ role: 'dj', name: 'Finlay Grant' }, t)).toThrow("An invitation for the next show's DJ comes with Full.");
    const invite = s.makeInvite({ role: 'guest', name: 'Gregor Stewart' }, t);
    expect(invite).toMatchObject({ role: 'guest', name: 'Gregor Stewart' });
    expect(stepNow()).toBe('joined');
    t = run(s, t, t + 12_000);
    // A guest is on air the moment he joins, on strip 2, and the desk says so.
    expect(s.person('gregor')).toMatchObject({ on_air: true, role: 'guest' });
    expect(s.view(t).sources.find((x) => x.identity === 'gregor')?.slot).toBe(2);
    expect(s.view(t).notices.find((n) => n.kind === 'person.joined')?.message).toBe('On air, on strip 2.');
    t = run(s, t, t + 26_000);
    expect(s.done('gregor')).toBe(true);
    expect(stepNow()).toBe('aside');
    // Off air, and talkback: he hears Eilidh, listeners do not.
    s.desk({ op: 'on_air', identity: 'gregor', on: false }, t);
    s.desk({ op: 'talk_to', identity: 'eilidh', to: 'gregor', on: true }, t);
    t = run(s, t, t + 300);
    expect(s.speaking?.line).toMatchObject({ id: 't-guest-aside-e', heard: 'booth' });
    t = run(s, t, t + 10_000);
    s.desk({ op: 'talk_to', identity: 'eilidh', to: 'gregor', on: false }, t);
    expect(s.has('talked')).toBe(true);
    expect(stepNow()).toBe('back');
    s.desk({ op: 'on_air', identity: 'gregor', on: true }, t);
    t = run(s, t, t + 10_000);
    expect(s.done('back')).toBe(true);
    expect(stepNow()).toBe('mute');
    s.desk({ op: 'mute', identity: 'gregor', on: true }, t);
    t = run(s, t, t + 300);
    expect(s.has('muted')).toBe(true);
    expect(stepNow()).toBe('mute');
    s.desk({ op: 'mute', identity: 'gregor', on: false }, t);
    t = run(s, t, t + 6_000);
    expect(s.done('thanks')).toBe(true);
    expect(stepNow()).toBe('end');
    s.endPass({ identity: 'gregor' }, t);
    expect(s.person('gregor')).toBeUndefined();
    expect(stepNow()).toBe('done');
    expect(sc.did(s, g)).toHaveLength(4);
    // Nobody rang at any point: call-in comes with Callers.
    expect(s.view(t).callers).toHaveLength(0);
  });
});

describe('a busy queue', () => {
  it('the simulated presenter takes a sent caller, declines one while busy, and takes him when sent again', () => {
    const s = new Station(T0, cat, scenarioFor('queue'));
    s.begin(T0);
    let t = run(s, T0, T0 + 6_000);
    expect(s.callers.map((c) => c.identity)).toEqual(['niall', 'dougal', 'flora', 'alastair']);
    s.screener({ op: 'send', identity: 'niall' }, t);
    t = run(s, t, t + 3_000);
    expect(s.open('niall')).toBe(true);
    s.screener({ op: 'send', identity: 'dougal' }, t);
    t = run(s, t, t + 2_000);
    expect(s.view(t).callers.find((c) => c.identity === 'dougal')?.declined?.reason).toBe('not now');
    expect(s.view(t).desk_taking_callers).toBe(false);
    s.screener({ op: 'screen', identity: 'flora' }, t);
    s.screener({ op: 'notes', identity: 'flora', note: 'Loves the show; a quiet moment' }, t);
    s.screener({ op: 'back', identity: 'flora' }, t);
    expect(s.backs).toBe(1);
    s.screener({ op: 'drop', identity: 'alastair' }, t);
    expect(s.callerRec('alastair')).toBeNull();
    t = run(s, t, t + 20_000);
    expect(s.done('niall-air')).toBe(true);
    expect(s.open('niall')).toBe(false);
    s.screener({ op: 'send', identity: 'dougal' }, t);
    t = run(s, t, t + 3_000);
    expect(s.open('dougal')).toBe(true);
    const steps = scenarioFor('queue').steps(s, { hear: 'studio', listened: false, heardListenersAfterDump: false });
    expect(steps.every((x) => x.done)).toBe(true);
  });
});

describe('banned before', () => {
  it('marks the account, is declined by the presenter, and comes back marked after a drop', () => {
    const s = new Station(T0, cat, scenarioFor('banned'));
    s.begin(T0);
    let t = run(s, T0, T0 + 1_500);
    expect(s.view(t).callers[0]).toMatchObject({ identity: 'tavish', banned_before: { date: '2026-09-12', reason: 'Abuse on air' } });
    s.screener({ op: 'send', identity: 'tavish' }, t);
    t = run(s, t, t + 2_000);
    expect(s.view(t).callers[0].declined?.reason).toBe('banned before');
    s.screener({ op: 'drop', identity: 'tavish' }, t);
    expect(s.view(t).callers).toHaveLength(0);
    t = run(s, t, t + 7_000);
    expect(s.view(t).callers[0]).toMatchObject({ identity: 'tavish', state: 'waiting', banned_before: { date: '2026-09-12' } });
    s.screener({ op: 'drop', identity: 'tavish' }, t);
    expect(s.drops.tavish).toBe(2);
  });
});

describe('a line goes bad', () => {
  it('degrades, drops with the place held, and comes back ready on the same strip', () => {
    const s = new Station(T0, cat, scenarioFor('link'));
    s.begin(T0);
    expect(s.view(T0).sources.find((x) => x.identity === 'iona')).toMatchObject({ slot: 4, caller: { sent: true } });
    s.desk({ op: 'on_air', identity: 'eilidh', on: true }, T0);
    s.desk({ op: 'on_air', identity: 'iona', on: true }, T0);
    let t = run(s, T0, T0 + 18_000);
    expect(s.done('iona-air')).toBe(true);
    expect(s.has('degraded')).toBe(true);
    expect(s.view(t).sources.find((x) => x.identity === 'iona')).toMatchObject({ linked: false, present: true, on_air: true });
    s.desk({ op: 'on_air', identity: 'iona', on: false }, t);
    t = run(s, t, t + 10_000);
    expect(s.has('dropped')).toBe(true);
    const v = s.view(t);
    expect(v.callers[0]).toMatchObject({ identity: 'iona', dropped_ms: expect.any(Number), present: false, slot: null });
    expect(v.notices.at(-1)).toMatchObject({ surface: 'screener', title: "Iona's link dropped" });
    t = run(s, t, t + 16_000);
    expect(s.has('returned')).toBe(true);
    expect(s.view(t).sources.find((x) => x.identity === 'iona')).toMatchObject({ slot: 4, present: true, linked: true });
    expect(s.view(t).callers[0].dropped_ms).toBeNull();
    s.desk({ op: 'on_air', identity: 'iona', on: true }, t);
    t = run(s, t, t + 12_000);
    expect(s.done('iona-back')).toBe(true);
  });
});

describe('your own microphone', () => {
  it('starts with the tab muted: on air, nothing heard, until the tab is unmuted; then the desk mute', () => {
    const s = new Station(T0, cat, scenarioFor('mic'));
    s.begin(T0);
    expect(s.mic).toBe(false);
    s.desk({ op: 'on_air', identity: 'eilidh', on: true }, T0);
    let t = run(s, T0, T0 + 2_000);
    expect(s.speaking).toBeNull();
    expect(s.heardFromYou()).toBe(false);
    s.setMic(true, t);
    t = run(s, t, t + 300);
    expect(s.speaking?.line.id).toBe('i-eilidh');
    t = run(s, t, t + 6_000);
    s.desk({ op: 'mute', identity: 'eilidh', on: true }, t);
    t = run(s, t, t + 300);
    expect(s.has('muted')).toBe(true);
    expect(s.heardFromYou()).toBe(false);
    s.desk({ op: 'mute', identity: 'eilidh', on: false }, t);
    t = run(s, t, t + 300);
    expect(s.speaking?.line.id).toBe('l-e-back');
  });
});

describe('playout takes over', () => {
  it('starts in auto mode, where listeners do not hear the desk, and goes live on the setting', () => {
    const s = new Station(T0, cat, scenarioFor('auto'));
    s.begin(T0);
    expect(s.view(T0).mode).toBe('auto');
    expect(s.listenersHearDesk()).toBe(false);
    s.desk({ op: 'on_air', identity: 'eilidh', on: true }, T0);
    let t = run(s, T0, T0 + 300);
    expect(s.speaking?.line.id).toBe('t-auto-room');
    t = run(s, t, t + 12_000);
    s.setMode('live', t);
    expect(s.listenersHearDesk()).toBe(true);
    expect(s.view(t).notices.at(-1)?.title).toBe('The station is live');
    t = run(s, t, t + 300);
    expect(s.speaking?.line.id).toBe('l-e-back');
    t = run(s, t, t + 12_000);
    // A Solo scenario: nobody stands by (the next show's DJ is a Full seat).
    expect(s.view(t).sources.map((x) => x.identity)).toEqual(['eilidh']);
    s.desk({ op: 'on_air', identity: 'eilidh', on: false }, t);
    s.setMode('auto', t);
    t = run(s, t, t + 300);
    expect(s.has('handed-back')).toBe(true);
  });
});

describe('set up the next DJ', () => {
  it('makes the invite at the desk, the DJ joins off air on strip 5, is cued for a level, agrees the plan and stands by', () => {
    const s = new Station(T0, cat, scenarioFor('next-dj'));
    s.begin(T0);
    expect(s.person('finlay')).toBeUndefined();
    s.desk({ op: 'on_air', identity: 'eilidh', on: true }, T0);
    let t = run(s, T0, T0 + 9_000);
    expect(s.done('on')).toBe(true);
    const stepNow = () => scenarioFor('next-dj').steps(s, { hear: 'studio', listened: false, heardListenersAfterDump: false }).find((x) => !x.done)?.id;
    expect(stepNow()).toBe('invite');
    // An invite with another role is nobody the scenario waits for: a link, and nobody joins.
    expect(s.makeInvite({ role: 'guest', name: 'Finlay Grant' }, t)).toMatchObject({ role: 'guest' });
    expect(stepNow()).toBe('invite');
    const invite = s.makeInvite({ role: 'dj', name: 'Finlay Grant' }, t);
    expect(invite).toMatchObject({ role: 'dj', name: 'Finlay Grant', expires_at_ms: t + 4 * 3600_000 });
    expect(invite.link).toBe(`/join/#${invite.pass}`);
    expect(stepNow()).toBe('joined');
    // A second invite brings no second Finlay.
    s.makeInvite({ role: 'dj', name: 'Finlay Grant' }, t);
    expect(() => s.makeInvite({ role: 'caller', name: 'Dougal Thomson' }, t)).toThrow(/a caller joins another way/);
    t = run(s, t, t + 3_000);
    expect(s.person('finlay')).toBeUndefined();
    t = run(s, t, t + 1_500);
    const v = s.view(t);
    expect(v.sources.find((x) => x.identity === 'finlay')).toMatchObject({ slot: 5, role: 'dj', on_air: false });
    expect(v.notices.at(-1)).toMatchObject({ surface: 'desk', title: 'Finlay joined the room' });
    expect(s.people.filter((p) => p.identity === 'finlay')).toHaveLength(1);
    s.desk({ op: 'cue', identity: 'finlay', on: true }, t);
    t = run(s, t, t + 300);
    expect(s.speaking?.line.id).toBe('i-finlay');
    t = run(s, t, t + 9_000);
    expect(s.done('level')).toBe(true);
    s.desk({ op: 'talk_to', identity: 'eilidh', to: 'finlay', on: true }, t);
    t = run(s, t, t + 300);
    expect(s.speaking?.line.id).toBe('t-nd-plan-e');
    s.desk({ op: 'talk_to', identity: 'eilidh', to: 'finlay', on: false }, t);
    t = run(s, t, t + 15_000);
    expect(s.done('plan')).toBe(true);
    expect(s.has('standing-by')).toBe(false);
    s.desk({ op: 'cue', identity: 'finlay', on: false }, t);
    run(s, t, t + 300);
    expect(s.has('standing-by')).toBe(true);
    expect(s.open('finlay')).toBe(false);
    const steps = scenarioFor('next-dj').steps(s, { hear: 'studio', listened: false, heardListenersAfterDump: false });
    expect(steps.every((x) => x.done)).toBe(true);
  });
});

describe('hand over to the next DJ', () => {
  it('counts the DJ in on talkback, overlaps on air, and leaves him the air and the show', () => {
    const s = new Station(T0, cat, scenarioFor('handover'));
    s.begin(T0);
    expect(s.view(T0).sources.find((x) => x.identity === 'finlay')).toMatchObject({ slot: 5, on_air: false });
    expect(s.view(T0).show.title).toBe('The Morning Kettle');
    s.desk({ op: 'on_air', identity: 'eilidh', on: true }, T0);
    let t = run(s, T0, T0 + 9_000);
    expect(s.done('last-link')).toBe(true);
    s.desk({ op: 'cue', identity: 'finlay', on: true }, t);
    s.desk({ op: 'talk_to', identity: 'eilidh', to: 'finlay', on: true }, t);
    t = run(s, t, t + 300);
    expect(s.speaking?.line.id).toBe('t-ho-count-e');
    s.desk({ op: 'talk_to', identity: 'eilidh', to: 'finlay', on: false }, t);
    t = run(s, t, t + 6_000);
    expect(s.done('count')).toBe(true);
    s.desk({ op: 'on_air', identity: 'finlay', on: true }, t);
    expect(s.person('finlay')?.cue).toBe(false);
    t = run(s, t, t + 300);
    expect(s.speaking?.line.id).toBe('t-ho-hello');
    t = run(s, t, t + 25_000);
    expect(s.done('overlap')).toBe(true);
    expect(s.has('handed-over')).toBe(false);
    s.desk({ op: 'on_air', identity: 'eilidh', on: false }, t);
    t = run(s, t, t + 300);
    expect(s.has('handed-over')).toBe(true);
    expect(s.view(t).show.title).toBe('Steeped');
    expect(s.speaking?.line.id).toBe('t-ho-steeped');
    t = run(s, t, t + 9_000);
    expect(s.has('his')).toBe(true);
    const steps = scenarioFor('handover').steps(s, { hear: 'studio', listened: false, heardListenersAfterDump: false });
    expect(steps.every((x) => x.done)).toBe(true);
  });

  it('hands over with the one key’s two presses, and the seat follows the air', () => {
    const s = new Station(T0, cat, scenarioFor('handover'));
    const steps = () => scenarioFor('handover').steps(s, { hear: 'studio', listened: false, heardListenersAfterDump: false });
    const press = (t: number) => {
      // What ⇧⏎ and its button do (lib/actions.ts): the step, then the desk commands.
      const v = s.view(t);
      const step = handoverStep(v.sources, v.seats.presenter, pending);
      if (step.kind === 'on') {
        s.desk({ op: 'on_air', identity: step.dj.identity, on: true }, t);
        pending = step.dj.identity;
      } else if (step.kind === 'off') {
        for (const x of step.off) s.desk({ op: 'on_air', identity: x.identity, on: false }, t);
        pending = null;
      }
      return step.kind;
    };
    let pending: string | null = null;
    s.begin(T0);
    s.desk({ op: 'on_air', identity: 'eilidh', on: true }, T0);
    let t = run(s, T0, T0 + 9_000);
    expect(handoverButton(handoverStep(s.view(t).sources, s.view(t).seats.presenter, pending))?.label).toBe('Bring Finlay on air');
    // The first press, with no cue and no count-in: the steps before it are done by it.
    expect(press(t)).toBe('on');
    t = run(s, t, t + 300);
    expect(s.open('eilidh') && s.open('finlay')).toBe(true);
    expect(s.view(t).seats.presenter).toBe('eilidh');
    expect(steps().find((x) => !x.done)?.id).toBe('off');
    expect(handoverButton(handoverStep(s.view(t).sources, s.view(t).seats.presenter, pending))).toMatchObject({ label: 'Hand over to Finlay', then: 'Eilidh comes off air.' });
    // The second press at once, before the hellos are said: Finlay finishes the overlap alone.
    expect(press(t)).toBe('off');
    t = run(s, t, t + 300);
    expect(s.open('eilidh')).toBe(false);
    expect(s.view(t).seats).toMatchObject({ presenter: 'finlay', presenter_name: 'Finlay Grant' });
    expect(handoverButton(handoverStep(s.view(t).sources, s.view(t).seats.presenter, pending))).toBeNull();
    t = run(s, t, t + 25_000);
    expect(s.has('handed-over')).toBe(true);
    expect(s.view(t).show.title).toBe('Steeped');
    expect(steps().every((x) => x.done)).toBe(true);
    expect(press(t)).toBe('done');
  });

  it('names ⇧⏎ on the rundown’s handover row, so the desk puts the button there', () => {
    for (const id of ['next-dj', 'handover']) {
      const rows = new Station(T0, cat, scenarioFor(id)).view(T0).show.rundown;
      expect(rows.find((r) => r.title === 'Handover')?.key, id).toBe(HANDOVER_KEY);
    }
  });

  it('waits for the overlap: taking yourself off before the next DJ is on leaves the show as it was', () => {
    const s = new Station(T0, cat, scenarioFor('handover'));
    s.begin(T0);
    s.desk({ op: 'on_air', identity: 'eilidh', on: true }, T0);
    let t = run(s, T0, T0 + 9_000);
    s.desk({ op: 'on_air', identity: 'eilidh', on: false }, t);
    s.desk({ op: 'on_air', identity: 'finlay', on: true }, t);
    t = run(s, t, t + 20_000);
    expect(s.has('handed-over')).toBe(false);
    expect(s.view(t).show.title).toBe('The Morning Kettle');
  });
});
