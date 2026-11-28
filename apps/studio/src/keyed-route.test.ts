// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The way to a keyed show (keyed shows, 1.3.2; the operator's click-through of 2026-11-25): the
// one control's words, the body a tick sends, what a refusal means, the line on an episode, the
// three steps, and the tick itself as it is drawn for whoever may and may not use it. From
// 1.3.3: what the tick means (a key is needed now, so a show that opened to everyone is not
// ticked), and what the words say of a keyed show's podcast feed and episode files.
import { render } from 'svelte/server';
import { beforeEach, describe, expect, it } from 'vitest';
import KeySteps from './keys/KeySteps.svelte';
import KeyedTick from './keys/KeyedTick.svelte';
import { APP_VERSION } from './appVersion.ts';
import { account } from './lib/account.svelte.ts';
import { FieldError } from './lib/api.ts';
import KeyFields from './keys/KeyFields.svelte';
import { FILES, KEEP, KEYED, KEY_STEPS, KEY_TIPS, LISTEN_SHORT, MEDIA_OLD, accessProblem, holdsFiles, keepBody, keptWords, listenLine, listenState, nextOpening, openedAgainWords, openedWords, tickTip, ticked } from './lib/keyed.ts';
import { MEDIA_WORD, formOf, freshForm } from './lib/keys.ts';
import { keyed } from './lib/keys.svelte.ts';
import { studio } from './lib/studio.svelte.ts';
import { blockAria, blockHtml } from './schedule/blockHtml.ts';
import type { Block } from './schedule/model.ts';
import ListenLine from './schedule/ListenLine.svelte';

// What is drawn: the tick's description for a screen reader (a hidden element the box names) is left out.
const words = (html: string) => html.replace(/<!--.*?-->/g, '').replace(/<span id="[^"]*-kt-tip" hidden[^>]*>.*?<\/span>/g, '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').replace(/ ([:,.])/g, '$1').trim();
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
  it('says, where a show is made keyed, that its podcast feed and episode files are held back', () => {
    expect(KEEP.hint).toContain('Its podcast feed and episode files are held back while it is keyed.');
  });
});

describe('what the tick means (1.3.3)', () => {
  it('is ticked only while a key is needed now: not on a public show, and not on one that opened to everyone', () => {
    expect(ticked('key')).toBe(true);
    expect(ticked('public')).toBe(false);
    expect(ticked('opened')).toBe(false);
  });
  it('says in its tooltip what the state means and what a press does', () => {
    expect(tickTip('key')).toBe('Ticked: only people with a key can listen now. Its podcast feed lists no episode and its episode files are held back while it is keyed, earlier episodes included. Untick to open the show to everyone again.');
    expect(tickTip('public')).toBe('Not ticked: everyone can listen. Tick and the show is kept for people with a key from that moment. Its podcast feed and episode files are held back too, earlier episodes included, until it is open to everyone again. Copies already downloaded cannot be called back.');
    expect(tickTip('opened', { opened: '21 Nov 2026, 15:00' })).toBe('Not ticked: the show opened to everyone 21 Nov 2026, 15:00, so no key is needed now. Tick to keep it for people with a key again.');
    expect(tickTip('opened')).toContain('the show opened to everyone, so no key is needed now');
  });
  it('says beside an unticked show that opened when it did, and the way back', () => {
    expect(openedWords('21 Nov 2026, 15:00')).toBe('Opened to everyone 21 Nov 2026, 15:00: no key is needed now. Tick to keep it for people with a key again.');
    expect(openedWords(null)).toBe('Opened to everyone: no key is needed now. Tick to keep it for people with a key again.');
    expect(LISTEN_SHORT.opened).toBe('Everyone can listen now.');
  });
  it('knows when the next show opens to everyone by itself, so the list is read again then', () => {
    const now = Date.UTC(2026, 10, 20, 13, 52);
    expect(nextOpening([], now)).toBeNull();
    expect(nextOpening([{ now: 'key', opens_at_ms: null }, { now: 'public', opens_at_ms: now - 60_000 }], now)).toBeNull();
    expect(nextOpening([{ now: 'key', opens_at_ms: now + 90_000 }, { now: 'key', opens_at_ms: now + 30_000 }, { now: 'key' }], now)).toBe(30_000);
    // Its time has come and the list still says key: read it again at once.
    expect(nextOpening([{ now: 'key', opens_at_ms: now - 5 }], now)).toBe(0);
  });
});

