// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Hosts in their own studios (ADR-0280, ADR-0284) in the studio app without a station: the
// rules as pure functions (a tile's lines, a key's status, the details and the text to send,
// the checks the forms run), the source's tile, Settings › Own studios and the show editor's
// Comes from rendered on the server, the Disconnect action, and the driver's stand-in station
// held against the station's own API description. Sample people are the designs' (ADR-0071).
import { render } from 'svelte/server';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { PANELS } from '../../../packages/ui/panels.ts';
import openapi from '../../../docs/openapi/weavesignal-v1.json';
import * as api from './lib/api.ts';
import type { DeskPerson, DeskSource, KeyList, KeyRow, MadeKey, Me, Source, SourceNow, SourceSettings, View } from './lib/api.ts';
import {
  ARRIVAL_MS,
  GRACES,
  MONITOR_BY_HAND,
  TELL_HOST,
  arrival,
  arrivalWords,
  detailBlocks,
  detailsText,
  waysFirst,
  monitorLink,
  hostRows,
  keyStatus,
  keysSummary,
  labelProblem,
  levellerLimit,
  mayManageHosts,
  nextWords,
  ownStudiosWords,
  refusalWords,
  routeAbsent,
  ruleFieldOf,
  rulesProblems,
  showChoices,
  showOption,
  stationHost,
  tileLink,
  tileNote,
  tileRole,
  unavailableWords,
  whenWords,
} from './lib/hosts.ts';
import { run } from './lib/actions.ts';
import { airPlan } from './lib/air.ts';
import { changesAir } from './lib/prepare.ts';
import { NO_SHOW } from './lib/steps.ts';
import { studio } from './lib/studio.svelte.ts';
import { dropMonitor, holdMonitor, monitors } from './lib/monitor.svelte.ts';
import DeskB from './desk/DeskB.svelte';
import Hosts from './settings/Hosts.svelte';
import Settings from './settings/Settings.svelte';
import ShowSource from './settings/ShowSource.svelte';

vi.mock('./lib/api.ts', async (orig) => ({ ...(await orig<typeof api>()), disconnectSource: vi.fn(), desk: vi.fn() }));

const ZONE = 'Europe/London';
/** Friday 20 November 2026, 13:52 in London (GMT). */
const NOW = Date.UTC(2026, 10, 20, 13, 52);
const MIN = 60_000;

const srt: DeskSource = { way: 'srt', format: 'mpegts/aac 48 kHz stereo', key: 'MREQM2YS', show: 'steeped', connected_ms: 291_000, leveller_db: -7.4, level_db: -15.2 };
const icecast: DeskSource = { ...srt, way: 'icecast', format: 'mp3 44.1 kHz stereo', key: 'PN6E32MR', show: 'kettle-drive', connected_ms: 760_000, leveller_db: 4.2 };

const key = (over: Partial<KeyRow> = {}): KeyRow => ({
  id: 'k1',
  show_id: 's1',
  label: 'Finlay Grant',
  staff_id: null,
  key_id: 'MREQM2YS',
  hint: 'srt',
  created_by: 'Shona Campbell',
  created_ms: NOW - 21 * 86_400_000,
  last_used_ms: null,
  revoked_ms: null,
  ends_on_disconnect: false,
  turned_away_until_ms: null,
  turned_away_by: null,
  last_refusal: null,
  details: {
    host: 'kettle.example',
    icecast: { port: 8005, mount: '/live', user: 'source', password: 'MREQM2YS-(shown once, when the key was made)' },
    srt: { port: 9710, streamid: 'MREQM2YS', passphrase: '(shown once, when the key was made)', pbkeylen: 16, url: null },
    whip: { port: 0, url: null, bearer_token: '(shown once, when the key was made)' },
  },
  ...over,
});

const live = (over: Partial<SourceNow> = {}): SourceNow => ({
  key_id: 'MREQM2YS',
  identity: 'source-mreqm2ys',
  label: 'Finlay Grant',
  show: 'steeped',
  show_name: 'Steeped',
  state: 'standby',
  slot: 5,
  way: 'srt',
  pull: false,
  format: 'mpegts/aac 48 kHz stereo',
  level: -23.6,
  leveller_db: -7.4,
  steady_ms: 291_000,
  monitor_open: false,
  door_closes_ms: NOW + 4 * 60 * MIN + 8 * MIN,
  title: null,
  turned_away_until_ms: null,
  ...over,
});

describe('the source’s tile, in words', () => {
  it('says Own studio, and at large sizes the way in and the show', () => {
    expect(tileRole(srt, 'Steeped')).toEqual({ short: 'Own studio', long: 'SRT · Steeped' });
    expect(tileRole(icecast).long).toBe('Icecast · kettle-drive');
  });

  it('gives the way in and how long the link has held; Icecast says how far behind it is', () => {
    expect(tileLink(srt)).toBe('SRT · steady 4:51');
    expect(tileLink(icecast)).toBe('Icecast · steady 12:40 · about 2 s behind');
    expect(tileLink({ ...srt, connected_ms: 6_140_000 })).toBe('SRT · steady 1:42:20');
  });

  it('says when the leveller is at the end of its reach, and which way', () => {
    expect(levellerLimit(-7.4)).toBeNull();
    expect(levellerLimit(12)).toContain('Too quiet');
    expect(levellerLimit(-12)).toContain('Too loud');
  });

  it('notes an Icecast source on a show with callers, and nothing for SRT or below Callers', () => {
    expect(tileNote(icecast, 'Heather', 'callers')).toBe('Callers will hear Heather about 2 s late. Use SRT for a show with callers.');
    expect(tileNote(icecast, 'Heather', 'full')).toContain('about 2 s late');
    expect(tileNote(icecast, 'Heather', 'guests')).toBeNull();
    expect(tileNote(srt, 'Finlay', 'full')).toBeNull();
    // The leveller's limit matters more than the note on callers.
    expect(tileNote({ ...icecast, leveller_db: 12 }, 'Heather', 'full')).toContain('Too quiet');
  });

  it('counts own studios apart from the room', () => {
    const p = (role: string, on_air: boolean) => ({ role, on_air });
    expect(ownStudiosWords([p('presenter', true)])).toBe('');
    expect(ownStudiosWords([p('source', false)])).toBe('1 standing by');
    expect(ownStudiosWords([p('source', true), p('source', false), p('guest', true)])).toBe('1 on air · 1 standing by');
  });

  it('says once that a source has arrived, until it is cued, aired or dismissed', () => {
    const s = { identity: 'source-mreqm2ys', name: 'Finlay Grant', role: 'source', slot: 5, on_air: false, cue: false, source: { ...srt, connected_ms: 6_000 } };
    expect(arrival([s], [])).toBe(s);
    expect(arrival([s], ['source-mreqm2ys'])).toBeNull();
    expect(arrival([{ ...s, cue: true }], [])).toBeNull();
    expect(arrival([{ ...s, on_air: true }], [])).toBeNull();
    expect(arrival([{ ...s, source: { ...srt, connected_ms: ARRIVAL_MS } }], [])).toBeNull();
    expect(arrival([{ ...s, role: 'dj', source: null }], [])).toBeNull();
    const next = { slug: 'steeped', name: 'Steeped', start_ms: NOW + 8 * MIN };
    expect(arrivalWords(s, next, '', ZONE)).toEqual({ title: 'Finlay is standing by', message: 'From their own studio, for Steeped at 14:00.' });
    // Not the next show by the schedule: no time is claimed.
    expect(arrivalWords(s, { ...next, slug: 'kettle-drive' }, 'Steeped', ZONE).message).toBe('From their own studio, for Steeped.');
  });
});

