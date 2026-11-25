// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Listener keys in the studio app (keyed shows, 1.3.2): the routes and field names against the
// station's own description, the stand-in station against the same, where a key stands, the
// words, the forms and what they send, a pasted list, the message, the leak report's words, the
// CSV, what a refusal of the screen means, the keymap's keys, and the QR code's fixed parts.
import { describe, expect, it } from 'vitest';
import openapi from '../../../docs/openapi/weavesignal-v1.json';
import { KeyEngine, TABS } from '../../../packages/ui/keymap.ts';
import { PANELS } from '../../../packages/ui/panels.ts';
import { formatBits, qr, qrCapacity, qrPath, qrSvg, rsRemainder, versionBits } from '../../../packages/ui/qr.ts';
import { FieldError, type Me } from './lib/api.ts';
import {
  LOST_WAITING,
  STANDING,
  emailFact,
  emailHint,
  lostWaiting,
  actionProblem,
  changeOf,
  countProblem,
  cutWords,
  csv,
  csvCell,
  deviceStanding,
  extendProblem,
  familyLabelProblem,
  familySpecOf,
  fileStem,
  firstProblem,
  formOf,
  freshForm,
  fromNow,
  greetingName,
  keyProblems,
  leakLevel,
  leakSummary,
  leaksCsv,
  limitOf,
  limitsWords,
  logWords,
  mayEraseKey,
  mayManageKeys,
  messageFor,
  opensWords,
  parseRecipients,
  shapeOfText,
  specOf,
  standing,
  stationAddress,
  tally,
  templateOf,
  textIn,
  unreadyOf,
  useWords,
  windowWords,
} from './lib/keys.ts';
import { ROUTES, type KeyLeak, type KeyLogEntry, type ListenerKey } from './lib/keysApi.ts';
import { changesAir } from './lib/prepare.ts';

const Z = 'Europe/London';
const NOW = Date.UTC(2026, 10, 20, 13, 52);
const DAY = 86_400_000;
const at = (v: string) => {
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/.exec(v);
  return m ? Date.UTC(+m[1], +m[2] - 1, +m[3], +m[4], +m[5]) : null;
};

const key = (o: Partial<ListenerKey> = {}): ListenerKey => ({
  id: 'key-0001',
  kind: 'personal',
  label: 'Isla Fraser',
  note: '',
  has_email: true,
  media: 'both',
  all_shows: false,
  shows: ['id-friday-supporters', 'id-the-archive-hour'],
  valid_from_ms: null,
  valid_until_ms: Date.UTC(2026, 10, 30, 22, 0),
  max_devices: 2,
  max_listens: null,
  max_people: null,
  state: 'active',
  state_reason: '',
  listens_used: 9,
  first_used_ms: NOW - 28 * DAY,
  last_used_ms: NOW - 26 * 3_600_000,
  family_id: 'fam-1',
  replaces_id: null,
  replaced_by_id: null,
  created_by: 'Shona Campbell',
  created_ms: NOW - 30 * DAY,
  updated_ms: NOW - 30 * DAY,
  ...o,
});
const SHOWS = [
  { id: 'id-friday-supporters', name: 'Friday Supporters’ Hour' },
  { id: 'id-the-archive-hour', name: 'The Archive Hour' },
  { id: 'id-ceilidh-night-live', name: 'Ceilidh Night Live' },
];
const staff = (role: 'admin' | 'producer' | 'presenter' | 'screener'): Me => ({ kind: 'staff', role, account: { id: 'a', username: 'u', name: 'N', role, disabled_ms: null, created_ms: 1 } }) as Me;

