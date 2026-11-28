// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Start now in New show (1.3.2): the length it gives, the rule made here for a station without
// the route (on the station's clock and zone, across midnight), what the station's preview of
// it means, the words, and where the offer is drawn. From 1.3.3 the DJ's setup has it too, at
// the head of its third part, where the show is set up for the first time.
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import type { PreviewDate, RulePreview, ShowSaved } from './lib/api.ts';
import type { Show } from './lib/schedule.ts';
import type { Me } from './lib/api.ts';
import NewShow from './schedule/rule/NewShow.svelte';
import ShowWizard from './settings/ShowWizard.svelte';
import wizardSrc from './settings/ShowWizard.svelte?raw';
import newShowSrc from './schedule/rule/NewShow.svelte?raw';
import { newForm } from './schedule/rule/form.ts';
import { START_NOW, nowMinutes, nowVerdict, ruleNowHere, ruleNowOr, startNowFlow, startedWords, type RuleNow } from './schedule/rule/startNow.ts';
import { CLASH_COST } from './schedule/rule/view.ts';

const ZONE = 'Europe/London';
const noop = () => {};
const base = { shows: [] as Show[], zone: ZONE, today: '2026-11-24', live: false, onMade: noop, onOpen: noop, onClose: noop };
const text = (html: string) => html.replace(/<!--.*?-->/g, '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ');
const at = (iso: string) => Date.parse(iso);

describe('how long a show that starts now runs', () => {
  it('is the length the form’s two times make, an hour to begin with', () => {
    expect(nowMinutes(newForm('2026-11-24'))).toBe(60);
    expect(nowMinutes({ from: '20:00', until: '22:30' })).toBe(150);
    expect(nowMinutes({ from: '23:30', until: '00:15' })).toBe(45);
  });

  it('is an hour when the form’s length is one the station does not take for this, or is not a length', () => {
    expect(nowMinutes({ from: '20:00', until: '20:10' })).toBe(60);
    expect(nowMinutes({ from: '08:00', until: '18:00' })).toBe(60);
    expect(nowMinutes({ from: '', until: '21:00' })).toBe(60);
    expect(nowMinutes({ from: '20:00', until: '20:15' })).toBe(15);
    expect(nowMinutes({ from: '10:00', until: '18:00' })).toBe(480);
  });
});

describe('the rule made here, for a station older than 1.3.2', () => {
  it('starts at the next whole minute, once, on the station’s date and clock', () => {
    const r = ruleNowHere(at('2026-11-24T14:06:20Z'), ZONE, 60);
    expect(r.rule).toEqual({ starts_on: '2026-11-24', start: 14 * 60 + 7, minutes: 60, repeat: { kind: 'once' }, end: { kind: 'never' }, skip: [], moves: [] });
    expect(r.time).toBe('14:07');
    expect(r.starts_at_ms).toBe(at('2026-11-24T14:07:00Z'));
  });

  it('is the next minute even on the minute: the show has not started when it is saved', () => {
    expect(ruleNowHere(at('2026-11-24T14:06:00Z'), ZONE, 60).time).toBe('14:07');
    expect(ruleNowHere(at('2026-11-24T14:06:59.999Z'), ZONE, 60).time).toBe('14:07');
  });

  it('rolls into the next day at midnight, in the station’s zone and not the window’s', () => {
    const r = ruleNowHere(at('2026-11-24T23:59:30Z'), ZONE, 30);
    expect([r.date, r.time, r.rule.start]).toEqual(['2026-11-25', '00:00', 0]);
    // In summer London is an hour on: 23:30 UTC is already tomorrow there.
    const s = ruleNowHere(at('2026-06-30T23:30:10Z'), ZONE, 60);
    expect([s.date, s.time]).toEqual(['2026-07-01', '00:31']);
    const ny = ruleNowHere(at('2026-11-24T14:06:20Z'), 'America/New_York', 60);
    expect([ny.date, ny.time]).toEqual(['2026-11-24', '09:07']);
  });
});