describe('a key’s status', () => {
  it('puts times on the station’s clock, with today and yesterday in words', () => {
    expect(whenWords(NOW - 11 * MIN, NOW, ZONE)).toBe('today 13:41');
    expect(whenWords(NOW - 22 * 60 * MIN - 12 * MIN, NOW, ZONE)).toBe('yesterday 15:40');
    expect(whenWords(NOW + 30 * 60 * MIN, NOW, ZONE)).toBe('tomorrow 19:52');
    expect(whenWords(Date.UTC(2026, 10, 27, 14), NOW, ZONE)).toBe('Friday 27 November, 14:00');
    // The station's day, not UTC's: 23:30 UTC on a summer evening is half past midnight in London.
    expect(whenWords(Date.UTC(2026, 6, 1, 23, 30), Date.UTC(2026, 6, 1, 22), ZONE)).toBe('tomorrow 00:30');
    expect(whenWords(Date.UTC(2026, 6, 1, 23, 30), Date.UTC(2026, 6, 1, 23, 10), ZONE)).toBe('today 00:30');
  });

  it('says a source standing by, since when, and that its level is in range', () => {
    const s = keyStatus(key(), live(), NOW, ZONE);
    expect(s).toMatchObject({ chip: 'standby', connected: true, onAir: false, ended: false });
    expect(s.lines).toEqual(['standing by since 13:47 · level in range', 'SRT · mpegts/aac 48 kHz stereo · strip 5']);
  });

  it('says ON AIR, and a level the leveller cannot reach', () => {
    expect(keyStatus(key(), live({ state: 'on_air' }), NOW, ZONE)).toMatchObject({ chip: 'on_air', onAir: true, lines: ['on air · connected since 13:47 · level in range', 'SRT · mpegts/aac 48 kHz stereo · strip 5'] });
    expect(keyStatus(key(), live({ leveller_db: 12 }), NOW, ZONE).lines[0]).toBe('standing by since 13:47 · too quiet for the leveller');
    // The host's monitor link is open: someone listens there (design section 4.3).
    expect(keyStatus(key(), live({ monitor_open: true }), NOW, ZONE).lines[0]).toBe('standing by since 13:47 · level in range · monitor open');
    expect(keyStatus(key(), live({ leveller_db: -12 }), NOW, ZONE).lines[0]).toContain('too loud for the leveller');
  });

  it('reads a test’s level as the station’s own dBFS, never as LUFS', () => {
    const s = keyStatus(key({ hint: 'icecast' }), live({ state: 'test', slot: null, way: 'icecast', format: 'mp3 44.1 kHz stereo', level: -18.3, leveller_db: null, steady_ms: 95_000, door_closes_ms: null, title: 'Runrig - Loch Lomond' }), NOW, ZONE);
    expect(s.chip).toBe('test');
    expect(s.lines).toEqual(['a private test since 13:50 · level −18.3 dBFS', 'Icecast · mp3 44.1 kHz stereo', 'playing: Runrig - Loch Lomond']);
  });

  it('says NOT CONNECTED while the slot is open and nobody has connected', () => {
    const s = keyStatus(key(), live({ state: 'not_connected', slot: null, way: null, format: null, level: null, leveller_db: null, steady_ms: null }), NOW, ZONE);
    expect(s).toMatchObject({ chip: 'not_connected', connected: false });
    expect(s.lines).toEqual(['its slot is open until 18:00', 'never connected yet']);
  });

  it('has no chip for a key that is not due, and says when it last connected', () => {
    const s = keyStatus(key({ last_used_ms: NOW - 22 * 60 * MIN - 12 * MIN }), undefined, NOW, ZONE);
    expect(s.chip).toBeNull();
    expect(s.lines).toEqual(['last connected yesterday 15:40']);
  });

  it('puts the last refusal in words, and shows a word it does not know as given', () => {
    const at = { at_ms: NOW - 11 * MIN, key_id: 'MREQM2YS', event: 'refused', way: 'icecast' as const };
    expect(refusalWords({ ...at, reason: 'wrong_key' }, NOW, ZONE)).toBe('refused today 13:41: the key is wrong');
    expect(refusalWords({ ...at, reason: 'in_use' }, NOW, ZONE)).toContain('another connection was using the key');
    expect(refusalWords({ ...at, reason: 'new_reason' }, NOW, ZONE)).toBe('refused today 13:41: new_reason');
    expect(keyStatus(key({ last_refusal: { ...at, reason: 'format' } }), undefined, NOW, ZONE).refusal).toContain('could not read what the software sent');
  });

  it('says who turned a key away and until when, and that a key ends on disconnect', () => {
    const away = keyStatus(key({ turned_away_until_ms: NOW + 4 * 60 * MIN + 8 * MIN, turned_away_by: 'Shona Campbell' }), undefined, NOW, ZONE);
    expect(away.turnedAway).toBe('turned away by Shona Campbell until 18:00');
    expect(keyStatus(key({ turned_away_until_ms: NOW - 1 }), undefined, NOW, ZONE).turnedAway).toBeNull();
    expect(keyStatus(key({ ends_on_disconnect: true }), live(), NOW, ZONE).lines.at(-1)).toBe('ends when this connection closes: a new key was made');
  });

  it('says a key has ended, plainly', () => {
    expect(keyStatus(key({ revoked_ms: NOW - 2 * MIN }), undefined, NOW, ZONE)).toMatchObject({ chip: 'ended', ended: true, lines: ['ended today 13:50'] });
  });

  it('orders the rows: on air, standing by, a test, due, the rest; the ended apart', () => {
    const shows = [
      { slug: 'steeped', name: 'Steeped', colour_slot: 3 },
      { slug: 'kettle-drive', name: 'Kettle Drive', colour_slot: 2 },
    ];
    const lists: KeyList[] = [
      { show: 'kettle-drive', source: 'push', keys: [key({ id: 'a', key_id: 'AAAAAAAA', label: 'Heather Douglas' }), key({ id: 'x', key_id: 'XXXXXXXX', label: 'Rory Bruce', revoked_ms: NOW - 1 })] },
      { show: 'steeped', source: 'push', keys: [key({ id: 'b', key_id: 'BBBBBBBB' }), key({ id: 'c', key_id: 'CCCCCCCC', label: 'Elspeth Gordon' })] },
    ];
    const rows = hostRows(shows, lists, [live({ key_id: 'CCCCCCCC', state: 'not_connected' }), live({ key_id: 'BBBBBBBB', state: 'on_air' })], NOW, ZONE);
    expect(rows.live.map((r) => r.key.key_id)).toEqual(['BBBBBBBB', 'CCCCCCCC', 'AAAAAAAA']);
    expect(rows.ended.map((r) => r.key.label)).toEqual(['Rory Bruce']);
    // Heather's is Kettle Drive's last live key: ending it sends the show back to the room.
    expect(rows.live.find((r) => r.key.key_id === 'AAAAAAAA')?.last).toBe(true);
    expect(rows.live.find((r) => r.key.key_id === 'BBBBBBBB')?.last).toBe(false);
    expect(keysSummary(rows.live.map((r) => r.status))).toBe('3 keys work · 1 on air');
    expect(showChoices(shows, lists)).toEqual([
      { slug: 'steeped', name: 'Steeped', holders: ['Finlay Grant', 'Elspeth Gordon'] },
      { slug: 'kettle-drive', name: 'Kettle Drive', holders: ['Heather Douglas'] },
    ]);
  });

  it('says when a show is next on, or that a key has nothing to open for', () => {
    expect(nextWords({ slug: 's', name: 'S', colour_slot: 1, next_on: { start: (NOW + 8 * MIN) / 1000 } }, NOW, ZONE)).toBe('next today 14:00');
    expect(nextWords({ slug: 's', name: 'S', colour_slot: 1, next_on: { start: null } }, NOW, ZONE)).toContain('no date to come');
  });
});

