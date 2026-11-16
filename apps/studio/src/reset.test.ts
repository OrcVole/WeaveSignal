// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A change of station clears every module-level store (ADR-0285;
// docs/reference/design/station-switcher.md, section 8), so nothing from one station shows or
// is sent on another. Three things are held here: every module that keeps anything at module
// level is either registered to be emptied or named as the app's own; each store, filled with a
// station's things, is back to what it was after the reset; and the chat's loop, which opens
// its socket from the page, stops for good and forgets its session.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// The app has no Node types, so the module is named in a way the type checker does not follow
// (as theme.test.ts does).
interface Entry {
  name: string;
  isDirectory(): boolean;
}
const fsModule = 'node:fs';
const { readdirSync, readFileSync } = (await import(/* @vite-ignore */ fsModule)) as {
  readdirSync(path: URL, options: { withFileTypes: true }): Entry[];
  readFileSync(path: URL, encoding: 'utf8'): string;
};
const SRC = new URL('./', import.meta.url);

/** Every file under `src/` (but not the sandbox, which has one simulated station and no
 *  switcher) whose name passes, as its path from `src/`. */
function files(pass: (name: string) => boolean, dir = ''): string[] {
  return readdirSync(new URL(dir, SRC), { withFileTypes: true }).flatMap((e) => {
    if (e.isDirectory()) return e.name === 'sandbox' ? [] : files(pass, `${dir}${e.name}/`);
    return pass(e.name) ? [`${dir}${e.name}`] : [];
  });
}
const read = (file: string) => readFileSync(new URL(file, SRC), 'utf8');

/** Modules whose module-level state is the app's, not a station's, with the reason. Anything
 *  else that keeps state at module level must register a reset (`onStationChange`). */
const APP_SCOPED: Record<string, string> = {
  'lib/reset.ts': 'the registry itself',
  'lib/theme.svelte.ts': 'the theme is this computer’s choice, the same on every station',
  'lib/zoom.svelte.ts': 'the text size is this computer’s choice, the same on every station',
  'lib/update.svelte.ts': 'the update feed and its download are the app’s; its gate reads the desk’s view, which is emptied',
  'lib/stations.svelte.ts': 'the list of stations is what the switcher shows on every station',
  'schedule/model.ts': 'a cache of date formatters by zone name: no station’s data',
};

/** A line that keeps something at module level: a `let`, or a constant that is a store, an
 *  instance of any class (a map, a set, a control), a list or a record. */
const KEEPS = /^(export )?(let|var) |^(export )?const [A-Za-z_$][\w$]*(: [^=]+)? = (new [A-Z][\w.]*\b|\$state\b|\[\];?$|\{\};?$)/;

describe('every module that keeps something at module level', () => {
  const found = files((n) => /\.ts$/.test(n) && !/\.(test|d)\.ts$/.test(n))
    .map((file) => ({ file, text: read(file) }))
    .map((m) => ({ ...m, keeps: m.text.split('\n').filter((l: string) => KEEPS.test(l)) }))
    .filter((m) => m.keeps.length > 0);

  it('finds the stores this test is about (the pattern still matches the code)', () => {
    const files = found.map((m) => m.file);
    for (const f of ['lib/studio.svelte.ts', 'lib/schedule.svelte.ts', 'lib/chat.svelte.ts', 'lib/account.svelte.ts', 'lib/actions.ts', 'lib/api.ts']) expect(files).toContain(f);
  });

  it('is emptied on a change of station, or is named as the app’s own with the reason', () => {
    const unplaced = found.filter((m) => !m.text.includes('onStationChange(') && !(m.file in APP_SCOPED)).map((m) => `${m.file}: ${m.keeps[0].trim()}`);
    expect(unplaced, 'register a reset with onStationChange (lib/reset.ts), or add the module to APP_SCOPED with the reason').toEqual([]);
  });

  it('is not both: a module named as the app’s own registers no reset', () => {
    const both = found.filter((m) => m.file in APP_SCOPED && m.file !== 'lib/reset.ts' && m.text.includes('onStationChange(')).map((m) => m.file);
    expect(both).toEqual([]);
    // And every name in the list is still a module that keeps something.
    for (const f of Object.keys(APP_SCOPED)) expect(found.map((m) => m.file), f).toContain(f);
  });

  it('has no module-level script in a component, where this test would not look', () => {
    const comps = files((n) => n.endsWith('.svelte'));
    expect(comps.length).toBeGreaterThan(20);
    expect(comps.filter((f) => /<script[^>]*\bmodule\b/.test(read(f)))).toEqual([]);
  });
});

