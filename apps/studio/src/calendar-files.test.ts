// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Calendar files in the studio (ADR-0281), tested without a page: the links Subscribe… gives
// out and the address they are made from, who is offered what, the import preview's reply made
// whole and drawn in New show's words, the ticks and names and the request they send, every
// refusal as a sentence under the right row, and the dialogs rendered on the server.
//
// The two replies are the control plane's own, recorded from a real one on 2026-11-15 reading
// `tests/newshow/kettle-old-schedule.ics` into a station that had The Late Kettle (Tuesdays,
// 21:00) and Kettle Drive (weekdays, 16:00): `tests/newshow/import-preview.json` and
// `import-made.json`. So the field names here are serde's, not this file's guess. The preview
// was recorded again that night with the file's last event, the old system's own *Kettle drive*
// at the same times: the show the station already has (`already`). Sample shows
// and people are Kettle FM's (ADR-0071).
import { render } from 'svelte/server';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { CALENDAR, panel } from '../../../packages/ui/panels.ts';
import previewJson from '../../../tests/newshow/import-preview.json';
import madeJson from '../../../tests/newshow/import-made.json';
import { FieldError, type Me } from './lib/api.ts';
import { account } from './lib/account.svelte.ts';
import { mayImportCalendar } from './lib/account.ts';
import { sched } from './lib/schedule.svelte.ts';
import { asPublicUrl } from './lib/schedule.ts';
import Schedule from './schedule/Schedule.svelte';
import ShowEditor from './schedule/ShowEditor.svelte';
import ImportCalendar from './schedule/files/ImportCalendar.svelte';
import Subscribe from './schedule/files/Subscribe.svelte';
import {
  afterWords,
  allChoices,
  allThere,
  alreadyNote,
  alreadyWords,
  asImportMade,
  asImportPreview,
  chosen,
  clashingTicked,
  decodeCalendar,
  droppedFile,
  fileProblem,
  fileWords,
  firstChoices,
  importRequest,
  madeWords,
  makeLabel,
  mappingWords,
  nameProblems,
  nothingTicked,
  overlapWords,
  pathName,
  placeRefusal,
  refusalWords,
  sameNames,
  sharedClash,
  warningWords,
} from './schedule/files/import.ts';
import { calendarLinks, calendarPath, fileName, isCalendar, publicAddress } from './schedule/files/links.ts';
import { calendarText, importPreview, importShows } from './schedule/files/station.ts';
import { hhmm } from './schedule/rule/form.ts';
import { rows } from './schedule/rule/view.ts';

const clean = (body: string) => body.replace(/<!--.*?-->/g, '');
const ZONE = 'Europe/London';
const TODAY = '2026-11-15';
const me = (role: Me['role'], kind: Me['kind'] = 'staff'): Me => ({ kind, role });
const preview = () => asImportPreview(previewJson);
const byName = (name: string) => preview().items.find((i) => i.name === name)!;

afterEach(() => {
  sched.publicUrl = undefined;
  sched.rules = null;
  account.me = null;
  account.loaded = false;
  vi.unstubAllGlobals();
});

describe('the links Subscribe… gives out', () => {
  it('are made from the station’s public address: webcal for a calendar app, https as a web address', () => {
    expect(calendarLinks('https://kettle.example')).toEqual({
      web: 'https://kettle.example/api/v1/public/schedule.ics',
      webcal: 'webcal://kettle.example/api/v1/public/schedule.ics',
    });
    expect(calendarLinks('https://kettle.example/', 'the-late-kettle')).toEqual({
      web: 'https://kettle.example/api/v1/public/shows/the-late-kettle.ics',
      webcal: 'webcal://kettle.example/api/v1/public/shows/the-late-kettle.ics',
    });
    // A test bench on plain http, with a port.
    expect(calendarLinks('http://127.0.0.1:8080')?.webcal).toBe('webcal://127.0.0.1:8080/api/v1/public/schedule.ics');
  });

  it('are not made at all without one: never from the address the studio talks to', () => {
    for (const none of [null, undefined, '', '   ', 'kettle.example', 'ftp://kettle.example', '/api', 'javascript:alert(1)']) {
      expect(calendarLinks(none), String(none)).toBeNull();
    }
    expect(publicAddress('https://kettle.example///')).toBe('https://kettle.example');
  });

  it('name the same route and file the station serves', () => {
    expect(calendarPath()).toBe('/api/v1/public/schedule.ics');
    expect(calendarPath('folk-night')).toBe('/api/v1/public/shows/folk-night.ics');
    expect(fileName()).toBe('schedule.ics');
    expect(fileName('folk-night')).toBe('folk-night.ics');
  });

  it('the week’s reply says the address, that there is none, or nothing (a station before 1.0.5)', () => {
    expect(asPublicUrl({ public_url: 'https://kettle.example' })).toBe('https://kettle.example');
    expect(asPublicUrl({ public_url: null })).toBeNull();
    expect(asPublicUrl({ public_url: '' })).toBeNull();
    expect(asPublicUrl({})).toBeUndefined();
  });

  it('only a calendar is saved as one', () => {
    expect(isCalendar('BEGIN:VCALENDAR\r\nVERSION:2.0\r\n')).toBe(true);
    expect(isCalendar('﻿begin:vcalendar\r\n')).toBe(true);
    expect(isCalendar('<!doctype html><title>Sign in</title>')).toBe(false);
    expect(isCalendar('')).toBe(false);
  });
});