describe('the setup and the details', () => {
  it('checks who the key is for as the station does', () => {
    expect(labelProblem('  ')).toContain('Say who the key is for');
    expect(labelProblem('Finlay Grant')).toBeNull();
    expect(labelProblem('x'.repeat(121))).toContain('120');
    expect(labelProblem('Finlay\nGrant')).toContain('one line');
  });

  it('names a show that already has a host', () => {
    expect(showOption('Steeped', [])).toBe('Steeped');
    expect(showOption('Steeped', ['Finlay Grant'])).toBe('Steeped · Finlay Grant has a key');
    expect(showOption('Steeped', ['Finlay Grant', 'Elspeth Gordon'])).toBe('Steeped · 2 hosts have keys');
  });

  it('gives the station’s address, or the one this studio uses', () => {
    expect(stationHost(key().details, 'https://studio.kettle.example')).toBe('kettle.example');
    expect(stationHost({ ...key().details, host: null }, 'https://studio.kettle.example:8443')).toBe('studio.kettle.example');
    expect(stationHost({ ...key().details, host: null }, '')).toBe('');
  });

  const made: MadeKey = {
    key: key(),
    secret: 'CJ95WZA67WVREB8C8KZ5NV2E',
    monitor_secret: 'H6VBH6KX3CVHNBDRGSZ57B5V',
    details: {
      host: 'kettle.example',
      icecast: { port: 8005, mount: '/live', user: 'source', password: 'MREQM2YS-CJ95WZA67WVREB8C8KZ5NV2E' },
      srt: { port: 9710, streamid: 'MREQM2YS', passphrase: 'CJ95WZA67WVREB8C8KZ5NV2E', pbkeylen: 16, url: 'srt://kettle.example:9710?streamid=MREQM2YS&passphrase=CJ95WZA67WVREB8C8KZ5NV2E&pbkeylen=16' },
      whip: { port: 9712, url: 'https://kettle.example/whip/MREQM2YS', bearer_token: 'CJ95WZA67WVREB8C8KZ5NV2E' },
    },
    as_text: 'the station’s own text',
  };

  it('lays out what a host’s software asks for, with Copy on each field that can be typed', () => {
    const b = detailBlocks(made.details, 'kettle.example', made.secret);
    expect(b.icecast.map((f) => [f.label, f.value, f.copy])).toEqual([
      ['Server', 'kettle.example', true],
      ['Port', '8005', true],
      ['Mount', '/live', true],
      ['User', 'source', true],
      ['Password', 'MREQM2YS-CJ95WZA67WVREB8C8KZ5NV2E', true],
    ]);
    expect(b.srt.map((f) => f.label)).toEqual(['Server', 'Port', 'Stream ID', 'Passphrase', 'Key length', 'As one address']);
    expect(b.srt.at(-1)?.value).toBe(made.details.srt.url);
    expect(b.srt.find((f) => f.label === 'Key length')).toMatchObject({ value: '16 (AES-128)', copy: false });
  });

  it('never offers Copy on a secret that cannot be shown again, and never echoes the station’s placeholder as one', () => {
    const b = detailBlocks(key().details, 'kettle.example', null);
    const password = b.icecast.find((f) => f.label === 'Password')!;
    const passphrase = b.srt.find((f) => f.label === 'Passphrase')!;
    expect(password).toMatchObject({ copy: false, words: true });
    expect(password.value).toBe('MREQM2YS- then the secret (shown once, when the key was made)');
    expect(passphrase).toMatchObject({ copy: false, words: true });
    expect(b.srt.some((f) => f.label === 'As one address')).toBe(false);
    // With no address at all, the server is words too.
    expect(detailBlocks(key().details, '', null).srt[0]).toMatchObject({ value: 'the station’s address', copy: false });
  });

  it('makes the monitor link from the station’s public address, and never from another', () => {
    expect(monitorLink('https://kettle.example', 'H6VBH6KX3CVHNBDRGSZ57B5V')).toBe('https://kettle.example/monitor/#H6VBH6KX3CVHNBDRGSZ57B5V');
    expect(monitorLink('https://kettle.example/', 'H6VB')).toBe('https://kettle.example/monitor/#H6VB');
    // No public address: no link. The address this window talks to may sit behind a login.
    expect(monitorLink(null, 'H6VB')).toBeNull();
    expect(monitorLink(undefined, 'H6VB')).toBeNull();
    expect(monitorLink('kettle.example', 'H6VB')).toBeNull();
    expect(monitorLink('https://kettle.example', '')).toBeNull();
  });

  it('writes the text to send: the way they will use first, both ways, the whole monitor link', () => {
    const link = monitorLink('https://kettle.example', made.monitor_secret);
    const text = detailsText(made, 'Steeped', 'kettle.example', link);
    expect(text.startsWith('Finlay Grant’s details for Steeped\n\nSRT (')).toBe(true);
    expect(text.indexOf('SRT (')).toBeLessThan(text.indexOf('Icecast ('));
    expect(text).toContain('  Passphrase: CJ95WZA67WVREB8C8KZ5NV2E');
    expect(text).toContain('  Password: MREQM2YS-CJ95WZA67WVREB8C8KZ5NV2E');
    expect(text).toContain(TELL_HOST);
    expect(text).toContain('Monitor link (open it in a browser, and keep it open beside your software)\n  https://kettle.example/monitor/#H6VBH6KX3CVHNBDRGSZ57B5V\n');
    expect(text).not.toMatch(/not built/i);
    expect(detailsText({ ...made, key: { ...made.key, hint: 'icecast' } }, 'Steeped', 'kettle.example', link).indexOf('Icecast (')).toBeLessThan(text.indexOf('Icecast ('));
    // A station with no public address: the secret, and how the link is formed, never a wrong link.
    const bare = detailsText(made, 'Steeped', 'kettle.example', null);
    expect(bare).toContain(`Monitor link\n  ${MONITOR_BY_HAND}\n  Secret: H6VBH6KX3CVHNBDRGSZ57B5V\n`);
    expect(bare).not.toContain('/monitor/#H6');
  });

  it('gives WHIP as OBS asks for it, first when it is the host’s way, and leaves it out while the station has it off', () => {
    const b = detailBlocks(made.details, 'kettle.example', made.secret);
    expect(b.whip.map((f) => [f.label, f.value, f.copy])).toEqual([
      ['Server', 'https://kettle.example/whip/MREQM2YS', true],
      ['Bearer Token', 'CJ95WZA67WVREB8C8KZ5NV2E', true],
      ['UDP port', '9712 must reach the station', false],
    ]);
    expect(waysFirst('whip')).toEqual(['whip', 'icecast', 'srt']);
    expect(waysFirst('srt')).toEqual(['srt', 'icecast', 'whip']);
    const text = detailsText({ ...made, key: { ...made.key, hint: 'whip' } }, 'Steeped', 'kettle.example', null);
    expect(text.startsWith('Finlay Grant’s details for Steeped\n\nWHIP (')).toBe(true);
    expect(text).toContain('  Bearer Token: CJ95WZA67WVREB8C8KZ5NV2E');
    // With no public address, how the address is formed; with WHIP off, nothing of it.
    const unknown = detailBlocks({ ...made.details, whip: { ...made.details.whip, url: null } }, 'kettle.example', null);
    expect(unknown.whip[0]).toMatchObject({ value: 'the station’s public address, then /whip/MREQM2YS', copy: false });
    expect(unknown.whip[1]).toMatchObject({ copy: false, words: true });
    expect(detailBlocks(key().details, 'kettle.example', null).whip).toEqual([]);
    const off = detailsText({ ...made, details: { ...made.details, whip: { port: 0, url: null, bearer_token: '' } } }, 'Steeped', 'kettle.example', null);
    expect(off).not.toContain('WHIP');
  });

  it('tells the host the design’s words: open the monitor link, and on air at the show’s time', () => {
    expect(TELL_HOST).toContain('open the monitor link');
    expect(TELL_HOST).toContain('You go on air at your show’s time, not when you connect.');
  });
});

