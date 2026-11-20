// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The monitor link's rules (ADR-0280): the lamp from the state alone, the station's clock kept
// from its views and never the device's, the counts, and the words the page makes itself.
import { describe, expect, it } from 'vitest';
import { frames, type MonitorView } from './lib/monitor/api.ts';
import { StationClock, hm, hms, zoneLine } from './lib/monitor/clock.ts';
import { barPct, count, counting, dumpedWords, headsUp, hurry, listenOff, insertWords, lampOf, lasted, lead, nextRows, nothingToPress, refusalWords, signed, sourceWords, openStreamWords, standbyStreamWords, thenWords, titleOf } from './lib/monitor/words.ts';

const ZONE = 'Europe/London';
// Friday 20 November 2026, the design's day; London keeps GMT then.
const at = (hms: string) => Date.parse(`2026-11-20T${hms.length === 5 ? `${hms}:00` : hms}Z`);
const no = (why: string) => ({ may: false, why_not: why });

function view(o: Partial<MonitorView> = {}): MonitorView {
  return {
    label: 'Finlay Grant',
    show: 'steeped',
    show_name: 'Steeped',
    state: 'standby',
    says: 'Standing by. The station hears you. Listeners do not.',
    then: { kind: 'at_post', words: 'You go on air at the post, by the schedule.', at_ms: at('14:00') },
    way: 'srt',
    format: 'mpegts/aac 48 kHz stereo',
    level: -22.6,
    leveller_db: -7.4,
    steady_ms: 291_000,
    title: null,
    monitor_open: false,
    door_closes_ms: at('16:00'),
    last_refusal: null,
    post: null,
    rundown: {
      before: { show_name: 'The Long Lunch', host: 'Duncan Boyd', state: 'in_room', start_ms: at('12:00'), end_ms: at('14:00') },
      you: { show_name: 'Steeped', host: 'Finlay Grant', state: 'standby', start_ms: at('14:00'), end_ms: at('16:00') },
      after: { show_name: 'Kettle Drive', host: 'Heather Douglas', state: 'not_connected', start_ms: at('16:00'), end_ms: at('18:00') },
    },
    insert: null,
    step: { step: 'full', word: 'Full', dump: true, handover: true },
    next_host: null,
    listeners_behind_ms: 23_000,
    monitor_has_programme: false,
    controls: { go: no('x'), not_yet: no('x'), cue: no('x'), bring: no('x'), hand_over: no('x'), end_show: no('x'), end_show_says: null, dump: no('x') },
    station: { now_ms: at('13:57:12'), zone: ZONE },
    ...o,
  };
}

describe('the event stream', () => {
  it('cuts whole events and keeps the rest for later', () => {
    const got = frames('event: view\ndata: {"a":1}\n\n: keep-alive\n\nevent: ended\ndata: {"error":"x"}\n\nevent: vi');
    expect(got.frames).toEqual([
      { event: 'view', data: '{"a":1}' },
      { event: 'ended', data: '{"error":"x"}' },
    ]);
    expect(got.rest).toBe('event: vi');
  });
  it('reads carriage returns and data on several lines', () => {
    expect(frames('event: view\r\ndata: {\r\ndata: }\r\n\r\n').frames).toEqual([{ event: 'view', data: '{\n}' }]);
  });
});