describe('a change of station', () => {
  const store = new Map<string, string>();
  beforeEach(() => {
    store.clear();
    vi.stubGlobal('localStorage', {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    });
    vi.resetModules();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it('puts every store back to what it was when the page first loaded', async () => {
    const { studio, say } = await import('./lib/studio.svelte.ts');
    const { account } = await import('./lib/account.svelte.ts');
    const { sched } = await import('./lib/schedule.svelte.ts');
    const { chat } = await import('./lib/chat.svelte.ts');
    const { prepare } = await import('./lib/prepare.svelte.ts');
    const { setup } = await import('./lib/wizard.svelte.ts');
    const { ruleDialog } = await import('./schedule/rule/dialog.svelte.ts');
    await import('./lib/actions.ts');
    const { resetStores, registered } = await import('./lib/reset.ts');
    const stores = { studio, account, sched, chat, prepare, setup, ruleDialog };
    // `now` is the clock, which a reset reads afresh.
    const snap = () => JSON.parse(JSON.stringify({ ...stores, studio: { ...studio, now: 0 }, sched: { ...sched, anchor: 0 } }));
    const fresh = snap();

    // Kettle FM's things, in every store.
    Object.assign(studio, {
      view: { station: 'Kettle FM', sources: [{ identity: 'eilidh', on_air: true }], callers: [{ identity: 'dougal', name: 'Dougal Thomson' }] },
      link: 'live',
      tab: 'screener',
      settings: true,
      selected: 'dougal',
      armed: { what: 'dump', at: 1 },
      talking: ['gregor'],
      catalogue: { version: 3, banks: [] },
      cartShow: 'saturday-night-kettle',
      bank: 4,
      cartFocus: 7,
      setupPending: ['a contact'],
      zone: 'Europe/London',
      airAsk: { kind: 'end' },
      handoverTo: 'finlay',
      showAll: true,
      stepMenu: true,
      offTold: ['callers'],
      dismissed: [4, 5],
      skew: 250,
    });
    say('good', 'Done', 'On Kettle FM');
    Object.assign(account, { me: { kind: 'staff', role: 'presenter', account: { id: 'a1', name: 'Eilidh Fraser-MacLeod' } }, loaded: true, ended: 'Your sign-in has ended.' });
    Object.assign(sched, { zone: 'Europe/London', zoneSet: true, shows: [{ slug: 'the-morning-kettle' }], episodes: [{ id: 'e1' }], selected: 'e1', editShow: 'the-morning-kettle', pane: 'show', rules: true, error: 'x', view: 'day' });
    Object.assign(chat, { link: 'live', me: { name: 'Eilidh' }, station: 'Kettle FM', show: 'The Morning Kettle', count: 41, compose: 'half a message', replyFor: 9, replyText: 'half a reply', pictures: { p1: null }, attachments: { 'a1:thumb': null }, replied: { 3: 'thanks' } });
    Object.assign(prepare, { menu: true, pick: 'live', busy: true, problem: 'x', settled: true, nudgedAt: 5 });
    Object.assign(setup, { open: true, shows: [{ slug: 'steeped' }], loaded: true, error: 'x' });
    ruleDialog.open = { slug: 'steeped' } as never;
    expect(snap()).not.toEqual(fresh);

    resetStores();

    expect(snap()).toEqual(fresh);
    // By name, the things that would show on, or be sent to, another station.
    expect(studio.view).toBeNull();
    expect(studio.link).toBe('connecting');
    expect(studio.selected).toBeNull();
    expect(studio.armed).toBeNull();
    expect(studio.talking).toEqual([]);
    expect(studio.catalogue).toBeNull();
    expect(studio.cartShow).toBeNull();
    expect(studio.note).toBeNull();
    expect(studio.handoverTo).toBeNull();
    expect(studio.tab).toBe('desk');
    expect(account.me).toBeNull();
    expect(account.loaded).toBe(false);
    expect(sched.shows).toEqual([]);
    expect(sched.episodes).toEqual([]);
    expect(chat.compose).toBe('');
    expect(chat.replyText).toBe('');
    expect(chat.me).toBeNull();
    expect(setup.shows).toEqual([]);
    expect(ruleDialog.open).toBeNull();
    expect(registered().resets.sort()).toEqual(['account', 'actions', 'api', 'chat', 'prepare', 'rule dialog', 'rule notice', 'schedule', 'studio', 'wizard']);
  });

  it('forgets what is kept beside the stores: held talk keys, the station’s version, saves this window marked, fetched pictures', async () => {
    const calls: string[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        calls.push(String(url));
        if (String(url).endsWith('/health')) return new Response(JSON.stringify({ version: '1.0.3' }));
        if (String(url).includes('/chat/mod/avatars/')) return new Response(new Blob(['png']));
        return new Response(JSON.stringify({ station: 'Kettle FM', sources: [], callers: [], notices: [] }));
      }),
    );
    const revoked: string[] = [];
    vi.stubGlobal('URL', Object.assign(URL, { createObjectURL: () => 'blob:kettle-picture', revokeObjectURL: (u: string) => void revoked.push(u) }));
    const { studio } = await import('./lib/studio.svelte.ts');
    const actions = await import('./lib/actions.ts');
    const sched = await import('./lib/schedule.svelte.ts');
    const notice = await import('./lib/ruleNotice.ts');
    const chat = await import('./lib/chat.svelte.ts');
    const { resetStores } = await import('./lib/reset.ts');

    // The presenter holds the talk key to the screener's seat on Kettle FM.
    studio.view = { seats: { presenter: 'eilidh', screener: 'moira', presenter_name: 'Eilidh', screener_name: 'Moira' }, sources: [], callers: [], notices: [] } as never;
    await actions.run({ type: 'talk-seat', held: true });
    await sched.loadStationFacts();
    notice.markOwn('the-morning-kettle');
    chat.pictureOf('p1');
    await vi.waitFor(() => expect(chat.chat.pictures.p1).toBe('blob:kettle-picture'));
    expect(sched.versionRead()).toBe('1.0.3');
    expect(notice.ownMarks()).toEqual(['the-morning-kettle']);
    // The key is held, and the window says so.
    expect(actions.heldTalkKeys()).toEqual(['seat']);
    expect(studio.talking).toEqual(['moira']);

    resetStores();

    // An older station's version must not decide what a newer one is sent, or the other way.
    expect(sched.versionRead()).toBeNull();
    expect(notice.ownMarks()).toEqual([]);
    expect(actions.heldTalkKeys()).toEqual([]);
    expect(chat.chatLinkState()).toEqual({ started: false, session: null, since: null, socket: null, asked: 0 });
    expect(chat.chat.pictures).toEqual({});
    expect(revoked).toEqual(['blob:kettle-picture']);
    expect(studio.talking).toEqual([]);
  });

  it('stops the chat’s loop for good: the socket closes, and a session still on its way is dropped, not kept for the next station', async () => {
    vi.useFakeTimers();
    const sockets: { url: string; closed: boolean; sent: string[]; onopen?: () => void; onclose?: (e: { code: number; reason: string }) => void }[] = [];
    class FakeSocket {
      static OPEN = 1;
      readyState = 1;
      closed = false;
      sent: string[] = [];
      onopen?: () => void;
      onmessage?: (e: { data: string }) => void;
      onclose?: (e: { code: number; reason: string }) => void;
      constructor(readonly url: string) {
        sockets.push(this);
        queueMicrotask(() => this.onopen?.());
      }
      send(s: string) {
        this.sent.push(s);
      }
      close() {
        this.closed = true;
        this.onclose?.({ code: 1000, reason: '' });
      }
    }
    vi.stubGlobal('WebSocket', FakeSocket);
    vi.stubGlobal('location', { origin: 'https://kettle.example', protocol: 'https:', hostname: 'kettle.example', search: '' });
    let answerSession!: (r: Response) => void;
    const asked: string[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn((url: string) => {
        asked.push(String(url));
        if (String(url).endsWith('/chat/staff/session')) return new Promise<Response>((r) => (answerSession = r));
        return Promise.resolve(new Response(JSON.stringify({})));
      }),
    );
    const { account } = await import('./lib/account.svelte.ts');
    const chat = await import('./lib/chat.svelte.ts');
    const { closeLinks, resetStores } = await import('./lib/reset.ts');
    Object.assign(account, { loaded: true, me: { kind: 'staff', role: 'screener', account: { id: 'a1', name: 'Moira' } } });

    // First: a link that is up. The session arrives, the socket opens and says hello with it.
    chat.startChat();
    await vi.advanceTimersByTimeAsync(10);
    answerSession(new Response(JSON.stringify({ session: 'chat-session-kettle', me: { name: 'Moira' } })));
    await vi.advanceTimersByTimeAsync(10);
    expect(sockets).toHaveLength(1);
    expect(sockets[0].sent[0]).toContain('chat-session-kettle');
    expect(chat.chatLinkState().session).toBe('chat-session-kettle');

    closeLinks();
    expect(sockets[0].closed).toBe(true);
    expect(chat.chatLinkState()).toMatchObject({ started: false, session: null, since: null, socket: null });
    // The loop does not come back: no new socket, no new session, however long it waits.
    const before = asked.length;
    await vi.advanceTimersByTimeAsync(120_000);
    expect(sockets).toHaveLength(1);
    expect(asked.length).toBe(before);

    // Second: a stop while the session is still on its way. The late answer is dropped.
    resetStores();
    chat.startChat();
    await vi.advanceTimersByTimeAsync(10);
    closeLinks();
    answerSession(new Response(JSON.stringify({ session: 'chat-session-kettle-late', me: { name: 'Moira' } })));
    await vi.advanceTimersByTimeAsync(120_000);
    expect(chat.chatLinkState().session).toBeNull();
    expect(chat.chat.me).toBeNull();
    expect(sockets).toHaveLength(1);
    expect(JSON.stringify(sockets.map((s) => s.sent))).not.toContain('chat-session-kettle-late');
  });

  it('closes every link before a store is emptied, and one that throws keeps none of the others open', async () => {
    const { closeLinks, onLeave, onStationChange, resetStores } = await import('./lib/reset.ts');
    const done: string[] = [];
    const off = onLeave('studio events', () => done.push('events closed'));
    onLeave('broken', () => {
      throw new Error('never opened');
    });
    onLeave('later', () => done.push('later closed'));
    onStationChange('probe', () => done.push('emptied'));
    closeLinks();
    resetStores();
    expect(done).toEqual(['events closed', 'later closed', 'emptied']);
    // The frame unregisters its stream when it goes.
    off();
    done.length = 0;
    closeLinks();
    expect(done).toEqual(['later closed']);
  });
});
