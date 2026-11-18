// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A show's presenter as a staff account (ADR-0218): who is offered whom, what a save sends,
// and the picker in the show editor's Presenter group.
import { render } from 'svelte/server';
import { afterEach, describe, expect, it } from 'vitest';
import ShowEditor from './schedule/ShowEditor.svelte';
import { accountPatch, mayListTeam, namesAnyone, offeredName, ownTeam, presenterChoice, presenterHint, type TeamMember } from './schedule/presenter.ts';
import { account } from './lib/account.svelte.ts';
import type { Me } from './lib/api.ts';

const TEAM: TeamMember[] = [
  { id: 'a-finlay', name: 'Finlay Grant', role: 'presenter' },
  { id: 'a-isla', name: 'Isla Robertson', role: 'screener' },
  { id: 'a-morag', name: 'Morag Campbell', role: 'admin' },
  { id: 'a-shona', name: 'Shona Campbell', role: 'producer' },
];
const staff = (id: string, role: Me['role']): Me => ({ kind: 'staff', role, account: { id, username: id, name: id, role, disabled_ms: null, created_ms: 1 } });
const ADMIN = staff('a-morag', 'admin');
const PRODUCER = staff('a-shona', 'producer');
const KEY: Me = { kind: 'station', role: 'admin' };

const labels = (c: ReturnType<typeof presenterChoice>) => c.options.map((o) => `${o.value || '-'}${o.disabled ? ' (disabled)' : ''}`);

describe('who is offered whom', () => {
  it('an admin and the station key may name anyone on the team', () => {
    for (const me of [ADMIN, KEY]) {
      expect(namesAnyone(me)).toBe(true);
      const c = presenterChoice(TEAM, me, null);
      expect(c.any).toBe(true);
      expect(labels(c)).toEqual(['-', 'a-finlay', 'a-isla', 'a-morag', 'a-shona']);
    }
    expect(presenterChoice(TEAM, ADMIN, null).options[3].label).toBe('Morag Campbell · admin (you)');
  });

  it('only a producer, an admin or the station key is given the team to ask for (ADR-0289)', () => {
    for (const me of [ADMIN, PRODUCER, KEY]) expect(mayListTeam(me), me.role).toBe(true);
    // A presenter or a screener would be refused by the station, so the editor never asks.
    for (const me of [staff('a-finlay', 'presenter'), staff('a-isla', 'screener'), null, undefined]) expect(mayListTeam(me)).toBe(false);
  });

  it('a window that may not ask for the team offers only the member of staff themselves, as before', () => {
    const finlay = staff('a-finlay', 'presenter');
    expect(ownTeam(finlay, null)).toEqual([{ id: 'a-finlay', name: 'a-finlay', role: 'presenter' }]);
    expect(ownTeam(finlay, 'a-finlay')).toHaveLength(1);
    expect(labels(presenterChoice(ownTeam(finlay, null)!, finlay, null))).toEqual(['-', 'a-finlay']);
    // Another's account would need their name, which this window is not given: no picker.
    expect(ownTeam(finlay, 'a-shona')).toBeNull();
    expect(ownTeam(null, null)).toBeNull();
    expect(ownTeam(KEY, null)).toBeNull();
  });

  it('anyone else is offered no account, and themselves', () => {
    for (const me of [PRODUCER, staff('a-finlay', 'presenter'), staff('a-isla', 'screener')]) {
      expect(namesAnyone(me)).toBe(false);
      const c = presenterChoice(TEAM, me, null);
      expect(c.any).toBe(false);
      expect(labels(c)).toEqual(['-', me.account!.id]);
    }
    expect(presenterChoice(TEAM, PRODUCER, null).options[1].label).toBe('Shona Campbell · producer (you)');
  });

  it('the account a show has stays in the list, named, even when this window could not pick it', () => {
    const c = presenterChoice(TEAM, PRODUCER, 'a-finlay');
    expect(labels(c)).toEqual(['-', 'a-finlay (disabled)', 'a-shona']);
    expect(c.options[1].label).toBe('Finlay Grant · presenter');
  });

  it('an account that is not on the team (turned off) is said to be, and cannot be picked again', () => {
    const c = presenterChoice(TEAM, ADMIN, 'a-gone');
    expect(c.options.at(-1)).toEqual({ value: 'a-gone', label: 'An account that is turned off', disabled: true });
  });

  it('before the station has said who this is, nobody else is offered', () => {
    expect(labels(presenterChoice(TEAM, null, null))).toEqual(['-']);
  });
});