describe('the rules', () => {
  const rules: SourceSettings = { door_before_min: 30, grace_s: 120, late_count_s: 20, test_outside: true, test_max_min: 15, steady_s: 5, silence_s: 4, place_kept_s: 300, fade_ms: 2000, port_icecast: 8005, port_srt: 9710, port_whip: 9712 };

  it('offers the four graces the station takes', () => {
    expect(GRACES.map((g) => g.s)).toEqual([0, 60, 120, 300]);
  });

  it('checks what the panel draws as the station does, before asking it', () => {
    expect(rulesProblems(rules)).toEqual({});
    expect(rulesProblems({ ...rules, door_before_min: 241 }).door_before_min).toBeTruthy();
    expect(rulesProblems({ ...rules, late_count_s: NaN }).late_count_s).toBeTruthy();
    expect(rulesProblems({ ...rules, test_max_min: 0 }).test_max_min).toBeTruthy();
    expect(rulesProblems({ ...rules, port_icecast: 70_000 }).port_icecast).toBeTruthy();
    expect(rulesProblems({ ...rules, port_srt: 8005 }).port_srt).toBe('Use a different number from the Icecast port.');
  });

  it('puts the station’s refusal under a field the panel draws, or leaves it for the form', () => {
    expect(ruleFieldOf('port_srt')).toBe('port_srt');
    expect(ruleFieldOf('steady_s')).toBeNull();
    expect(ruleFieldOf(null)).toBeNull();
  });

  it('knows a station with no such routes from one that cannot keep keys', () => {
    expect(routeAbsent(new api.FieldError(404, 'not found', null))).toBe(true);
    expect(routeAbsent(new api.FieldError(403, 'no', null))).toBe(false);
    expect(unavailableWords(new api.FieldError(503, 'show keys are kept in the database: set WEAVESIGNAL_DATABASE_URL', null))).toContain('kept in the database');
    expect(unavailableWords(new api.FieldError(404, 'not found', null))).toBeNull();
  });
});