describe('Subscribe…', () => {
  const dialog = (props: Parameters<typeof render<typeof Subscribe>>[1]['props']) => clean(render(Subscribe, { props }).body);

  it('shows the schedule’s link both ways, Copy, and Download .ics, under its panel name', () => {
    const body = dialog({ publicUrl: 'https://kettle.example', onClose: () => {} });
    expect(body).toContain('data-panel="calendar.subscribe"');
    expect(panel('calendar.subscribe').what).toContain('Download .ics');
    expect(body).toContain('Subscribe to the schedule');
    expect(body).toContain('value="webcal://kettle.example/api/v1/public/schedule.ics"');
    expect(body).toContain('value="https://kettle.example/api/v1/public/schedule.ics"');
    expect(body).toContain('>Copy link</button>');
    expect(body).toContain('>Download .ics</button>');
    expect(body).toContain('no episode titles and no guests');
  });

  it('for one show, links that show’s file and says when it has no dates', () => {
    const body = dialog({ show: { slug: 'the-late-kettle', name: 'The Late Kettle', hasDates: true }, publicUrl: 'https://kettle.example', onClose: () => {} });
    expect(body).toContain('Subscribe to The Late Kettle');
    expect(body).toContain('value="webcal://kettle.example/api/v1/public/shows/the-late-kettle.ics"');
    expect(body).not.toContain('has no dates yet');
    const empty = dialog({ show: { slug: 'record-fair', name: 'Record Fair', hasDates: false }, publicUrl: 'https://kettle.example', onClose: () => {} });
    expect(empty).toContain('Record Fair has no dates yet');
  });

  it('with no public address shows no link, says who sets one, and still offers the file', () => {
    const body = dialog({ publicUrl: null, onClose: () => {} });
    expect(body).toContain('This station has no public address, so there is no link to give out.');
    expect(body).toContain('publishing.public_url');
    expect(body).not.toContain('webcal://');
    expect(body).not.toContain('>Copy link</button>');
    expect(body).toContain('>Download .ics</button>');
  });
});

describe('who is offered what', () => {
  it('an admin, a producer and the station key import; a presenter and a screener do not', () => {
    expect(mayImportCalendar(me('admin'))).toBe(true);
    expect(mayImportCalendar(me('producer'))).toBe(true);
    expect(mayImportCalendar(me('admin', 'station'))).toBe(true);
    expect(mayImportCalendar(me('presenter'))).toBe(false);
    expect(mayImportCalendar(me('screener'))).toBe(false);
    expect(mayImportCalendar(null)).toBe(false);
  });

  const toolbar = () => {
    sched.zone = ZONE;
    const body = clean(render(Schedule, { props: { live: false } }).body);
    return body.slice(body.indexOf('data-panel="calendar.toolbar"'), body.indexOf('class="legend"'));
  };

  it('the toolbar offers both to a producer, and Subscribe… alone to a presenter', () => {
    sched.publicUrl = 'https://kettle.example';
    account.loaded = true;
    account.me = me('producer');
    expect(toolbar()).toContain('>Import a calendar…</button>');
    expect(toolbar()).toContain('>Subscribe…</button>');
    account.me = me('presenter');
    expect(toolbar()).not.toContain('Import a calendar…');
    expect(toolbar()).toContain('>Subscribe…</button>');
    // In a narrower window the same two are one menu: an entry each, and Subscribe… alone for a presenter.
    expect(toolbar()).toMatch(/aria-haspopup="menu"[^>]*aria-label="Calendar files"[^>]*title="Calendar files: Subscribe…"/);
    expect(toolbar().match(/role="menuitem"/g)).toHaveLength(1);
    account.me = me('producer');
    expect(toolbar()).toContain('title="Calendar files: Import a calendar…, Subscribe…"');
    expect(toolbar().match(/role="menuitem"/g)).toHaveLength(2);
    // Nobody is offered the import before the station has said who this window is.
    account.me = me('admin');
    account.loaded = false;
    expect(toolbar()).not.toContain('Import a calendar…');
  });

  it('a station with no public address still has both; one before 1.0.5 has neither', () => {
    account.loaded = true;
    account.me = me('admin');
    sched.publicUrl = null;
    expect(toolbar()).toContain('>Import a calendar…</button>');
    expect(toolbar()).toContain('>Subscribe…</button>');
    sched.publicUrl = undefined;
    expect(toolbar()).not.toContain('Import a calendar…');
    expect(toolbar()).not.toContain('Subscribe…');
  });

  it('the show editor offers Subscribe… beside When it airs, on a station that has calendar files', () => {
    const SHOW = { id: 'i', slug: 'folk-club', name: 'The Folk Club', description: '', colour_slot: 2, layout: 'poster' as const, slot: null, hosts: [], tags: [], explicit: false, feed_guid: null, cards: null, presenter: '', contact: '', links: [], commercial_music: false, presenter_account: null };
    const editor = () => clean(render(ShowEditor, { props: { slug: SHOW.slug, show: SHOW as never, onSaved: () => {}, onCard: () => {} } }).body);
    sched.publicUrl = 'https://kettle.example';
    const body = editor();
    const rule = body.slice(body.indexOf('data-panel="calendar.show-rule"'));
    expect(rule.slice(0, rule.indexOf('Hosts'))).toContain('>Subscribe…</button>');
    sched.publicUrl = undefined;
    expect(editor()).not.toContain('Subscribe…');
  });

  it('opens each dialog over the Schedule tab, with only registered panel names', () => {
    sched.zone = ZONE;
    sched.publicUrl = 'https://kettle.example';
    for (const [dialog, name] of [['subscribe', 'calendar.subscribe'], ['import', 'calendar.import']] as const) {
      const body = clean(render(Schedule, { props: { live: false, dialog } }).body);
      expect(body).toContain(`data-panel="${name}"`);
      for (const m of body.matchAll(/data-panel="([^"]+)"/g)) expect(() => panel(m[1])).not.toThrow();
    }
    expect(CALENDAR.map((p) => p.name)).toEqual(expect.arrayContaining(['calendar.subscribe', 'calendar.import']));
    expect(panel('calendar.toolbar').what).toContain('Import a calendar…');
  });
});