describe('the station’s own description', () => {
  interface Schema {
    properties?: Record<string, unknown>;
    required?: string[];
  }
  const spec = openapi as unknown as { components: { schemas: Record<string, Schema> }; paths: Record<string, Record<string, unknown>> };
  function matches(value: object, name: string) {
    const s = spec.components.schemas[name];
    expect(s, name).toBeTruthy();
    const all = Object.keys(s.properties ?? {});
    const got = Object.keys(value);
    expect((s.required ?? []).filter((f) => !got.includes(f)), `${name}: missing`).toEqual([]);
    expect(got.filter((f) => !all.includes(f)), `${name}: not the station’s`).toEqual([]);
  }

  it('has every route the Keys tab calls, with its method', () => {
    const has = (path: string, ...methods: string[]) => {
      expect(spec.paths[path], path).toBeTruthy();
      for (const m of methods) expect(spec.paths[path][m], `${m} ${path}`).toBeTruthy();
    };
    has(ROUTES.keys, 'get', 'post');
    has(ROUTES.key('{id}').replace('%7Bid%7D', '{id}'), 'get', 'patch', 'delete');
    for (const r of [ROUTES.text, ROUTES.extend, ROUTES.pause, ROUTES.resume, ROUTES.revoke, ROUTES.replace]) has(r('{id}').replace('%7Bid%7D', '{id}'), 'post');
    has(ROUTES.log('{id}').replace('%7Bid%7D', '{id}'), 'get');
    has(ROUTES.devices('{id}').replace('%7Bid%7D', '{id}'), 'get');
    has(ROUTES.device('{id}', '{device}').replace('%7Bid%7D', '{id}').replace('%7Bdevice%7D', '{device}'), 'delete');
    has(ROUTES.find, 'post');
    has(ROUTES.leaks, 'get');
    has(ROUTES.families, 'get', 'post');
    has(ROUTES.familyExport('{id}').replace('%7Bid%7D', '{id}'), 'get');
    has(ROUTES.access('{show}').replace('%7Bshow%7D', '{show}'), 'get', 'put');
    has(ROUTES.keyedShows, 'get');
  });

  it('names a key’s fields as the station does', () => {
    matches(key(), 'ListenerKey');
    const f = { ...freshForm(), label: 'Tara Douglas', email: 't@mail.example', note: 'n', family: 'fam-1' };
    matches(specOf(f, at), 'ListenerKeyNew');
    matches(templateOf(f, at), 'ListenerKeyTemplate');
    matches(changeOf(key(), { ...formOf(key(), () => ''), label: 'New', note: 'x', media: 'sound', opens: 'all', devices: '', listens: '3', people: '4', email: 'e@mail.example' }, at, 'set'), 'ListenerKeyChange');
    matches(familySpecOf('Autumn patrons', 'a note', templateOf(f, at), { recipients: [{ label: 'Isla Fraser', email: 'i@mail.example', note: 'x' }] }), 'ListenerKeyFamilyNew');
    matches(familySpecOf('Tickets', '', templateOf(f, at), { count: 200, prefix: 'Ticket' }), 'ListenerKeyFamilyNew');
    matches({ label: 'Isla Fraser', email: 'i@mail.example', note: 'x' }, 'ListenerKeyRecipient');
  });

  it('is what the stand-in station answers with', async () => {
    const path = new URL('../../../tests/keys/station.mjs', import.meta.url).href;
    const { KeysStation } = (await import(/* @vite-ignore */ path)) as { KeysStation: new (o?: object) => any };
    const st = new KeysStation({ role: 'admin' });
    const list = st.handle('GET', '/api/v1/listener-keys?limit=3')[1];
    matches(list, 'ListenerKeyList');
    matches(list.keys[0], 'ListenerKey');
    matches(st.handle('GET', `/api/v1/listener-keys/${st.isla.id}/devices`)[1].devices[0], 'ListenerKeyDevice');
    matches(st.handle('GET', `/api/v1/listener-keys/${st.katie.id}/log`)[1].entries[0], 'ListenerKeyLogEntry');
    matches(st.handle('POST', `/api/v1/listener-keys/${st.isla.id}/text`, {})[1], 'ListenerKeyText');
    matches(st.handle('GET', '/api/v1/listener-keys/families')[1].families[0], 'ListenerKeyFamily');
    const leaks = st.handle('GET', '/api/v1/listener-keys/leaks')[1];
    matches(leaks, 'ListenerKeyLeakReport');
    matches(leaks.entries[0], 'ListenerKeyLeak');
    const shows = st.handle('GET', '/api/v1/public/keyed-shows')[1];
    matches(shows, 'KeyedShowList');
    matches(shows.shows[0], 'KeyedShowRow');
    matches(st.handle('GET', '/api/v1/shows/friday-supporters/access')[1], 'ShowAccess');
    const made = st.handle('POST', '/api/v1/listener-keys', { label: 'Tara Douglas' });
    expect(made[0]).toBe(201);
    matches(made[1], 'ListenerKeyMade');
    const replaced = st.handle('POST', `/api/v1/listener-keys/${st.callum.id}/replace`, {})[1];
    matches(replaced, 'ListenerKeyReplaced');
    // Who may: a presenter is refused, a producer is not offered erasure.
    expect(new KeysStation({ role: 'presenter' }).handle('GET', '/api/v1/listener-keys')[0]).toBe(403);
    const producer = new KeysStation({ role: 'producer' });
    expect(producer.handle('DELETE', `/api/v1/listener-keys/${producer.katie.id}`)[0]).toBe(403);
    expect(st.handle('DELETE', `/api/v1/listener-keys/${st.katie.id}`)[0]).toBe(204);
  });
});

describe('who may', () => {
  it('manage keys: a producer, an admin and the station key', () => {
    expect(mayManageKeys(staff('admin'))).toBe(true);
    expect(mayManageKeys(staff('producer'))).toBe(true);
    expect(mayManageKeys({ kind: 'station', role: 'admin' } as Me)).toBe(true);
    expect(mayManageKeys(staff('presenter'))).toBe(false);
    expect(mayManageKeys(staff('screener'))).toBe(false);
    expect(mayManageKeys(null)).toBe(false);
  });
  it('erase a key: an admin and the station key', () => {
    expect(mayEraseKey(staff('admin'))).toBe(true);
    expect(mayEraseKey({ kind: 'station', role: 'admin' } as Me)).toBe(true);
    expect(mayEraseKey(staff('producer'))).toBe(false);
  });
});

describe('where a key stands', () => {
  it('reads the window and the limits into the station’s state', () => {
    expect(standing(key({ valid_until_ms: null }), NOW)).toBe('works');
    expect(standing(key(), NOW)).toBe('works');
    expect(standing(key({ valid_until_ms: NOW + 6 * DAY }), NOW)).toBe('ends-soon');
    expect(standing(key({ valid_until_ms: NOW + 6 * DAY }), NOW, 3)).toBe('works');
    expect(standing(key({ valid_from_ms: NOW + DAY }), NOW)).toBe('not-started');
    expect(standing(key({ valid_until_ms: NOW }), NOW)).toBe('ended');
    expect(standing(key({ max_listens: 9 }), NOW)).toBe('used-up');
    expect(standing(key({ state: 'paused', valid_until_ms: NOW - 1 }), NOW)).toBe('paused');
    expect(standing(key({ state: 'revoked' }), NOW)).toBe('revoked');
    expect(standing(key({ state: 'revoked', replaced_by_id: 'key-2' }), NOW)).toBe('replaced');
  });

  it('is never a lamp: no chip is filled or red', () => {
    for (const s of Object.values(STANDING)) {
      expect(s.cls).toMatch(/^hollow /);
      expect(s.cls).not.toMatch(/fill-|critical|onair/);
      expect(`${s.word} ${s.line}`).not.toMatch(/token|access code|on air/i);
    }
  });

  it('is counted for the head of the list', () => {
    expect(tally([key({ valid_until_ms: null }), key({ valid_from_ms: NOW + DAY }), key({ state: 'paused' }), key({ state: 'revoked' }), key({ valid_until_ms: NOW - 1 })], NOW)).toEqual({ works: 2, paused: 1, over: 2 });
  });
});

