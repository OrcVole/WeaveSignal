// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A schedule time's two arrows (`schedule/time.ts`, `TimeField.svelte`) and an episode's Move…
// saved at once (`schedule/move.ts`), tested without a page: where a press takes a time, what
// Move sends and what Undo sends back, and what the tab's line says.
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import TimeField from './schedule/TimeField.svelte';
import { instant } from './schedule/model.ts';
import { moveAsk, movedSaid, ruleAsk, wayBack } from './schedule/move.ts';
import { STEP_MINUTES, stepTime, stepWords } from './schedule/time.ts';

const ZONE = 'Europe/London';
const at = (m: number, d: number, hh: number, mm = 0) => instant({ y: 2026, m, d, hh, mm }, ZONE);

describe('a schedule time’s arrows', () => {
  it('steps a quarter of an hour from a time on a quarter', () => {
    expect(STEP_MINUTES).toBe(15);
    expect(stepTime('18:00', 1)).toEqual({ time: '18:15', carry: 0 });
    expect(stepTime('18:15', 1)).toEqual({ time: '18:30', carry: 0 });
    expect(stepTime('18:45', 1)).toEqual({ time: '19:00', carry: 0 });
    expect(stepTime('18:00', -1)).toEqual({ time: '17:45', carry: 0 });
    expect(stepTime('18:30', -1)).toEqual({ time: '18:15', carry: 0 });
  });

  it('takes an odd minute to the quarter that way, never past one', () => {
    expect(stepTime('18:07', 1)).toEqual({ time: '18:15', carry: 0 });
    expect(stepTime('18:07', -1)).toEqual({ time: '18:00', carry: 0 });
    expect(stepTime('18:14', 1)).toEqual({ time: '18:15', carry: 0 });
    expect(stepTime('18:16', -1)).toEqual({ time: '18:15', carry: 0 });
    expect(stepTime('18:59', 1)).toEqual({ time: '19:00', carry: 0 });
    expect(stepTime('18:01', -1)).toEqual({ time: '18:00', carry: 0 });
    // Two presses from an odd minute are a quarter apart, as from any other.
    expect(stepTime(stepTime('18:07', 1).time, 1).time).toBe('18:30');
  });

  it('wraps past midnight and says which way, for a date beside it', () => {
    expect(stepTime('23:45', 1)).toEqual({ time: '00:00', carry: 1 });
    expect(stepTime('23:52', 1)).toEqual({ time: '00:00', carry: 1 });
    expect(stepTime('00:00', -1)).toEqual({ time: '23:45', carry: -1 });
    expect(stepTime('00:07', -1)).toEqual({ time: '00:00', carry: 0 });
    expect(stepTime('00:00', 1)).toEqual({ time: '00:15', carry: 0 });
    expect(stepTime('23:30', 1)).toEqual({ time: '23:45', carry: 0 });
  });

  it('reads what a time field holds, and starts an empty one at midnight', () => {
    expect(stepTime('07:30:00', 1).time).toBe('07:45');
    expect(stepTime('7:30', -1).time).toBe('07:15');
    expect(stepTime('', 1)).toEqual({ time: '00:00', carry: 0 });
    expect(stepTime('', -1)).toEqual({ time: '00:00', carry: 0 });
    expect(stepTime('25:00', 1)).toEqual({ time: '00:00', carry: 0 });
    expect(stepTime('12:75', 1)).toEqual({ time: '00:00', carry: 0 });
  });

  it('draws a time field with two named buttons that Tab passes over', () => {
    const { body } = render(TimeField, { props: { value: '21:00', id: 'x-time', label: 'New time, station time' } });
    expect(body).toMatch(/<input[^>]*id="x-time"[^>]*class="time"[^>]*type="time"[^>]*value="21:00"/);
    expect(body).toContain('aria-label="New time, station time"');
    expect(stepWords(-1)).toBe('15 minutes earlier');
    expect(stepWords(1)).toBe('15 minutes later');
    const buttons = body.match(/<button[^>]*>/g) ?? [];
    expect(buttons).toHaveLength(2);
    expect(buttons[0]).toContain('aria-label="15 minutes earlier"');
    expect(buttons[1]).toContain('aria-label="15 minutes later"');
    for (const b of buttons) {
      expect(b).toContain('type="button"');
      expect(b).toContain('tabindex="-1"');
    }
    // Earlier stands before later, and the field before both.
    expect(body.indexOf('<input')).toBeLessThan(body.indexOf('<button'));
  });
});

