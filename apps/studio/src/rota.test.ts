// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The rota (1.2; handover-window.md): the station's rows in a person's words, and its key.
import { describe, expect, it } from 'vitest';
import type { RotaRow, RotaView } from './lib/api.ts';
import { inWords, rotaHead, rotaLines } from './lib/rota.ts';
import { KeyEngine, type KeyInput } from '../../../packages/ui/keymap.ts';

const NOW = Date.UTC(2026, 10, 20, 13, 52);
const H = 3600_000;
const row = (o: Partial<RotaRow> = {}): RotaRow => ({ show: 'steeped', show_name: 'Steeped', start_ms: NOW + 8 * 60_000, end_ms: NOW + 8 * 60_000 + 2 * H, hosts: ['Finlay Grant'], from: 'push', state: 'not_connected', ready: false, now: false, ...o });
const view = (airings: RotaRow[]): RotaView => ({ now_ms: NOW, until_ms: NOW + 12 * H, airings });

describe('the rota’s rows', () => {
  it('says when on the station’s clock, and how soon', () => {
    const [r] = rotaLines(view([row()]), 'UTC');
    expect(r.time).toBe('14:00 – 16:00');
    expect(r.when).toBe('in 8 min');
    expect(inWords(128 * 60_000)).toBe('in 2 h 08 min');
    expect(inWords(3 * H)).toBe('in 3 h');
    expect(inWords(20_000)).toBe('in 1 min');
    expect(rotaLines(view([row()]), 'Europe/London')[0].time).toBe('14:00 – 16:00');
    expect(rotaLines(view([row()]), 'America/New_York')[0].time).toBe('09:00 – 11:00');
  });

  it('marks the one on air, and says its host’s state as a chip', () => {
    const [r] = rotaLines(view([row({ show: 'the-long-lunch', show_name: 'The Long Lunch', start_ms: NOW - 112 * 60_000, end_ms: NOW + 8 * 60_000, hosts: ['Duncan Boyd'], from: 'room', state: 'on_air', ready: true, now: true })]), 'UTC');
    expect(r).toMatchObject({ now: true, when: 'on air now', who: 'Duncan Boyd', from: 'in the room' });
    expect(r.chip?.word).toBe('On air');
    expect(r.said).toBe('The Long Lunch, 12:00 to 14:00, on air now. Duncan Boyd, in the room: on air.');
  });

  it('a host not connected is news within the hour, and only where things stand before that', () => {
    const [soon, later] = rotaLines(view([row(), row({ show: 'kettle-drive', show_name: 'Kettle Drive', start_ms: NOW + 128 * 60_000, end_ms: NOW + 248 * 60_000, hosts: ['Heather Douglas'] })]), 'UTC');
    expect(soon.chip?.word).toBe('Not here yet');
    expect(later.chip).toBeNull();
    expect(later.quiet).toBe('Not due yet');
    expect(rotaLines(view([row({ state: 'ready', ready: true })]), 'UTC')[0].chip?.word).toBe('Ready');
    expect(rotaLines(view([row({ state: 'connecting' })]), 'UTC')[0].chip?.word).toBe('Connecting');
  });

  it('a show nobody is down for says so, and claims no state for anyone', () => {
    const [r] = rotaLines(view([row({ hosts: [], from: 'room' })]), 'UTC');
    expect(r).toMatchObject({ nobody: true, who: '', from: null, chip: null, quiet: null });
    expect(r.said).toBe('Steeped, 14:00 to 16:00, in 8 min. Nobody is down for it.');
  });

  it('the head counts the shows and those with nobody down', () => {
    expect(rotaHead(view([row(), row({ show: 'x', hosts: [] })]), 'UTC')).toBe('13:52 to 01:52 · 2 shows · 1 with nobody down');
    expect(rotaHead(view([row()]), 'UTC')).toBe('13:52 to 01:52 · 1 show');
    expect(rotaLines(null, 'UTC')).toEqual([]);
  });
});

describe('the rota’s key', () => {
  const key = (code: string, mods: Partial<KeyInput> = {}): KeyInput => ({ code, ctrl: false, shift: false, alt: false, meta: false, repeat: false, ...mods });
  const schedule = { tab: 'schedule' as const, inTextField: false };
  it('R on the Schedule tab opens the rota; ⌃R stays Restart playout’s', () => {
    const k = new KeyEngine();
    expect(k.down(key('KeyR'), schedule, 0).action).toEqual({ type: 'view', view: 'rota' });
    expect(k.down(key('KeyR', { ctrl: true }), schedule, 0).action).not.toEqual({ type: 'view', view: 'rota' });
  });
});