describe('the words for a key', () => {
  it('say its window in the station’s time', () => {
    expect(windowWords(key(), NOW, Z)).toBe('Ends 30 Nov 2026, 22:00');
    expect(windowWords(key({ valid_until_ms: null }), NOW, Z)).toBe('No end');
    expect(windowWords(key({ valid_until_ms: Date.UTC(2026, 10, 15, 22, 0) }), NOW, Z)).toBe('Ended 15 Nov 2026, 22:00');
    expect(windowWords(key({ valid_from_ms: Date.UTC(2026, 10, 28, 19, 30), valid_until_ms: Date.UTC(2026, 10, 29, 2, 0) }), NOW, Z)).toBe('Opens 28 Nov 2026, 19:30 · ends 29 Nov 2026, 02:00');
    expect(windowWords(key({ valid_from_ms: Date.UTC(2026, 10, 28, 19, 30), valid_until_ms: null }), NOW, Z)).toBe('Opens 28 Nov 2026, 19:30 · no end');
    // A start that has passed is not said.
    expect(windowWords(key({ valid_from_ms: NOW - DAY, valid_until_ms: null }), NOW, Z)).toBe('No end');
    // British summer time: the station's wall clock, not UTC.
    expect(windowWords(key({ valid_until_ms: Date.UTC(2026, 6, 1, 21, 0) }), Date.UTC(2026, 5, 1), Z)).toBe('Ends 1 Jul 2026, 22:00');
  });

  it('say its limits', () => {
    expect(limitsWords(key())).toBe('2 devices at once');
    expect(limitsWords(key({ max_devices: 1, max_listens: 2 }))).toBe('1 device at once · 2 listens');
    expect(limitsWords(key({ max_devices: null }))).toBe('any number of devices');
    expect(limitsWords(key({ kind: 'group', max_people: 40 }))).toBe('2 devices at once · up to 40 people');
    expect(limitsWords(key({ kind: 'group', max_people: 1 }))).toBe('2 devices at once · up to 1 person');
    expect(limitsWords(key({ kind: 'machine', max_devices: null }))).toBe('No limits');
  });

  it('say what it opens, by the shows’ names', () => {
    expect(opensWords(key(), SHOWS)).toBe('Friday Supporters’ Hour and The Archive Hour');
    expect(opensWords(key({ all_shows: true, shows: [] }), SHOWS)).toBe('Every keyed show');
    expect(opensWords(key({ shows: SHOWS.map((s) => s.id) }), SHOWS)).toBe('Friday Supporters’ Hour and 2 more');
    expect(opensWords(key({ shows: [] }), SHOWS)).toBe('No show');
    // A show the key names that is no longer keyed is counted, not named.
    expect(opensWords(key({ shows: ['id-friday-supporters', 'id-gone'] }), SHOWS)).toBe('Friday Supporters’ Hour (and 1 show not keyed now)');
    expect(opensWords(key({ shows: ['id-gone'] }), SHOWS)).toBe('1 show that is not keyed now');
  });

  it('say its use', () => {
    expect(useWords(key(), NOW)).toBe('9 listens · last used 26 hours ago');
    expect(useWords(key({ max_listens: 20 }), NOW)).toBe('9 of 20 listens · last used 26 hours ago');
    expect(useWords(key({ last_used_ms: null, listens_used: 0 }), NOW)).toBe('Not used yet');
    expect(fromNow(NOW + 5 * DAY, NOW)).toBe('in 5 days');
    expect(fromNow(NOW - 90_000, NOW)).toBe('2 minutes ago');
    expect(fromNow(NOW - 3_600_000, NOW)).toBe('1 hour ago');
  });

  it('say which devices are listening now', () => {
    const d = { id: 'd', label: 'Firefox on Linux', first_seen_ms: NOW - DAY, last_seen_ms: NOW - 60_000, ended_ms: null };
    expect(deviceStanding(d, NOW)).toEqual({ word: 'Listening now', live: true });
    expect(deviceStanding({ ...d, last_seen_ms: NOW - 2 * DAY }, NOW)).toEqual({ word: 'Last seen 2 days ago', live: false });
    expect(deviceStanding({ ...d, ended_ms: NOW }, NOW)).toEqual({ word: 'Signed out', live: false });
  });

  it('say each entry of the change log, from field names alone', () => {
    const e = (action: string, detail: Record<string, unknown> = {}) => ({ action, actor: 'Shona Campbell', at_ms: NOW, detail });
    expect(logWords(e('created'))).toBe('Made');
    expect(logWords(e('changed', { fields: ['label', 'valid_until', 'max_devices', 'shows', 'all_shows'] }))).toBe('Changed who it is for, when it ends, the devices limit, the shows it opens');
    expect(logWords(e('extended', { from_ms: 1, to_ms: Date.UTC(2026, 11, 7, 22, 0) }), Z)).toBe('Extended to 7 Dec 2026, 22:00');
    expect(logWords(e('paused', { reason: 'Payment late' }))).toBe('Paused: Payment late');
    expect(logWords(e('revoked'))).toBe('Revoked');
    expect(logWords(e('replaced', { by_key: 'k2' }))).toBe('Replaced by a new key');
    expect(logWords(e('text_shown'))).toBe('Its text was shown');
    expect(logWords(e('text_shown', { export: 'fam-1' }))).toBe('Its text went out in the family’s export');
    expect(logWords(e('lost_key_asked'))).toMatch(/Lost your key/);
    expect(logWords(e('device_ended', { device: 'd1' }))).toBe('A device was signed out');
    expect(logWords(e('something_new'))).toBe('something new');
  });
});