describe("the station's clock", () => {
  it('is unknown until the station has said', () => {
    const c = new StationClock();
    expect(c.known).toBe(false);
    expect(c.at(1000)).toBeNull();
  });
  it('takes the view that travelled fastest, so a slow one does not pull it back', () => {
    const c = new StationClock();
    // The page's steady time runs 1,000 ms behind the station's epoch time less a million.
    c.read(1_000_000, 50); // 50 ms on the way
    c.read(1_001_000, 1400); // 400 ms on the way: a later reading, and a worse one
    c.read(1_002_000, 2020); // 20 ms on the way: the best
    expect(c.at(2020)).toBe(1_001_980 + 20);
    expect(c.at(3020)).toBe(1_003_000);
  });
  it('does not stutter from view to view', () => {
    const c = new StationClock();
    c.read(5_000_000, 10);
    const first = c.at(1010);
    c.read(5_001_000, 1300); // held up by 290 ms
    expect(c.at(1310)).toBe(first! + 300);
  });
  it("forgets old readings, so it follows a device whose steady time drifts", () => {
    const c = new StationClock();
    c.read(1_000_000, 0);
    for (let i = 1; i <= 30; i++) c.read(1_000_000 + i * 1000 - 40, i * 1000);
    expect(c.at(30_000)).toBe(1_030_000 - 40);
  });
  it("follows the station at once when its own clock is set back", () => {
    const c = new StationClock();
    for (let i = 0; i < 10; i++) c.read(1_000_000 + i * 1000, i * 1000 + 20);
    // The station corrects its clock by a minute.
    c.read(1_010_000 - 60_000, 10_020);
    expect(c.at(10_020)).toBe(950_000);
    // One view held up for two seconds is only a slow view: the clock stays.
    c.read(951_000, 13_020);
    expect(c.at(13_020)).toBe(953_000);
  });
  it("writes times in the station's zone, whatever the device's", () => {
    expect(hms(at('14:00:00'), ZONE)).toBe('14:00:00');
    expect(hm(at('14:00:00'), 'Europe/Berlin')).toBe('15:00');
    expect(hms(at('14:00:00'), 'Not/AZone')).toBe('14:00:00');
  });
  it("puts the host's own time beside the station's when it differs", () => {
    expect(zoneLine(at('16:01:18'), ZONE, ZONE)).toBe('Europe/London · the same as yours');
    expect(zoneLine(at('16:01:18'), ZONE, 'Europe/Lisbon')).toBe('Europe/London · the same as yours');
    expect(zoneLine(at('16:01:18'), ZONE, 'Europe/Berlin')).toBe('17:01 your time · Europe/Berlin');
    expect(zoneLine(at('16:01:18'), ZONE, null)).toBe('Europe/London');
  });
});

describe('the lamp', () => {
  it('is picked from the state alone', () => {
    expect(lampOf('on_air')).toMatchObject({ kind: 'onair', chip: { word: 'On air', cls: 'fill-onair' } });
    expect(lampOf('open')).toMatchObject({ kind: 'open', chip: { word: 'Open' } });
    expect(lampOf('standby')).toMatchObject({ kind: 'standby', chip: { word: 'Standby' } });
    expect(lampOf('link_lost')).toMatchObject({ kind: 'lost', chip: { word: 'Link lost', glyph: 'g-link-degraded' } });
    expect(lampOf('test')).toMatchObject({ kind: 'test', chip: { word: 'Test' } });
    expect(lampOf('not_connected')).toMatchObject({ kind: 'notconn', chip: { word: 'Not connected' } });
  });
  it('is none at all for a state the page does not know: never ON AIR by mistake', () => {
    for (const s of ['', 'rehearsing', 'ON_AIR', 'onair', 'toString', 'constructor', '__proto__']) expect(lampOf(s)).toBeNull();
  });
  it("sets the sentence's opening apart, and leaves the station's words as they are", () => {
    expect(lead('On air. Listeners hear you, about 23 s behind.')).toEqual({ lead: 'On air.', rest: 'Listeners hear you, about 23 s behind.' });
    expect(lead('The station has lost your stream.')).toEqual({ lead: 'The station has lost your stream.', rest: '' });
    expect(lead('')).toEqual({ lead: '', rest: '' });
  });
  it('does not say what happens next twice', () => {
    expect(thenWords(view())).toBe('You go on air at the post, by the schedule.');
    const gone = view({ says: 'Your place was kept for 5 minutes. Connect again and you will be standing by.', then: { kind: 'place_gone', words: 'Connect again and you will be standing by.', at_ms: null } });
    expect(thenWords(gone)).toBe('');
    expect(thenWords(view({ then: null }))).toBe('');
  });
  it('titles the page with the show and the host', () => {
    expect(titleOf(view())).toBe('Steeped · Finlay Grant');
  });
});

describe('a count in its last seconds', () => {
  const c = { figure: '0:45', at_ms: at('16:02:00') };
  it('is plain until its last 30 s, amber to 11 s and red from 10 s, by the figure shown', () => {
    expect(hurry(c, at('16:01:15'))).toBeNull(); // 0:45
    expect(hurry(c, at('16:01:30'))).toEqual({ level: 'amber', word: 'Last 30 seconds' }); // shows 0:30
    expect(hurry(c, at('16:01:35'))).toEqual({ level: 'amber', word: 'Last 30 seconds' });
    expect(hurry(c, at('16:01:49'))).toEqual({ level: 'amber', word: 'Last 30 seconds' }); // shows 0:11
    expect(hurry(c, at('16:01:50'))).toEqual({ level: 'red', word: 'Last 10 seconds' }); // shows 0:10
    expect(hurry(c, at('16:01:59'))).toEqual({ level: 'red', word: 'Last 10 seconds' });
  });
  it('is never there without a figure or after the instant', () => {
    expect(hurry({ figure: null, at_ms: at('16:02:00') }, at('16:01:50'))).toBeNull();
    expect(hurry(c, at('16:02:00'))).toBeNull();
    expect(hurry(c, at('16:03:00'))).toBeNull();
  });
});