describe('an episode’s Move…, saved at once', () => {
  const start = at(11, 18, 21);

  it('sends the time alone when only the time changed: an inherited length stays inherited', () => {
    expect(moveAsk({ date: '2026-11-18', time: '20:30', minutes: 120 }, start, 120, ZONE)).toEqual({ patch: { air_at: '2026-11-18T20:30:00+00:00[Europe/London]' } });
    expect(moveAsk({ date: '2026-11-25', time: '21:00', minutes: 120 }, start, 120, ZONE)).toEqual({ patch: { air_at: '2026-11-25T21:00:00+00:00[Europe/London]' } });
  });

  it('sends the length when it changed, with the time, which the station judges a clash by', () => {
    expect(moveAsk({ date: '2026-11-18', time: '20:30', minutes: 90 }, start, 120, ZONE)).toEqual({ patch: { air_at: '2026-11-18T20:30:00+00:00[Europe/London]', minutes: 90 } });
    expect(moveAsk({ date: '2026-11-18', time: '21:00', minutes: 150 }, start, 120, ZONE)).toEqual({ patch: { air_at: '2026-11-18T21:00:00+00:00[Europe/London]', minutes: 150 } });
  });

  it('sends nothing but the time and the length, whatever else the editor holds', () => {
    const ask = moveAsk({ date: '2026-11-19', time: '10:00', minutes: 45 }, start, 120, ZONE);
    expect('patch' in ask && Object.keys(ask.patch).sort()).toEqual(['air_at', 'minutes']);
  });

  it('sends nothing when nothing changed, and says what it cannot read', () => {
    expect(moveAsk({ date: '2026-11-18', time: '21:00', minutes: 120 }, start, 120, ZONE)).toEqual({ same: true });
    expect(moveAsk({ date: '', time: '21:00', minutes: 120 }, start, 120, ZONE)).toEqual({ problem: 'Give a date and a time, such as 20:00.', about: 'when' });
    expect(moveAsk({ date: '2026-11-18', time: '', minutes: 120 }, start, 120, ZONE)).toEqual({ problem: 'Give a date and a time, such as 20:00.', about: 'when' });
    for (const minutes of [null, undefined, '', 0, -5, 1441, 12.5]) {
      expect(moveAsk({ date: '2026-11-18', time: '20:00', minutes }, start, 120, ZONE)).toEqual({ problem: 'Give a length in whole minutes, from 1 to 1,440.', about: 'length' });
    }
  });

  it('gives an episode with no air time its first one', () => {
    expect(moveAsk({ date: '2026-11-18', time: '21:00', minutes: 60 }, null, 60, ZONE)).toEqual({ patch: { air_at: '2026-11-18T21:00:00+00:00[Europe/London]' } });
  });

  it('writes the time in the station’s zone, across the clock change', () => {
    const summer = at(10, 24, 20);
    expect(moveAsk({ date: '2026-10-24', time: '19:30', minutes: 60 }, summer, 60, ZONE)).toEqual({ patch: { air_at: '2026-10-24T19:30:00+01:00[Europe/London]' } });
    expect(moveAsk({ date: '2026-10-25', time: '19:30', minutes: 60 }, summer, 60, ZONE)).toEqual({ patch: { air_at: '2026-10-25T19:30:00+00:00[Europe/London]' } });
  });

  it('the way back holds the same fields as they were: an inherited length goes back to inheriting', () => {
    const ep = { air_at: start / 1000, minutes: null };
    expect(wayBack(ep, { air_at: 'x' }, ZONE)).toEqual({ air_at: '2026-11-18T21:00:00+00:00[Europe/London]' });
    expect(wayBack(ep, { air_at: 'x', minutes: 90 }, ZONE)).toEqual({ air_at: '2026-11-18T21:00:00+00:00[Europe/London]', minutes: null });
    expect(wayBack({ air_at: start / 1000, minutes: 150 }, { air_at: 'x', minutes: null }, ZONE)).toEqual({ air_at: '2026-11-18T21:00:00+00:00[Europe/London]', minutes: 150 });
    expect(wayBack({ air_at: null, minutes: null }, { air_at: 'x' }, ZONE)).toEqual({ air_at: null });
  });

  it('Back to the rule sends the rule’s own time and gives the length back to the rule', () => {
    expect(ruleAsk(start, ZONE)).toEqual({ air_at: '2026-11-18T21:00:00+00:00[Europe/London]', minutes: null });
  });

  it('says where the episode is now, as a show’s dates do, and any clash it made', () => {
    expect(movedSaid(5, at(11, 18, 20, 30), at(11, 18, 22, 30), ZONE)).toBe('Episode 5 is on Wednesday 18 November, 20:30 to 22:30.');
    expect(movedSaid(5, start, at(11, 18, 23), ZONE, { back: true })).toBe('Episode 5 is back on Wednesday 18 November, as the rule has it.');
    expect(movedSaid(4, at(11, 18, 15), at(11, 18, 18), ZONE, { clashes: 'The Tea Break and Schools Hour' })).toBe('Episode 4 is on Wednesday 18 November, 15:00 to 18:00. It now clashes with The Tea Break and Schools Hour.');
    expect(movedSaid(4, at(11, 18, 15), at(11, 18, 18), ZONE, { clashes: '' })).toBe('Episode 4 is on Wednesday 18 November, 15:00 to 18:00.');
  });
});