describe('the forms', () => {
  it('start from the common case', () => {
    expect(specOf({ ...freshForm(), label: ' Tara Douglas ' }, at)).toEqual({ label: 'Tara Douglas', kind: 'personal', media: 'both', all_shows: true, shows: [], valid_from_ms: null, valid_until_ms: null, max_devices: 2, max_listens: null, max_people: null });
  });

  it('read a limit as typed', () => {
    expect(limitOf('')).toBeNull();
    expect(limitOf(' any ')).toBeNull();
    expect(limitOf('5')).toBe(5);
    for (const bad of ['0', '-1', '2.5', 'two', '1e3']) expect(limitOf(bad), bad).toBe('bad');
  });

  it('catch a slip before the station is asked, and say which field first', () => {
    expect(keyProblems({ ...freshForm(), label: 'Tara Douglas' }, at)).toEqual({});
    const p = keyProblems({ ...freshForm(), label: ' ', email: 'tara@', opens: 'some', from: '2026-12-01T10:00', until: '2026-11-30T10:00', devices: '0', listens: 'x' }, at);
    expect(Object.keys(p).sort()).toEqual(['devices', 'email', 'label', 'listens', 'shows', 'until']);
    expect(firstProblem(p)).toBe('label');
    expect(p.until).toBe('The end is after the start.');
    expect(keyProblems({ ...freshForm(), label: 'x'.repeat(121) }, at).label).toBe('Up to 120 characters.');
    expect(keyProblems({ ...freshForm(), label: 'A', opens: 'some', shows: Array.from({ length: 33 }, (_, i) => `s${i}`) }, at).shows).toMatch(/up to 32/);
    // A machine key has no devices, and only a group has people.
    expect(keyProblems({ ...freshForm(), label: 'Relay', kind: 'machine', devices: 'x', people: 'x' }, at)).toEqual({});
    // A family's template has no name of its own.
    expect(keyProblems({ ...freshForm(), label: '' }, at, false)).toEqual({});
  });

  it('send a machine key no devices, and only a group its people', () => {
    expect(templateOf({ ...freshForm(), kind: 'machine', devices: '2', people: '9' }, at)).toMatchObject({ max_devices: null, max_people: null });
    expect(templateOf({ ...freshForm(), kind: 'group', people: '40' }, at)).toMatchObject({ max_devices: 2, max_people: 40 });
    expect(templateOf({ ...freshForm(), opens: 'some', shows: ['a'], from: '2026-11-28T19:30', until: '2026-11-29T02:00' }, at)).toMatchObject({ all_shows: false, shows: ['a'], valid_from_ms: Date.UTC(2026, 10, 28, 19, 30), valid_until_ms: Date.UTC(2026, 10, 29, 2, 0) });
  });

  it('send only what changed, and null for what was taken away', () => {
    const k = key();
    const f = formOf(k, (ms) => new Date(ms).toISOString().slice(0, 16));
    expect(changeOf(k, f, at, 'keep')).toEqual({});
    expect(changeOf(k, { ...f, until: '' }, at, 'keep')).toEqual({ valid_until_ms: null });
    expect(changeOf(k, { ...f, devices: '' }, at, 'keep')).toEqual({ max_devices: null });
    expect(changeOf(k, { ...f, label: 'Isla Fraser-Reid', note: 'married' }, at, 'keep')).toEqual({ label: 'Isla Fraser-Reid', note: 'married' });
    expect(changeOf(k, { ...f, opens: 'all' }, at, 'keep')).toEqual({ all_shows: true, shows: [] });
    expect(changeOf(k, { ...f, shows: ['id-friday-supporters'] }, at, 'keep')).toEqual({ all_shows: false, shows: ['id-friday-supporters'] });
    expect(changeOf(k, f, at, 'remove')).toEqual({ email: null });
    expect(changeOf(k, { ...f, email: 'new@mail.example' }, at, 'set')).toEqual({ email: 'new@mail.example' });
    // The address typed is not sent unless that is what was chosen.
    expect(changeOf(k, { ...f, email: 'new@mail.example' }, at, 'keep')).toEqual({});
  });

  it('say how soon a change reaches a listener, from the station’s own setting', () => {
    expect(cutWords(60)).toBe('within a minute');
    expect(cutWords(120)).toBe('within two minutes');
    expect(cutWords(300)).toBe('within 5 minutes');
    expect(cutWords(30)).toBe('within 30 seconds');
    expect(cutWords(null)).toBe('within a few minutes');
  });

  it('know when a key cannot be extended', () => {
    expect(extendProblem(key())).toBeNull();
    expect(extendProblem(key({ valid_until_ms: null }))).toMatch(/no end/);
    expect(extendProblem(key({ state: 'revoked' }))).toMatch(/Replace it/);
  });
});

