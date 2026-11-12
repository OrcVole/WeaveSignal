// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The show's step in the show editor (ADR-0271, `calendar.show-step`): the field under the
// name, with the longer lines that say what each step adds and the hint that says what "off"
// means; a new show starts at Solo, and a show from before steps reads as Full.
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import ShowEditor from './schedule/ShowEditor.svelte';

/** One panel's markup, from its name to the next panel's. */
function part(body: string, name: string): string {
  const at = body.indexOf(`data-panel="${name}"`);
  expect(at, name).toBeGreaterThan(-1);
  const next = body.indexOf('data-panel="', at + 1);
  return body.slice(at, next < 0 ? undefined : next);
}

describe('the show editor', () => {
  it('has the Step field under the name, with the longer lines and what "off" means', () => {
    const { body } = render(ShowEditor, { props: { slug: null, show: null, onSaved: () => {}, onCard: () => {} } });
    const field = part(body, 'calendar.show-step');
    expect(body.indexOf('data-field="name"')).toBeLessThan(body.indexOf('data-panel="calendar.show-step"'));
    expect(field).toMatch(/<span data-panel-title[^>]*>Step<\/span>/);
    expect([...field.matchAll(/data-step="(\w+)"/g)].map((m) => m[1])).toEqual(['solo', 'guests', 'callers', 'full']);
    expect(field).toContain('Just you: your microphone, the music and carts, the listener count, the chat to read');
    expect(field).toContain('Adds a screener on the phones, the picture, moderating the chat, handing over to the next DJ');
    expect(field).toContain('How much of the studio this show uses. Whatever a step leaves out is switched off while the show is on air, so nothing can wait unseen');
    expect(field).toContain('under <b>Solo</b> and <b>Guests</b> the call-in page says the lines are closed');
    // A new show starts at Solo.
    expect(field).toMatch(/<input[^>]*value="solo"[^>]*checked/);
  });

  it('shows a show’s own step, and Full for a show from before steps', () => {
    const show = { id: 'i', slug: 'steeped', name: 'Steeped', description: '', colour_slot: 2, layout: 'poster' as const, slot: null, hosts: [], tags: [], explicit: false, feed_guid: null, cards: null, presenter: '', contact: '', links: [], commercial_music: false, presenter_account: null };
    const picked = (s: object) => part(render(ShowEditor, { props: { slug: 'steeped', show: s as never, onSaved: () => {}, onCard: () => {}, embedded: true } }).body, 'calendar.show-step').match(/<input[^>]*value="(\w+)"[^>]*checked/)?.[1];
    expect(picked({ ...show, step: 'guests' })).toBe('guests');
    expect(picked(show)).toBe('full');
  });
});
