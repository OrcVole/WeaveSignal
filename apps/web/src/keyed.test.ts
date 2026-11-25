// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A listener's key on the public pages (keyed shows, 1.3.2): the addresses and the calls of
// lib/keyed/routes.ts against the station's own description, the words for every way a key
// does not work (never "token", never "access code"), the welcome, the help, and what is kept.
import { describe, expect, it, vi } from 'vitest';
import openapi from '../../../docs/openapi/weavesignal-v1.json';
import { readKept } from './lib/keyed/keyed.svelte.ts';
import * as routes from './lib/keyed/routes.ts';
import { ROUTES, Refusal, isKeyPage, keyInAddress, keyedHls, keyedStream, keyedStreams, refusalOf, withToken, withoutKey, type Device, type KeyedShow, type KeyedShows, type ListenKey, type ListenToken, type RefusalCode } from './lib/keyed/routes.ts';
import { BADGE, GREETING, dayWords, deviceLine, emailProblem, endsNotice, entryWords, helpItems, invitedKeyed, invitedWords, keptWords, keyedNow, lostAnswer, lostWords, opens, opensToAll, opensWords, pressToHear, problemWords, publicHears, waitWords, welcome, whenWords, type HelpFacts } from './lib/keyed/words.ts';

const NOW = Date.UTC(2026, 10, 23, 12, 0);
const Z = 'Europe/London';
const DAY = 86_400_000;

const key = (o: Partial<ListenKey> = {}): ListenKey => ({
  id: 'k1',
  kind: 'personal',
  label: 'Isla Fraser',
  media: 'both',
  all_shows: false,
  shows: [
    { id: 's-fri', slug: 'friday-supporters', name: 'Friday Supporters’ Hour' },
    { id: 's-arc', slug: 'the-archive-hour', name: 'The Archive Hour' },
    { id: 's-cei', slug: 'ceilidh-night', name: 'Ceilidh Night' },
    { id: 's-pre', slug: 'press-preview', name: 'Press Preview' },
  ],
  valid_from_ms: null,
  valid_until_ms: Date.UTC(2026, 10, 30, 22, 0),
  ends_soon: true,
  ...o,
});
const show = (o: Partial<KeyedShow> = {}): KeyedShow => ({ id: 's-fri', slug: 'friday-supporters', name: 'Friday Supporters’ Hour', now: 'key', opens_at_ms: null, slate: 'announcement', slate_note: '', ...o });
const refusal = (code: RefusalCode, at: number | null = null, devices: Device[] = [], retry: number | null = null) => new Refusal(code, 403, at, devices, retry);
const dev = (o: Partial<Device> = {}): Device => ({ id: 'd1', label: 'Firefox on Linux', first_seen_ms: NOW - DAY, last_seen_ms: NOW - 60_000, ended_ms: null, last_show_id: null, ...o });

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

  it('has every route the page calls', () => {
    for (const path of Object.values(ROUTES)) expect(spec.paths[path], path).toBeTruthy();
    expect(spec.paths[ROUTES.keyedShows].get).toBeTruthy();
    for (const p of [ROUTES.redeem, ROUTES.renew, ROUTES.devices, ROUTES.endDevice, ROUTES.lost]) expect(spec.paths[p].post, p).toBeTruthy();
  });

  it('names its fields as the station does', () => {
    const k = key();
    matches(k, 'ListenKey');
    matches(k.shows[0], 'ListenShow');
    const t: ListenToken = {
      token: 'ks1.x.y',
      device: 'd1',
      starts_ms: NOW - 3_000,
      expires_ms: NOW + 3_600_000,
      show: k.shows[0],
      urls: { hls: '/hls/k/s-fri/ks1.x.y/live.m3u8', opus: '/listen/k/s-fri/ks1.x.y/programme-keyed.opus', mp3: null },
      renew_in_s: 60,
      key: k,
    };
    matches(t, 'ListenToken');
    matches(t.urls, 'ListenUrls');
    matches(dev(), 'ListenerKeyDevice');
    matches(show(), 'KeyedShowRow');
    const list: KeyedShows = { shows: [show()], on_now: null, get_a_key_text: '', get_a_key_url: '', mail: true };
    matches(list, 'KeyedShowList');
    matches({ key: 'KETM-7QMP-2D4X', show: 's-fri', device: 'd1' }, 'ListenRedeem');
    matches({ token: 'ks1.x.y' }, 'ListenRenew');
    matches({ devices: [dev()], device_hold_s: 360 }, 'ListenDeviceList');
    matches({ key: 'KETM-7QMP-2D4X' }, 'ListenDevices');
    matches({ key: 'KETM-7QMP-2D4X', device: 'd1' }, 'ListenDeviceEnd');
    matches({ email: 'isla@example.org' }, 'ListenLost');
    // 1.3.2: the list (above) and the lost-key answer say whether the station sends email.
    matches({ message: 'This station does not send email itself.', mail: false }, 'ListenLostAnswer');
  });
});