describe('a pasted list', () => {
  it('is read a person a line, with an email address and a note where there are any', () => {
    const p = parseRecipients('Isla Fraser, isla@mail.example\nCallum Reid,callum@mail.example, joined in October, by post\n\nKatie Ross\n');
    expect(p.problems).toEqual([]);
    expect(p.recipients).toEqual([{ label: 'Isla Fraser', email: 'isla@mail.example' }, { label: 'Callum Reid', email: 'callum@mail.example', note: 'joined in October, by post' }, { label: 'Katie Ross' }]);
  });

  it('takes a spreadsheet’s columns, semicolons and quoted cells, and leaves a line of column names out', () => {
    expect(parseRecipients('Name\tEmail\tNote\nIsla Fraser\tisla@mail.example\tpatron').recipients).toEqual([{ label: 'Isla Fraser', email: 'isla@mail.example', note: 'patron' }]);
    expect(parseRecipients('Name\tEmail\tNote\nIsla Fraser\tisla@mail.example\tpatron').header).toBe(true);
    expect(parseRecipients('Fraser; isla@mail.example; a note').recipients).toEqual([{ label: 'Fraser', email: 'isla@mail.example', note: 'a note' }]);
    expect(parseRecipients('"Fraser, Isla", isla@mail.example').recipients).toEqual([{ label: 'Fraser, Isla', email: 'isla@mail.example' }]);
    expect(parseRecipients('name\nIsla Fraser').recipients).toEqual([{ label: 'Isla Fraser' }]);
  });

  it('says every line that cannot be a key, and drops none in silence', () => {
    const p = parseRecipients(`Isla Fraser, isla@mail.example\n, nobody@mail.example\nKatie Ross, katie@\nIsla Fraser, ISLA@mail.example\n${'x'.repeat(121)}`);
    expect(p.recipients).toHaveLength(1);
    expect(p.problems).toEqual(['Line 2 has no name.', 'Line 3: “katie@” is not an email address.', 'Line 4 repeats Isla Fraser (ISLA@mail.example).', 'Line 5: the name is longer than 120 characters.']);
    const many = parseRecipients(Array.from({ length: 1001 }, (_, i) => `Ticket holder ${i}`).join('\n'));
    expect(many.problems.at(-1)).toMatch(/up to 1000 keys in one go, and this list has 1001/);
  });

  it('checks a count and a family’s name', () => {
    expect(countProblem('200')).toBeNull();
    expect(countProblem('0')).toMatch(/from 1/);
    expect(countProblem('1001')).toMatch(/Up to 1000/);
    expect(countProblem('many')).toMatch(/how many/);
    expect(familyLabelProblem('Autumn patrons')).toBeNull();
    expect(familyLabelProblem(' ')).toMatch(/Name the family/);
    expect(familyLabelProblem('autumn PATRONS', ['Autumn patrons'])).toMatch(/has that name already/);
  });
});

describe('CSV', () => {
  it('writes a cell as the station’s own export does', () => {
    expect(csvCell('Isla Fraser')).toBe('Isla Fraser');
    expect(csvCell('a,b')).toBe('"a,b"');
    expect(csvCell('say "hi"')).toBe('"say ""hi"""');
    // A cell a spreadsheet would run as a formula is written with a quote in front.
    expect(csvCell('=1+1')).toBe('"\'=1+1"');
    expect(csvCell('+44 7700')).toBe('"\'+44 7700"');
    expect(csvCell('@sum')).toBe('"\'@sum"');
    expect(csvCell(null)).toBe('');
    expect(csvCell(7)).toBe('7');
    expect(csv(['a', 'b'], [['1', 'x,y']])).toBe('a,b\r\n1,"x,y"\r\n');
  });

  it('names a file from a family’s name', () => {
    expect(fileStem('Autumn patrons')).toBe('autumn-patrons');
    expect(fileStem('Cèilidh Night: tickets!')).toBe('ceilidh-night-tickets');
    expect(fileStem('  ')).toBe('keys');
  });
});

describe('the leak report', () => {
  const leak = (o: Partial<KeyLeak> = {}): KeyLeak => ({ key: key(), family: 'Autumn patrons', first_seen_ms: NOW - DAY, last_seen_ms: NOW, listens: 3, devices: 1, addresses: 1, addresses_24h: 0, addresses_7d: 1, peak_new_addresses_in_an_hour: 1, spike: false, score: 0, reasons: [], ...o });
  const hugh = leak({ key: key({ label: 'Hugh Wallace' }), score: 151, spike: true, addresses_7d: 41, addresses_24h: 33, addresses: 44, devices: 9, peak_new_addresses_in_an_hour: 17, reasons: ['41 addresses in the last 7 days, where about 2 is usual', '17 new addresses within one hour'] });

  it('reads a row: a burst is likely passed on, any other sign is worth a look', () => {
    expect(leakLevel(hugh)).toBe('likely');
    expect(leakLevel(leak({ score: 16 }))).toBe('look');
    expect(leakLevel(leak())).toBe('none');
    expect(leakLevel(leak({ first_seen_ms: null, last_seen_ms: null, listens: 0 }))).toBe('unused');
  });

  it('sums the report up in a line, and says when nothing is used yet', () => {
    expect(leakSummary([hugh, leak({ score: 16 }), leak()]).line).toBe('2 keys show a sign of being passed on, of 3 used; 1 looks posted in public.');
    expect(leakSummary([leak(), leak()]).line).toBe('2 keys have been used, and none shows a sign of being passed on.');
    expect(leakSummary([leak({ first_seen_ms: null, listens: 0 })]).line).toBe('No key has been used yet, so there is nothing to compare.');
    expect(leakSummary([]).line).toBe('No keys to report on.');
    // A key already replaced keeps its row and is not counted among those to look at.
    const dealt = leak({ key: key({ state: 'revoked', replaced_by_id: 'k2' }), score: 34, spike: true });
    expect(leakSummary([hugh, dealt, leak()])).toMatchObject({ signs: 1, likely: 1, used: 3 });
  });

  it('exports counts and signs, never a key’s text or an address', () => {
    const out = leaksCsv([hugh], Z);
    expect(out.split('\r\n')[0]).toBe('key,family,state,score,signs,addresses_7_days,addresses_24_hours,addresses_ever,devices,listens,most_new_addresses_in_an_hour,first_seen,last_seen');
    expect(out).toContain('Hugh Wallace,Autumn patrons,active,151,"41 addresses in the last 7 days, where about 2 is usual; 17 new addresses within one hour",41,33,44,9,3,17,');
  });
});