describe('the import preview, as the control plane sent it', () => {
  it('is made whole: the file, each show with how it was read, its rule and its words', () => {
    const p = preview();
    expect(p.zone).toBe(ZONE);
    expect(p.events).toBe(10);
    expect(p.notes).toEqual(['1 cancelled event was left out.']);
    expect(p.ready).toBe(6);
    expect(p.items.map((i) => [i.name, i.mapping])).toEqual([
      ['Folk Night', 'exact'],
      ['The Kettle Book Club', 'exact'],
      ['Every Other Morning', 'dates'],
      ['Letter from New York', 'dates'],
      ['Outside Broadcast', 'exact'],
      ['Record Fair (all day)', 'refused'],
      ['Kettle drive', 'exact'],
    ]);
    expect(fileWords(p)).toBe('10 events read · 7 shows proposed, 6 can be made');
    // Only the last is a show the station has: said by the station, with its path and its name.
    expect(p.items.map((i) => i.already)).toEqual([null, null, null, null, null, null, { slug: 'kettle-drive', name: 'Kettle Drive' }]);
    expect(byName('Kettle drive').clashCount).toBe(0);
    expect(byName('Kettle drive').slug).toBe('kettle-drive-2');
    const folk = byName('Folk Night');
    expect(folk.index).toBe(0);
    expect(folk.slug).toBe('folk-night');
    expect(folk.rule?.repeat).toEqual({ kind: 'weekly', every: 1, weekdays: [2] });
    expect(folk.words?.sentence).toBe('Every week on Tuesday, 20:00 to 22:00, from 17 November. Not on 29 December.');
    expect(folk.clashCount).toBe(4);
    const club = byName('The Kettle Book Club');
    expect(club.words?.last).toBe('The last is Thursday 6 May 2027.');
    const fair = byName('Record Fair (all day)');
    expect(fair.rule).toBeNull();
    expect(fair.words).toBeNull();
    expect(fair.reason).toBe('It is an all-day event: a show needs a time to start.');
    const ny = byName('Letter from New York');
    expect(ny.zone).toBe('America/New_York');
    expect(ny.clockDiffers).toBe(true);
    expect(byName('Outside Broadcast').events).toBe(3);
  });

  it('draws each show’s next dates and clashes with New show’s own rows', () => {
    const folk = byName('Folk Night');
    const r = rows(folk.preview, ZONE, TODAY, hhmm(folk.rule!.start));
    expect(r.map((x) => [x.day, x.times])).toEqual([
      ['Tue 17 Nov', '20:00–22:00'],
      ['Tue 24 Nov', '20:00–22:00'],
      ['Tue 1 Dec', '20:00–22:00'],
      ['Tue 8 Dec', '20:00–22:00'],
    ]);
    expect(r[0].clash).toBe('The Late Kettle, 21:00 to 23:00 · overlaps 1 hour');
    expect(afterWords(folk)).toBe('and so on, with no end');
    // A date at another time is a moved date, said as New show says it.
    const ob = byName('Outside Broadcast');
    const moved = rows(ob.preview, ZONE, TODAY, hhmm(ob.rule!.start));
    expect(moved.map((x) => x.day)).toEqual(['Sat 28 Nov', 'Sat 19 Dec', 'Sat 30 Jan 2027']);
    expect(moved[2].times).toBe('15:00–17:00');
    expect(moved[2].moved).toBe('was Sat 30 Jan');
    expect(afterWords(ob)).toBe('Those are all 3.');
    expect(afterWords(byName('The Kettle Book Club'))).toBe('and more after these');
    expect(afterWords(byName('Record Fair (all day)'))).toBe('');
  });

  it('a clash every date shares is told from one that only some have', () => {
    const folk = byName('Folk Night');
    expect(sharedClash(folk.preview)).toBe(true);
    expect(sharedClash(byName('The Kettle Book Club').preview)).toBe(false);
    // One date clear, or one date against another show: each row then says its own.
    const some = { dates: folk.preview.dates.map((d, i) => (i === 1 ? { ...d, clashes: [] } : d)) };
    expect(sharedClash(some)).toBe(false);
    const other = { dates: folk.preview.dates.map((d, i) => (i === 2 ? { ...d, clashes: [{ ...d.clashes[0], show: 'Kettle Drive', slug: 'kettle-drive' }] } : d)) };
    expect(sharedClash(other)).toBe(false);
    expect(sharedClash({ dates: folk.preview.dates.slice(0, 1) })).toBe(false);
  });

  it('marks each: exact, chosen dates, or not a show', () => {
    expect(mappingWords({ mapping: 'exact' }).label).toBe('Exact');
    expect(mappingWords({ mapping: 'dates' }).label).toBe('Chosen dates');
    expect(mappingWords({ mapping: 'refused' }).label).toBe('Not a show');
  });

  it('reads a reply with parts left out as empty, never undefined', () => {
    const p = asImportPreview({ items: [{ name: 'Bare', mapping: 'exact' }] });
    expect(p.items[0]).toMatchObject({ mapping: 'refused', rule: null, notes: [], overlaps: [], clashCount: 0 });
    expect(asImportPreview(null)).toMatchObject({ zone: 'UTC', events: 0, items: [], ready: 0 });
  });
});

