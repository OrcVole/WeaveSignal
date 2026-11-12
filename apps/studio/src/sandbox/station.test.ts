// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The sandbox's simulated station (ADR-0212): commands in, the view out, by the control
// plane's rules (crates/control/src/studio.rs), the show's step among them (ADR-0271,
// crates/control/src/steps.rs), so the sandbox cannot teach a desk the product would refuse.
import { describe, expect, it } from 'vitest';
import { catalogueFor, catalogueFrom, hiddenSeat, INVITE_TO_JOIN_MS, OTHER_SHOW, Refusal, Station, type PackCart } from './station.ts';
import { SHOW } from './scenarios/show.ts';
import { scenarioFor } from './scenarios/index.ts';
import { STEPS } from '../lib/steps.ts';
import pack from '../../../../assets/carts-demo/carts.json';

const cat = catalogueFrom(pack as { banks: Record<string, string>; carts: PackCart[] });
const T0 = Date.UTC(2026, 10, 4, 7, 0);

function run(s: Station, from: number, to: number): number {
  for (let t = from; t <= to; t += 100) s.tick(t);
  return to;
}

describe('the sandbox station', () => {
  it('seats the presenter on strip 1 and the guest on strip 2', () => {
    const v = new Station(T0, cat).view(T0);
    expect(v.sources.map((x) => [x.slot, x.identity])).toEqual([
      [1, 'eilidh'],
      [2, 'gregor'],
    ]);
    expect(v.seats).toMatchObject({ presenter: 'eilidh', screener: 'shona' });
  });

  it('waits for Eilidh on air and the jingle fired and finished before anyone speaks', () => {
    const s = new Station(T0, cat);
    run(s, T0, T0 + 5_000);
    expect(s.speaking).toBeNull();
    s.desk({ op: 'on_air', identity: 'eilidh', on: true }, T0 + 5_000);
    // On air but no jingle yet: she waits, so the guide's jingle step is not skipped.
    run(s, T0 + 5_000, T0 + 15_000);
    expect(s.speaking).toBeNull();
    s.carts({ op: 'fire', cart: 'jingle-1' }, T0 + 15_000);
    run(s, T0 + 15_000, T0 + 15_200);
    expect(s.speaking).toBeNull(); // the jingle is still sounding
    run(s, T0 + 15_200, T0 + 40_000);
    expect(s.done('open') || s.speaking?.line.id === 'l-open' || s.speaking?.line.id === 'l-intro-gregor').toBe(true);
  });

  it('takes a caller from the queue through screening to strip 4 and on air', () => {
    const s = new Station(T0, cat);
    s.ring('niall', T0);
    s.tick(T0 + 100);
    expect(s.view(T0 + 100).callers[0]).toMatchObject({ identity: 'niall', state: 'waiting', slot: null });
    s.screener({ op: 'screen', identity: 'niall' }, T0 + 200);
    s.screener({ op: 'notes', identity: 'niall', line: 'Bus lanes' }, T0 + 300);
    s.screener({ op: 'send', identity: 'niall' }, T0 + 400);
    let v = s.view(T0 + 400);
    expect(v.sources.find((x) => x.identity === 'niall')).toMatchObject({ slot: 4, caller: { line: 'Bus lanes', sent: true, taken: false } });
    expect(v.notices.at(-1)?.title).toBe('Niall is ready for you');
    s.desk({ op: 'on_air', identity: 'niall', on: true }, T0 + 500);
    v = s.view(T0 + 500);
    expect(v.sources.find((x) => x.identity === 'niall')?.caller?.taken).toBe(true);
    expect(() => s.screener({ op: 'back', identity: 'niall' }, T0 + 600)).toThrow(Refusal);
  });

  it('a decline says the desk is not taking callers until sent again', () => {
    const s = new Station(T0, cat);
    s.ring('niall', T0);
    s.tick(T0 + 100);
    s.screener({ op: 'send', identity: 'niall' }, T0 + 200);
    s.screener({ op: 'decline', identity: 'niall', reason: 'not now' }, T0 + 300);
    expect(s.view(T0 + 300).desk_taking_callers).toBe(false);
    s.screener({ op: 'send', identity: 'niall' }, T0 + 400);
    expect(s.view(T0 + 400).desk_taking_callers).toBe(true);
  });

  it('a dump empties the delay, which builds back to its target', () => {
    const s = new Station(T0, cat);
    s.desk({ op: 'dump' }, T0);
    expect(s.view(T0).desk.delay).toMatchObject({ delay_ms: 0, rebuilding: true, dumps: 1 });
    run(s, T0, T0 + 40_000);
    expect(s.view(T0 + 40_000).desk.delay).toMatchObject({ delay_ms: 8_000, rebuilding: false });
  });

  it('marks a caller whose account was banned before, by identity, as the station setting does', () => {
    const s = new Station(T0, cat, { ...SHOW, bans: { niall: { date: '2026-09-12', reason: 'Abuse on air' } } });
    s.ring('niall', T0);
    s.ring('jessie', T0);
    s.tick(T0 + 100);
    const v = s.view(T0 + 100);
    expect(v.callers.find((c) => c.identity === 'niall')?.banned_before).toEqual({ date: '2026-09-12', reason: 'Abuse on air' });
    expect(v.callers.find((c) => c.identity === 'jessie')?.banned_before).toBeNull();
  });

  it('a degraded link stays in the room and on its strip; a dropped one keeps the strip for 30 s and the place for 5 min', () => {
    const s = new Station(T0, cat);
    s.ring('niall', T0);
    s.tick(T0 + 100);
    s.screener({ op: 'send', identity: 'niall' }, T0 + 200);
    s.desk({ op: 'on_air', identity: 'niall', on: true }, T0 + 300);
    s.degrade('niall', true, T0 + 400);
    expect(s.view(T0 + 400).sources.find((x) => x.identity === 'niall')).toMatchObject({ slot: 4, linked: false, present: true });
    expect(s.view(T0 + 400).callers[0]).toMatchObject({ linked: false, dropped_ms: null });
    s.dropLink('niall', T0 + 1_000);
    let v = s.view(T0 + 3_000);
    // On air, the desk keeps them on their strip while it keeps them at all (studio.rs).
    expect(v.sources.find((x) => x.identity === 'niall')).toMatchObject({ slot: 4, present: false, away_ms: 2_000 });
    expect(v.callers[0]).toMatchObject({ dropped_ms: T0 + 1_000, present: false });
    expect(v.notices.at(-1)).toMatchObject({ surface: 'screener', tone: 'warning', kind: 'caller.dropped', title: "Niall's link dropped" });
    expect(() => s.screener({ op: 'send', identity: 'niall' }, T0 + 3_000)).toThrow(Refusal);
    run(s, T0 + 3_000, T0 + 32_000);
    v = s.view(T0 + 32_000);
    expect(v.sources.find((x) => x.identity === 'niall')).toBeUndefined();
    expect(v.callers[0]).toMatchObject({ identity: 'niall', dropped_ms: T0 + 1_000, slot: null });
    s.returnLink('niall', T0 + 40_000);
    v = s.view(T0 + 40_000);
    expect(v.callers[0]).toMatchObject({ state: 'ready', dropped_ms: null, present: true, linked: true, slot: 4 });
  });

  it('a dropped place is let go after 5 min, or released by the screener', () => {
    const s = new Station(T0, cat);
    s.ring('niall', T0);
    s.ring('jessie', T0);
    s.tick(T0 + 100);
    s.dropLink('niall', T0 + 200);
    expect(() => s.screener({ op: 'release', identity: 'jessie' }, T0 + 300)).toThrow(Refusal);
    s.screener({ op: 'release', identity: 'niall' }, T0 + 300);
    expect(s.view(T0 + 300).callers.map((c) => c.identity)).toEqual(['jessie']);
    s.dropLink('jessie', T0 + 400);
    run(s, T0 + 400, T0 + 5 * 60_000 + 1_000);
    expect(s.view(T0 + 5 * 60_000 + 1_000).callers).toHaveLength(0);
  });

  it('in auto mode listeners do not hear the desk, and the view says so', () => {
    const s = new Station(T0, cat);
    expect(s.view(T0).mode).toBe('live');
    s.setMode('auto', T0);
    expect(s.view(T0).mode).toBe('auto');
    expect(s.listenersHearDesk()).toBe(false);
    expect(s.view(T0).notices.at(-1)?.title).toBe('Automation holds the air');
    s.setPlayout(false, T0 + 100);
    s.setMode('live', T0 + 200);
    expect(s.listenersHearDesk()).toBe(false);
    s.setPlayout(true, T0 + 300);
    expect(s.listenersHearDesk()).toBe(true);
  });

  it("the tab's microphone off means the DJ is not heard, however open the desk is", () => {
    const s = new Station(T0, cat);
    s.desk({ op: 'on_air', identity: 'eilidh', on: true }, T0);
    expect(s.heardFromYou()).toBe(true);
    s.setMic(false, T0);
    expect(s.heardFromYou()).toBe(false);
    expect(s.view(T0).sources[0].on_air).toBe(true);
    s.setMic(true, T0);
    s.desk({ op: 'mute', identity: 'eilidh', on: true }, T0);
    expect(s.heardFromYou()).toBe(false);
  });

  it('timers fire in order from the moment the sound starts, not from when the page opened', () => {
    const s = new Station(T0, cat, { ...SHOW, start: (s, now) => s.ring('calum', now + 2_000) });
    run(s, T0, T0 + 10_000);
    expect(s.callerState('calum')).toBeNull();
    s.begin(T0 + 10_000);
    const seen: number[] = [];
    s.at(T0 + 11_500, () => seen.push(2));
    s.at(T0 + 11_000, () => seen.push(1));
    run(s, T0 + 10_000, T0 + 12_100);
    expect(seen).toEqual([1, 2]);
    expect(s.callerState('calum')).toBe('waiting');
  });

  it('a booth conversation is cut short when its caller is taken to air', () => {
    const s = new Station(T0, cat);
    s.ring('niall', T0);
    s.tick(T0 + 100);
    s.screener({ op: 'screen', identity: 'niall' }, T0 + 200);
    run(s, T0 + 200, T0 + 4_000);
    expect(s.beat?.beat.id).toBe('niall-booth');
    s.screener({ op: 'send', identity: 'niall' }, T0 + 4_000);
    s.desk({ op: 'on_air', identity: 'niall', on: true }, T0 + 4_100);
    // Shona's lines still play (she can talk to anyone); his next line is where it ends.
    run(s, T0 + 4_100, T0 + 12_000);
    expect(s.done('niall-booth')).toBe(true);
    expect(s.beat?.beat.id).toBe('niall-air');
  });

  it('plays a cart and stops it, and a new bed replaces the old', () => {
    const s = new Station(T0, cat);
    s.carts({ op: 'toggle', cart: 'jingle-1' }, T0);
    expect(s.view(T0).carts.sounding).toHaveLength(1);
    s.carts({ op: 'toggle', cart: 'bed-1' }, T0 + 100);
    s.carts({ op: 'toggle', cart: 'bed-2' }, T0 + 200);
    const beds = s.view(T0 + 200).carts.sounding.filter((x) => x.pool === 'bed');
    expect(beds.map((b) => [b.cart, b.stopping])).toEqual([
      ['bed-1', true],
      ['bed-2', false],
    ]);
    run(s, T0 + 200, T0 + 5_000);
    expect(s.view(T0 + 5_000).carts.sounding.map((x) => x.cart)).toEqual(['bed-2']);
  });

  // ---- the show's step (ADR-0271): the same rules as the control plane's steps.rs ----

  const at = (id: string) => new Station(T0, cat, scenarioFor(id));

  it('carries the step in its view, with the scenario’s show on air and nothing counted for the chat', () => {
    const v = at('mic').view(T0);
    expect(v.step).toEqual({ step: 'solo', show: { slug: 'the-morning-kettle', name: 'The Morning Kettle' }, on_air: true, next: null, may_change: true });
    expect(v.chat.held).toBeNull();
    expect(v.chat.open_reports).toBeNull();
    expect(at('show').view(T0).step.step).toBe('full');
  });

  it('closes call-in below Callers: nobody can join the queue', () => {
    // The same people at each step: only the step decides whether Niall can ring.
    for (const step of ['solo', 'guests'] as const) {
      const closed = new Station(T0, cat, { ...scenarioFor('alone'), step });
      closed.arrive('niall', T0);
      expect(closed.view(T0).callers, step).toHaveLength(0);
      expect(closed.person('niall'), step).toBeUndefined();
    }
    const callers = at('alone');
    callers.arrive('niall', T0);
    expect(callers.view(T0).callers.map((c) => c.identity)).toEqual(['niall']);
  });

  it('refuses an invitation to a seat the step hides, in the station’s own words', () => {
    const refusal = (s: Station, body: object): [number, string] | null => {
      try {
        s.makeInvite(body, T0);
        return null;
      } catch (e) {
        if (e instanceof Refusal) return [e.status, e.message];
        throw e;
      }
    };
    const solo = at('mic');
    expect(refusal(solo, { role: 'guest', name: 'Gregor Stewart' })).toEqual([422, 'This show runs at Solo, so it has no seat for a guest or a second presenter. They come with Guests.']);
    expect(refusal(solo, { role: 'presenter', name: 'Shona Campbell' })).toEqual([422, 'This show runs at Solo, so it has no seat for a guest or a second presenter. They come with Guests.']);
    expect(refusal(solo, { role: 'dj', name: 'Finlay Grant' })).toEqual([422, "An invitation for the next show's DJ comes with Full."]);
    const guests = at('guest');
    expect(refusal(guests, { role: 'guest', name: 'Gregor Stewart' })).toBeNull();
    expect(refusal(guests, { role: 'dj', name: 'Finlay Grant' })).toEqual([422, "An invitation for the next show's DJ comes with Full."]);
    expect(refusal(at('alone'), { role: 'dj', name: 'Finlay Grant' })?.[0]).toBe(422);
    expect(refusal(at('next-dj'), { role: 'dj', name: 'Finlay Grant' })).toBeNull();
    // A role must be named, unless the invitation is for yourself.
    expect(refusal(guests, { name: 'Gregor Stewart' })?.[0]).toBe(422);
    // The seats table, as steps.rs has it.
    expect(STEPS.map((s) => hiddenSeat(s, 'presenter', true))).toEqual([null, null, null, null]);
    expect(STEPS.map((s) => hiddenSeat(s, 'guest') === null)).toEqual([false, true, true, true]);
    expect(STEPS.map((s) => hiddenSeat(s, 'caller') === null)).toEqual([false, false, true, true]);
    expect(STEPS.map((s) => hiddenSeat(s, 'dj') === null)).toEqual([false, false, false, true]);
    expect(STEPS.map((s) => hiddenSeat(s, 'screener') === null)).toEqual([false, false, false, true]);
  });

  it('makes the Solo DJ’s own link at every step: a presenter’s, for the chair you sit in', () => {
    for (const id of ['mic', 'guest', 'alone', 'show']) {
      const inv = at(id).makeInvite({ for_me: true }, T0);
      expect(inv, id).toMatchObject({ role: 'presenter', identity: 'presenter-me-eilidh', name: 'Eilidh Fraser-MacLeod', expires_at_ms: T0 + 4 * 3600_000 });
      expect(inv.link).toBe(`/join/#${inv.pass}`);
    }
    expect(() => at('mic').makeInvite({ for_me: true, role: 'guest' }, T0)).toThrow(/an invitation for yourself is a presenter's/);
  });

  it('checks a pass again when it is used: an invited guest stays out once the step has hidden the seat', () => {
    const s = at('guest');
    s.makeInvite({ role: 'guest', name: 'Gregor Stewart' }, T0);
    s.setStep('the-morning-kettle', 'solo', T0 + 1_000);
    s.tick(T0 + INVITE_TO_JOIN_MS + 500);
    expect(s.person('gregor')).toBeUndefined();
    // At Guests he joins from the same kind of invitation, on air at once.
    const g = at('guest');
    g.makeInvite({ role: 'guest', name: 'Gregor Stewart' }, T0);
    g.tick(T0 + INVITE_TO_JOIN_MS + 500);
    expect(g.person('gregor')).toMatchObject({ role: 'guest', on_air: true });
    expect(g.view(T0 + INVITE_TO_JOIN_MS + 500).sources.find((x) => x.identity === 'gregor')?.slot).toBe(2);
  });

  it('refuses Dump below Callers, the handover below Full and the picture below Full, each with its sentence', () => {
    const says = (s: Station, c: Parameters<Station['desk']>[0]): [number, string] | null => {
      try {
        s.desk(c, T0);
        return null;
      } catch (e) {
        if (e instanceof Refusal) return [e.status, e.message];
        throw e;
      }
    };
    expect(says(at('mic'), { op: 'dump' })).toEqual([409, 'Dump is off at Solo: there is nothing for it to do. It comes on at Callers.']);
    expect(says(at('guest'), { op: 'dump' })).toEqual([409, 'Dump is off at Guests: there is nothing for it to do. It comes on at Callers.']);
    expect(says(at('alone'), { op: 'dump' })).toBeNull();
    expect(says(at('mic'), { op: 'picture', mode: 'slate' })).toEqual([409, 'The picture is held on the slate at Solo: switching it comes with Full.']);
    // The handover is the next DJ going on air: a DJ left in the room below Full cannot be put on.
    const s = at('handover');
    expect(says(s, { op: 'on_air', identity: 'finlay', on: true })).toBeNull();
    s.desk({ op: 'on_air', identity: 'finlay', on: false }, T0);
    s.setStep('the-morning-kettle', 'callers', T0);
    expect(says(s, { op: 'on_air', identity: 'finlay', on: true })).toEqual([409, 'The handover is off at Callers: it comes on at Full.']);
    // He is still in the room: a step closes doors, it never hides people.
    expect(s.person('finlay')).toBeDefined();
    expect(says(s, { op: 'cue', identity: 'finlay', on: true })).toBeNull();
  });

  it('a step moved down lets the waiting go and keeps whoever holds the strip; moved up, the lines open at once', () => {
    const s = at('alone');
    s.arrive('niall', T0);
    s.arrive('jessie', T0 + 1_000);
    s.screener({ op: 'send', identity: 'niall' }, T0 + 2_000);
    expect(s.view(T0 + 2_000).sources.find((x) => x.identity === 'niall')?.slot).toBe(4);
    const changed = s.setStep('the-morning-kettle', 'guests', T0 + 3_000);
    expect(changed).toMatchObject({ show: 'the-morning-kettle', name: 'The Morning Kettle', step: 'guests', previous: 'callers', station: { step: 'guests', may_change: true } });
    const v = s.view(T0 + 3_000);
    expect(v.callers.map((c) => c.identity)).toEqual(['niall']);
    expect(s.released).toEqual(['jessie']);
    expect(s.person('jessie')).toBeUndefined();
    expect(v.sources.find((x) => x.identity === 'niall')?.slot).toBe(4);
    // Every window is told who moved it.
    expect(v.notices.at(-1)).toMatchObject({ surface: 'desk', kind: 'step.moved', title: 'Eilidh moved The Morning Kettle to Guests.' });
    // The lines are closed now, and open again at once when the step rises.
    s.arrive('jessie', T0 + 4_000);
    expect(s.callerState('jessie')).toBeNull();
    s.setStep('the-morning-kettle', 'callers', T0 + 5_000);
    s.arrive('jessie', T0 + 6_000);
    expect(s.callerState('jessie')).toBe('waiting');
    // The same step again changes nothing and tells nobody.
    const before = s.view(T0 + 6_000).notices.length;
    expect(s.setStep('the-morning-kettle', 'callers', T0 + 7_000).previous).toBe('callers');
    expect(s.view(T0 + 7_000).notices.length).toBe(before);
  });

  it('answers an unknown show or an unknown step as the route does', () => {
    const s = at('mic');
    expect(() => s.setStep('steeped', 'guests', T0)).toThrow(/no show `steeped`/);
    expect(() => s.setStep('the-morning-kettle', 'telephony', T0)).toThrow(/step is `solo`, `guests`, `callers` or `full`/);
    expect(s.view(T0).step.step).toBe('solo');
  });
});

describe('the sandbox station’s cart wall per show (1.0.2)', () => {
  it('holds The Morning Kettle’s own bank 1 beside the station’s four, as the API lists them', () => {
    expect(cat.banks.map((b) => [b.number, b.show])).toEqual([
      [1, 'the-morning-kettle'],
      [2, null],
      [3, null],
      [4, null],
      [5, null],
    ]);
    const station = catalogueFor(cat, null);
    expect(station.banks.map((b) => b.number)).toEqual([2, 3, 4, 5]);
    expect(station.carts.every((c) => c.show === null)).toBe(true);
    const kettle = catalogueFor(cat, 'the-morning-kettle');
    expect(kettle.show).toBe('the-morning-kettle');
    expect(kettle.banks).toHaveLength(5);
  });

  it('fires a show’s own cart only in that show, and names what sounds', () => {
    const s = new Station(T0, cat);
    s.carts({ op: 'fire', cart: 'jingle-1' }, T0);
    expect(s.view(T0).carts.sounding[0].label).toBe(cat.carts.find((c) => c.id === 'jingle-1')!.label);
    const other = { ...cat, carts: cat.carts.map((c) => (c.id === 'bed-1' ? { ...c, show: 'steeped' } : c)) };
    const t = new Station(T0, other);
    let refused: unknown = null;
    try {
      t.carts({ op: 'fire', cart: 'bed-1' }, T0);
    } catch (e) {
      refused = e;
    }
    expect(refused).toBeInstanceOf(Refusal);
    expect((refused as Refusal).status).toBe(409);
    expect((refused as Refusal).message).toBe(OTHER_SHOW(other.carts.find((c) => c.id === 'bed-1')!.label));
  });
});