describe('Start listening', () => {
  it('may be pressed standing by, on air and open, and is off, with the reason, where nothing can play', () => {
    for (const state of ['standby', 'on_air', 'open']) expect(listenOff({ state }, true)).toBeNull();
    expect(listenOff({ state: 'link_lost' }, true)).toBe('Nothing plays here while the station has lost your stream.');
    expect(listenOff({ state: 'test' }, true)).toMatch(/^Nothing plays here until you are standing by\./);
    expect(listenOff({ state: 'not_connected' }, true)).toMatch(/^Nothing plays here until you are standing by\./);
  });
  it('is off while the page has lost the station, whatever the last view said', () => {
    expect(listenOff({ state: 'on_air' }, false)).toMatch(/^This page has lost the station\./);
  });
});

describe('counts', () => {
  it('writes minutes and seconds, hours when there are any, and nothing from a day away', () => {
    expect(count(168_000)).toBe('2:48');
    expect(count(167_001)).toBe('2:48');
    expect(count(0)).toBe('0:00');
    expect(count(-5)).toBe('0:00');
    expect(count(3_727_000)).toBe('1:02:07');
    expect(count(86_400_000)).toBeNull();
    expect(lasted(291_999)).toBe('4:51');
    expect(lasted(7_491_000)).toBe('2:04:51');
  });
  it('counts to the post', () => {
    expect(counting(view(), at('13:57:12'), ZONE)).toMatchObject({ label: 'On air at 14:00:00', figure: '2:48', small: 'by the schedule, to the second' });
    expect(counting(view(), at('14:00:00'), ZONE)).toBeNull();
  });
  it('counts a late start down', () => {
    const v = view({ then: { kind: 'late_count', words: 'Automation holds your air.', at_ms: at('16:10:55') } });
    expect(counting(v, at('16:10:41'), ZONE)).toMatchObject({ label: 'On air in', figure: '0:14' });
  });
  it('counts the grace for the host who is on air, and for the one behind them', () => {
    const then = { kind: 'grace', words: 'x', at_ms: at('16:02:00') };
    const post = { show_name: 'Kettle Drive', start_ms: at('16:00'), started: true, mine: false, host: 'Heather Douglas', host_state: 'standing_by', on_air_at_ms: null, late_ends_ms: null, grace_ends_ms: at('16:02:00'), held_by: null, on_air: [] };
    expect(counting(view({ state: 'on_air', then, post }), at('16:01:18'), ZONE)).toMatchObject({ label: 'Kettle Drive takes the air', figure: '0:42', small: 'by the schedule at 16:02:00' });
    expect(counting(view({ then, post: { ...post, mine: true } }), at('16:01:18'), ZONE)).toMatchObject({ label: 'On air at 16:02:00', figure: '0:42' });
  });
  it('says the heads-up as the station gave it, with its time on the station’s clock', () => {
    const post = { show_name: 'Steeped', start_ms: at('14:00'), started: false, mine: true, host: 'Finlay Grant', host_state: 'standing_by', on_air_at_ms: at('14:00'), late_ends_ms: null, grace_ends_ms: null, held_by: null, on_air: [] };
    const given = { at_ms: at('13:55:00'), words: 'You are on next, at the start of Steeped.' };
    expect(headsUp(view({ post: { ...post, heads_up: given } }), ZONE)).toEqual({ words: 'You are on next, at the start of Steeped.', when: 'Heads-up at 13:55' });
    // The station's clock, not the device's zone.
    expect(headsUp(view({ post: { ...post, heads_up: given } }), 'Europe/Berlin')?.when).toBe('Heads-up at 14:55');
    // Nothing from a station older than 1.2, for a host who is not next, or with no post at all.
    expect(headsUp(view({ post }), ZONE)).toBeNull();
    expect(headsUp(view({ post: { ...post, heads_up: null } }), ZONE)).toBeNull();
    expect(headsUp(view(), ZONE)).toBeNull();
    // Once on air the lamp says it; an empty line says nothing.
    expect(headsUp(view({ state: 'on_air', post: { ...post, heads_up: given } }), ZONE)).toBeNull();
    expect(headsUp(view({ post: { ...post, heads_up: { at_ms: at('13:55'), words: '  ' } } }), ZONE)).toBeNull();
  });
  it('counts a kept place', () => {
    const v = view({ state: 'link_lost', then: { kind: 'link_lost', words: 'x', at_ms: at('14:46:00') } });
    expect(counting(v, at('14:41:07'), ZONE)).toMatchObject({ label: 'Your place is kept until 14:46:00', figure: '4:53' });
  });
  it('says when the next show is: counted today, by its day from a day away, and when it has started', () => {
    const slot = (ms: number) => view({ state: 'not_connected', then: { kind: 'slot', words: 'Steeped is next.', at_ms: ms } });
    expect(counting(slot(at('16:00')), at('13:41:52'), ZONE)).toMatchObject({ label: 'Your show starts in', figure: '2:18:08', small: 'today at 16:00' });
    expect(counting(slot(Date.parse('2026-11-27T14:00:00Z')), at('13:41:52'), ZONE)).toMatchObject({ label: 'Your show starts', figure: null, small: 'Friday 27 November at 14:00' });
    expect(counting(slot(at('13:00')), at('13:41:52'), ZONE)).toMatchObject({ label: 'Your show started at 13:00', figure: null });
  });
  it('counts nothing for a sentence with no instant, or a kind it does not know', () => {
    expect(counting(view({ then: { kind: 'held', words: 'x', at_ms: null } }), at('13:57:12'), ZONE)).toBeNull();
    expect(counting(view({ then: { kind: 'something_new', words: 'x', at_ms: at('14:00') } }), at('13:57:12'), ZONE)).toBeNull();
    expect(counting(view({ then: null }), at('13:57:12'), ZONE)).toBeNull();
  });
  it('runs the bar over the last five minutes to the post, and from first sight otherwise', () => {
    expect(barPct({ kind: 'at_post', at_ms: at('14:00') }, at('13:57:12'), 0)).toBe(44);
    expect(barPct({ kind: 'at_post', at_ms: at('14:00') }, at('13:00'), 0)).toBe(0);
    expect(barPct({ kind: 'grace', at_ms: at('16:02:00') }, at('16:01:18'), at('16:00:00'))).toBe(65);
    expect(barPct({ kind: 'late_count', at_ms: at('16:10:55') }, at('16:10:41'), at('16:10:35'))).toBe(30);
    expect(barPct({ kind: 'grace', at_ms: at('16:02:00') }, at('16:02:01'), at('16:00:00'))).toBe(100);
  });
});

