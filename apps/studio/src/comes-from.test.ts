// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// New show's Comes from (ADR-0280): offered on part 1 only where the station can take a host's
// own studio and this window may make the key; a pick of the host's own studio makes Done offer
// Set up a host…; and who may.
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import NewShow from './schedule/rule/NewShow.svelte';
import { mayMakeFirstKey } from './lib/account.ts';
import type { Me, ShowSaved } from './lib/api.ts';
import type { Show } from './lib/schedule.ts';

const noop = () => {};
const base = { shows: [] as Show[], zone: 'Europe/London', today: '2026-11-20', live: false, onMade: noop, onOpen: noop, onClose: noop };
const saved = { show: { slug: 'the-folk-club', name: 'The Folk Club', rule: null, slot: null }, resolved: {}, series: [], drafted: null } as unknown as ShowSaved;
const text = (html: string) => html.replace(/<!--.*?-->/g, '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ');

describe('Comes from in New show', () => {
  it('is on part 1 after the step, with the station’s room picked, where hosts can be had', () => {
    const body = render(NewShow, { props: { ...base, hosts: true } }).body;
    expect(body).toContain('data-testid="ns-comes"');
    expect(body).not.toContain('data-panel="calendar.show-source"');
    expect(body.indexOf('Step')).toBeLessThan(body.indexOf('Comes from'));
    expect(body).toMatch(/aria-pressed="true"[^>]*>The station’s room<\/button>/);
    expect(body).toMatch(/aria-pressed="false"[^>]*>The host’s own studio<\/button>/);
    expect(text(body)).toContain('The presenter and guests join the studio, as they do today.');
  });

  it('says what the host’s own studio means for the show when it is picked', () => {
    const body = render(NewShow, { props: { ...base, hosts: true, comes: 'push' } }).body;
    expect(body).toMatch(/aria-pressed="true"[^>]*>The host’s own studio<\/button>/);
    expect(text(body)).toContain('with a key that belongs to this show. You make the key when the show is made.');
  });

  it('is left out where the station cannot take hosts or this window may not make the key', () => {
    expect(render(NewShow, { props: { ...base, hosts: false } }).body).not.toContain('Comes from');
    // Until the station has been asked nothing is offered, and nothing is asked on the server.
    expect(render(NewShow, { props: base }).body).not.toContain('Comes from');
  });

  it('Done offers Set up a host… for a show that comes from a host’s own studio', () => {
    const body = render(NewShow, { props: { ...base, hosts: true, comes: 'push', part: 3, name: 'The Folk Club', saved } }).body;
    expect(text(body)).toContain('It comes from the host’s own studio once a key is made. Until then it is the station’s room.');
    expect(body).toMatch(/<button[^>]*>Set up a host…<\/button>/);
    // Its dialogs are drawn beside New show's, not inside it.
    expect(body.indexOf('settings.hosts.setup')).toBeGreaterThan(body.indexOf('</dialog>'));
  });

  it('Done says nothing of hosts for a show from the room', () => {
    const body = render(NewShow, { props: { ...base, hosts: true, comes: 'room', part: 3, name: 'The Folk Club', saved } }).body;
    expect(body).not.toContain('Set up a host');
    expect(body).not.toContain('host’s own studio');
  });
});

describe('who may make the first key of a new show', () => {
  const me = (role: Me['role'], kind: Me['kind'] = 'staff'): Me => ({ kind, role });
  it('an admin, a producer and the station key; not a presenter or a screener, who are not its presenter yet', () => {
    expect(mayMakeFirstKey(me('admin'))).toBe(true);
    expect(mayMakeFirstKey(me('producer'))).toBe(true);
    expect(mayMakeFirstKey(me('admin', 'station'))).toBe(true);
    expect(mayMakeFirstKey(me('presenter'))).toBe(false);
    expect(mayMakeFirstKey(me('screener'))).toBe(false);
    expect(mayMakeFirstKey(null)).toBe(false);
  });
});