describe('a keyed show’s podcast feed and episode files (1.3.3)', () => {
  it('are held back by a station of 1.3.3 or later, and not by 1.3.2', () => {
    expect(holdsFiles('1.3.3')).toBe(true);
    expect(holdsFiles('1.3.3-test.1')).toBe(true);
    expect(holdsFiles('1.4.0')).toBe(true);
    expect(holdsFiles('2.0.0')).toBe(true);
    expect(holdsFiles('1.3.2')).toBe(false);
    expect(holdsFiles('1.3.2-test.2')).toBe(false);
    // Not read yet: the app's own release.
    expect(holdsFiles(null)).toBe(true);
    expect(holdsFiles('')).toBe(true);
  });
  it('go by the station’s version in the released app, and by 1.3.3 in a build made before the release’s version is set', () => {
    // The released app: the station's version decides.
    expect(holdsFiles('1.3.2', '1.3.3')).toBe(false);
    expect(holdsFiles('1.3.2-test.2', '1.4.0')).toBe(false);
    expect(holdsFiles('1.3.3', '1.3.3')).toBe(true);
    // A candidate or a test image still says 1.3.2, and so does the station built with it,
    // which holds the files back: the app does not tell it to update.
    expect(holdsFiles('1.3.2', '1.3.2')).toBe(true);
    expect(holdsFiles('1.3.2', '1.3.2-test.1')).toBe(true);
  });
  it('are said after a tick, and that the episodes return after it is opened again', () => {
    expect(keptWords('Steeped')).toBe('Steeped is kept for people with a key, from now. Its podcast feed and episode files are held back meanwhile, earlier episodes included. Make keys for its listeners on the Keys tab.');
    expect(openedAgainWords('Steeped')).toBe('Steeped is open to everyone again. Its keys are kept, and open nothing until it is keyed again. Its episodes return to the podcast feed within a few minutes.');
  });
  it('are said to stay open at a station of 1.3.2, with what to do', () => {
    expect(keptWords('Steeped', false)).toBe(`Steeped is kept for people with a key, from now. ${FILES.older} Make keys for its listeners on the Keys tab.`);
    expect(FILES.older).toBe('This station runs WeaveSignal 1.3.2, which leaves a keyed show’s podcast feed and episode files open to anyone. Update the station to hold them back.');
    expect(openedAgainWords('Steeped', false)).toBe('Steeped is open to everyone again. Its keys are kept, and open nothing until it is keyed again.');
    expect(tickTip('public', { held: false })).toBe(`Not ticked: everyone can listen. Tick and the show is kept for people with a key from that moment. ${FILES.older}`);
    expect(listenLine('key', { show: 'Steeped', held: false }).rest).toBe(`Every episode of Steeped, this one too: it is advertised to everyone and heard only with a key. ${FILES.older}`);
    expect(listenLine('public', { show: 'Steeped', aired: true, held: false }).rest).toBe(`This one has aired, so nothing here changes who heard it on air. Steeped is open to everyone. The tick keeps the episodes to come for people with a key. ${FILES.older}`);
  });
  it('never promise what a tick cannot do: a copy already downloaded stays where it is', () => {
    expect(FILES.gone).toBe('Copies already downloaded cannot be called back.');
    for (const st of ['public', 'key'] as const) expect(listenLine(st, { show: 'Steeped', aired: true }).rest).toContain(FILES.gone);
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
    expect(LISTEN_SHORT.public).toBe('Everyone can listen.');
  });
  it('says the rule is the show’s, every episode, this one too', () => {
    const l = listenLine('key', { show: 'Friday Supporters’ Hour', opens: '29 Nov 2026, 15:00' });
    expect(l.lead).toBe('This show is kept for people with a key.');
    expect(l.rest).toBe('Every episode of Friday Supporters’ Hour, this one too: it is advertised to everyone and heard only with a key. It opens to everyone 29 Nov 2026, 15:00. Its podcast feed lists no episode and its episode files are held back while it is keyed, earlier episodes included.');
    expect(listenLine('public', { show: 'Steeped' }).rest).toBe('A key keeps the whole show, not one episode: the tick keeps every episode of Steeped for people with a key, this one too. Its podcast feed and episode files are held back too, earlier episodes included, until it is open to everyone again.');
  });
  it('says of a show that opened to everyone when it did, and that the tick keeps it again', () => {
    const l = listenLine('opened', { show: 'The Archive Hour', opened: '15 Nov 2026, 15:00' });
    expect(l.lead).toBe('This show has opened to everyone.');
    expect(l.rest).toBe('The Archive Hour was kept for people with a key until 15 Nov 2026, 15:00; the tick keeps it again. A key keeps the whole show, not one episode.');
    expect(listenLine('opened', { show: 'The Archive Hour' }).rest).toContain('until then; the tick keeps it again');
  });
  it('says plainly on an aired episode that a key keeps the whole show, not that episode', () => {
    for (const st of ['public', 'key', 'opened'] as const) {
      const l = listenLine(st, { show: 'The Long Lunch', aired: true });
      expect(l.lead).toBe('A key keeps the whole show, not one episode.');
      expect(l.rest).toMatch(/^This one has aired, so nothing here changes who heard it on air\./);
    }
    expect(listenLine('key', { show: 'The Long Lunch', aired: true }).rest).toContain('The Long Lunch is kept for people with a key now');
  });
  it('says on an aired episode what the tick does to its file too, not only to the episodes to come (1.3.3)', () => {
    const air = 'This one has aired, so nothing here changes who heard it on air.';
    expect(listenLine('public', { show: 'The Long Lunch', aired: true }).rest).toBe(`${air} The Long Lunch is open to everyone. The tick keeps the episodes to come for people with a key, and holds back the show’s podcast feed and episode files, this episode’s too, until it is open to everyone again. Copies already downloaded cannot be called back.`);
    expect(listenLine('key', { show: 'The Long Lunch', aired: true }).rest).toBe(`${air} The Long Lunch is kept for people with a key now, so its podcast feed lists no episode and its episode files are held back, this episode’s too, until the show is open to everyone. Copies already downloaded cannot be called back.`);
    expect(listenLine('opened', { show: 'The Long Lunch', aired: true }).rest).toBe(`${air} The Long Lunch was kept for people with a key and has opened to everyone, so its episodes are in the podcast feed and its episode files can be fetched again.`);
    // The sentence 1.3.2 had is gone: the tick does not change the episodes to come alone.
    for (const st of ['public', 'key', 'opened'] as const) expect(listenLine(st, { show: 'The Long Lunch', aired: true }).rest).not.toContain('the tick changes the episodes to come');
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

describe('what a key is for (1.3.3: Picture, with its sound is no longer offered)', () => {
  const noop = () => null;
  const fields = (props: object) => render(KeyFields, { props: { problems: {}, shows: [], zone: 'Europe/London', now: Date.UTC(2026, 10, 20, 13, 52), id: 'kf', ...props } as never }).body;
  const options = (html: string) => [...html.slice(html.indexOf('id="kf-media"')).split('</select>')[0].matchAll(/<option value="([a-z]+)"[^>]*>([^<]*)</g)].map((m) => `${m[1]}=${m[2]}`);
  const stored = { id: 'k1', kind: 'personal', label: 'Isla Fraser', note: '', has_email: false, media: 'video', all_shows: true, shows: [], state: 'active', state_reason: '', listens_used: 0, created_by: 'Shona Campbell', created_ms: 0, updated_ms: 0 } as never;

  it('offers a new key Sound and picture or Sound only, and nothing for the picture alone', () => {
    expect(options(fields({ form: freshForm(), mode: 'make' }))).toEqual(['both=Sound and picture', 'sound=Sound only']);
    expect(options(fields({ form: freshForm(), mode: 'template' }))).toEqual(['both=Sound and picture', 'sound=Sound only']);
    expect(fields({ form: freshForm(), mode: 'make' })).not.toContain('data-testid="media-old"');
  });
  it('says so in the hint: the two choices, and no third', () => {
    expect(KEY_TIPS.media).toBe('Sound and picture: the holder hears the show, and watches it where the station sends a picture. Sound only keeps the picture back.');
  });
  it('shows a key made for the picture as it is: its choice stays in its own list, chosen, with a line that says what it opens', () => {
    const html = fields({ form: formOf(stored, noop as never), mode: 'change', video: true });
    expect(options(html)).toEqual(['both=Sound and picture', 'sound=Sound only', 'video=Picture, with its sound']);
    expect(html).toMatch(/<option value="video" selected[^>]*>Picture, with its sound</);
    expect(words(html)).toContain(MEDIA_OLD);
    expect(MEDIA_OLD).toBe('This key was made for the picture, with its sound. It opens the same as Sound and picture, and goes on working as it is. The choice is no longer offered for new keys.');
    expect(MEDIA_WORD.video).toBe('Picture, with its sound');
  });
  it('drops the choice from that key’s list once it is something else, and the line with it', () => {
    const html = fields({ form: { ...formOf(stored, noop as never), media: 'both' }, mode: 'change', video: false });
    expect(options(html)).toEqual(['both=Sound and picture', 'sound=Sound only']);
    expect(html).not.toContain('data-testid="media-old"');
  });
});

describe('the tick', () => {
  beforeEach(() => {
    Object.assign(keyed, { list: { shows: [row(), row({ id: 'id-the-archive-hour', slug: 'the-archive-hour', name: 'The Archive Hour', now: 'public', opens_at_ms: Date.UTC(2026, 10, 15, 15, 0) })], get_a_key_text: '', get_a_key_url: '' }, state: 'ok', at: Date.now() });
    account.me = staff('producer');
    account.loaded = true;
    studio.zone = 'Europe/London';
    studio.stationVersion = null;
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
  it('is unticked on a show that was kept and has opened to everyone, and says when it opened and the way back (1.3.3)', () => {
    const html = tick({ slug: 'the-archive-hour', name: 'The Archive Hour' });
    expect(html).toMatch(/data-listen="opened"/);
    expect(html).not.toMatch(/<input[^>]* checked/);
    expect(html).not.toMatch(/class="keyedtick[^"]* on[ "]/);
    expect(words(html)).toBe('Keyed show: The Archive Hour Opened to everyone 15 Nov 2026, 15:00: no key is needed now. Tick to keep it for people with a key again.');
    // In a row of a list too, where 1.3.2 showed a tick and no words.
    expect(words(tick({ slug: 'the-archive-hour', name: 'The Archive Hour', compact: true }))).toContain('Opened to everyone 15 Nov 2026, 15:00: no key is needed now.');
    // Not where the words beside it already say so.
    expect(words(tick({ slug: 'the-archive-hour', name: 'The Archive Hour', plain: true }))).toBe('Keyed show: The Archive Hour');
  });
  it('describes itself to a screen reader in the tooltip’s words, on a hidden element the box names, before what a press said', () => {
    const html = tick({ slug: 'the-long-lunch', name: 'The Long Lunch' });
    expect(html).toMatch(/<input[^>]*aria-describedby="t-kt-tip t-kt-say"/);
    expect(html).toMatch(/<span id="t-kt-tip" hidden[^>]*>Not ticked: everyone can listen\. Tick and the show is kept for people with a key from that moment\. Its podcast feed and episode files are held back too, earlier episodes included, until it is open to everyone again\. Copies already downloaded cannot be called back\.<\/span>/);
  });
  it('says in its tooltip what its state means, on each of the three', () => {
    expect(tick({})).toContain('title="Ticked: only people with a key can listen now.');
    expect(tick({ slug: 'the-long-lunch', name: 'The Long Lunch' })).toContain('title="Not ticked: everyone can listen. Tick and the show is kept for people with a key from that moment.');
    expect(tick({ slug: 'the-archive-hour', name: 'The Archive Hour' })).toContain('title="Not ticked: the show opened to everyone 15 Nov 2026, 15:00, so no key is needed now.');
  });
  it('is no mark for a presenter on a show that opened to everyone: nothing says Keyed there', () => {
    account.me = staff('presenter');
    expect(words(tick({ slug: 'the-archive-hour', name: 'The Archive Hour' }))).toBe('');
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
    expect(line({})).toBe('Keyed show: Steeped This show is kept for people with a key. Every episode of Steeped, this one too: it is advertised to everyone and heard only with a key. Its podcast feed lists no episode and its episode files are held back while it is keyed, earlier episodes included. Who can listen…');
    expect(line({ slug: 'the-long-lunch', show: 'The Long Lunch', aired: true })).toBe('Keyed show: The Long Lunch A key keeps the whole show, not one episode. This one has aired, so nothing here changes who heard it on air. The Long Lunch is open to everyone. The tick keeps the episodes to come for people with a key, and holds back the show’s podcast feed and episode files, this episode’s too, until it is open to everyone again. Copies already downloaded cannot be called back. Who can listen…');
    // A show that opened to everyone: the tick is not ticked, and the line says when it opened, once.
    const opened = render(ListenLine, { props: { slug: 'the-archive-hour', show: 'The Archive Hour', zone: 'Europe/London' } as never }).body;
    expect(opened).toMatch(/data-listen="opened"/);
    expect(opened).not.toMatch(/<input[^>]* checked/);
    expect(words(opened)).toBe('Keyed show: The Archive Hour This show has opened to everyone. The Archive Hour was kept for people with a key until 15 Nov 2026, 15:00; the tick keeps it again. A key keeps the whole show, not one episode. Who can listen…');
    // At a station of 1.3.2 the released app promises nothing that station does not do; a build
    // that is not yet 1.3.3 by its own version words it as 1.3.3 does (`holdsFiles`).
    studio.stationVersion = '1.3.2';
    const released = !holdsFiles('1.3.2', APP_VERSION);
    if (released) {
      expect(line({})).toContain('This station runs WeaveSignal 1.3.2, which leaves a keyed show’s podcast feed and episode files open to anyone.');
      expect(line({})).not.toContain('are held back');
    } else {
      expect(line({})).toContain('its episode files are held back while it is keyed');
      expect(line({})).not.toContain('This station runs WeaveSignal 1.3.2');
    }
    studio.stationVersion = null;
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