describe('the message a key is sent with', () => {
  const facts = { station: 'Kettle FM', label: 'Isla Fraser', kind: 'personal' as const, link: 'https://kettle.example/listen/#key=KETM-7QMP-2D4X', text: 'KETM-7QMP-2D4X', opens: 'Friday Supporters’ Hour and The Archive Hour', valid_from_ms: null, valid_until_ms: Date.UTC(2026, 10, 30, 22, 0), max_devices: 2, personal: true, contact: '' };

  it('has the link, the key to type, the window, and what to do if it fails', () => {
    const m = messageFor(facts, NOW, Z);
    expect(m.subject).toBe('Your key for Kettle FM');
    expect(m.body).toBe(
      [
        'Hello Isla,',
        '',
        'Here is your key for Friday Supporters’ Hour and The Archive Hour.',
        '',
        'Open this link and press Listen:',
        'https://kettle.example/listen/#key=KETM-7QMP-2D4X',
        '',
        'Or go to https://kettle.example/listen/ and type this into “Your key”:',
        'KETM-7QMP-2D4X',
        'Capitals, spaces and dashes do not matter.',
        '',
        'It works until 30 Nov 2026, 22:00 (Europe/London time).',
        'It works on 2 devices at a time. The same link works on your phone and your computer.',
        'This key is yours alone. Please do not forward it or post it anywhere.',
        '',
        'If it does not work, the page says why. If you are stuck, reply to this message.',
        '',
        'Kettle FM',
      ].join('\n'),
    );
  });

  it('never says token or access code, whatever the key', () => {
    for (const o of [{}, { kind: 'group' as const, label: 'Dunfermline Gaelic Choir' }, { kind: 'machine' as const, label: 'Radio Ross relay', max_devices: null }, { valid_from_ms: NOW + DAY }, { valid_until_ms: null, personal: false, opens: 'Every keyed show', contact: 'ring 01383 496 0123' }]) {
      const m = messageFor({ ...facts, ...o }, NOW, Z);
      expect(`${m.subject}\n${m.body}`).not.toMatch(/token|access code|passcode|password/i);
    }
  });

  it('greets a person by name and nobody else, and words a group’s rule for a group', () => {
    expect(greetingName('Isla Fraser', 'personal')).toBe('Isla');
    expect(greetingName('Dr Una Robertson', 'personal')).toBe('Dr Una Robertson');
    expect(greetingName('Ticket 014', 'personal')).toBeNull();
    expect(greetingName('Dunfermline Gaelic Choir', 'group')).toBeNull();
    const group = messageFor({ ...facts, kind: 'group', label: 'Dunfermline Gaelic Choir' }, NOW, Z).body;
    expect(group.startsWith('Hello,')).toBe(true);
    expect(group).toContain('This key is for your group only.');
    expect(messageFor({ ...facts, opens: 'Every keyed show' }, NOW, Z).body).toContain('Here is your key for Kettle FM’s shows for key holders.');
    expect(messageFor({ ...facts, valid_from_ms: Date.UTC(2026, 10, 28, 19, 30), valid_until_ms: Date.UTC(2026, 10, 29, 2, 0) }, NOW, Z).body).toContain('It opens on 28 Nov 2026, 19:30 and ends on 29 Nov 2026, 02:00 (Europe/London time).');
    expect(messageFor({ ...facts, personal: false }, NOW, Z).body).not.toContain('yours alone');
    expect(stationAddress(facts.link)).toBe('https://kettle.example/listen/');
  });
});

describe('a key’s text, as typed or found', () => {
  it('is written in three groups, whatever the case and the dashes', () => {
    expect(shapeOfText('ketm 7qmp2d4x')).toBe('KETM-7QMP-2D4X');
    expect(shapeOfText('KETM-7QMP-2D4X')).toBe('KETM-7QMP-2D4X');
    // No 0, 1, I, L or O in a key.
    expect(shapeOfText('KETM-7QMP-2D40')).toBeNull();
    expect(shapeOfText('KETM-7QMP')).toBeNull();
  });
  it('is found in a link that carries one', () => {
    expect(textIn('seen at https://kettle.example/listen/#key=ketm-7qmp-2d4x')).toBe('KETM-7QMP-2D4X');
    expect(textIn(' KETM7QMP2D4X ')).toBe('KETM-7QMP-2D4X');
    expect(textIn('hello')).toBeNull();
  });
});

