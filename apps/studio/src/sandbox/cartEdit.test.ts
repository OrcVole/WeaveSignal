// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The sandbox's cart wall changed from the page (1.2.1): the routes answered on the catalogue
// the page holds, with the station's own refusals, so the wall's dialogs behave in "Try the
// desk" as they do on a station. The catalogue is made for the test, not read from the pack.
import { describe, expect, it } from 'vitest';
import { cartChange, isCartChange, isUpload } from './cartEdit.ts';
import { catalogueFor } from './station.ts';
import type { Cart, Catalogue } from '../../../../packages/ui/carts.ts';

const cart = (bank: number, slot: number, label: string, show: string | null = null): Cart => ({
  id: `${show ?? 'station'}-${bank}-${slot}`, bank, slot, label, detail: '', mode: 'one_shot', file: `${label}.opus`, duration_ms: 3_000,
  gain_db: 0, loudness_lufs: -16, true_peak_dbtp: -1, ramp_ms: null, origin: '', licence: '', missing: false, show,
});
const fresh = (): Catalogue => ({
  version: 1,
  show: null,
  banks: [{ number: 1, name: 'Steeped', colour_slot: 1, show: 'steeped' }, { number: 2, name: 'Reactions', colour_slot: 2, show: null }],
  carts: [cart(1, 1, 'Woof', 'steeped'), cart(2, 1, 'Applause'), cart(2, 2, 'Rimshot')],
  loudness_lufs: -16,
  max_upload_mb: 64,
  max_seconds: 600,
});
const q = (s = '') => new URLSearchParams(s);
const ask = (cat: Catalogue, method: string, path: string, body: Record<string, unknown> = {}, query = q(), decoded: { file: string; durationMs: number } | null = null) =>
  cartChange(cat, method, path, query, body, decoded, (owner) => catalogueFor(cat, owner));