describe('who may', () => {
  const staff = (role: Me['role']): Me => ({ kind: 'staff', role, account: { id: 'a', username: 'u', name: 'N', role, disabled_ms: null, created_ms: 1 } });

  it('an admin, a producer or the station key sets up hosts; a presenter does so from Your show', () => {
    expect(mayManageHosts({ kind: 'station', role: 'admin' })).toBe(true);
    expect(mayManageHosts(staff('admin'))).toBe(true);
    expect(mayManageHosts(staff('producer'))).toBe(true);
    expect(mayManageHosts(staff('presenter'))).toBe(false);
    expect(mayManageHosts(staff('screener'))).toBe(false);
    expect(mayManageHosts(null)).toBe(false);
  });

  it('Settings draws Own studios for an admin and a producer, after Staff, and for nobody else', () => {
    const admin = render(Settings, { props: { me: staff('admin'), initial: null, initialHolds: null } }).body;
    expect(admin).toContain('data-panel="settings.hosts"');
    expect(admin.indexOf('data-panel="settings.staff"')).toBeLessThan(admin.indexOf('data-panel="settings.hosts"'));
    expect(admin.indexOf('data-panel="settings.hosts"')).toBeLessThan(admin.indexOf('data-panel="settings.tokens"'));
    expect(render(Settings, { props: { me: staff('producer'), initial: null, initialHolds: null } }).body).toContain('data-panel="settings.hosts"');
    expect(render(Settings, { props: { me: staff('presenter'), initial: null, initialHolds: null } }).body).not.toContain('data-panel="settings.hosts"');
    expect(render(Settings, { props: { me: staff('screener'), initial: null, initialHolds: null } }).body).not.toContain('data-panel="settings.hosts"');
  });
});

// ---- the screens, rendered on the server ----

const person = (identity: string, name: string, role: DeskPerson['role'], on_air: boolean): DeskPerson => ({ identity, name, role, on_air, mute: false, cue: false, talk: false, talk_to: null, present: true, away_ms: null, linked: true, lufs: -22.6, lufs_peak: -19.8 });

