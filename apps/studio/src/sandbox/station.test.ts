// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The sandbox's simulated station (ADR-0212): commands in, the view out, by the control
// plane's rules (crates/control/src/studio.rs).
import { describe, expect, it } from 'vitest';
import { catalogueFrom, Refusal, Station, type PackCart } from './station.ts';
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

  it('waits for Eilidh to be on air before anyone speaks', () => {
    const s = new Station(T0, cat);
    run(s, T0, T0 + 5_000);
    expect(s.speaking).toBeNull();
    s.desk({ op: 'on_air', identity: 'eilidh', on: true }, T0 + 5_000);
    run(s, T0 + 5_000, T0 + 5_200);
    expect(s.speaking?.line.id).toBe('l-open');
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