describe('what a save sends', () => {
  it('nothing when the account is as stored, even a name the station would refuse', () => {
    expect(accountPatch('a-finlay', 'a-finlay')).toEqual({});
    expect(accountPatch('', null)).toEqual({});
    expect(accountPatch('', '')).toEqual({});
  });
  it('the id when it was set or changed, and null when it was cleared', () => {
    expect(accountPatch('a-shona', null)).toEqual({ presenter_account: 'a-shona' });
    expect(accountPatch('a-morag', 'a-finlay')).toEqual({ presenter_account: 'a-morag' });
    expect(accountPatch('', 'a-finlay')).toEqual({ presenter_account: null });
  });
});

describe('the on-air name a pick offers', () => {
  it('fills an empty one with the account’s name and never overwrites what was typed', () => {
    expect(offeredName(TEAM, 'a-finlay', '')).toBe('Finlay Grant');
    expect(offeredName(TEAM, 'a-finlay', '  ')).toBe('Finlay Grant');
    expect(offeredName(TEAM, 'a-finlay', 'Fin')).toBe('Fin');
    expect(offeredName(TEAM, 'a-nobody', '')).toBe('');
  });
});

describe('the sentence under the picker', () => {
  it('says who may pick whom', () => {
    expect(presenterHint({ options: [], any: true })).toContain('The member of staff who presents this show');
    expect(presenterHint({ options: [], any: false })).toBe('Only an admin names another member of staff. You can name yourself, or leave the show with a name only.');
  });
});

const SHOW = { id: 'i', slug: 'steeped', name: 'Steeped', description: '', colour_slot: 3, layout: 'poster' as const, slot: null, hosts: [], tags: [], explicit: false, feed_guid: null, cards: null, presenter: 'Finlay Grant', contact: '', links: [], commercial_music: false, presenter_account: 'a-finlay' };
const editor = (team: TeamMember[] | null) => render(ShowEditor, { props: { slug: SHOW.slug, show: SHOW as never, onSaved: () => {}, onCard: () => {}, team } }).body;
const group = (body: string) => body.slice(body.indexOf('data-panel="calendar.show-presenter"'), body.indexOf('Takedown contact'));

afterEach(() => {
  account.me = null;
  account.loaded = false;
});

describe('the picker in the show editor', () => {
  it('is the first field of the Presenter group, with the account the show has chosen', () => {
    account.loaded = true;
    account.me = ADMIN;
    const g = group(editor(TEAM));
    expect(g).toContain('Staff account');
    expect(g).toContain(`aria-label="The presenter's staff account"`);
    expect(g).toMatch(/<option value="a-finlay"[^>]*selected[^>]*>Finlay Grant · presenter<\/option>/);
    for (const name of ['Isla Robertson', 'Morag Campbell', 'Shona Campbell']) expect(g).toContain(name);
    expect(g.indexOf('Staff account')).toBeLessThan(g.indexOf('On-air name'));
  });

  it('offers a producer only themselves and the account the show has, the second not for picking', () => {
    account.loaded = true;
    account.me = PRODUCER;
    const g = group(editor(TEAM));
    expect(g).toContain('Shona Campbell · producer (you)');
    expect(g).toMatch(/<option value="a-finlay"[^>]*disabled/);
    expect(g).not.toContain('Isla Robertson');
    expect(g).not.toContain('Morag Campbell');
    expect(g).toContain('Only an admin names another member of staff');
  });

  it('is left out when the station could not say who is on the team', () => {
    account.loaded = true;
    account.me = ADMIN;
    expect(group(editor(null))).not.toContain('Staff account');
  });
});
