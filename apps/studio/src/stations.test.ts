// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The station switcher's rules and words (ADR-0285; docs/reference/design/station-switcher.md):
// what a station is called here, its badge, what a row and the arrival note say of a station
// on another version, and when leaving the station the window is on is asked first.
import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import type { StationRow, View } from './lib/api.ts';
import { arrivalNote, badges, hostOf, leaveAsk, stationName, versionNote, versionTag, versus, whoWords } from './lib/stations.ts';
import { PANELS } from '../../../packages/ui/panels.ts';

const row = (id: string, o: Partial<StationRow> = {}): StationRow => ({ id, url: `https://${id}.example`, label: null, name: null, who: null, version: null, kept: 'keychain', ...o });
const KETTLE = row('s1', { url: 'https://kettle.example', name: 'Kettle FM', who: 'Eilidh Fraser-MacLeod, presenter', version: '1.0.5' });
const ROSS = row('s2', { url: 'https://radio-ross.example', name: 'Radio Ross', who: 'Eilidh Fraser-MacLeod, producer', version: '1.0.5' });
const TEST = row('s3', { url: 'https://test.kettle.example', name: 'Kettle FM', label: 'Kettle FM test', who: 'the station key', version: '1.0.3' });

describe('what a station is called on this computer', () => {
  it('is the label given here, else the station’s own name, else its address', () => {
    expect(stationName(TEST)).toBe('Kettle FM test');
    expect(stationName(KETTLE)).toBe('Kettle FM');
    expect(stationName(row('s9', { url: 'http://192.168.1.20:8080' }))).toBe('192.168.1.20:8080');
    expect(hostOf('https://kettle.example')).toBe('kettle.example');
  });

  it('says who is signed in, or that nobody is', () => {
    expect(whoWords(KETTLE)).toBe('Eilidh Fraser-MacLeod, presenter');
    expect(whoWords(row('s9', { kept: null, who: 'Eilidh Fraser-MacLeod, presenter' }))).toBe('signed out');
    expect(whoWords(row('s9'))).toBe('signed in');
  });
});

describe('the badge of two letters', () => {
  it('is the first letters of the first two words, and no two stations share one', () => {
    expect(badges([KETTLE, ROSS, TEST])).toEqual({ s1: 'KF', s2: 'RR', s3: 'KT' });
  });

  it('falls back to the first two letters, then a letter and a digit', () => {
    const b = badges([row('a', { name: 'Kettle' }), row('b', { name: 'Kettle' }), row('c', { name: 'Kettle' }), row('d', { name: 'Kettle FM' }), row('e', { name: 'Kettle FM' })]);
    expect(b).toEqual({ a: 'KE', b: 'K2', c: 'K3', d: 'KF', e: 'K4' });
    expect(new Set(Object.values(b)).size).toBe(5);
  });

  it('keeps a station’s badge when one is added after it, and reads names beyond ASCII', () => {
    expect(badges([KETTLE]).s1).toBe(badges([KETTLE, TEST]).s1);
    expect(badges([row('g', { name: 'Rèidio nan Gàidheal' })]).g).toBe('RN');
    expect(badges([row('h', { name: 'éire fm' })]).h).toBe('ÉF');
    // An address stands in for a name that is not known yet.
    expect(badges([row('i', { url: 'https://kettle.example' })]).i).toBe('KE');
  });
});

describe('a station on another version', () => {
  it('is compared by its numbers alone', () => {
    expect(versus('1.0.5', '1.0.5')).toBe('same');
    expect(versus('1.0.5', '1.0.5-test.2')).toBe('same');
    expect(versus('1.0.5', '1.0.3')).toBe('older');
    expect(versus('1.0.5', '1.0.10')).toBe('newer');
    expect(versus('1.0.5', '1.1.0')).toBe('newer');
    expect(versus('1.2.0', '1.1.9')).toBe('older');
    expect(versus('1.0.5', '2.0.0')).toBe('major');
    expect(versus('1.0.5', null)).toBe('unknown');
    expect(versus('1.0.5', 'dev')).toBe('unknown');
  });

  it('is said on its row, and nothing is said when it is the app’s own or not known', () => {
    expect(versionTag('1.0.5', '1.0.3')).toBe('1.0.3, older than this app');
    expect(versionTag('1.0.5', '1.1.0')).toBe('1.1.0, newer than this app');
    expect(versionTag('1.0.5', '2.0.0')).toBe('2.0.0, a different major version');
    expect(versionTag('1.0.5', '1.0.5')).toBe('');
    expect(versionTag('1.0.5', null)).toBe('');
  });

  it('is never refused: the arrival note says which way the difference runs', () => {
    expect(versionNote('1.0.5', '1.0.3')).toContain('older than this app (1.0.5): what it does not have yet is hidden or switched off here');
    expect(versionNote('1.0.5', '1.1.0')).toContain('update the app to use everything there');
    expect(versionNote('1.0.5', '2.0.0')).toContain('some screens may not work');
    expect(versionNote('1.0.5', '1.0.5')).toBe('');
  });
});

