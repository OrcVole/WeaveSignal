// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The way to a keyed show (keyed shows, 1.3.2; the operator's click-through of 2026-11-25): the
// one control's words, the body a tick sends, what a refusal means, the line on an episode, the
// three steps, and the tick itself as it is drawn for whoever may and may not use it.
import { render } from 'svelte/server';
import { beforeEach, describe, expect, it } from 'vitest';
import KeySteps from './keys/KeySteps.svelte';
import KeyedTick from './keys/KeyedTick.svelte';
import { account } from './lib/account.svelte.ts';
import { FieldError } from './lib/api.ts';
import { KEEP, KEYED, KEY_STEPS, KEY_TIPS, LISTEN_SHORT, accessProblem, keepBody, listenLine, listenState } from './lib/keyed.ts';
import { keyed } from './lib/keys.svelte.ts';
import { blockAria, blockHtml } from './schedule/blockHtml.ts';
import type { Block } from './schedule/model.ts';
import ListenLine from './schedule/ListenLine.svelte';

const words = (html: string) => html.replace(/<!--.*?-->/g, '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').replace(/ ([:,.])/g, '$1').trim();
const staff = (role: string) => ({ kind: 'staff', role, account: { id: 'a1', name: 'Shona Campbell', role } }) as never;
const row = (o: object = {}) => ({ id: 'id-steeped', slug: 'steeped', name: 'Steeped', now: 'key' as const, opens_at_ms: null, slate: 'announcement' as const, slate_note: '', ...o });

describe('the one control’s words', () => {
  it('says Keyed show wherever a show is made keyed, and Keyed on a show that is', () => {
    expect(KEEP.tick).toBe('Keyed show');
    expect(KEEP.fresh).toBe('New keyed show');
    expect(KEYED.word).toBe('Keyed');
    expect(KEYED.tip).toMatch(/^Keyed: advertised to everyone, heard and watched only with a key/);
    expect(KEEP.hint).toMatch(/You make the keys on the Keys tab\.$/);
  });
});

describe('what a tick sends', () => {
  const now = Date.UTC(2026, 10, 20, 13, 52);
  it('keeps what the public hears as it was set, so a show keyed again comes back as it was', () => {
    expect(keepBody({ slate: 'teaser', slate_note: 'Tickets at kettle.example/ceilidh.', opens_at_ms: null }, now)).toEqual({ rule: 'key', slate: 'teaser', slate_note: 'Tickets at kettle.example/ceilidh.', opens_at_ms: null });
  });
  it('keeps a time it opens to everyone only while that is still to come', () => {
    expect(keepBody({ slate: 'fallback', slate_note: '', opens_at_ms: now + 60_000 }, now).opens_at_ms).toBe(now + 60_000);
    // A past one would leave the show open to everyone, and the tick would seem to do nothing.
    expect(keepBody({ slate: 'fallback', slate_note: '', opens_at_ms: now - 60_000 }, now).opens_at_ms).toBeNull();
  });
  it('sends the rule alone when nothing was read', () => {
    expect(keepBody(null, now)).toEqual({ rule: 'key' });
  });
});