describe('a key in the address', () => {
  it('is read from after the #, and from nowhere else', () => {
    expect(keyInAddress('#key=KETM-7QMP-2D4X')).toBe('KETM-7QMP-2D4X');
    expect(keyInAddress('#x=1&key=ketm7qmp2d4x')).toBe('ketm7qmp2d4x');
    expect(keyInAddress('#key=KETM%2D7QMP%2D2D4X')).toBe('KETM-7QMP-2D4X');
    expect(keyInAddress('')).toBeNull();
    expect(keyInAddress('#rsp1abc')).toBeNull();
    expect(keyInAddress('#monkey=1')).toBeNull();
    expect(keyInAddress('#key=')).toBeNull();
  });

  it('leads to /listen/, which is told from the player at / by its path alone', () => {
    for (const p of ['/listen/', '/listen', '/listen/index.html']) expect(isKeyPage(p), p).toBe(true);
    for (const p of ['/', '/index.html', '/join/', '/listen/k/abc/ks1.x.y/programme-keyed.opus', '/listening/', '/x/listen/']) expect(isKeyPage(p), p).toBe(false);
  });

  it('is taken out of the address, so a copied address does not carry it', () => {
    expect(withoutKey('https://kettle.example/listen/#key=KETM-7QMP-2D4X')).toBe('https://kettle.example/listen/');
    expect(withoutKey('https://kettle.example/listen/?x=1#key=K&chat=open')).toBe('https://kettle.example/listen/?x=1#chat=open');
    expect(withoutKey('https://kettle.example/')).toBe('https://kettle.example/');
  });
});