describe('when the whole screen is refused', () => {
  it('says what it means and what to do, in words', () => {
    expect(unreadyOf(new FieldError(401, 'Your sign-in has ended. Sign in again.', null))).toMatchObject({ kind: 'signed-out', said: '' });
    expect(unreadyOf(new FieldError(403, "Only a producer or an admin can manage keys. Ask the station's admin.", null))).toMatchObject({ kind: 'not-allowed', retry: false, said: "Only a producer or an admin can manage keys. Ask the station's admin." });
    expect(unreadyOf(new FieldError(404, '404 Not Found', null))).toMatchObject({ kind: 'older', said: '' });
    expect(unreadyOf(new FieldError(503, 'Keys are kept in the database.', null))).toMatchObject({ kind: 'not-set-up', said: 'Keys are kept in the database.' });
    expect(unreadyOf(new FieldError(500, 'the database refused that', null))).toMatchObject({ kind: 'problem', said: 'the database refused that' });
    expect(unreadyOf(new TypeError('Failed to fetch'))).toMatchObject({ kind: 'problem', said: '' });
    for (const s of [401, 403, 404, 503, 500]) {
      const u = unreadyOf(new FieldError(s, 'x', null));
      expect(`${u.title} ${u.body}`).not.toMatch(/token|access code/i);
    }
  });

  it('gives the station’s refusal of one action a person’s words where it has none', () => {
    expect(actionProblem(new FieldError(409, "the station's token key changed since this key was made: its text can no longer be read; replace the key", null))).toBe('This key’s text can no longer be read: the station’s own key changed after it was made. Replace the key and send the new one.');
    expect(actionProblem(new FieldError(404, 'no such key', null))).toMatch(/no longer there/);
    expect(actionProblem(new FieldError(409, 'a revoked key cannot be changed: replace it instead', null))).toBe('a revoked key cannot be changed: replace it instead');
    expect(actionProblem(new TypeError('Failed to fetch'))).toBe('Failed to fetch');
  });
});

describe('the keymap', () => {
  const ctx = (tab: (typeof TABS)[number], inTextField = false) => ({ tab, inTextField, dialog: null });
  const down = (code: string, o: Partial<{ ctrl: boolean; shift: boolean }> = {}, c = ctx('keys')) => new KeyEngine().down({ code, ctrl: false, shift: false, alt: false, meta: false, repeat: false, ...o }, c, 0);

  it('gives the Keys tab ⌃6, and keeps every other tab’s number', () => {
    expect(TABS).toEqual(['desk', 'screener', 'carts', 'chat', 'schedule', 'keys']);
    expect(down('Digit6', { ctrl: true }, ctx('desk')).action).toEqual({ type: 'tab', tab: 'keys' });
    expect(down('Digit5', { ctrl: true }, ctx('desk')).action).toEqual({ type: 'tab', tab: 'schedule' });
    expect(down('Digit7', { ctrl: true }, ctx('desk')).handled).toBe(false);
  });

  it('gives the tab N and /, and nothing that ends a key', () => {
    expect(down('KeyN').action).toEqual({ type: 'keys', what: 'make' });
    expect(down('Slash').action).toEqual({ type: 'keys', what: 'search' });
    for (const code of ['KeyR', 'KeyP', 'KeyX', 'Backspace', 'Enter', 'Space']) expect(down(code).handled, code).toBe(false);
    // A text field owns the unmodified keys (rule 3), and N on another tab is that tab's.
    expect(down('KeyN', {}, ctx('keys', true)).handled).toBe(false);
    expect(down('KeyN', {}, ctx('desk')).action).not.toEqual({ type: 'keys', what: 'make' });
    expect(down('KeyN', { shift: true }).handled).toBe(false);
  });

  it('counts the tab’s keys as changing nothing on air', () => {
    expect(changesAir({ type: 'keys', what: 'make' })).toBe(false);
    expect(changesAir({ type: 'tab', tab: 'keys' })).toBe(false);
  });

  it('has a line for every panel of the tab', () => {
    for (const p of ['keys.toolbar', 'keys.list', 'keys.families', 'keys.leaks', 'keys.key', 'keys.about', 'keys.unready', 'keys.make', 'keys.make-many', 'keys.made-many', 'keys.send', 'keys.change', 'keys.ask', 'keys.extend', 'keys.find', 'keys.keep-show', 'calendar.show-access', 'desk.keyed']) expect(PANELS[p], p).toBeTruthy();
  });
});