const source = (on_air: boolean, mute = false) => ({ on_air, mute, cue: false }) as unknown as View['sources'][number];
const caller = (name: string, state: 'waiting' | 'screening' | 'ready') => ({ name, name_on_air: '', state }) as unknown as View['callers'][number];
const view = (sources: View['sources'], callers: View['callers'] = [], playout = true): View =>
  ({ station: 'Kettle FM', desk: { reachable: true, error: null, playout: { connected: playout, for_ms: null } }, sources, callers }) as unknown as View;

describe('leaving the station the window is on', () => {
  it('is not asked on a quiet desk', () => {
    expect(leaveAsk(view([source(false), source(true, true)]), 'live', false, 'Kettle FM')).toBeNull();
    expect(leaveAsk(view([], [caller('Dougal Thomson', 'waiting')]), 'live', false, 'Kettle FM')).toBeNull();
  });

  it('is asked of a Live window while a strip is open, with what stays on named', () => {
    const a = leaveAsk(view([source(true), source(true), source(false)], [caller('Dougal Thomson', 'screening')]), 'live', false, 'Kettle FM');
    expect(a).toEqual({
      heading: 'Leave Kettle FM while it is on air?',
      lead: 'Kettle FM is on air: 2 strips are open on the desk. Dougal Thomson is in screening.',
      lines: ['It stays on air. Switching changes nothing there.', 'This window stops controlling it. Nobody here can mute, dump or take a call on Kettle FM until you switch back.'],
      live: true,
    });
    expect(leaveAsk(view([source(true)]), 'live', false, 'Kettle FM')?.lead).toBe('Kettle FM is on air: 1 strip is open on the desk.');
  });

  it('follows the desk’s own facts: the room open while playout is down, a caller in screening alone', () => {
    const room = leaveAsk(view([source(true)], [], false), 'live', false, 'Kettle FM');
    expect(room?.heading).toBe('Leave Kettle FM while its room is open?');
    expect(room?.lead).toBe('Kettle FM has its room open: 1 strip is open on the desk.');
    expect(room?.lines[0]).toBe('The room stays open. Switching changes nothing there.');
    const call = leaveAsk(view([], [caller('Dougal Thomson', 'screening')]), 'live', false, 'Kettle FM');
    expect(call?.heading).toBe('Leave Kettle FM?');
    expect(call?.lead).toBe('Dougal Thomson is in screening on Kettle FM.');
    expect(leaveAsk(view([], [caller('A', 'screening'), caller('B', 'screening')]), 'live', false, 'Kettle FM')?.lead).toBe('2 callers are in screening on Kettle FM.');
  });

  it('is never asked of a Preparing window, which changes nothing on air', () => {
    expect(leaveAsk(view([source(true), source(true)]), 'live', true, 'Kettle FM')).toBeNull();
    expect(leaveAsk(null, 'lost', true, 'Kettle FM')).toBeNull();
  });

  it('says so, and asks the same way, when the window cannot see the desk', () => {
    for (const [v, link] of [
      [null, 'connecting'],
      [view([source(true)]), 'lost'],
    ] as const) {
      const a = leaveAsk(v, link, false, 'Kettle FM');
      expect(a?.heading).toBe('Leave Kettle FM?');
      expect(a?.lead).toBe('This window cannot see Kettle FM’s desk, so it cannot tell whether a show is on air.');
      expect(a?.live).toBe(false);
    }
  });
});

describe('the arrival note', () => {
  it('says where the window is and who it is signed in as', () => {
    expect(arrivalNote('Radio Ross', 'Eilidh Fraser-MacLeod, producer', { from: 'Kettle FM', leftLive: false }, '1.0.5', '1.0.5')).toEqual({
      title: 'You are on Radio Ross',
      message: 'Signed in as Eilidh Fraser-MacLeod, producer.',
    });
  });

  it('says what was left on air, and the station’s version when it is not the app’s', () => {
    expect(arrivalNote('Radio Ross', 'Eilidh Fraser-MacLeod, producer', { from: 'Kettle FM', leftLive: true }, '1.0.5', '1.0.5').message).toBe(
      'Signed in as Eilidh Fraser-MacLeod, producer. Kettle FM carries on without this window.',
    );
    expect(arrivalNote('Kettle FM test', 'the station key', { from: 'Kettle FM', leftLive: false }, '1.0.5', '1.0.3').message).toBe(
      'Signed in with the station key. It runs 1.0.3, older than this app (1.0.5): what it does not have yet is hidden or switched off here.',
    );
    // Who is signed in is not known yet (the station has not answered): only where the window is.
    expect(arrivalNote('Radio Ross', null, { from: null, leftLive: false }, '1.0.5', null)).toEqual({ title: 'You are on Radio Ross', message: '' });
  });
});