describe('where a key’s holder plays', () => {
  const page = 'https://kettle.example/listen/';
  const urls = { hls: 'https://edge.example/hls/k/s-fri/T/live.m3u8', opus: '/listen/k/s-fri/T/programme-keyed.opus', mp3: '/listen/k/s-fri/T/programme-keyed.mp3' };
  it('is the station’s own keyed stream, of the kind the listener chose', () => {
    expect(keyedStream(urls, 'https://stream.kettle.example/programme.opus', page)).toBe('https://kettle.example/listen/k/s-fri/T/programme-keyed.opus');
    expect(keyedStream(urls, 'https://stream.kettle.example/programme-128.mp3', page)).toBe('https://kettle.example/listen/k/s-fri/T/programme-keyed.mp3');
    // A station with its MP3 stream off has no keyed MP3: the Opus one.
    expect(keyedStream({ ...urls, mp3: null }, '/stream/programme.mp3', page)).toBe('https://kettle.example/listen/k/s-fri/T/programme-keyed.opus');
  });

  it('keeps each stream’s kind and rate', () => {
    const got = keyedStreams(
      [
        { codec: 'opus', kbps: 96, url: 'https://s.example/programme.opus' },
        { codec: 'mp3', kbps: 128, url: 'https://s.example/programme.mp3' },
      ],
      urls,
      page,
    );
    expect(got.map((s) => [s.codec, s.kbps, s.url])).toEqual([
      ['opus', 96, 'https://kettle.example/listen/k/s-fri/T/programme-keyed.opus'],
      ['mp3', 128, 'https://kettle.example/listen/k/s-fri/T/programme-keyed.mp3'],
    ]);
  });

  it('is the station’s keyed HLS, and its sound alone beside it', () => {
    expect(keyedHls(urls, page)).toEqual({ live: 'https://edge.example/hls/k/s-fri/T/live.m3u8', audio: 'https://edge.example/hls/k/s-fri/T/audio.m3u8' });
    expect(keyedHls({ ...urls, hls: '/hls/k/s-fri/T/live.m3u8' }, page).audio).toBe('https://kettle.example/hls/k/s-fri/T/audio.m3u8');
  });

  it('sends a later request with the token held now', () => {
    expect(withToken('https://edge.example/hls/k/s-fri/OLD/v0/seg00012.m4s', 'NEW')).toBe('https://edge.example/hls/k/s-fri/NEW/v0/seg00012.m4s');
    expect(withToken('https://edge.example/hls/live.m3u8', 'NEW')).toBe('https://edge.example/hls/live.m3u8');
  });
});

describe('a refusal, as the station sends it', () => {
  it('is read by its code, with what came with it', () => {
    expect(refusalOf(404, { code: 'not_recognised', error: 'That key is not recognised.' }).code).toBe('not_recognised');
    const early = refusalOf(403, { code: 'not_yet_valid', from_ms: 1234 });
    expect([early.code, early.at_ms]).toEqual(['not_yet_valid', 1234]);
    const over = refusalOf(403, { code: 'ended', until_ms: 99 });
    expect([over.code, over.at_ms]).toEqual(['ended', 99]);
    const many = refusalOf(409, { code: 'too_many_devices', devices: [dev(), dev({ id: 'd2' })] });
    expect(many.devices.map((d) => d.id)).toEqual(['d1', 'd2']);
    expect(refusalOf(401, { code: 'token_ended', error: 'That token has ended. Enter the key again.' }).code).toBe('token_ended');
  });

  it('knows a station too busy, one without keys, an older one and one that is not there', () => {
    const slow = refusalOf(429, {}, '120');
    expect([slow.code, slow.retry_s]).toEqual(['slow_down', 120]);
    expect(refusalOf(503, { error: 'Keys are kept in the database.' }).code).toBe('not_set_up');
    // No code on a 404: the route itself is missing, as on a station older than 1.3.2.
    expect(refusalOf(404, {}).code).toBe('not_set_up');
    expect(refusalOf(500, {}).code).toBe('no_answer');
    expect(refusalOf(403, { code: 'something_new' }).code).toBe('no_answer');
  });
});