describe('what the station’s preview of it means', () => {
  const date = (clashes: PreviewDate['clashes'] = []): PreviewDate => ({ on: '2026-11-24', number: 1, start: at('2026-11-24T14:07:00Z') / 1000, minutes: 60, state: 'airs', now: false, moved_from: null, clock: null, episode: null, clashes });
  const preview = (dates: PreviewDate[], problems: RulePreview['problems'] = []): RulePreview => ({ sentence: '', short: '', last: null, dates, more: false, total: 1, clear: null, problems, hints: [], dropped: { skip: [], moves: [] }, change: null });
  const clash = (refused: boolean): PreviewDate['clashes'][number] => ({ show: 'Steeped', slug: 'steeped', start: at('2026-11-24T14:00:00Z') / 1000, minutes: 60, overlap_minutes: 53, when: null, with: ['Eilidh Fraser-MacLeod'], refused });

  it('goes on when nothing else has the time', () => {
    expect(nowVerdict(preview([date()]), ZONE)).toEqual({ kind: 'go' });
  });

  it('says another show at that time once, with what a clash costs, and does not refuse it', () => {
    expect(nowVerdict(preview([date([clash(false)])]), ZONE)).toEqual({ kind: 'clash', title: 'Steeped is on the schedule now (14:00 to 15:00).', body: CLASH_COST });
  });

  it('is held where the station refuses the clash, or the rule, in a sentence', () => {
    const held = nowVerdict(preview([date([clash(true)])]), ZONE);
    expect(held.kind).toBe('held');
    expect(held).toMatchObject({ why: 'Steeped has the air now (14:00 to 15:00), and this account may not schedule over it. Give your show a later time below, or ask the station.' });
    expect(nowVerdict(preview([], [{ field: 'starts_on', message: 'That time has passed. Pick a time still to come. (today)' }]), ZONE)).toEqual({ kind: 'held', why: 'That time has passed. Pick a time still to come. (today)' });
    expect(nowVerdict(preview([]), ZONE).kind).toBe('held');
  });
});

describe('the words', () => {
  it('say what one press does, with the length, and that more dates can follow', () => {
    expect(START_NOW.button).toBe('Start now');
    expect(START_NOW.what(60)).toBe('Makes the show and starts it at the next whole minute, station time, for 1 hour, once. Give it more dates later, from the show’s pane.');
    expect(START_NOW.what(150)).toContain('for 2 h 30 min, once');
  });

  it('say on Done when it starts and ends, and that ON AIR is the lamp (listeners can hear someone), not the schedule', () => {
    const w = startedWords({ time: '14:07', starts_at_ms: at('2026-11-24T14:07:00Z'), minutes: 60 }, ZONE);
    expect(w.lead).toBe('It starts at 14:07, station time:');
    expect(w.rest).toBe('within a minute, until 15:07. When its time comes the step at the top of the studio becomes this show’s. ON AIR is the lamp, not the schedule: it lights when listeners can hear someone.');
  });
});