describe('the station hears you', () => {
  const NOW = at('13:57:12');
  it('sets out the way in, the level as it arrived and what the leveller does, and how long steady', () => {
    const s = sourceWords(view(), NOW, ZONE);
    expect(s.connected).toBe(true);
    expect(s.rows).toEqual([
      ['Way in', 'SRT · mpegts/aac 48 kHz stereo'],
      ['Level', '−15.2 LUFS · turned down 7.4 dB to match the station'],
      ['Steady for', '4:51'],
    ]);
    expect(s.problem).toBeNull();
    expect(s.icecast).toBe(false);
    expect(s.meter).toBe(62);
  });
  it('says so when the level is past what the station can make up, either way', () => {
    expect(sourceWords(view({ level: -31, leveller_db: 12 }), NOW, ZONE).problem).toMatch(/^Too quiet for the station to make up/);
    expect(sourceWords(view({ level: -20, leveller_db: -12 }), NOW, ZONE).problem).toMatch(/^Too loud for the station to bring down/);
    expect(sourceWords(view({ leveller_db: 0 }), NOW, ZONE).rows[1][1]).toBe('−22.6 LUFS · left as it is');
    expect(sourceWords(view({ leveller_db: 3 }), NOW, ZONE).rows[1][1]).toBe('−25.6 LUFS · turned up 3.0 dB to match the station');
  });
  it("says Icecast's delay for a host who sends that way", () => {
    expect(sourceWords(view({ way: 'icecast' }), NOW, ZONE).icecast).toBe(true);
  });
  it('gives a test its checks, with the level as a reading and no verdict', () => {
    const s = sourceWords(view({ state: 'test', level: -19, leveller_db: null, steady_ms: 18_000 }), NOW, ZONE);
    expect(s.checks.map((k) => [k.mark, k.title])).toEqual([
      ['ok', 'Connected'],
      ['read', 'Level'],
      ['wait', 'Steady for 30 s'],
    ]);
    expect(s.checks[1].detail).toContain('−19.0 dBFS');
    expect(s.checks[2].detail).toBe('18 s so far');
    expect(sourceWords(view({ state: 'test', level: null, leveller_db: null, steady_ms: 31_000 }), NOW, ZONE).checks.map((k) => k.mark)).toEqual(['ok', 'wait', 'ok']);
  });
  it('says where the fault is when the link is lost', () => {
    const s = sourceWords(view({ state: 'link_lost', format: null, level: null, leveller_db: null, steady_ms: null }), NOW, ZONE);
    expect(s.connected).toBe(false);
    expect(s.problem).toContain('the fault is between your software and the station');
  });
  it('gives the last refusal in words while nothing is connected, or when it came after this connection began', () => {
    const refusal = { at_ms: at('13:41:20'), reason: 'wrong_key', words: 'The key is wrong.' };
    expect(refusalWords(refusal, ZONE)).toBe('The station refused your software at 13:41: the key is wrong.');
    const none = view({ state: 'not_connected', way: null, format: null, level: null, leveller_db: null, steady_ms: null, last_refusal: refusal });
    expect(sourceWords(none, at('13:41:52'), ZONE).problem).toBe('The station refused your software at 13:41: the key is wrong.');
    // Connected since 13:52:21: a refusal at 13:41 is history, one at 13:55 is another program still trying.
    expect(sourceWords(view({ last_refusal: refusal }), NOW, ZONE).problem).toBeNull();
    expect(sourceWords(view({ last_refusal: { at_ms: at('13:55:00'), reason: 'in_use', words: 'Another connection is using this key. Close your other software first.' } }), NOW, ZONE).problem).toBe(
      'The station refused your software at 13:55: another connection is using this key. Close your other software first.',
    );
    expect(sourceWords(view({ state: 'not_connected', way: null, format: null, level: null, leveller_db: null, steady_ms: null }), NOW, ZONE).rows).toEqual([['Your software', 'Nothing is arriving from it.']]);
  });
  it('writes a true minus', () => {
    expect(signed(-15.24)).toBe('−15.2');
    expect(signed(0)).toBe('0.0');
    expect(signed(-0.01)).toBe('0.0');
  });
});