describe('the welcome', () => {
  it('greets everyone the same, and never by the name staff gave the key', () => {
    // A key's name is staff's own, to find the key again: "Agent test (delete me)" was shown to
    // its holder (the operator, 2026-11-25). No key carries a name of the holder's own today.
    for (const label of ['Isla Fraser', 'Agent test (delete me)', 'Ticket 014', 'Dunfermline Gaelic Choir', 'Radio Ross relay']) {
      for (const kind of ['personal', 'group', 'machine'] as const) {
        const said = welcome(key({ label, kind }), 'Kettle FM', NOW, Z);
        expect(said.lead).toBe('Welcome.');
        expect(`${said.lead} ${said.rest}`).not.toContain(label.split(' ')[0]);
      }
    }
    expect(GREETING).toBe('Welcome.');
  });

  it('says what the key opens and until when', () => {
    expect(welcome(key(), 'Kettle FM', NOW, Z)).toEqual({ lead: 'Welcome.', rest: 'This key opens Friday Supporters’ Hour and 3 more, until 30 November.' });
    expect(opensWords(key({ shows: key().shows.slice(0, 2) }), 'Kettle FM')).toBe('Friday Supporters’ Hour and The Archive Hour');
    expect(opensWords(key({ shows: key().shows.slice(0, 1) }), 'Kettle FM')).toBe('Friday Supporters’ Hour');
    expect(welcome(key({ all_shows: true, shows: [], valid_until_ms: null, ends_soon: false }), 'Kettle FM', NOW, Z).rest).toBe('This key opens every show Kettle FM keeps for key holders.');
    expect(welcome(key({ media: 'sound' }), 'Kettle FM', NOW, Z).rest).toMatch(/It is for the sound, not the picture\.$/);
    // A key for the picture carries the show's sound: it reads as a key for both does.
    expect(welcome(key({ media: 'video' }), 'Kettle FM', NOW, Z)).toEqual(welcome(key({ media: 'both' }), 'Kettle FM', NOW, Z));
    expect(welcome(key({ media: 'video' }), 'Kettle FM', NOW, Z).rest).not.toMatch(/picture|sound/);
  });

  it('names the year when the end is not this year', () => {
    expect(dayWords(Date.UTC(2027, 1, 13, 12), NOW, Z)).toBe('13 February 2027');
    expect(whenWords(Date.UTC(2026, 11, 3, 19, 0), NOW, Z)).toBe('3 December at 19:00');
  });

  it('has a quiet badge, and says the end only when the station says it is soon', () => {
    expect(BADGE).toBe('Your key is on this device');
    expect(endsNotice(key(), NOW, Z)).toBe('Your key ends on 30 November at 22:00.');
    expect(endsNotice(key({ ends_soon: false }), NOW, Z)).toBeNull();
    expect(endsNotice(key({ valid_until_ms: null }), NOW, Z)).toBeNull();
  });
});

describe('/listen/ opened without its key', () => {
  it('keeps the line for whoever has the link, and offers the box for the key itself', () => {
    const say = entryWords('Kettle FM');
    expect(say.title).toBe('Listen with a key');
    expect(say.link).toBe('Have the link Kettle FM sent you? Open that link, all of it, on this device, and your key comes with it.');
    expect(say.box).toBe('Or type or paste the key itself here; the whole link works too.');
    expect(entryWords('').link).toContain('the link the station sent you');
    for (const line of Object.values(say)) expect(line).not.toMatch(/token|access code/i);
  });

  it('says what to press when the browser will not start the sound after a key is accepted', () => {
    expect(pressToHear('Listen live', 'Friday Supporters’ Hour')).toBe('Your key is accepted. Press Listen live to hear Friday Supporters’ Hour.');
    expect(pressToHear('Listen or watch live', null)).toBe('Your key is accepted. Press Listen or watch live to hear the show.');
  });
});

describe('which show needs a key', () => {
  const list: KeyedShows = { shows: [show(), show({ id: 's-arc', name: 'The Archive Hour', now: 'public' })], get_a_key_text: '', get_a_key_url: '' };
  it('is the one on now by its id, while its keyed window is open', () => {
    expect(keyedNow(list, { show_id: 's-fri', keyed: true })?.id).toBe('s-fri');
    // The same show between its windows, or a public show on now: no key is asked for.
    expect(keyedNow(list, { show_id: 's-fri', keyed: false })).toBeNull();
    expect(keyedNow(list, { show_id: 's-lunch', keyed: false })).toBeNull();
    expect(keyedNow(list, null)).toBeNull();
    // The station's list says which, when the player's facts are older than 1.3.2.
    expect(keyedNow({ ...list, on_now: 's-fri' }, { keyed: true })?.id).toBe('s-fri');
    // A show the list does not hold yet is named by the key's own token for it.
    expect(keyedNow(null, { show_id: 's-new', keyed: true }, { id: 's-new', slug: 'new', name: 'New Show' })?.name).toBe('New Show');
    expect(keyedNow(null, { show_id: 's-new', keyed: true })).toBeNull();
  });

  it('knows whether a key opens it', () => {
    expect(opens(key(), show())).toBe(true);
    expect(opens(key({ shows: [] }), show())).toBe(false);
    expect(opens(key({ all_shows: true, shows: [] }), show({ id: 'anything' }))).toBe(true);
  });

  it('says what everyone else hears, and when it opens to all', () => {
    // One slate in 1.3.2, whatever the rule names: the page does not say which.
    expect(publicHears('Kettle FM')).toBe('Without a key you hear what Kettle FM plays for everyone else until the public programme returns.');
    expect(opensToAll(show({ opens_at_ms: Date.UTC(2026, 10, 28, 19, 0) }), NOW, Z)).toBe('It opens to everyone on 28 November at 19:00.');
    expect(opensToAll(show(), NOW, Z)).toBeNull();
    expect(opensToAll(show({ opens_at_ms: NOW - 1 }), NOW, Z)).toBeNull();
  });
});