describe('ticks, names and the request', () => {
  it('everything that can be made starts ticked, under the calendar’s name', () => {
    const p = preview();
    const c = firstChoices(p);
    // The refused one cannot be ticked; the one the station already has starts unticked.
    expect(c.map((x) => x.on)).toEqual([true, true, true, true, true, false, false]);
    expect(c[0].name).toBe('Folk Night');
    expect(chosen(p, c)).toHaveLength(5);
    expect(makeLabel(5)).toBe('Make 5 shows');
    expect(makeLabel(1)).toBe('Make the show');
  });

  it('a show the station already has starts unticked, is no clash, and is said once for the file', () => {
    const p = preview();
    const c = firstChoices(p);
    const drive = byName('Kettle drive');
    expect(alreadyNote(p)).toBe('One of these shows is already on the station, with the same dates and times. It starts unticked, so it is not made twice.');
    expect(alreadyWords(drive, c[6])).toBe('It is there as Kettle Drive, with the same dates and times. Left unticked, so it is not made twice.');
    expect(alreadyWords(drive, { on: false, name: 'Kettle Drive' })).toBe('It is there, with the same dates and times. Left unticked, so it is not made twice.');
    expect(alreadyWords(byName('Folk Night'), c[0])).toBe('');
    // The file's one warning counts Folk Night alone: the show already there is not a clash.
    expect(clashingTicked(p, c)).toBe(1);
    // Tick all ticks what is new and leaves it be; ticked by hand it stays ticked, and counts.
    expect(allChoices(p, allChoices(p, c, false), true).map((x) => x.on)).toEqual([true, true, true, true, true, false, false]);
    const twice = c.map((x, i) => (i === 6 ? { ...x, on: true } : x));
    expect(allChoices(p, twice, true)[6].on).toBe(true);
    expect(allChoices(p, twice, false).some((x) => x.on)).toBe(false);
    expect(alreadyWords(drive, twice[6])).toBe('It is there as Kettle Drive, with the same dates and times. Ticked, a second show is made, and the two clash on every date.');
    expect(clashingTicked(p, twice)).toBe(2);
    expect(chosen(p, twice).map((x) => x.name)).toContain('Kettle drive');
    // A reply from a station before this (no `already`) reads as none.
    expect(asImportPreview({ items: [{ name: 'Folk Night', mapping: 'exact', rule: drive.rule }] }).items[0].already).toBeNull();
  });

  it('a station’s own calendar read back has nothing to make, and says so', () => {
    // The test station's case: five shows, every one already there.
    const p = preview();
    const drive = p.items[6];
    const names = ['Kettle Breakfast', 'Kettle Drive', 'The Bothy Sessions', 'Budget Night Special', 'Saturday Night Kettle'];
    const own = { ...p, events: 5, notes: [], ready: 5, items: names.map((name, index) => ({ ...drive, index, name, already: { slug: `show-${index}`, name } })) };
    const c = firstChoices(own);
    expect(c.some((x) => x.on)).toBe(false);
    expect(allThere(own)).toBe(true);
    expect(allThere(p)).toBe(false);
    expect(alreadyNote(own)).toBe('All 5 shows in this file are already on the station, with the same dates and times. There is nothing to make.');
    expect(nothingTicked(own)).toBe('Every show in this file is already on the station.');
    expect(nothingTicked(p)).toBe('Tick at least one show.');
    expect(clashingTicked(own, c)).toBe(0);
    expect(allChoices(own, c, true).some((x) => x.on)).toBe(false);
    expect(alreadyNote({ ...own, ready: 1, items: [own.items[0]] })).toBe('The show in this file is already on the station, with the same dates and times. There is nothing to make.');
    expect(alreadyNote({ ...p, items: p.items.slice(0, 6), ready: 5 })).toBeNull();
  });

  it('sends the ticked shows only, renamed, each as a name, a description and the rule as it came', () => {
    const p = preview();
    const c = firstChoices(p);
    c[1].on = false;
    c[2].on = false;
    c[3].on = false;
    c[0].name = '  The Folk Hour ';
    // A refused item is never sent, ticked or not.
    c[5].on = true;
    const picked = chosen(p, c);
    expect(picked.map((x) => [x.index, x.name])).toEqual([
      [0, 'The Folk Hour'],
      [4, 'Outside Broadcast'],
    ]);
    const body = importRequest(picked);
    expect(body.items).toHaveLength(2);
    // The station refuses a field it does not know (`deny_unknown_fields`).
    for (const item of body.items) expect(Object.keys(item).sort()).toEqual(['description', 'name', 'rule']);
    expect(body.items[0].description).toBe('Two hours of folk from Fife and further, with a session in the second hour.');
    expect(body.items[0].rule).toEqual(previewJson.items[0].rule);
    expect(body.items[1].rule).toEqual(previewJson.items[4].rule);
  });

  it('holds the import for a name that will not do, on ticked rows only', () => {
    const p = preview();
    const c = firstChoices(p);
    expect(nameProblems(p, c).size).toBe(0);
    c[0].name = '   ';
    c[1].name = 'x'.repeat(161);
    c[2].name = '…';
    const problems = nameProblems(p, c);
    expect(problems.get(0)).toBe('Give the show a name.');
    expect(problems.get(1)).toBe('At most 160 characters.');
    expect(problems.get(2)).toBe('The name needs at least one letter or digit.');
    expect(problems.has(3)).toBe(false);
    c[0].on = false;
    expect([...nameProblems(p, c).keys()]).toEqual([1, 2]);
  });

  it('two ticked shows of one name are both made, and the second says so', () => {
    // A calendar may hold a weekday Breakfast and a weekend one: the station tells their paths apart.
    const p = preview();
    const c = firstChoices(p);
    expect(sameNames(p, c).size).toBe(0);
    c[4].name = 'letter from new york';
    expect(nameProblems(p, c).size).toBe(0);
    const twins = sameNames(p, c);
    expect([...twins.keys()]).toEqual([4]);
    expect(twins.get(4)).toBe('Another ticked show is named letter from new york too. Both are made; their paths are told apart by a number. Rename one to tell them apart in the lists.');
    expect(chosen(p, c)).toHaveLength(5);
    c[3].on = false;
    expect(sameNames(p, c).size).toBe(0);
  });

  it('says which other ticked shows of the file a show overlaps, under the names they have now', () => {
    const p = preview();
    p.items[0].overlaps = [1, 4, 5];
    const c = firstChoices(p);
    c[4].name = 'The OB';
    expect(overlapWords(p.items[0], p, c)).toBe('Overlaps The Kettle Book Club and The OB in this file.');
    c[1].on = false;
    expect(overlapWords(p.items[0], p, c)).toBe('Overlaps The OB in this file.');
    c[4].on = false;
    expect(overlapWords(p.items[0], p, c)).toBeNull();
  });
});