describe('before you, you, after you', () => {
  it('names each show, its host and where they are', () => {
    expect(nextRows(view(), at('13:57:12'), ZONE)).toEqual([
      { where: 'Before you', title: 'The Long Lunch', detail: "Duncan Boyd · in the station's room", chip: null },
      { where: 'You', title: 'Steeped', detail: '14:00 to 16:00', chip: expect.objectContaining({ word: 'Standby' }) },
      { where: 'After you', title: 'Kettle Drive', detail: 'Heather Douglas · 16:00 · not connected yet', chip: null },
    ]);
  });
  it('says running over, ended, and standing by with its chip', () => {
    const v = view({ state: 'on_air' });
    v.rundown.you = { ...v.rundown.you!, state: 'on_air' };
    v.rundown.after = { ...v.rundown.after!, state: 'standing_by' };
    const rows = nextRows(v, at('16:01:18'), ZONE);
    expect(rows[0].detail).toBe('ended 14:00');
    expect(rows[1]).toMatchObject({ detail: '14:00 to 16:00 · running over', chip: { word: 'On air' } });
    expect(rows[2]).toMatchObject({ detail: 'Heather Douglas · 16:00', chip: { word: 'Standby' } });
  });
  it('says the show before is running over while it holds the air', () => {
    const v = view();
    v.rundown.before = { show_name: 'Steeped', host: 'Finlay Grant', state: 'on_air', start_ms: at('12:00'), end_ms: at('14:00') };
    expect(nextRows(v, at('14:01:18'), ZONE)[0]).toMatchObject({ detail: 'Finlay Grant · running over', chip: { word: 'On air' } });
  });
  it('says so when nothing is scheduled after, and names the day of a show that is not today', () => {
    const v = view({ state: 'test' });
    v.rundown.before = null;
    v.rundown.after = null;
    v.rundown.you = { ...v.rundown.you!, state: 'test' };
    const rows = nextRows(v, Date.parse('2026-11-18T20:14:36Z'), ZONE);
    expect(rows).toEqual([
      { where: 'You', title: 'Steeped', detail: 'Friday 20 November, 14:00 to 16:00', chip: expect.objectContaining({ word: 'Test' }) },
      { where: 'After you', title: 'Nobody', detail: 'Nothing is scheduled after you.', chip: null },
    ]);
  });
  it('carries no chip and no state while the page has lost the station', () => {
    const v = view({ state: 'on_air' });
    v.rundown.you = { ...v.rundown.you!, state: 'on_air' };
    v.rundown.before = { ...v.rundown.before!, state: 'on_air' };
    v.rundown.after = { ...v.rundown.after!, state: 'standing_by' };
    const rows = nextRows(v, at('14:30'), ZONE, false);
    expect(rows.every((r) => r.chip === null)).toBe(true);
    expect(rows[2].detail).toBe('Heather Douglas · 16:00');
  });
  it('is empty when the show has no airing in sight', () => {
    const v = view();
    v.rundown = { before: null, you: null, after: null };
    expect(nextRows(v, at('13:57:12'), ZONE)).toEqual([]);
  });
});