describe('the words for a key that does not work', () => {
  const o = { station: 'Kettle FM', show: 'Friday Supporters’ Hour', now: NOW, zone: Z };
  const CODES: RefusalCode[] = ['not_recognised', 'not_yet_valid', 'ended', 'paused', 'revoked', 'does_not_open_show', 'not_scheduled', 'too_many_devices', 'people_limit', 'listens_used_up', 'device_signed_out', 'token_ended', 'token_invalid', 'slow_down', 'not_set_up', 'no_answer'];

  it('never say token or access code, for any code', () => {
    for (const code of CODES) {
      const p = problemWords(refusal(code, NOW, [dev()], 90), o);
      expect(`${p.title} ${p.body}`, code).not.toMatch(/token|access code|passcode|password/i);
      expect(p.title.length, code).toBeGreaterThan(10);
      expect(p.body.length, code).toBeGreaterThan(10);
    }
  });

  it('are specific', () => {
    expect(problemWords(refusal('not_recognised'), o)).toMatchObject({ title: 'That key is not recognised.', act: 'retype', keep: false });
    expect(problemWords(refusal('not_yet_valid', Date.UTC(2026, 11, 3, 19, 0)), o)).toMatchObject({ title: 'This key opens on 3 December at 19:00.', act: 'wait', keep: true });
    expect(problemWords(refusal('ended', Date.UTC(2026, 10, 22, 22, 0)), o)).toMatchObject({ title: 'Your key ended on 22 November at 22:00.', body: 'Ask Kettle FM for a new one.', keep: false });
    expect(problemWords(refusal('revoked'), o).title).toBe('This key was ended by Kettle FM.');
    expect(problemWords(refusal('paused'), o).title).toBe('This key is switched off for now.');
    expect(problemWords(refusal('does_not_open_show'), o).title).toBe('This key does not open Friday Supporters’ Hour.');
    expect(problemWords(refusal('does_not_open_show'), { ...o, show: null }).title).toBe('This key does not open this show.');
    expect(problemWords(refusal('listens_used_up'), o).title).toBe('This key’s listens are used up.');
    expect(problemWords(refusal('people_limit'), o).title).toBe('This key has reached the number of people it is for.');
    expect(problemWords(refusal('device_signed_out'), o)).toMatchObject({ act: 'again', keep: true });
  });

  it('count the devices that hold the key, and offer to sign one out', () => {
    const two = problemWords(refusal('too_many_devices', null, [dev(), dev({ id: 'd2' }), dev({ id: 'd3', ended_ms: NOW })]), o);
    expect(two).toMatchObject({ title: 'This key is already in use on 2 devices.', act: 'devices', keep: true });
    expect(problemWords(refusal('too_many_devices', null, [dev()]), o).title).toBe('This key is already in use on one device.');
    expect(problemWords(refusal('too_many_devices'), o).title).toMatch(/as many devices as it allows/);
  });

  it('say how long to wait', () => {
    expect(problemWords(refusal('slow_down', null, [], 45), o).body).toBe('Wait 45 seconds and try again.');
    expect(problemWords(refusal('slow_down', null, [], 600), o).body).toBe('Wait 10 minutes and try again.');
    expect(problemWords(refusal('slow_down'), o).body).toBe('Wait a few minutes and try again.');
    expect(waitWords(7200)).toBe('2 hours');
  });

  it('do without the station’s name', () => {
    expect(problemWords(refusal('ended'), { station: '', now: NOW }).body).toBe('Ask the station for a new one.');
    expect(problemWords(refusal('paused'), { station: '', now: NOW }).body).toMatch(/^The station has paused it/);
  });
});