describe('New show, part 2', () => {
  const part2 = (props: Record<string, unknown> = {}) => render(NewShow, { props: { ...base, part: 2, name: 'The Morning Kettle', ...props } }).body;
  const row = (body: string) => body.slice(body.indexOf('data-testid="ns-now"'), body.indexOf('class="foot2'));

  it('opens with Start now over the questions: one button, what it does, and not the primary button', () => {
    const body = part2();
    expect(body.indexOf('data-testid="ns-now"')).toBeGreaterThan(-1);
    expect(body.indexOf('data-testid="ns-now"')).toBeLessThan(body.indexOf('id="ns-first"'));
    expect(text(row(body))).toContain('On air straight away? Makes the show and starts it at the next whole minute, station time, for 1 hour, once.');
    expect(body).toMatch(/<button id="ns-now" type="button" class="btn"[^>]*aria-describedby="ns-now-what"[^>]*>Start now<\/button>/);
    // Make the show keeps the primary button, and ⏎.
    expect(body).toMatch(/<button type="submit" class="btn primary"[^>]*>Make the show<\/button>/);
    expect(body.match(/class="btn primary/g)).toHaveLength(1);
  });

  it('gives the length the two times make', () => {
    const body = part2({ form: { ...newForm('2026-11-24'), from: '20:00', until: '22:00' } });
    expect(text(row(body))).toContain('for 2 hours, once');
  });

  it('is not offered on part 1, or at a station that keeps a weekly time only', () => {
    expect(render(NewShow, { props: base }).body).not.toContain('data-testid="ns-now"');
    expect(part2({ rules: false })).not.toContain('data-testid="ns-now"');
  });

  it('asks once when another show has the time: the clash and its cost, Start now anyway, and a way back', () => {
    const body = part2({ nowAsk: { title: 'Steeped is on the schedule now (14:00 to 15:00).', body: CLASH_COST } });
    const r = row(body);
    expect(body).toMatch(/class="startnow [^"]*asks[^"]*" data-testid="ns-now"/);
    expect(r).toContain('role="alert"');
    expect(text(r)).toContain(`Steeped is on the schedule now (14:00 to 15:00). ${CLASH_COST}`);
    expect(r).toMatch(/>Not now<\/button>.*<button id="ns-now"[^>]*>Start now anyway<\/button>/s);
  });

  it('asks the station at the press, never when the dialog opens, and falls back only where the route is absent', () => {
    const fn = newShowSrc.slice(newShowSrc.indexOf('async function startNow()'), newShowSrc.indexOf('/** Make the show:'));
    expect(fn).toContain('ruleNowOr(() => api.ruleNow(len), routeAbsent, () => ruleNowHere(stationNow(), zone, len))');
    expect(fn).toContain("preview: (rule) => api.previewRule({ rule, limit: 1 })");
    expect(fn).toContain('save: (rule) => api.saveShow(slug, newShowInput({ name, colour, step }, rule, rules))');
    expect(newShowSrc.match(/api\.ruleNow\(/g)).toHaveLength(1);
    // Done says when it starts only where the station drafted the episode that airs then.
    expect(fn).toContain('started = saved.drafted?.episode ? done.now : null;');
  });
});

describe('one press of Start now', () => {
  const rule = (hhmm: string) => ruleNowHere(at(`2026-11-24T${hhmm}:20Z`), ZONE, 60);
  const date = (clashes: PreviewDate['clashes'] = []): PreviewDate => ({ on: '2026-11-24', number: 1, start: 0, minutes: 60, state: 'airs', now: false, moved_from: null, clock: null, episode: null, clashes });
  const preview = (dates: PreviewDate[], problems: RulePreview['problems'] = []): RulePreview => ({ sentence: '', short: '', last: null, dates, more: false, total: 1, clear: null, problems, hints: [], dropped: { skip: [], moves: [] }, change: null });
  const clash = (refused: boolean): PreviewDate['clashes'][number] => ({ show: 'Steeped', slug: 'steeped', start: at('2026-11-24T14:00:00Z') / 1000, minutes: 60, overlap_minutes: 53, when: null, with: [], refused });
  const PASSED = { field: 'starts_on', message: 'That time has passed. Pick a time still to come. (today)' };
  class Refused extends Error {
    constructor(readonly status: number, readonly field: string | null) {
      super(field === 'name' ? 'That name does not fit the card.' : 'That time has passed.');
    }
  }
  const minuteTurned = (e: unknown) => e instanceof Refused && e.status === 422 && e.field !== 'name';

  /** A station that answers each call from a list, and remembers what it was asked. */
  function station(o: { rules?: RuleNow[]; previews?: RulePreview[]; saves?: (string | Error)[] }) {
    const log: string[] = [];
    const [rules, previews, saves] = [[...(o.rules ?? [rule('14:06'), rule('14:07')])], [...(o.previews ?? [])], [...(o.saves ?? ['saved'])]];
    return {
      log,
      calls: {
        ask: async () => {
          const r = rules.shift()!;
          log.push(`ask ${r.time}`);
          return r;
        },
        preview: async (r: RuleNow['rule']) => {
          log.push(`preview ${r.start}`);
          return previews.shift() ?? preview([date()]);
        },
        save: async (r: RuleNow['rule']) => {
          log.push(`save ${r.start}`);
          const a = saves.shift()!;
          if (a instanceof Error) throw a;
          return a;
        },
      },
    };
  }

  it('asks for the rule, asks what the station says of it, and saves that rule', async () => {
    const st = station({});
    const done = await startNowFlow(st.calls, { anyway: false, zone: ZONE, minuteTurned });
    expect(done).toMatchObject({ kind: 'made', saved: 'saved', now: { time: '14:07' } });
    expect(st.log).toEqual(['ask 14:07', 'preview 847', 'save 847']);
  });

  it('saves nothing when another show has the time: the clash is returned, to be asked about once', async () => {
    const st = station({ previews: [preview([date([clash(false)])])] });
    const done = await startNowFlow(st.calls, { anyway: false, zone: ZONE, minuteTurned });
    expect(done).toEqual({ kind: 'clash', title: 'Steeped is on the schedule now (14:00 to 15:00).', body: CLASH_COST });
    expect(st.log).toEqual(['ask 14:07', 'preview 847']);
  });

  it('Start now anyway asks for the rule again, at that press’s minute, and saves without a second preview', async () => {
    const st = station({ rules: [rule('14:09')] });
    const done = await startNowFlow(st.calls, { anyway: true, zone: ZONE, minuteTurned });
    expect(done).toMatchObject({ kind: 'made', now: { time: '14:10' } });
    expect(st.log).toEqual(['ask 14:10', 'save 850']);
  });

  it('saves nothing where the station refuses the clash for this account', async () => {
    const st = station({ previews: [preview([date([clash(true)])])] });
    const done = await startNowFlow(st.calls, { anyway: false, zone: ZONE, minuteTurned });
    expect(done.kind).toBe('held');
    expect(st.log).toEqual(['ask 14:07', 'preview 847']);
  });

  it('asks once more when the preview says the minute has passed, then goes on with the new minute', async () => {
    const st = station({ previews: [preview([], [PASSED]), preview([date()])] });
    const done = await startNowFlow(st.calls, { anyway: false, zone: ZONE, minuteTurned });
    expect(done).toMatchObject({ kind: 'made', now: { time: '14:08' } });
    expect(st.log).toEqual(['ask 14:07', 'preview 847', 'ask 14:08', 'preview 848', 'save 848']);
  });

  it('and only once: a second passed minute is the station’s sentence, and nothing is saved', async () => {
    const st = station({ previews: [preview([], [PASSED]), preview([], [PASSED])] });
    const done = await startNowFlow(st.calls, { anyway: false, zone: ZONE, minuteTurned });
    expect(done).toEqual({ kind: 'held', why: PASSED.message });
    expect(st.log).toEqual(['ask 14:07', 'preview 847', 'ask 14:08', 'preview 848']);
  });

  it('asks once more when the save is refused for its rule, and saves the new minute', async () => {
    const st = station({ saves: [new Refused(422, 'rule'), 'saved'] });
    const done = await startNowFlow(st.calls, { anyway: false, zone: ZONE, minuteTurned });
    expect(done).toMatchObject({ kind: 'made', now: { time: '14:08' }, saved: 'saved' });
    expect(st.log).toEqual(['ask 14:07', 'preview 847', 'save 847', 'ask 14:08', 'preview 848', 'save 848']);
  });

  it('and only once: a second refusal is thrown, for the dialog to say', async () => {
    const st = station({ saves: [new Refused(422, 'rule'), new Refused(422, 'rule')] });
    await expect(startNowFlow(st.calls, { anyway: false, zone: ZONE, minuteTurned })).rejects.toThrow('That time has passed.');
    expect(st.log.filter((l) => l.startsWith('save'))).toHaveLength(2);
  });

  it('does not try again for a refusal that is not about the minute: a name that does not fit, a clash the station refuses', async () => {
    const name = station({ saves: [new Refused(422, 'name')] });
    await expect(startNowFlow(name.calls, { anyway: false, zone: ZONE, minuteTurned })).rejects.toThrow('That name does not fit the card.');
    expect(name.log).toEqual(['ask 14:07', 'preview 847', 'save 847']);
    const clashed = station({ saves: [new Refused(409, null)] });
    await expect(startNowFlow(clashed.calls, { anyway: true, zone: ZONE, minuteTurned })).rejects.toBeInstanceOf(Refused);
    expect(clashed.log).toEqual(['ask 14:07', 'save 847']);
  });

  it('makes the rule here only where the station has no such route; any other failure is the station’s', async () => {
    const here = () => rule('14:06');
    const absent = (e: unknown) => (e as { status?: number }).status === 404;
    expect((await ruleNowOr(async () => rule('09:00'), absent, here)).time).toBe('09:01');
    expect((await ruleNowOr(async () => Promise.reject({ status: 404 }), absent, here)).time).toBe('14:07');
    await expect(ruleNowOr(async () => Promise.reject({ status: 503 }), absent, here)).rejects.toEqual({ status: 503 });
  });
});

describe('Done, for a show started now', () => {
  const saved = { show: { slug: 'the-morning-kettle', name: 'The Morning Kettle', rule: { starts_on: '2026-11-24', start: 847, minutes: 60, repeat: { kind: 'once' }, end: { kind: 'never' }, skip: [], moves: [] }, slot: null }, resolved: {}, series: [], drafted: null } as unknown as ShowSaved;
  const started: RuleNow = { ...ruleNowHere(at('2026-11-24T14:06:20Z'), ZONE, 60) };

  it('says first when it starts, then where the studio starts', () => {
    const body = render(NewShow, { props: { ...base, part: 3, name: 'The Morning Kettle', saved, started } }).body;
    const t = text(body);
    expect(t).toContain('It starts at 14:07, station time: within a minute, until 15:07.');
    expect(body.indexOf('data-testid="ns-started"')).toBeLessThan(body.indexOf('data-step="solo"'));
    expect(t).toContain('The studio starts at Solo:');
  });

  it('says nothing of it for a show made with dates', () => {
    expect(render(NewShow, { props: { ...base, part: 3, name: 'The Morning Kettle', saved } }).body).not.toContain('data-testid="ns-started"');
  });
});

describe('Start now in the DJ’s setup (1.3.3)', () => {
  const ailsa: Me = { kind: 'staff', role: 'presenter', account: { id: 'a-ailsa', username: 'ailsa', name: 'Ailsa Drummond', role: 'presenter', disabled_ms: null, created_ms: 1 } };
  const wizard = { me: ailsa, shows: [] as Show[], zone: ZONE, today: '2026-11-24', rules: true, live: false, onDone: noop, onSkip: noop, onClose: noop };
  const part3 = (props: Record<string, unknown> = {}) => render(ShowWizard, { props: { ...wizard, part: 3, ...props } as never }).body;
  const row = (body: string) => body.slice(body.indexOf('data-testid="sw-now"'), body.indexOf('class="keep"'));
  const mine = { slug: 'the-wee-small-hours', name: 'The Wee Small Hours', colour_slot: 3, slot: null, rule: null, hosts: [], tags: [], explicit: false, presenter: 'Ailsa Drummond', contact: '', links: [], commercial_music: false, presenter_account: 'a-ailsa' } as unknown as Show;

  it('opens the third part with Start now over the questions: one button, what it does, and not the primary button', () => {
    const body = part3();
    expect(body.indexOf('data-testid="sw-now"')).toBeGreaterThan(-1);
    expect(body.indexOf('data-testid="sw-now"')).toBeLessThan(body.indexOf('id="sw-first"'));
    expect(text(row(body))).toContain('On air straight away? Saves your show and starts it at the next whole minute, station time, for 1 hour, once. Give it more dates later, from Settings, under Your show.');
    expect(body).toMatch(/<button id="sw-now" type="button" class="btn"[^>]*aria-describedby="sw-now-what"[^>]*>Start now<\/button>/);
    // Save my show keeps the primary button, and ⏎.
    expect(body).toMatch(/<button type="submit" class="btn primary"[^>]*>Save my show<\/button>/);
    expect(body.match(/class="btn primary/g)).toHaveLength(1);
  });

  it('gives the length the two times make', () => {
    expect(text(row(part3({ form: { ...newForm('2026-11-24'), from: '21:00', until: '23:00' } })))).toContain('for 2 hours, once');
    expect(START_NOW.whatMine(150)).toContain('for 2 h 30 min, once');
  });

  it('is not offered on the other parts, at a station that keeps a weekly time only, or for a show that exists', () => {
    expect(render(ShowWizard, { props: wizard as never }).body).not.toContain('data-testid="sw-now"');
    expect(render(ShowWizard, { props: { ...wizard, part: 2 } as never }).body).not.toContain('data-testid="sw-now"');
    expect(part3({ rules: false })).not.toContain('data-testid="sw-now"');
    // Run again from Settings: the show may have episodes, and the station drafts the one that
    // airs now only for a show that has none.
    expect(part3({ initial: mine, shows: [mine] })).not.toContain('data-testid="sw-now"');
  });

  it('asks once when another show has the time: the clash and its cost, Start now anyway, and a way back', () => {
    const body = part3({ nowAsk: { title: 'Steeped is on the schedule now (14:00 to 15:00).', body: CLASH_COST } });
    const r = row(body);
    expect(body).toMatch(/class="startnow [^"]*asks[^"]*" data-testid="sw-now"/);
    expect(r).toContain('role="alert"');
    expect(text(r)).toContain(`Steeped is on the schedule now (14:00 to 15:00). ${CLASH_COST}`);
    expect(r).toMatch(/>Not now<\/button>.*<button id="sw-now"[^>]*>Start now anyway<\/button>/s);
  });

  it('is New show’s press: the station’s rule at the press, its preview, and one save with the DJ’s own answers and the first episode drafted', () => {
    const fn = wizardSrc.slice(wizardSrc.indexOf('async function startNow()'), wizardSrc.indexOf('function skip()'));
    expect(fn).toContain('ruleNowOr(() => rulesApi.ruleNow(len), routeAbsent, () => ruleNowHere(stationNow(), zone, len))');
    expect(fn).toContain('preview: (rule) => rulesApi.previewRule({ rule, limit: 1 })');
    expect(fn).toContain('save: (rule) => rulesApi.saveShow(slug, toShowInput(a, me, null, nextColour(shows.length), whenFields(rule, { rules: true, draftFirst: true })))');
    expect(wizardSrc.match(/rulesApi\.ruleNow\(/g)).toHaveLength(1);
    // A refusal of the DJ's own answers is theirs to put right, not a minute that turned.
    expect(fn).toContain("!PLACED.some((f) => f === e.field)");
    expect(wizardSrc).toContain("const PLACED = ['name', 'presenter', 'contact', 'links'] as const;");
    // Done says when it starts only where the station drafted the episode that airs then.
    expect(fn).toContain('started = saved.drafted?.episode ? done.now : null;');
  });

  it('says on Done when it starts, before the episode and the rest', () => {
    const saved = { show: { ...mine, rule: { starts_on: '2026-11-24', start: 847, minutes: 60, repeat: { kind: 'once' }, end: { kind: 'never' }, skip: [], moves: [] }, rule_words: { sentence: 'Once, on Tuesday 24 November, 14:07 to 15:07.', short: '24 Nov, 14:07–15:07', last: null, last_on: null, notes: [] } }, resolved: {}, series: [], drafted: { episode: { id: 'e1', number: 1, air_at: at('2026-11-24T14:07:00Z') / 1000, minutes: 60 }, resolved: {} } } as unknown as ShowSaved;
    const started: RuleNow = { ...ruleNowHere(at('2026-11-24T14:06:20Z'), ZONE, 60) };
    const body = render(ShowWizard, { props: { ...wizard, part: 4, saved, started } as never }).body;
    const t = text(body);
    expect(t).toContain('The Wee Small Hours is set up as your show.');
    expect(t).toContain('It starts at 14:07, station time: within a minute, until 15:07.');
    // The first thing Done says: it is the one line with a minute in it.
    expect(body.indexOf('data-testid="sw-started"')).toBeLessThan(body.indexOf('callers agree to be recorded'));
    expect(body.indexOf('data-testid="sw-started"')).toBeLessThan(body.indexOf('is drafted'));
    expect(t).toContain('It airs once, so there is nothing more to draft.');
    // Without it, Done is as it was.
    expect(render(ShowWizard, { props: { ...wizard, part: 4, saved } as never }).body).not.toContain('data-testid="sw-started"');
  });
});