describe('the insert, the controls and the dump', () => {
  const insert = { label: 'Station ID', at_ms: at('15:00'), duration_ms: 7000, plays: 'unknown' };
  it('says when the next insert is and how long it plays, and no more than the station knows', () => {
    expect(insertWords(insert, view(), at('14:57:12'), ZONE)).toEqual({ title: 'Station ID at 15:00:00', detail: 'In 2:48. It plays for 0:07.' });
    expect(insertWords({ ...insert, at_ms: at('17:00') }, view(), at('16:01:18'), ZONE).detail).toBe('After your time. Nothing for you to do.');
  });
  it('says what to do when there is nothing to press', () => {
    expect(nothingToPress(view(), at('13:57:12'), ZONE)).toBe('Nothing to press. You go on air at 14:00:00. Start when the lamp turns red.');
    expect(nothingToPress(view({ then: { kind: 'held', words: 'x', at_ms: null } }), at('13:57:12'), ZONE)).toBe('Nothing to press. Start when the lamp turns red.');
    expect(nothingToPress(view({ then: { kind: 'grace', words: 'x', at_ms: at('16:02:00') } }), at('16:01:18'), ZONE)).toBe('Nothing to press. You go on air by 16:02:00. Start when the lamp turns red.');
    expect(nothingToPress(view({ state: 'test' }), at('13:57:12'), ZONE)).toContain('Nothing here can put you on air.');
    expect(nothingToPress(view({ state: 'not_connected' }), at('13:57:12'), ZONE)).toBe('Nothing to press until your software reaches the station.');
  });
  it('says what listeners lost to a dump', () => {
    expect(dumpedWords(8000)).toBe('Listeners lost 8 s. Carry on, and do not repeat the words.');
    expect(dumpedWords(null)).toBe('Carry on, and do not repeat the words.');
  });
});

describe('the stream while open', () => {
  it('says it is the station’s output, how far behind, and that the room is not in it', () => {
    expect(openStreamWords(23_400)).toBe("You are hearing the station's output, about 23 s behind. The room's voices are not in it. Cue and talkback still come through, live.");
  });
  it('says will hear before the press, and never says while you stand by', () => {
    expect(openStreamWords(23_400, false)).toBe("You will hear the station's output, about 23 s behind. The room's voices are not in it. Cue and talkback still come through, live.");
    expect(openStreamWords(null)).toContain('a little behind');
    expect(openStreamWords(0, false)).not.toContain('stand by');
  });
});

describe('the stream while standing by', () => {
  it('says how far behind it is, and what takes over on air', () => {
    expect(standbyStreamWords(23_400)).toBe('You are hearing what listeners hear, about 23 s behind, while you stand by. When you are on air: everyone but you · cue · talkback, live.');
  });
  it('does not guess a delay it was not told', () => {
    // Before Start listening is pressed nothing plays, so it is what the press will bring.
    expect(standbyStreamWords(23_400, false)).toBe('You will hear what listeners hear, about 23 s behind, while you stand by. When you are on air: everyone but you · cue · talkback, live.');
    expect(standbyStreamWords(null)).toContain('what listeners hear, a little behind,');
    expect(standbyStreamWords(0)).toContain('a little behind');
  });
});
