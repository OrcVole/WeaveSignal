// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The station's time zone in the studio app (ADR-0189): what is checked before the station is
// asked, the panel that asks until someone chooses, and the warning on a later change.
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import { PANELS } from '../../../packages/ui/panels.ts';
import { ZONE_CHANGE_WARNING, zoneProblem } from './lib/zone.ts';
import type { Me } from './lib/api.ts';
import Settings from './settings/Settings.svelte';
import Zone from './settings/Zone.svelte';

const ZONES = ['Europe/London', 'Europe/Dublin', 'America/New_York', 'UTC'];
const asAdmin: Me = { kind: 'station', role: 'admin' };

describe('the zone a form takes', () => {
  it('wants a name, and one the window knows when it can list them', () => {
    expect(zoneProblem('', ZONES)).toContain('Choose a time zone');
    expect(zoneProblem('   ', ZONES)).toContain('Choose a time zone');
    expect(zoneProblem('Europe/Londno', ZONES)).toContain('is not a time zone');
    expect(zoneProblem(' Europe/London ', ZONES)).toBeNull();
    // With no list to check against, only the station can refuse a name.
    expect(zoneProblem('Europe/Londno', [])).toBeNull();
  });
});

describe('Settings > Station time zone', () => {
  it('asks until someone chooses, offering the configuration’s zone and no UTC default', () => {
    const { body } = render(Zone, { props: { initial: { zone: 'Europe/London', set: false }, zones: ZONES } });
    const text = body.replace(/<!--[^>]*-->/g, '');
    expect(body).toContain('data-panel="settings.zone"');
    expect(PANELS['settings.zone']).toBeTruthy();
    expect(text).toContain('Choose the station’s time zone.');
    expect(text).toContain('Not chosen yet');
    expect(text).toContain('using Europe/London from its configuration');
    expect(body).toContain('value="Europe/London"');
    expect(text).toContain('Use Europe/London');
    expect(text).not.toContain(ZONE_CHANGE_WARNING);
  });

  it('once chosen, shows the zone and offers a change only when the name differs', () => {
    const { body } = render(Zone, { props: { initial: { zone: 'Europe/Dublin', set: true }, zones: ZONES } });
    const text = body.replace(/<!--[^>]*-->/g, '');
    expect(text).not.toContain('Choose the station’s time zone.');
    expect(text).toContain('Change time zone');
    expect(body).toMatch(/<button[^>]*disabled[^>]*>\s*Change time zone/);
    // The warning appears with a differing name (a draft the server render cannot type).
    expect(text).not.toContain(ZONE_CHANGE_WARNING);
  });

  it('is an admin’s panel: Settings draws it for an admin and not for anyone else', () => {
    const admin = render(Settings, { props: { me: asAdmin, initial: null, initialHolds: null } });
    expect(admin.body).toContain('data-panel="settings.zone"');
    const presenter: Me = { kind: 'staff', role: 'presenter', account: { id: 'a2', username: 'fraser', name: 'Fraser Macleod', role: 'presenter', disabled_ms: null, created_ms: 2 }, expires_ms: 9 };
    const other = render(Settings, { props: { me: presenter, initial: null, initialHolds: null } });
    expect(other.body).not.toContain('data-panel="settings.zone"');
  });
});