describe('the devices', () => {
  it('say which is this one and when each last listened', () => {
    expect(deviceLine(dev(), 'd1', NOW)).toEqual({ name: 'Firefox on Linux', when: 'listening now', here: true, over: false });
    expect(deviceLine(dev({ id: 'd2', label: 'Chrome on Android', last_seen_ms: NOW - 40 * 60_000 }), 'd1', NOW)).toMatchObject({ when: 'last listened 40 minutes ago', here: false });
    expect(deviceLine(dev({ last_seen_ms: NOW - 5 * 3_600_000 }), null, NOW).when).toBe('last listened 5 hours ago');
    expect(deviceLine(dev({ last_seen_ms: NOW - 4 * DAY }), null, NOW).when).toBe('last listened 4 days ago');
    expect(deviceLine(dev({ ended_ms: NOW, label: '' }), null, NOW)).toMatchObject({ name: 'A browser', when: 'signed out', over: true });
    // The station's own hold: a device seen ten minutes ago still holds its place at a station that holds for fifteen.
    expect(deviceLine(dev({ last_seen_ms: NOW - 10 * 60_000 }), null, NOW, 900).when).toBe('listening now');
    expect(deviceLine(dev({ last_seen_ms: NOW - 10 * 60_000 }), null, NOW).when).toBe('last listened 10 minutes ago');
  });
});

describe('a lost key', () => {
  it('is answered the same whether an address has a key or not', () => {
    expect(lostAnswer('Kettle FM')).toMatch(/^If Kettle FM has a key for that address/);
    expect(lostAnswer('')).toMatch(/^If the station has a key/);
  });

  it('says the key is emailed again at a station that sends email, and when the page does not know', () => {
    for (const mail of [true, null, undefined]) {
      const w = lostWords('Kettle FM', mail);
      expect(w.hint).toBe('The one Kettle FM has for you. If a key was made with it, the key’s link is sent there again.');
      expect(w.button).toBe('Send my key again');
      expect(w.answer).toBe(lostAnswer('Kettle FM'));
      expect(w.after).toContain('The link in that email works on any device.');
    }
  });

  it('says staff send it themselves at a station that does not send email, before and after asking', () => {
    const w = lostWords('Kettle FM', false);
    expect(w.hint).toBe('The one Kettle FM has for you. Kettle FM does not send email by itself: if a key was made with that address, its staff see your request and send the key to you again.');
    expect(w.button).toBe('Ask for my key again');
    expect(w.answer).toBe('Asked. If Kettle FM has a key for that address, its staff can see your request and will send the key to you again.');
    expect(w.after).toBe('Nothing is emailed by itself here, so it may take a while: a person at Kettle FM sends it. Not heard from them? Ask Kettle FM; the key may have been made without an email address.');
    // Nothing promises an email that will not come.
    for (const line of Object.values(w)) expect(line).not.toMatch(/on its way|inbox|spam|in that email|sent there/);
  });

  it('never says whether the address has a key, with mail or without', () => {
    for (const mail of [true, false]) expect(lostWords('Kettle FM', mail).answer).toMatch(/If Kettle FM has a key for that address/);
    expect(lostWords('', false).hint).toMatch(/^The one the station has for you\. The station does not send email by itself/);
  });

  it('reads the station’s answer: mail or not, and nothing from a station that sends no body', async () => {
    const answer = (body: string | null) => vi.stubGlobal('fetch', async () => new Response(body, { status: 202, headers: body ? { 'Content-Type': 'application/json' } : {} }));
    answer(JSON.stringify({ message: 'x', mail: false }));
    expect(await routes.lost('isla@example.org')).toEqual({ mail: false });
    answer(JSON.stringify({ message: 'x', mail: true }));
    expect(await routes.lost('isla@example.org')).toEqual({ mail: true });
    // A station older than this answers 202 with nothing: the page then says what it always said.
    answer(null);
    expect(await routes.lost('isla@example.org')).toEqual({ mail: null });
    answer(JSON.stringify({ message: 'x' }));
    expect(await routes.lost('isla@example.org')).toEqual({ mail: null });
    vi.unstubAllGlobals();
  });

  it('asks for something that could be an email address', () => {
    expect(emailProblem('isla@example.org')).toBeNull();
    expect(emailProblem('')).toMatch(/Enter the email address/);
    for (const bad of ['isla', 'isla@', '@example.org', 'isla@example', 'is la@example.org', 'isla@.org']) expect(emailProblem(bad), bad).toBeTruthy();
  });
});