/** The server's markup without the markers it leaves for the browser. */
const plain = (html: string) => html.replace(/<!--.*?-->/g, '');

describe('the Stations dialog and the frame’s button', () => {
  it('lists every station with its badge, address, who is signed in and its version, and marks the window’s own', async () => {
    const { stations } = await import('./lib/stations.svelte.ts');
    const { default: Stations } = await import('./lib/Stations.svelte');
    stations.list = { app_version: '1.0.5', current: 's1', stations: [KETTLE, ROSS, TEST, row('s4', { url: 'https://away.example', name: 'Away FM', kept: null })] };
    const body = plain(render(Stations).body);
    expect(body).toContain('data-panel="studio.station"');
    expect(body).toContain('data-panel="studio.stations"');
    // The frame: the badge takes the mark's place from two stations, and the name is the button.
    expect(body).toMatch(/class="stationbtn"[^>]*aria-haspopup="dialog"/);
    expect(body).toMatch(/<span class="stbadge"[^>]*>KF<\/span>\s*<span class="name">Kettle FM<\/span>/);
    expect(body).toContain('This window');
    expect(body).toContain('radio-ross.example · Eilidh Fraser-MacLeod, producer');
    expect(body).toContain('test.kettle.example · the station key · <span class="ver">1.0.3, older than this app</span>');
    // A label shows the station's own name beside it; a signed-out row says so.
    expect(body).toMatch(/Kettle FM test<\/span>\s*<span class="own">Kettle FM<\/span>/);
    expect(body).toContain('away.example · signed out');
    expect(body).toContain('Add a station…');
    // No key, token or password is anywhere in what the page holds.
    expect(JSON.stringify(stations.list)).not.toMatch(/rss_|token|password/);
    stations.list = { app_version: '1.0.5', current: 's1', stations: [KETTLE] };
    const one = plain(render(Stations).body);
    expect(one).not.toMatch(/class="stbadge"[^>]*>KF<\/span>\s*<span class="name">/);
    expect(one).toContain('class="mark');
    for (const name of ['studio.station', 'studio.stations', 'studio.stations.leave', 'setup.stations']) expect(PANELS[name], name).toBeTruthy();
    stations.list = null;
  });

  it('turns to the question, with Stay and Switch, when leaving is asked', async () => {
    const { stations } = await import('./lib/stations.svelte.ts');
    const { default: Stations } = await import('./lib/Stations.svelte');
    stations.list = { app_version: '1.0.5', current: 's1', stations: [KETTLE, ROSS] };
    stations.ask = { leave: { kind: 'switch', id: 's2' }, words: leaveAsk(view([source(true), source(true)]), 'live', false, 'Kettle FM')! };
    const body = plain(render(Stations).body);
    expect(body).toContain('data-panel="studio.stations.leave"');
    expect(body).toContain('Leave Kettle FM while it is on air?');
    expect(body).toContain('Kettle FM is on air: 2 strips are open on the desk.');
    expect(body).toMatch(/Stay on Kettle FM <kbd[^>]*>Esc<\/kbd>/);
    expect(body).toMatch(/Switch to Radio Ross <kbd[^>]*>⏎<\/kbd>/);
    stations.ask = null;
    stations.list = null;
  });

  it('adds a station from the sign-in screen with a way back, and lists the others when signed out', async () => {
    const { stations } = await import('./lib/stations.svelte.ts');
    const { default: StationSetup } = await import('./lib/StationSetup.svelte');
    stations.list = { app_version: '1.0.5', current: 's1', stations: [KETTLE, ROSS] };
    stations.adding = true;
    const add = plain(render(StationSetup).body);
    expect(add).toContain('Add a station');
    expect(add).toContain('‹ Back to Kettle FM');
    expect(add).toContain('Kettle FM carries on underneath.');
    expect(add).toContain('>Sign in and add</button>');
    expect(add).not.toContain('data-panel="setup.stations"');
    stations.adding = false;
    stations.list = { app_version: '1.0.5', current: 's1', stations: [{ ...KETTLE, kept: null }, ROSS] };
    const again = plain(render(StationSetup).body);
    expect(again).toContain('Sign in to Kettle FM');
    expect(again).toContain('data-panel="setup.stations"');
    expect(again).toContain('Or open another station');
    expect(again).toContain('Remove Kettle FM from this computer');
    expect(again).toContain('radio-ross.example · Eilidh Fraser-MacLeod, producer');
    stations.list = null;
  });
});