describe('the sandbox’s cart wall, changed from the page', () => {
  it('knows its own routes', () => {
    expect(isCartChange('PATCH', '/api/v1/carts/banks/2/slots/1')).toBe(true);
    expect(isCartChange('DELETE', '/api/v1/shows/steeped/carts/banks/1')).toBe(true);
    expect(isCartChange('GET', '/api/v1/carts')).toBe(false);
    expect(isCartChange('POST', '/api/v1/carts/commands')).toBe(false);
    expect(isUpload('PUT', '/api/v1/shows/steeped/carts/banks/1/slots/9')).toBe(true);
    expect(isUpload('PUT', '/api/v1/carts/banks/3')).toBe(false);
  });

  it('changes a cart where it is, moves it to an empty slot, and never onto another', () => {
    const cat = fresh();
    const a = ask(cat, 'PATCH', '/api/v1/carts/banks/2/slots/1', { label: ' Applause, long ', mode: 'sweeper', ramp_ms: 1_000 });
    expect(a.status).toBe(200);
    expect(a.body).toMatchObject({ id: 'station-2-1', label: 'Applause, long', mode: 'sweeper', ramp_ms: 1_000, slot: 1 });
    expect(cat.version).toBe(2);
    expect(ask(cat, 'PATCH', '/api/v1/carts/banks/2/slots/1', { slot: 2 })).toMatchObject({ status: 409, body: { error: 'bank 2 slot 2 already holds “Rimshot”; move or remove that cart first' } });
    expect(ask(cat, 'PATCH', '/api/v1/carts/banks/2/slots/1', { slot: 24, ramp_ms: null }).body).toMatchObject({ id: 'station-2-1', slot: 24, ramp_ms: null });
    // An empty slot, a ramp longer than the cart, a bank nobody made, a show's cart by the station's route.
    expect(ask(cat, 'PATCH', '/api/v1/carts/banks/2/slots/1', { label: 'x' }).status).toBe(404);
    expect(ask(cat, 'PATCH', '/api/v1/carts/banks/2/slots/2', { ramp_ms: 3_001 }).status).toBe(422);
    expect(ask(cat, 'PATCH', '/api/v1/carts/banks/2/slots/2', { bank: 4 }).status).toBe(404);
    expect(ask(cat, 'PATCH', '/api/v1/carts/banks/1/slots/1', { label: 'x' }).status).toBe(404);
    expect(ask(cat, 'PATCH', '/api/v1/shows/steeped/carts/banks/1/slots/1', { label: 'Woof, twice' }).body).toMatchObject({ label: 'Woof, twice', show: 'steeped' });
  });

  it('takes a sound the page decoded, and refuses one it could not or one too long', () => {
    const cat = fresh();
    const words = q('label=Kettle&mode=bed&origin=kettle.wav');
    const made = ask(cat, 'PUT', '/api/v1/carts/banks/2/slots/7', {}, words, { file: 'yours-1.snd', durationMs: 8_000 });
    expect(made.status).toBe(201);
    expect(made.body).toMatchObject({ bank: 2, slot: 7, label: 'Kettle', mode: 'bed', file: 'yours-1.snd', duration_ms: 8_000, loudness_lufs: null, origin: 'kettle.wav', show: null });
    expect(cat.carts).toHaveLength(4);
    // In place of what was there.
    ask(cat, 'PUT', '/api/v1/carts/banks/2/slots/7', {}, q('label=Kettle%202&mode=one_shot'), { file: 'yours-2.snd', durationMs: 1_000 });
    expect(cat.carts.filter((c) => c.bank === 2 && c.slot === 7).map((c) => c.label)).toEqual(['Kettle 2']);
    expect(ask(cat, 'PUT', '/api/v1/carts/banks/2/slots/8', {}, words, null)).toMatchObject({ status: 422 });
    expect((ask(cat, 'PUT', '/api/v1/carts/banks/2/slots/8', {}, words, null).body as { error: string }).error).toMatch(/^the file was refused: not an audio file/);
    expect((ask(cat, 'PUT', '/api/v1/carts/banks/2/slots/8', {}, words, { file: 'x', durationMs: 601_000 }).body as { error: string }).error).toBe('the file was refused: the file is longer than 600 s');
    expect((ask(cat, 'PUT', '/api/v1/carts/banks/4/slots/1', {}, words, { file: 'x', durationMs: 1 }).body as { error: string }).error).toBe('bank 4 has not been made; make it first');
  });

  it('makes and renames banks, empties slots, and removes only a show’s own bank', () => {
    const cat = fresh();
    const made = ask(cat, 'PUT', '/api/v1/carts/banks/4', { name: ' Idents ', colour_slot: 11 });
    expect(made.status).toBe(200);
    expect((made.body as Catalogue).banks.find((b) => b.number === 4)).toEqual({ number: 4, name: 'Idents', colour_slot: 11, show: null });
    expect(ask(cat, 'PUT', '/api/v1/carts/banks/4', { name: '', colour_slot: 11 }).status).toBe(422);
    expect(ask(cat, 'PUT', '/api/v1/carts/banks/4', { name: 'Idents', colour_slot: 13 }).status).toBe(422);
    expect(ask(cat, 'PUT', '/api/v1/shows/steeped/carts/banks/4', { name: 'Dog woofs', colour_slot: 5 }).status).toBe(200);
    expect(cat.banks.filter((b) => b.number === 4)).toHaveLength(2);
    expect((ask(cat, 'DELETE', '/api/v1/carts/banks/2/slots/2').body as Catalogue).carts.map((c) => c.label)).toEqual(['Applause']);
    expect(ask(cat, 'DELETE', '/api/v1/carts/banks/2/slots/2').status).toBe(422);
    expect(ask(cat, 'DELETE', '/api/v1/carts/banks/2').status).toBe(422);
    const gone = ask(cat, 'DELETE', '/api/v1/shows/steeped/carts/banks/1');
    expect(gone.status).toBe(200);
    expect(cat.carts.some((c) => c.show === 'steeped')).toBe(false);
    expect(ask(cat, 'DELETE', '/api/v1/shows/steeped/carts/banks/1').status).toBe(404);
  });
});