describe('a refusal, as a sentence', () => {
  it('a file too large is said before it is sent', () => {
    expect(fileProblem({ name: 'old.ics', size: 1024 * 1024 + 1 })).toBe('old.ics is larger than 1 MB. Export a shorter range of dates, or one calendar at a time.');
    expect(fileProblem({ name: 'old.ics', size: 0 })).toBe('old.ics is empty.');
    expect(fileProblem({ name: 'old.ics', size: 1024 * 1024 })).toBe('');
  });

  it('a file that is not UTF-8 is refused before it is sent, not read with spoilt letters', () => {
    const utf8 = new TextEncoder().encode('BEGIN:VCALENDAR\r\nSUMMARY:Cèilidh na h-Oidhche\r\nEND:VCALENDAR\r\n');
    expect(decodeCalendar(utf8)).toContain('SUMMARY:Cèilidh na h-Oidhche');
    expect(decodeCalendar(utf8.buffer as ArrayBuffer)).toContain('Cèilidh');
    // The same line as Latin-1: `è` is the single byte E8, which is not UTF-8.
    const latin1 = Uint8Array.from('SUMMARY:C\u00e8ilidh', (ch) => ch.charCodeAt(0));
    expect(() => decodeCalendar(latin1)).toThrow('The file is not text in UTF-8, so it is not a calendar file we can read.');
    expect(refusalWords(new Error('The file is not text in UTF-8, so it is not a calendar file we can read.'))).toBe('The file is not text in UTF-8, so it is not a calendar file we can read.');
  });

  it('the station’s own sentence is shown as given: over 200 events, not a calendar', () => {
    expect(refusalWords(new FieldError(413, 'The file holds 201 events; at most 200 can be read at once. Export a shorter range of dates.', 'ics'))).toBe(
      'The file holds 201 events; at most 200 can be read at once. Export a shorter range of dates.',
    );
    expect(refusalWords(new FieldError(422, 'This is not a calendar file: it does not begin with BEGIN:VCALENDAR.', 'ics'))).toBe('This is not a calendar file: it does not begin with BEGIN:VCALENDAR.');
  });

  it('a bare status gets a plain sentence: a body limit’s 413, a 422, a presenter’s 403, an older station', () => {
    expect(refusalWords(new FieldError(413, '413 Payload Too Large', null))).toContain('up to 1 MB and hold up to 200 events');
    expect(refusalWords(new FieldError(413, '413', null))).toContain('too large');
    expect(refusalWords(new FieldError(422, '422 Unprocessable Entity', null))).toBe('That is not a calendar file the station can read. Export it again as iCalendar (.ics).');
    expect(refusalWords(new FieldError(403, 'Only an admin or a producer imports a calendar.', null))).toBe('Only an admin or a producer imports a calendar.');
    expect(refusalWords(new FieldError(404, '404 Not Found', null))).toBe('This station cannot read calendar files yet: it is older than 1.0.5.');
    expect(refusalWords(new TypeError('Failed to fetch'))).toBe('The station did not answer. Nothing was read; try again.');
    expect(refusalWords('The station did not answer.')).toBe('The station did not answer. Nothing was read; try again.');
  });

  it('a show the station refuses is found by its place among the ticked shows, not among the rows', () => {
    const p = preview();
    const c = firstChoices(p);
    c[0].on = false;
    c[1].on = false;
    const sent = chosen(p, c);
    expect(sent.map((x) => x.index)).toEqual([2, 3, 4]);
    const name = placeRefusal(new FieldError(422, '“Letter from New York”: the name does not fit the card at its smallest size.', 'items[1].name'), sent);
    expect(name).toEqual({ index: 3, part: 'name', message: '“Letter from New York”: the name does not fit the card at its smallest size.' });
    expect(placeRefusal(new FieldError(422, '“Outside Broadcast”: a rule needs a date.', 'items[2].rule'), sent)).toMatchObject({ index: 4, part: 'rule' });
    // A field that names no row, or a row that was not sent, is the form's.
    expect(placeRefusal(new FieldError(422, 'Choose 1 to 200 shows to make.', 'items'), sent)).toMatchObject({ index: null, part: null });
    expect(placeRefusal(new FieldError(422, 'x', 'items[7].name'), sent)).toMatchObject({ index: null });
    expect(placeRefusal(new FieldError(403, 'Only an admin or a producer imports a calendar.', null), sent)).toEqual({ index: null, part: null, message: 'Only an admin or a producer imports a calendar.' });
  });
});