describe('what a refusal means', () => {
  it('says playout has keyed shows off, or is not answering, when the station will not key a show', () => {
    expect(accessProblem(new FieldError(409, 'Playout has keyed shows off (its data’s `keyed.enabled`)…', null), 'save', 'key')).toMatch(/^Playout has keyed shows off, so nothing would keep this show from the public\. Turn them on \(KEYED_SHOWS/);
    expect(accessProblem(new FieldError(503, 'Playout is not answering, so a keyed show cannot be checked: timed out', null), 'save', 'key')).toMatch(/^Playout is not answering, so the station cannot make sure/);
  });
  it('says keys are not set up when the rule cannot even be read, in the station’s words', () => {
    expect(accessProblem(new FieldError(503, 'Keys are kept in the database.', null), 'read')).toBe('Keys are not set up at this station: Keys are kept in the database.');
  });
  it('says who may, a sign-in that ended, and a station older than keyed shows', () => {
    expect(accessProblem(new FieldError(403, 'forbidden', null), 'save')).toMatch(/^Only a producer or an admin can change who can listen/);
    expect(accessProblem(new FieldError(401, '', null), 'read')).toMatch(/^Your sign-in has ended/);
    expect(accessProblem(new FieldError(404, 'not found', null), 'read')).toMatch(/they came with WeaveSignal 1\.3\.2/);
    expect(accessProblem(new FieldError(404, 'no such show', null), 'read')).toBe('no such show');
  });
  it('gives the station’s own sentence for anything else, and says so when there was no answer', () => {
    expect(accessProblem(new FieldError(422, 'A slate note is at most 500 characters on one line.', 'slate_note'), 'save')).toBe('A slate note is at most 500 characters on one line.');
    expect(accessProblem(new TypeError('Failed to fetch'), 'save')).toBe('The station did not answer. Try again.');
    expect(accessProblem(new FieldError(500, '500 Internal Server Error', null), 'save')).toBe('The station did not answer. Try again.');
  });
});

describe('who can listen, on an episode', () => {
  it('reads the show’s standing from the public list: none is a public show', () => {
    expect(listenState(null)).toBe('public');
    expect(listenState({ now: 'key' })).toBe('key');
    expect(listenState({ now: 'public' })).toBe('opened');
    expect(LISTEN_SHORT.key).toBe('Only people with a key can listen.');
  });
  it('says the rule is the show’s, every episode, this one too', () => {
    const l = listenLine('key', { show: 'Friday Supporters’ Hour', opens: '29 Nov 2026, 15:00' });
    expect(l.lead).toBe('This show is kept for people with a key.');
    expect(l.rest).toBe('Every episode of Friday Supporters’ Hour, this one too: it is advertised to everyone and heard only with a key. It opens to everyone 29 Nov 2026, 15:00.');
    expect(listenLine('public', { show: 'Steeped' }).rest).toBe('A key keeps the whole show, not one episode: the tick keeps every episode of Steeped for people with a key, this one too.');
  });
  it('says plainly on an aired episode that a key keeps the whole show, not that episode', () => {
    for (const st of ['public', 'key', 'opened'] as const) {
      const l = listenLine(st, { show: 'The Long Lunch', aired: true });
      expect(l.lead).toBe('A key keeps the whole show, not one episode.');
      expect(l.rest).toMatch(/^This one has aired, so nothing here changes who heard it\./);
    }
    expect(listenLine('key', { show: 'The Long Lunch', aired: true }).rest).toContain('The Long Lunch is kept for people with a key now');
  });
});

describe('the three steps', () => {
  it('are keep a show, make a key, send it, each with where it is found', () => {
    expect(KEY_STEPS.map((s) => `${s.n} ${s.label}`)).toEqual(['1 Keep a show for people with a key', '2 Make a key for it', '3 Send it']);
    expect(KEY_STEPS[0].where).toBe('Schedule › show › Who can listen');
  });
  it('mark the step they stand at in words, make the others buttons where there is somewhere to go, and say why where there is not', () => {
    const html = render(KeySteps, { props: { current: 2, go: { 1: () => {} }, waits: { 3: 'Send opens by itself the moment the key is made.' } } }).body;
    expect(html).toMatch(/<li[^>]*aria-current="step"[^>]*data-step="2"/);
    expect(html.match(/<button/g)).toHaveLength(1);
    expect(words(html)).toBe('1 Step 1: Keep a show for people with a key Schedule › show › Who can listen 2 Step 2, where you are: Make a key for it You are here 3 Step 3: Send it opens when the key is made Send opens by itself the moment the key is made.');
    expect(html.match(/<svg class="pic/g)).toHaveLength(3);
  });
  it('have a hint for each of a key’s less obvious fields', () => {
    expect(Object.keys(KEY_TIPS).sort()).toEqual(['family', 'kind', 'limits', 'media', 'opens', 'works']);
    for (const t of Object.values(KEY_TIPS)) expect(t.length).toBeGreaterThan(60);
    expect(KEY_TIPS.family).toContain('Find a key says whose it was');
  });
});

describe('the tick', () => {
  beforeEach(() => {
    Object.assign(keyed, { list: { shows: [row()], get_a_key_text: '', get_a_key_url: '' }, state: 'ok', at: Date.now() });
    account.me = staff('producer');
    account.loaded = true;
  });
  const tick = (props: object) => render(KeyedTick, { props: { slug: 'steeped', name: 'Steeped', id: 't', ...props } as never }).body;

  it('is a tick box with the key and the words Keyed show, ticked on a keyed show, for a producer', () => {
    const html = tick({});
    expect(html).toMatch(/<input[^>]*type="checkbox"[^>]*checked/);
    expect(html).toContain('<use href="#g-key"');
    expect(words(html)).toBe('Keyed show: Steeped');
    expect(html).toMatch(/data-listen="key"/);
  });
  it('is unticked on a show everyone can hear', () => {
    const html = tick({ slug: 'the-long-lunch', name: 'The Long Lunch' });
    expect(html).toMatch(/data-listen="public"/);
    expect(html).not.toMatch(/<input[^>]* checked/);
  });
  it('is no control for a presenter: the word Keyed with the key on a keyed show, and nothing on a public one', () => {
    account.me = staff('presenter');
    const html = tick({});
    expect(html).not.toContain('<input');
    expect(words(html)).toBe('Keyed');
    expect(html).toContain('<use href="#g-key"');
    expect(words(tick({ slug: 'the-long-lunch', name: 'The Long Lunch' }))).toBe('');
  });
  it('is not drawn at a station older than keyed shows', () => {
    keyed.state = 'none';
    expect(words(tick({}))).toBe('');
  });
  it('stands on an episode with whose rule it is, and on an aired one says a key keeps the whole show', () => {
    const line = (props: object) => words(render(ListenLine, { props: { slug: 'steeped', show: 'Steeped', zone: 'Europe/London', ...props } as never }).body);
    expect(line({})).toBe('Keyed show: Steeped This show is kept for people with a key. Every episode of Steeped, this one too: it is advertised to everyone and heard only with a key. Who can listen…');
    expect(line({ slug: 'the-long-lunch', show: 'The Long Lunch', aired: true })).toBe('Keyed show: The Long Lunch A key keeps the whole show, not one episode. This one has aired, so nothing here changes who heard it. The Long Lunch is open to everyone; the tick changes the episodes to come. Who can listen…');
    account.me = staff('presenter');
    expect(line({})).toMatch(/^Keyed This show is kept for people with a key\..* A producer or an admin sets who can listen\.$/);
  });
});

describe('a keyed show’s blocks in the week', () => {
  const b = { id: 'e1', kind: 'episode', slug: 'steeped', show: 'Steeped', start: Date.UTC(2026, 10, 20, 14), end: Date.UTC(2026, 10, 20, 16), colour: 3, hosts: [], weekly: true, conflict: false, moved: null, prerecorded: false, number: 1, title: '' } as unknown as Block;
  const ctx = { now: Date.UTC(2026, 10, 20, 13), zone: 'Europe/London', selected: null, keyed: new Set(['steeped']) };
  it('carry the key beside the time with the word for where there is room, and Keyed on a line of its own', () => {
    const html = blockHtml(b, ctx);
    expect(html).toMatch(/<span class="rep keyed" title="Keyed: advertised to everyone[^"]*"><svg aria-hidden="true"><use href="#g-key"\/><\/svg><span class="kw">Keyed<\/span><\/span>/);
    expect(html).toMatch(/<span class="kind kk" title="Keyed: [^"]*"><svg[^>]*><use href="#g-key"\/><\/svg>Keyed<\/span>/);
    expect(blockAria(b, ctx)).toContain('keyed, for people with a key');
  });
  it('have the word beside the key alone in the Day view, where the line has the room', () => {
    const html = blockHtml(b, { ...ctx, wide: true });
    expect(html).toContain('<span class="kw">Keyed</span>');
    expect(html).not.toContain('kind kk');
  });
  it('carry nothing of it on a public show, or on automation', () => {
    expect(blockHtml({ ...b, slug: 'the-long-lunch' } as Block, ctx)).not.toContain('g-key');
    expect(blockHtml({ ...b, kind: 'automation' } as Block, ctx)).not.toContain('g-key');
  });
});