function view(sources: Source[]): View {
  return {
    station: 'Kettle FM',
    now_ms: NOW,
    show: { title: 'The Long Lunch', rundown: [] },
    desk: { reachable: true, error: null, playout: { connected: true, for_ms: 14_000 } },
    seats: { presenter: 'duncan', screener: null, presenter_name: 'Duncan Boyd', screener_name: '' },
    sources,
    callers: [],
    desk_taking_callers: false,
    chat: { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false, influx: false, influx_since_ms: null },
    notices: [],
    carts: { version: 0, desk: true, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
    listener_delay: { ms: 15_000, measured: false },
    mode: 'live',
    step: NO_SHOW,
  };
}

const duncan: Source = { ...person('duncan', 'Duncan Boyd', 'presenter', true), slot: 1, caller: null };
const finlay: Source = { ...person('source-mreqm2ys', 'Finlay Grant', 'source', false), slot: 5, caller: null, source: srt };

function tile(body: string, slot: number): string {
  const at = body.indexOf(`data-panel="desk.tile.${slot}"`);
  expect(at, `desk.tile.${slot}`).toBeGreaterThan(-1);
  const next = body.indexOf('data-panel="desk.tile.', at + 1);
  return body.slice(at, next < 0 ? undefined : next);
}

beforeEach(() => {
  studio.view = view([duncan, finlay]);
  studio.armed = null;
  studio.note = null;
  studio.toldSources = [];
  vi.mocked(api.disconnectSource).mockReset();
  vi.mocked(api.desk).mockReset();
});

describe('the source’s tile', () => {
  it('stands by under Not on air with its way in, format, level, leveller and steady time', () => {
    const { body } = render(DeskB);
    expect(body.indexOf('data-panel="desk.waiting"')).toBeLessThan(body.indexOf('data-panel="desk.tile.5"'));
    const t = tile(body, 5);
    expect(t).toMatch(/hollow-standby[^>]*>.*?Standby/s);
    expect(t).toContain('Own studio');
    expect(t).toContain('title="SRT · steeped · mpegts/aac 48 kHz stereo"');
    expect(t).toContain('SRT · steady 4:51');
    expect(t).toContain('mpegts/aac 48 kHz stereo');
    expect(t).toMatch(/leveller <b>−7\.4<\/b> dB/);
    expect(t).toMatch(/<b>−22\.6<\/b> LUFS/);
    // A person's lines are not a source's.
    expect(t).not.toContain('Own mix-minus');
    expect(t).not.toContain('Linked · in the room');
    expect(t).not.toContain('fader <b>');
  });

  it('offers Disconnect in two presses, and never End pass or Drop', () => {
    let t = tile(render(DeskB).body, 5);
    expect(t).toContain('aria-label="Disconnect Finlay Grant"');
    expect(t).not.toContain('End pass');
    expect(t).not.toContain('Drop');
    studio.armed = { what: 'disconnect', at: NOW, identity: 'source-mreqm2ys' };
    const body = render(DeskB).body;
    t = tile(body, 5);
    expect(t).toContain('aria-label="Press again to disconnect Finlay Grant"');
    expect(t).toContain('class="btn quiet armed"');
    // The arm is this tile's own.
    expect(tile(body, 1)).not.toContain('Press again');
  });

  it('with the monitor link not open, draws Talk off with why, and keeps On, Cue and Mute on the strip’s keys', () => {
    const t = tile(render(DeskB).body, 5);
    expect(t).toMatch(/aria-disabled="true"[^>]*title="Finlay’s monitor link is not open, so they would not hear you\."[^>]*aria-label="Talk to Finlay Grant, held"/);
    expect(t).toMatch(/data-testid="source-monitor"[^>]*>.*?Monitor not open/s);
    for (const k of ['5', 'T', 'G', 'B']) expect(t).toContain(`<kbd>${k}</kbd>`);
  });

  it('with the monitor link open, says so and Talk is live', () => {
    studio.view = view([duncan, { ...finlay, source: { ...srt, monitor: true } }]);
    const t = tile(render(DeskB).body, 5);
    expect(t).toMatch(/class="mon open"[^>]*>.*?Monitor open/s);
    expect(t).toMatch(/aria-disabled="false"[^>]*aria-label="Talk to Finlay Grant, held"/);
    expect(t).not.toContain('would not hear you');
    // What their software sends has moved to the tooltips.
    expect(t).toContain('Their software sends mpegts/aac 48 kHz stereo.');
  });

  it('on air it is on the stage with the people, and says ON AIR', () => {
    studio.view = view([duncan, { ...finlay, on_air: true }]);
    const body = render(DeskB).body;
    expect(body.indexOf('data-panel="desk.tile.5"')).toBeLessThan(body.indexOf('data-panel="desk.waiting"'));
    expect(tile(body, 5)).toMatch(/fill-onair[^>]*>.*?On air/s);
    expect(body).toMatch(/Own studios <b>1 on air<\/b>/);
  });

  it('is counted apart from the room in the foot', () => {
    const { body } = render(DeskB);
    expect(body).toMatch(/Room <b>LiveKit<\/b> · 1 in/);
    expect(body).toMatch(/Own studios <b>1 standing by<\/b>/);
    studio.view = view([duncan]);
    expect(render(DeskB).body).not.toContain('Own studios');
  });

  it('says once that it has arrived, with Cue on the tile’s own key', () => {
    studio.view = view([duncan, { ...finlay, source: { ...srt, connected_ms: 6_000 } }]);
    let body = render(DeskB).body;
    expect(body).toContain('Finlay is standing by');
    expect(body).toMatch(/class="btn primary">Cue\s*(<!--[^>]*-->)?<kbd>T<\/kbd>/);
    studio.toldSources = ['source-mreqm2ys'];
    body = render(DeskB).body;
    expect(body).not.toContain('Finlay is standing by');
  });
});

describe('the desk’s actions on a source', () => {
  it('Disconnect closes the connection by the key, and says where to let them back in', async () => {
    vi.mocked(api.disconnectSource).mockResolvedValue({ disconnected: true, until_ms: NOW });
    await run({ type: 'arm', what: 'disconnect', identity: 'source-mreqm2ys' });
    expect(studio.armed).toMatchObject({ what: 'disconnect', identity: 'source-mreqm2ys' });
    expect(api.disconnectSource).not.toHaveBeenCalled();
    await run({ type: 'disconnect', identity: 'source-mreqm2ys', key: 'MREQM2YS' });
    expect(api.disconnectSource).toHaveBeenCalledWith('MREQM2YS');
    expect(studio.armed).toBeNull();
    expect(studio.note?.title).toBe('Finlay is disconnected');
    expect(studio.note?.message).toContain('Settings › Own studios');
  });

  it('shows the station’s own sentence when it refuses', async () => {
    vi.mocked(api.disconnectSource).mockRejectedValue(new api.FieldError(404, 'no key `MREQM2YS`', null));
    await run({ type: 'disconnect', identity: 'source-mreqm2ys', key: 'MREQM2YS' });
    expect(studio.note).toMatchObject({ title: 'Not done', message: 'no key `MREQM2YS`' });
  });

  it('On and Cue go through the desk’s own commands for the strip', async () => {
    vi.mocked(api.desk).mockResolvedValue(view([duncan, finlay]));
    await run({ type: 'on', slot: 5 });
    expect(api.desk).toHaveBeenCalledWith({ op: 'on_air', identity: 'source-mreqm2ys', on: true });
    await run({ type: 'cue', slot: 5 });
    expect(api.desk).toHaveBeenCalledWith({ op: 'cue', identity: 'source-mreqm2ys', on: true });
  });

  it('Talk sends nothing to a source whose monitor link is not open, and says why', async () => {
    await run({ type: 'talk', slot: 5, held: true });
    expect(api.desk).not.toHaveBeenCalled();
    expect(studio.note).toMatchObject({ title: 'Finlay would not hear you', message: 'Finlay’s monitor link is not open, so they would not hear you.' });
    studio.note = null;
    await run({ type: 'talk', slot: 5, held: false });
    expect(studio.note).toBeNull();
    expect(api.desk).not.toHaveBeenCalled();
  });

  it('Talk goes to a source’s monitor link while it is open, as the presenter in the room, and lets go', async () => {
    const open = { ...finlay, source: { ...srt, monitor: true } };
    studio.view = { ...view([duncan, open]), seats: { presenter: 'duncan', screener: null, presenter_name: 'Duncan Boyd', screener_name: '' } };
    vi.mocked(api.desk).mockResolvedValue(studio.view);
    await run({ type: 'talk', slot: 5, held: true });
    expect(api.desk).toHaveBeenCalledWith({ op: 'talk_to', identity: 'duncan', to: 'source-mreqm2ys', on: true });
    expect(studio.talking).toContain('source-mreqm2ys');
    // The monitor closes while the key is held: letting go still closes the talk.
    studio.view = { ...studio.view, sources: [duncan, finlay] };
    await run({ type: 'talk', slot: 5, held: false });
    expect(api.desk).toHaveBeenLastCalledWith({ op: 'talk_to', identity: 'duncan', to: 'source-mreqm2ys', on: false });
    expect(studio.talking).not.toContain('source-mreqm2ys');
  });

  it('Talk with nobody in the presenter’s seat says whose voice it would be', async () => {
    studio.view = { ...view([{ ...finlay, source: { ...srt, monitor: true } }]), seats: { presenter: null, screener: null, presenter_name: '', screener_name: '' } };
    await run({ type: 'talk', slot: 5, held: true });
    expect(api.desk).not.toHaveBeenCalled();
    expect(studio.note).toMatchObject({ title: 'Nobody to talk as' });
  });

  it('Esc dismisses the arrival', async () => {
    studio.view = view([duncan, { ...finlay, source: { ...srt, connected_ms: 6_000 } }]);
    studio.tab = 'desk';
    await run({ type: 'escape' });
    expect(studio.toldSources).toEqual(['source-mreqm2ys']);
  });

  it('End show says a host’s own studio is left standing by, not in the room', () => {
    expect(airPlan('end', view([{ ...finlay, on_air: true }])).lines[0]).toBe('Finlay goes off air now. They stay standing by, and callers stay in the queue.');
    expect(airPlan('end', view([duncan, { ...finlay, on_air: true }])).lines[0]).toBe('Duncan and Finlay go off air now. They stay in the room or standing by, and callers stay in the queue.');
    expect(airPlan('end', view([duncan])).lines[0]).toContain('They stay in the room,');
  });

  it('a window that is preparing does not disconnect: it changes the air', () => {
    expect(changesAir({ type: 'disconnect', identity: 'source-mreqm2ys', key: 'MREQM2YS' })).toBe(true);
    expect(changesAir({ type: 'arm', what: 'disconnect', identity: 'source-mreqm2ys' })).toBe(true);
  });
});

describe('Settings › Own studios', () => {
  const shows = [
    { slug: 'steeped', name: 'Steeped', colour_slot: 3, next_on: { start: (NOW + 8 * MIN) / 1000 } },
    { slug: 'kettle-drive', name: 'Kettle Drive', colour_slot: 2, next_on: { start: (NOW + 128 * MIN) / 1000 } },
  ];
  const lists: KeyList[] = [
    { show: 'steeped', source: 'push', keys: [key()] },
    {
      show: 'kettle-drive',
      source: 'push',
      keys: [
        key({ id: 'k2', key_id: 'PN6E32MR', label: 'Heather Douglas', hint: 'icecast', last_refusal: { at_ms: NOW - 11 * MIN, key_id: 'PN6E32MR', event: 'refused', reason: 'wrong_key', way: 'icecast' }, turned_away_until_ms: NOW + 60 * MIN, turned_away_by: 'Shona Campbell' }),
        key({ id: 'k3', key_id: 'ZZZZZZZZ', label: 'Rory Bruce', revoked_ms: NOW - MIN }),
      ],
    },
  ];
  const producer: Me = { kind: 'staff', role: 'producer', account: { id: 'a-shona', username: 'shona', name: 'Shona Campbell', role: 'producer', disabled_ms: null, created_ms: 1 } };
  const admin: Me = { kind: 'station', role: 'admin' };
  const rules: SourceSettings = { door_before_min: 30, grace_s: 120, late_count_s: 20, test_outside: true, test_max_min: 15, steady_s: 5, silence_s: 4, place_kept_s: 300, fade_ms: 2000, port_icecast: 8005, port_srt: 9710, port_whip: 9712 };
  const seed = { shows, lists, sources: { listening: true, sources: [live()] }, zone: ZONE };

  it('is registered under its names, with the dialogs', () => {
    for (const n of ['settings.hosts', 'settings.hosts.rules', 'settings.hosts.setup', 'settings.hosts.details', 'settings.hosts.new-key', 'settings.hosts.end', 'calendar.show-source']) expect(PANELS[n], n).toBeTruthy();
  });

  it('draws a row for each live key, with the vocabulary’s chip and the actions', () => {
    const { body } = render(Hosts, { props: { me: producer, initial: seed, now: NOW } });
    expect(body).toContain('data-panel="settings.hosts"');
    expect(body).toContain('2 keys work · 1 standing by');
    expect(body).toMatch(/data-key="MREQM2YS".*?hollow-standby.*?Standby.*?standing by since 13:47 · level in range/s);
    expect(body).toContain('next today 14:00');
    expect(body).toContain('refused today 13:41: the key is wrong');
    expect(body).toContain('turned away by Shona Campbell until 14:52');
    expect(body).toContain('Let back in');
    for (const a of ['Details…', 'New key…', 'End key…']) expect(body).toContain(a);
    // The ended key is kept apart, folded.
    expect(body).toMatch(/<details class="ended[^"]*">.*?Rory Bruce/s);
    expect(body).not.toMatch(/data-key="ZZZZZZZZ"/);
    for (const d of ['setup', 'details', 'new-key', 'end']) expect(body).toContain(`data-panel="settings.hosts.${d}"`);
  });

  it('never draws a secret, or the station’s placeholder for one, in the rows', () => {
    const { body } = render(Hosts, { props: { me: producer, initial: seed, now: NOW } });
    expect(body).not.toContain('shown once, when the key was made');
  });

  it('says what a key is for when there is none, and that a station is not listening', () => {
    const none = render(Hosts, { props: { me: producer, initial: { ...seed, lists: [] }, now: NOW } }).body;
    expect(none).toContain('No host has a key yet. Set one up when a presenter wants to use their own software instead of joining the room.');
    const deaf = render(Hosts, { props: { me: producer, initial: { ...seed, sources: { listening: false, sources: [] } }, now: NOW } }).body;
    expect(deaf).toContain('The station is not listening for hosts.');
  });

  it('shows the rules to an admin only, with nothing marked as not acted on', () => {
    const mine = render(Hosts, { props: { me: admin, initial: { ...seed, rules }, now: NOW } }).body;
    expect(mine).toContain('data-panel="settings.hosts.rules"');
    expect(mine).not.toContain('not acted on yet');
    expect(mine).toContain('Save the rules');
    const theirs = render(Hosts, { props: { me: producer, initial: { ...seed, rules }, now: NOW } }).body;
    expect(theirs).not.toContain('data-panel="settings.hosts.rules"');
  });
});

describe('the show editor’s Comes from', () => {
  const push: KeyList = { show: 'steeped', source: 'push', keys: [key()] };

  it('reads The host’s own studio when a key exists, with the host and the way in', () => {
    const { body } = render(ShowSource, { props: { slug: 'steeped', name: 'Steeped', initial: { list: push, sources: [live()] }, now: NOW } });
    expect(body).toContain('data-panel="calendar.show-source"');
    expect(body).toMatch(/aria-pressed="false"[^>]*>The station’s room/);
    expect(body).toMatch(/aria-pressed="true"[^>]*>The host’s own studio/);
    expect(body).toContain('Finlay Grant');
    expect(body).toContain('SRT · standing by since 13:47');
    expect(body).toContain('Details…');
    expect(body).toContain('Add a host…');
  });

  it('reads The station’s room with no key, and offers nothing until it is set', () => {
    const { body } = render(ShowSource, { props: { slug: 'the-long-lunch', name: 'The Long Lunch', initial: { list: { show: 'the-long-lunch', source: 'room', keys: [] }, sources: [] }, now: NOW } });
    expect(body).toMatch(/aria-pressed="true"[^>]*>The station’s room/);
    expect(body).not.toContain('Set up a host…');
  });

  it('as Your show’s block it offers the presenter their own key', () => {
    const { body } = render(ShowSource, { props: { slug: 'steeped', name: 'Steeped', as: 'block', initial: { list: { show: 'steeped', source: 'room', keys: [] }, sources: [] }, now: NOW } });
    expect(body).toContain('Broadcast from your own software…');
    expect(body).not.toContain('data-panel="calendar.show-source"');
    // No key, no monitor to open; and nothing is said to be still to build.
    expect(body).not.toContain('Open my monitor');
    expect(body).not.toMatch(/not built/i);
  });

  it('Your show’s Open my monitor is live only in the window that holds the link, and says why elsewhere', async () => {
    const { resetStores } = await import('./lib/reset.ts');
    const props = { slug: 'steeped', name: 'Steeped', as: 'block' as const, initial: { list: push, sources: [live()] }, now: NOW };
    let body = render(ShowSource, { props }).body;
    expect(body).toMatch(/<button type="button" class="btn" aria-disabled="true" title="This window does not hold your monitor link[^"]*">Open my monitor<\/button>/);
    expect(body).toContain('Your monitor link came with your details, shown once.');
    // The window that made the key holds its link, in memory.
    holdMonitor('MREQM2YS', 'https://kettle.example/monitor/#H6VBH6KX3CVHNBDRGSZ57B5V');
    body = render(ShowSource, { props }).body;
    expect(body).toMatch(/<button type="button" class="btn" title="Opens your monitor link in your browser">Open my monitor<\/button>/);
    // The secret is never drawn.
    expect(body).not.toContain('H6VBH6KX3CVHNBDRGSZ57B5V');
    // A renewed or ended key lets its link go, and so does a change of station.
    dropMonitor('MREQM2YS');
    expect(monitors.links).toEqual({});
    holdMonitor('MREQM2YS', 'https://kettle.example/monitor/#H6VB');
    resetStores();
    expect(monitors.links).toEqual({});
  });
});

// ---- the driver's stand-in against the station's own description ----

describe('the driver’s stand-in station (tests/hosts/station.mjs)', () => {
  interface Schema {
    properties?: Record<string, unknown>;
    required?: string[];
    allOf?: (Schema & { $ref?: string })[];
  }
  const spec = openapi as unknown as { components: { schemas: Record<string, Schema> }; paths: Record<string, Record<string, unknown>> };
  /** A schema's own fields and those of what it flattens in. */
  function fields(name: string): { all: string[]; required: string[] } {
    const s = spec.components.schemas[name];
    expect(s, name).toBeTruthy();
    const parts = [s, ...(s.allOf ?? []).map((p) => (p.$ref ? spec.components.schemas[p.$ref.split('/').at(-1)!] : p))];
    return { all: parts.flatMap((p) => Object.keys(p.properties ?? {})), required: parts.flatMap((p) => p.required ?? []) };
  }
  /** Every field the station requires is there, and none it does not have. */
  function matches(value: object, name: string) {
    const f = fields(name);
    const got = Object.keys(value);
    expect(f.required.filter((k) => !got.includes(k)), `${name}: missing`).toEqual([]);
    expect(got.filter((k) => !f.all.includes(k)), `${name}: not the station’s`).toEqual([]);
  }

  it('answers in the station’s own field names', async () => {
    const path = new URL('../../../tests/hosts/station.mjs', import.meta.url).href;
    const { HostStation } = (await import(/* @vite-ignore */ path)) as { HostStation: new (o?: object) => any };
    const st = new HostStation({ role: 'admin' });
    st.join('duncan', 'Duncan Boyd', 'presenter');
    const [status, made] = st.handle('POST', '/api/v1/shows/steeped/keys', { label: 'Finlay Grant', hint: 'srt' });
    expect(status).toBe(201);
    matches(made, 'MadeKey');
    matches(made.key, 'ShowKey');
    matches(made.details, 'Details');
    matches(made.details.icecast, 'IcecastDetails');
    matches(made.details.srt, 'SrtDetails');
    st.connect(made.key.key_id, 'srt', 'mpegts/aac 48 kHz stereo');

    const list = st.handle('GET', '/api/v1/shows/steeped/keys')[1];
    matches(list, 'KeyList');
    // `KeyRow` flattens the key in and adds two fields.
    expect(Object.keys(list.keys[0]).sort()).toEqual([...fields('ShowKey').all, 'details', 'last_refusal'].sort());
    expect(list.source).toBe('push');

    const now = st.handle('GET', '/api/v1/sources')[1];
    matches(now, 'SourcesView');
    matches(now.sources[0], 'SourceView');
    expect(now.sources[0]).toMatchObject({ state: 'standby', slot: 5 });

    matches(st.handle('GET', '/api/v1/settings/sources')[1], 'SourceSettings');
    const source = st.studioView().sources.find((s: Source) => s.role === 'source');
    matches(source.source, 'DeskSource');

    // The station refuses a field it does not know, and spells the renewal's choice its own way.
    expect(st.handle('POST', '/api/v1/shows/steeped/keys', { label: 'x', hint: 'srt', show: 'steeped' })[0]).toBe(422);
    expect(st.handle('POST', `/api/v1/shows/steeped/keys/${made.key.key_id}/renew`, { old: 'on-disconnect' })[0]).toBe(422);
    expect(st.handle('POST', `/api/v1/shows/steeped/keys/${made.key.key_id}/renew`, { old: 'on_disconnect' })[0]).toBe(201);
  });

  it('answers the routes the station has, and no other', () => {
    for (const p of ['/api/v1/shows/{show}/keys', '/api/v1/shows/{show}/keys/{key}', '/api/v1/shows/{show}/keys/{key}/renew', '/api/v1/sources', '/api/v1/sources/{key}/disconnect', '/api/v1/settings/sources']) {
      expect(spec.paths[p], p).toBeTruthy();
    }
    expect(Object.keys(spec.paths['/api/v1/sources/{key}/disconnect']).sort()).toEqual(['delete', 'post']);
    expect(Object.keys(spec.paths['/api/v1/settings/sources']).sort()).toEqual(['get', 'put']);
  });

  it('the studio’s own shapes carry the fields the station sends', () => {
    // The fixtures above are typed as the studio's own; held against the description here.
    matches(srt, 'DeskSource');
    matches(live(), 'SourceView');
    const { last_refusal: _r, details, ...plain } = key();
    matches(plain, 'ShowKey');
    matches(details, 'Details');
  });
});