describe('the requests', () => {
  const answer = (status: number, body: unknown, type = 'application/json') => new Response(typeof body === 'string' ? body : JSON.stringify(body), { status, headers: { 'content-type': type } });

  it('the preview is a POST of the file as it is, as JSON, and writes nothing', async () => {
    const calls: { url: string; init?: RequestInit }[] = [];
    vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
      calls.push({ url: String(url), init });
      return answer(200, previewJson);
    });
    const ics = 'BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n';
    const p = await importPreview(ics);
    expect(p.items).toHaveLength(7);
    expect(calls).toHaveLength(1);
    expect(calls[0].url).toMatch(/\/api\/v1\/schedule\/import\/preview$/);
    expect(calls[0].init?.method).toBe('POST');
    expect(new Headers(calls[0].init?.headers).get('content-type')).toBe('application/json');
    expect(JSON.parse(String(calls[0].init?.body))).toEqual({ ics, limit: 4 });
  });

  it('a 413 with no sentence (the body limit’s plain text) keeps its status', async () => {
    vi.stubGlobal('fetch', async () => answer(413, 'Failed to buffer the request body: length limit exceeded', 'text/plain'));
    const err = await importPreview('x').catch((e: unknown) => e);
    expect(err).toBeInstanceOf(FieldError);
    expect((err as FieldError).status).toBe(413);
    expect(refusalWords(err)).toContain('That file is too large.');
  });

  it('a 422 carries the station’s sentence and the field', async () => {
    vi.stubGlobal('fetch', async () => answer(422, { error: 'The file holds no events.', field: 'ics' }));
    const err = (await importPreview('x').catch((e: unknown) => e)) as FieldError;
    expect(err.status).toBe(422);
    expect(err.field).toBe('ics');
    expect(refusalWords(err)).toBe('The file holds no events.');
  });

  it('Make the shows is one POST of the chosen items, and the reply is the shows and the warnings', async () => {
    const calls: { url: string; init?: RequestInit }[] = [];
    vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
      calls.push({ url: String(url), init });
      return answer(201, madeJson);
    });
    const p = preview();
    const c = firstChoices(p).map((x, i) => ({ ...x, on: i === 0 || i === 4 }));
    const made = await importShows(chosen(p, c));
    expect(calls).toHaveLength(1);
    expect(calls[0].url).toMatch(/\/api\/v1\/schedule\/import$/);
    expect(JSON.parse(String(calls[0].init?.body)).items.map((i: { name: string }) => i.name)).toEqual(['Folk Night', 'Outside Broadcast']);
    expect(made.made.map((s) => [s.slug, s.rule_words?.short, s.next_on?.on])).toEqual([
      ['folk-night', 'Tue, 20:00–22:00', '2026-11-17'],
      ['outside-broadcast', expect.any(String), '2026-11-28'],
    ]);
    expect(made.warnings).toHaveLength(1);
    expect(made.warnings[0]).toMatchObject({ show: 'folk-night', name: 'Folk Night', dates: 52 });
    expect(warningWords(made.warnings[0])).toBe('Folk Night overlaps The Late Kettle on 52 dates in its first twelve months.');
    expect(madeWords(2)).toBe('2 shows are made.');
    expect(madeWords(1)).toBe('1 show is made.');
  });

  it('the file to download is read from the studio’s own address, and only a calendar is accepted', async () => {
    const urls: string[] = [];
    vi.stubGlobal('fetch', async (url: string) => {
      urls.push(String(url));
      return answer(200, 'BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n', 'text/calendar; charset=utf-8');
    });
    expect(await calendarText('folk-night')).toContain('BEGIN:VCALENDAR');
    expect(urls[0]).toMatch(/^\/api\/v1\/public\/shows\/folk-night\.ics$/);
    // A login page in its place is not saved as a calendar.
    vi.stubGlobal('fetch', async () => answer(200, '<!doctype html><title>Sign in</title>', 'text/html'));
    await expect(calendarText()).rejects.toThrow('The station did not answer with a calendar file.');
    vi.stubGlobal('fetch', async () => answer(404, { error: 'not found' }));
    await expect(calendarText()).rejects.toMatchObject({ status: 404 });
  });
});