describe('Can’t hear the show?', () => {
  const base: HelpFacts = { keyed: true, problem: null, onAir: true, keyedShow: 'Friday Supporters’ Hour', opensIt: true, playing: false, streamFailed: false, station: 'Kettle FM' };
  const leads = (f: Partial<HelpFacts>) => helpItems({ ...base, ...f }).map((h) => h.lead);

  it('with a good key and its show on, is about the browser', () => {
    expect(leads({})).toEqual(['Your browser may be holding the sound back.', 'Still nothing?']);
    expect(leads({ streamFailed: true })[0]).toBe('The sound did not start.');
  });

  it('says when the key is for another show', () => {
    expect(leads({ opensIt: false })[0]).toBe('Your key is for another show.');
  });

  it('says when the station is not on air yet, or the keyed show is not on', () => {
    expect(leads({ onAir: false, keyedShow: null })[0]).toBe('Kettle FM is not on air yet.');
    expect(leads({ keyedShow: null })[0]).toBe('The keyed show is not on now.');
  });

  it('puts the key’s own problem first, and asks for a key where there is none', () => {
    const p = problemWords(refusal('paused'), { station: 'Kettle FM', now: NOW });
    expect(leads({ keyed: false, problem: p })[0]).toBe('Your key needs attention.');
    expect(leads({ keyed: false })[0]).toBe('Friday Supporters’ Hour needs a key.');
  });

  it('never says token', () => {
    for (const f of [{}, { opensIt: false }, { onAir: false }, { keyed: false }]) {
      for (const h of helpItems({ ...base, ...f })) expect(`${h.lead} ${h.body}`).not.toMatch(/token|access code/i);
    }
    expect(keptWords('Kettle FM')).not.toMatch(/token/i);
  });
});

describe('the key kept on this device', () => {
  it('is read back, and nothing else is', () => {
    // The name an earlier build kept beside the key is staff's own: it is not read back.
    expect(readKept(JSON.stringify({ text: 'KETM-7QMP-2D4X', device: 'd1', label: 'Isla Fraser' }))).toEqual({ text: 'KETM-7QMP-2D4X', device: 'd1' });
    expect(readKept(JSON.stringify({ text: 'KETM-7QMP-2D4X' }))).toEqual({ text: 'KETM-7QMP-2D4X', device: null });
    expect(readKept('not json')).toBeNull();
    expect(readKept(JSON.stringify({ text: '' }))).toBeNull();
    expect(readKept(null)).toBeNull();
    // Never a token: it lives in memory only.
    expect(Object.keys(readKept(JSON.stringify({ text: 'K', token: 'wk1e.x.y' }))!)).toEqual(['text', 'device']);
  });
});

