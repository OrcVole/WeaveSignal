// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The sandbox's simulated station (ADR-0212): commands in, the view out, by the control
// plane's rules (crates/control/src/studio.rs).
import { describe, expect, it } from 'vitest';
import { catalogueFrom, Refusal, Station, type PackCart } from './station.ts';
import { SHOW } from './scenarios/show.ts';
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
});