describe('Import a calendar…', () => {
  const dialog = (props: Partial<Parameters<typeof render<typeof ImportCalendar>>[1]['props']>) =>
    clean(render(ImportCalendar, { props: { today: TODAY, onMade: () => {}, onOpen: () => {}, onClose: () => {}, ...props } }).body);

  it('part 1 asks for a file and says the limits', () => {
    const body = dialog({});
    expect(body).toContain('data-panel="calendar.import"');
    expect(body).toContain('data-part="1"');
    expect(body).toContain('>Choose a file…</button>');
    expect(body).toContain('Up to 1 MB and 200 events.');
    expect(body).toMatch(/<input[^>]*type="file"[^>]*accept="\.ics,\.ical,text\/calendar"/);
    expect(body).not.toContain('Make the show');
    // A file may be dropped; a page cannot read a path, so a browser build has no field for one.
    expect(body).toContain('Or drop the file anywhere on this dialog.');
    expect(body).not.toContain('id="ic-path"');
    expect(body).not.toContain('paste the file');
  });

  it('what is dropped is the first file named as a calendar, and anything else is said', () => {
    const name = (f: string) => f;
    expect(droppedFile(['/home/iona/Downloads/schedule (2).ics'], name)).toEqual({ file: '/home/iona/Downloads/schedule (2).ics' });
    expect(droppedFile(['C:\\Users\\Iona\\notes.txt', 'C:\\Users\\Iona\\OLD.ICS'], name)).toEqual({ file: 'C:\\Users\\Iona\\OLD.ICS' });
    expect(droppedFile([{ name: 'old.ical' }], (f) => f.name)).toEqual({ file: { name: 'old.ical' } });
    expect(droppedFile(['/home/iona/notes.txt'], name)).toEqual({ problem: 'notes.txt is not a calendar file: its name does not end in .ics.' });
    expect(droppedFile(['/home/iona/a.txt', '/home/iona/b.pdf'], name)).toEqual({ problem: 'None of those is a calendar file: a calendar file’s name ends in .ics.' });
    expect(droppedFile([], name)).toEqual({ problem: 'Nothing that was dropped is a file.' });
    expect(pathName('/home/iona/Downloads/schedule (2).ics')).toBe('schedule (2).ics');
    expect(pathName('C:\\Users\\Iona\\schedule.ics')).toBe('schedule.ics');
    expect(pathName('schedule.ics')).toBe('schedule.ics');
  });

  it('part 2 lists each proposed show with its sentence, dates and clashes, ticked and renameable', () => {
    const body = dialog({ file: 'kettle-old-schedule.ics', preview: preview() });
    expect(body).toContain('data-part="2"');
    expect(body).toContain('kettle-old-schedule.ics');
    expect(body).toContain('10 events read · 7 shows proposed, 6 can be made.');
    expect(body).toContain('1 cancelled event was left out.');
    expect(body).toContain('5 of 6 ticked');
    // Each show said back in New show's words.
    expect(body).toContain('Every week on Tuesday, 20:00 to 22:00, from 17 November. Not on 29 December.');
    expect(body).toContain('Every month on the first Thursday, 19:00 to 20:00, from 3 December, for 6 episodes.');
    expect(body).toContain('The last is Thursday 6 May 2027.');
    expect(body).toContain('Tue 17 Nov');
    expect(body).toContain('The Late Kettle, 21:00 to 23:00 · overlaps 1 hour');
    // A clash every date shares is said once, in the sentence New show's preview uses.
    expect(body).toContain('Every date overlaps The Late Kettle (Tue, 21:00–23:00) by an hour.');
    expect(body).toContain('One ticked show clashes with shows the station has.');
    expect(body).toContain('Moved · was Sat 30 Jan');
    // Marked exact, chosen dates, or refused with the reason.
    expect(body.match(/data-mapping="exact"/g)).toHaveLength(4);
    expect(body.match(/data-mapping="dates"/g)).toHaveLength(2);
    expect(body.match(/data-mapping="refused"/g)).toHaveLength(1);
    expect(body).toContain('It repeats every 2 days, which a show’s rule cannot say.'.replace('’', "'"));
    expect(body).toContain('It is an all-day event: a show needs a time to start.');
    expect(body).toContain('Made from 3 events of the file.');
    // Six can be ticked and renamed; the refused one has neither a tick nor a name to type.
    expect(body.match(/type="checkbox"/g)).toHaveLength(6);
    expect(body.match(/aria-label="Name of show \d"/g)).toHaveLength(6);
    // The one the station already has is unticked, says so in place of a clash, and is not in
    // the warning's count.
    expect(body.match(/type="checkbox"[^>]*checked/g)).toHaveLength(5);
    expect(body).toMatch(/<li class="item off there[^"]*" data-item="6" data-mapping="exact" data-already="kettle-drive">/);
    expect(body).toMatch(/data-there=""[^>]*>.*Already on the station<\/span><span>It is there as Kettle Drive, with the same dates and times\. Left unticked, so it is not made twice\.</s);
    expect(body).toContain('One of these shows is already on the station, with the same dates and times. It starts unticked, so it is not made twice.');
    expect(body.match(/Conflict<\/span><span>/g)).toHaveLength(1);
    expect(body).not.toContain('id="ic-on-5"');
    expect(body).toMatch(/id="ic-make"[^>]*>Make 5 shows<\/button>/);
    expect(body).toContain('A clash does not stop the import');
    expect(body).toContain('made at Solo, with no episode drafted');
  });

  it('a station’s own calendar holds Make the shows and says every show is there', () => {
    const p = preview();
    const names = ['Kettle Breakfast', 'Kettle Drive', 'The Bothy Sessions', 'Budget Night Special', 'Saturday Night Kettle'];
    const own = { ...p, events: 5, notes: [], ready: 5, items: names.map((name, index) => ({ ...p.items[6], index, name, already: { slug: `show-${index}`, name } })) };
    const body = dialog({ file: 'schedule.ics', preview: own });
    expect(body).toContain('0 of 5 ticked');
    expect(body).toContain('All 5 shows in this file are already on the station, with the same dates and times. There is nothing to make.');
    expect(body.match(/Already on the station/g)).toHaveLength(5);
    expect(body).not.toContain('Conflict');
    expect(body).not.toContain('clashes with shows the station has');
    expect(body).not.toMatch(/type="checkbox"[^>]*checked/);
    expect(body).toMatch(/id="ic-make"[^>]*disabled[^>]*title="Every show in this file is already on the station\."/);
    expect(body).toMatch(/<button[^>]*disabled[^>]*title="Every show here is already on the station\. Tick one on its own row to make it a second time\."[^>]*>Tick all<\/button>/);
  });

  it('a file in which nothing can be a show holds Make the shows and says why', () => {
    const p = preview();
    const body = dialog({ file: 'fair.ics', preview: { ...p, items: [{ ...p.items[5], index: 0 }], ready: 0 } });
    expect(body).toMatch(/id="ic-make"[^>]*disabled[^>]*title="Nothing in this file can be made into a show\."/);
    expect(body).not.toContain('Tick all');
  });

  it('part 3 reports what was made and what overlaps, with a way into each show', () => {
    const body = dialog({ file: 'kettle-old-schedule.ics', preview: preview(), made: asImportMade(madeJson) });
    expect(body).toContain('data-part="3"');
    expect(body).toContain('2 shows are made.');
    expect(body).toContain('/folk-night');
    expect(body).toContain('Tue, 20:00–22:00 · next Tue 17 Nov');
    expect(body).toContain('aria-label="Open Folk Night"');
    expect(body).toContain('One of them overlaps another show.');
    expect(body).toContain('Folk Night overlaps The Late Kettle on 52 dates in its first twelve months.');
    expect(body).toContain('Each starts at Solo.');
  });
});