describe('the join page’s keyed line follows the invitation’s show (1.3.2)', () => {
  const fri = show({ name: 'Friday Folk' });
  const sat = show({ id: 's-sat', slug: 'saturday-session', name: 'Saturday Session', opens_at_ms: NOW + 3 * DAY });
  const list = (on_now: string | null = null): KeyedShows => ({ shows: [fri, sat], on_now, get_a_key_text: '', get_a_key_url: '', mail: true });

  it('names the invitation’s own show, before it is on and while another keyed show is', () => {
    // Nothing is on: a guest of Saturday's keyed show is told all the same.
    expect(invitedKeyed({ show: 'Saturday Session', show_id: 's-sat', keyed: true, studio: 'live' }, list())).toBe(sat);
    // Friday's window is open; the invitation is Saturday's.
    expect(invitedKeyed({ show: 'Saturday Session', show_id: 's-sat', keyed: true, studio: 'live' }, list(fri.id))).toBe(sat);
  });

  it('says nothing for a public show’s guest while another show’s keyed window is open', () => {
    expect(invitedKeyed({ show: 'The Morning Kettle', show_id: 's-kettle', keyed: false, studio: 'live' }, list(fri.id))).toBeNull();
    expect(invitedKeyed({ show: 'Kettle FM', show_id: null, keyed: false }, list(fri.id))).toBeNull();
  });

  it('names a keyed show the list does not hold yet by the pass’s own words', () => {
    const made = invitedKeyed({ show: 'Sunday Sessions', show_id: 's-sun', keyed: true }, list());
    expect(made).toMatchObject({ id: 's-sun', name: 'Sunday Sessions', now: 'key', opens_at_ms: null });
    expect(invitedKeyed({ show: 'Sunday Sessions', show_id: 's-sun', keyed: true }, null)?.name).toBe('Sunday Sessions');
  });

  it('follows a rehearsal link’s rehearsed show too', () => {
    expect(invitedKeyed({ show: 'Saturday Session', show_id: 's-sat', keyed: true, studio: 'rehearsal' }, list())).toBe(sat);
  });

  it('reads a station that does not say as before: a live link’s show is the keyed show on now', () => {
    expect(invitedKeyed({ show: 'Friday Folk', studio: 'live' }, list(fri.id))).toBe(fri);
    expect(invitedKeyed({ show: 'Friday Folk' }, list(fri.id))).toBe(fri);
    expect(invitedKeyed({ show: 'Friday Folk', studio: 'live' }, list())).toBeNull();
    expect(invitedKeyed({ show: 'Friday Folk', studio: 'rehearsal' }, list(fri.id))).toBeNull();
    expect(invitedKeyed(null, list(fri.id))).toBeNull();
  });

  it('says who will hear the guest, that they need no key, and when it opens to everyone', () => {
    expect(invitedWords(fri, { now: NOW, zone: Z })).toBe('Only listeners who were given a key hear Friday Folk; everyone else hears what the station plays meanwhile. You are joining the studio, so you need no key.');
    expect(invitedWords(sat, { now: NOW, zone: Z })).toMatch(/You are joining the studio, so you need no key\. It opens to everyone on /);
  });

  it('speaks of when the show airs on a rehearsal link, where nobody is listening', () => {
    const w = invitedWords(fri, { rehearsal: true, now: NOW, zone: Z });
    expect(w).toBe('When Friday Folk airs, only listeners who were given a key hear it; everyone else hears what the station plays meanwhile. You need no key to rehearse, or to join the show.');
    expect(w).not.toContain('You are joining the studio');
  });

  it('the pass’s facts are named as the station names them', () => {
    const spec = openapi as unknown as { components: { schemas: Record<string, { properties?: Record<string, unknown> }> } };
    expect(Object.keys(spec.components.schemas.PassInfo.properties ?? {})).toEqual(expect.arrayContaining(['show', 'show_id', 'keyed']));
  });
});