describe('the QR code', () => {
  it('has the standard’s format and version bits', () => {
    // ISO/IEC 18004, Annex C: level M, mask 5 is 100000011001110; and Annex D: version 7 is
    // 000111110010010100.
    expect(formatBits(5).toString(2).padStart(15, '0')).toBe('100000011001110');
    expect(formatBits(0).toString(2).padStart(15, '0')).toBe('101010000010010');
    expect(versionBits(7).toString(2).padStart(18, '0')).toBe('000111110010010100');
  });

  it('computes the standard’s error correction for its own example', () => {
    // The 1-M example of the standard's Annex I ("01234567"): data codewords, then their ten
    // error-correction codewords.
    const data = Uint8Array.from([0x10, 0x20, 0x0c, 0x56, 0x61, 0x80, 0xec, 0x11, 0xec, 0x11, 0xec, 0x11, 0xec, 0x11, 0xec, 0x11]);
    expect([...rsRemainder(data, 10)]).toEqual([0xa5, 0x24, 0xd4, 0xc1, 0xed, 0x36, 0xc7, 0x87, 0x2c, 0x55]);
  });

  it('chooses the smallest version that holds the text', () => {
    expect([1, 2, 3, 4, 5, 10].map(qrCapacity)).toEqual([14, 26, 42, 62, 84, 213]);
    expect(qr('hello').version).toBe(1);
    const link = qr('https://kettle.example/listen/#key=KETM-7QMP-2D4X');
    expect([link.version, link.size]).toEqual([4, 33]);
    expect(() => qr('x'.repeat(214))).toThrow(/too long/);
  });

  it('draws the three finders, the timing rows and the dark module, whatever the text', () => {
    for (const text of ['A', 'https://kettle.example/listen/#key=KETM-7QMP-2D4X', 'x'.repeat(150)]) {
      const c = qr(text);
      const m = c.modules;
      const n = c.size;
      const finder = (r0: number, c0: number) => {
        for (let r = 0; r < 7; r++) for (let col = 0; col < 7; col++) expect(m[r0 + r][c0 + col], `${text.length}:${r},${col}`).toBe(r === 0 || r === 6 || col === 0 || col === 6 || (r >= 2 && r <= 4 && col >= 2 && col <= 4));
      };
      finder(0, 0);
      finder(0, n - 7);
      finder(n - 7, 0);
      for (let i = 8; i < n - 8; i++) {
        expect(m[6][i]).toBe(i % 2 === 0);
        expect(m[i][6]).toBe(i % 2 === 0);
      }
      expect(m[n - 8][8]).toBe(true);
      // The format bits round the top-left finder are those of the mask it chose.
      const bits = formatBits(c.mask);
      for (let i = 0; i <= 5; i++) expect(m[i][8]).toBe(((bits >>> i) & 1) === 1);
    }
  });

  it('is drawn as one path with a quiet zone, dark on white', () => {
    const c = qr('https://kettle.example/listen/#key=KETM-7QMP-2D4X');
    const p = qrPath(c);
    expect(p.box).toBe(c.size + 8);
    expect(p.d.startsWith('M4 4h7v1h-7z')).toBe(true);
    const svg = qrSvg(c);
    expect(svg).toContain(`viewBox="0 0 ${p.box} ${p.box}"`);
    expect(svg).toContain('fill="#fff"');
  });
});

describe('Lost your key?, on the staff’s side (1.3.2: the words follow whether the station sends mail)', () => {
  const entry = (action: string, at_ms: number): KeyLogEntry => ({ action, actor: 'a listener', at_ms, detail: {} });

  it('says what the address is for where the station sends email, and when it does not say', () => {
    for (const mail of [true, null, undefined]) {
      expect(emailFact(true, mail)).toBe('An address is kept, sealed. It is shown with the key’s text, in Send.');
      expect(emailFact(false, mail)).toBe('None kept, so Lost your key? cannot send this one.');
      expect(emailHint(mail)).toBe('Kept sealed. With it, Lost your key? on the player can send the key to its holder again.');
    }
  });

  it('says staff send the key themselves where the station sends no email', () => {
    expect(emailFact(true, false)).toBe('An address is kept, sealed. This station does not send email, so Lost your key? only notes the request here: you send the key yourself, with Send.');
    expect(emailFact(false, false)).toBe('None kept, so a Lost your key? request cannot be matched to this key.');
    expect(emailHint(false)).toContain('This station does not send email');
    for (const line of [emailFact(true, false), emailFact(false, false), emailHint(false)]) expect(line).not.toMatch(/can send the key to its holder/);
  });

  it('a request waits for a person when the station sent nothing, until the key’s text is shown or the key replaced', () => {
    const ask = (at_ms: number, sent?: boolean): KeyLogEntry => ({ ...entry('lost_key_asked', at_ms), detail: sent === undefined ? {} : { sent } });
    // The entry itself says whether the station emailed the key.
    expect(lostWaiting([entry('created', 1), ask(50, false)], null)).toEqual({ at_ms: 50 });
    expect(lostWaiting([entry('created', 1), ask(50, true)], false)).toBeNull();
    // An entry that does not say is read by whether the station sends email at all.
    expect(lostWaiting([ask(50)], false)).toEqual({ at_ms: 50 });
    expect(lostWaiting([ask(50)], true)).toBeNull();
    expect(lostWaiting([ask(50)], null)).toBeNull();
    // Answered: Send was opened (the station logs the text shown), or the key was replaced.
    expect(lostWaiting([ask(50, false), entry('text_shown', 60)], false)).toBeNull();
    expect(lostWaiting([ask(50, false), entry('replaced', 60)], false)).toBeNull();
    // Asked again after an answer: it waits again, from the newest request. The order of the log does not matter.
    expect(lostWaiting([ask(90, false), entry('text_shown', 60), ask(50, false)], false)).toEqual({ at_ms: 90 });
    // The newest request was emailed: an older unsent one no longer waits.
    expect(lostWaiting([ask(90, true), ask(50, false)], false)).toBeNull();
    // A text shown before the request is not an answer to it.
    expect(lostWaiting([entry('text_shown', 10), ask(50, false)], false)).toEqual({ at_ms: 50 });
    expect(lostWaiting([], false)).toBeNull();
    expect(lostWaiting(null, false)).toBeNull();
  });

  it('the log says whether the station emailed the key', () => {
    const ask = (sent?: boolean): KeyLogEntry => ({ ...entry('lost_key_asked', 5), detail: sent === undefined ? {} : { sent } });
    expect(logWords(ask(true))).toBe('Its holder asked for it again (Lost your key?): the station emailed it');
    expect(logWords(ask(false))).toBe('Its holder asked for it again (Lost your key?): nothing was sent');
    expect(logWords(ask())).toBe('Its holder asked for it again (Lost your key?)');
  });

  it('words the note: who asked, that nothing went out, and Send', () => {
    expect(`${LOST_WAITING.lead} ${LOST_WAITING.body}`).toBe('Its holder asked for it again with Lost your key? on the player. The station sent nothing, because it has no outgoing mail: send it to them yourself.');
    expect(LOST_WAITING.hint).toContain('Send… shows the link, the key and a message ready to copy.');
  });
});
