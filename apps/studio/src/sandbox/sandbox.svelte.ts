// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The sandbox's link (ADR-0212): `sandboxFetch` answers the studio app's requests in the page,
// from the simulated station, as the control plane would; the clock drives the station and the
// sound. The page's hash picks the scenario (ADR-0222: `#alone`, `#queue`...; none is the
// show) or a step (`#solo`, `#guests`, `#callers`, `#full`: the chooser at that rung,
// ADR-0271), and choosing another reloads the page, so every scenario starts from a fresh
// station and a fresh sound. Only a sandbox build reaches this module (`api.ts` imports it behind the
// build's mode).

import type { Catalogue } from '../../../../packages/ui/carts.ts';
import type { Me, ShowStep } from '../lib/api.ts';
import { catalogueFor, catalogueFrom, Refusal, Station, type PackCart } from './station.ts';
import { Sound, type Hear } from './sound.ts';
import { PREPARING_REFUSAL } from '../lib/prepare.ts';
import { rungFor, scenarioFor, type Lever } from './scenarios/index.ts';
import pack from '../../../../assets/carts-demo/carts.json';

const catalogue: Catalogue = catalogueFrom(pack as { banks: Record<string, string>; carts: PackCart[] });
const hash = location.hash.replace(/^#/, '');
/** The scenario the page opened on. */
export const scenario = scenarioFor(hash);
/** The page opened on a scenario's own hash, so its start card is up (not the chooser). */
export const opened = hash !== '' && rungFor(hash) === null;
export const station = new Station(Date.now(), catalogue, scenario);
// For the browser tests (tests/sandbox/) and the curious: the simulated station.
(globalThis as { __kettle?: Station }).__kettle = station;

/** What the guide shows: whether the sound has started, which path is heard, and whether the
 *  chooser is open over a running scenario. */
export const sandbox = $state({
  started: false,
  loading: false,
  hear: 'studio' as Hear,
  /** Moves with the station, so the guide's steps follow it. */
  version: 0,
  error: '' as string,
  /** The chooser is open (over the start card, or over a scenario that runs). */
  chooser: false,
  /** The rung the chooser opens at: the step a link named, or the one pressed on the ladder. */
  rung: rungFor(hash) as ShowStep | null,
});

let sound: Sound | null = null;

/** The visitor's first press: the sound may start only from a gesture. */
export async function begin(): Promise<void> {
  if (sandbox.started || sandbox.loading) return;
  sandbox.loading = true;
  try {
    sound ??= new Sound();
    await sound.resume();
    await sound.preload(catalogue, station.lines);
    station.onLine = (sp) => void sound?.say(sp, station);
    station.begin(Date.now());
    sandbox.started = true;
    push();
  } catch (e) {
    sandbox.error = e instanceof Error ? e.message : String(e);
  } finally {
    sandbox.loading = false;
  }
}

export function hear(h: Hear): void {
  sandbox.hear = h;
  sound?.setHear(h);
}

/** Open another scenario, or the chooser (no id): a fresh page, a fresh station. */
export function choose(id: string | null): void {
  const hash = id ? `#${id}` : '';
  if (location.hash === hash) location.reload();
  else {
    location.hash = hash;
    location.reload();
  }
}

export function pullPlayout(): void {
  station.setPlayout(false, Date.now());
  setTimeout(() => station.setPlayout(true, Date.now()), 15_000);
  push();
}

export function endShow(): void {
  station.endShow(Date.now());
  push();
}

/** The microphone of the browser tab the DJ speaks through (the guide's stand-in panel). */
export function setMic(on: boolean): void {
  station.setMic(on, Date.now());
  push();
}

/** A guide step's button. */
export function pull(lever: Lever): void {
  switch (lever) {
    case 'pull-playout':
      return pullPlayout();
    case 'end-show':
      return endShow();
    case 'go-live':
      station.setMode('live', Date.now());
      return push();
    case 'go-auto':
      station.setMode('auto', Date.now());
      return push();
  }
}

// ---- the clock ----

const streams = new Set<ReadableStreamDefaultController<Uint8Array>>();
const enc = new TextEncoder();
let sent = -1;
let lastPush = 0;

function push(): void {
  const now = Date.now();
  const frame = enc.encode(`data: ${JSON.stringify(station.view(now))}\n\n`);
  for (const c of streams) {
    try {
      c.enqueue(frame);
    } catch {
      streams.delete(c);
    }
  }
  sent = station.version;
  lastPush = now;
  sandbox.version = station.version;
}

setInterval(() => {
  const now = Date.now();
  // The director waits for the visitor's first press, so nobody speaks into a silent page.
  if (sandbox.started) station.tick(now);
  if (sound) {
    station.levels = sound.levels();
    sound.follow(station);
  }
  // A change at once; the meters and the clock four times a second.
  if (station.version !== sent || now - lastPush >= 250) push();
}, 50);

// ---- the routes ----

const json = (status: number, body: unknown) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });

/** Who the studio app runs as: the chair the scenario puts you in, with an admin's keys. */
const ME: Me = {
  kind: 'staff',
  role: 'admin',
  account: { id: scenario.you.identity, username: scenario.you.identity, name: scenario.you.name, role: 'admin', disabled_ms: null, created_ms: Date.UTC(2026, 9, 1), has_email: false },
  preparing: false,
};
/** The routes a real station refuses a preparing window (ADR-0276): what changes the air. */
const ON_AIR_ROUTES = ['/api/v1/desk/commands', '/api/v1/screener/commands', '/api/v1/carts/commands', '/api/v1/passes/end', '/api/v1/show/end', '/api/v1/playout/mode'];

const NOT_HERE = 'Not in the sandbox: this part needs a real station (weavesignal.top/run/ says how to run one)';
const NO_CHAT = 'The live chat is not in the sandbox; on a real station listeners chat from the player page';

async function body(init?: RequestInit): Promise<unknown> {
  if (!init?.body) return {};
  return JSON.parse(typeof init.body === 'string' ? init.body : await new Response(init.body).text());
}

/** `fetch` for the sandbox build: the station's routes answered in the page. */
export const sandboxFetch: typeof fetch = async (input, init) => {
  const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input.href : input.url, location.href);
  const method = (init?.method ?? 'GET').toUpperCase();
  const path = url.pathname.replace(/^.*?(?=\/api\/)/, '');
  const now = Date.now();
  if (method === 'GET' && path === '/api/v1/studio/events') {
    let me: ReadableStreamDefaultController<Uint8Array>;
    const stream = new ReadableStream<Uint8Array>({
      start(c) {
        me = c;
        streams.add(c);
        c.enqueue(enc.encode(`data: ${JSON.stringify(station.view(now))}\n\n`));
      },
      cancel() {
        streams.delete(me);
      },
    });
    init?.signal?.addEventListener('abort', () => streams.delete(me));
    return new Response(stream, { status: 200, headers: { 'content-type': 'text/event-stream' } });
  }
  if (method === 'GET' && path === '/api/v1/carts') return json(200, catalogueFor(catalogue, url.searchParams.get('show') || null));
  if (method === 'GET' && path === '/api/v1/sessions/me') return json(200, ME);
  if (method === 'PUT' && path === '/api/v1/sessions/me/mode') {
    // Live or Preparing, as a real station keeps it on the session; the sandbox starts live.
    const b = (await body(init)) as { preparing?: unknown };
    if (typeof b.preparing !== 'boolean') return json(422, { error: '`preparing` is true or false' });
    ME.preparing = b.preparing;
    return json(200, ME);
  }
  if (ME.preparing && method === 'POST' && ON_AIR_ROUTES.includes(path)) return json(409, { error: PREPARING_REFUSAL });
  const commands: Record<string, (b: never, now: number) => void> = {
    '/api/v1/desk/commands': (b, n) => station.desk(b, n),
    '/api/v1/screener/commands': (b, n) => station.screener(b, n),
    '/api/v1/carts/commands': (b, n) => station.carts(b, n),
    '/api/v1/chat/modes': (b, n) => station.chat(b, n),
    '/api/v1/passes/end': (b, n) => station.endPass(b, n),
  };
  const stepOf = /^\/api\/v1\/shows\/([^/]+)\/step$/.exec(path);
  if (method === 'PUT' && stepOf) {
    // The frame's step dialog acts on the simulated show, by the station's own rules.
    try {
      const changed = station.setStep(decodeURIComponent(stepOf[1]), ((await body(init)) as { step?: unknown }).step, now);
      push();
      return json(200, changed);
    } catch (e) {
      if (e instanceof Refusal) return json(e.status, { error: e.message });
      return json(400, { error: e instanceof Error ? e.message : String(e) });
    }
  }
  if (method === 'POST' && path === '/api/v1/invites') {
    try {
      const invite = station.makeInvite((await body(init)) as { role?: unknown; name?: unknown; for_me?: unknown }, now);
      push();
      return json(201, invite);
    } catch (e) {
      if (e instanceof Refusal) return json(e.status, { error: e.message });
      return json(400, { error: e instanceof Error ? e.message : String(e) });
    }
  }
  if (method === 'POST' && path === '/api/v1/show/end') {
    // The real route takes everyone off air and hands the air to automation once the delay has
    // played out; here the scenario's goodbye is said first (`endShow`), then the same two steps.
    const onAir = station.view(now).sources.filter((s) => s.on_air);
    const wait = station.view(now).desk.delay?.delay_ms ?? 0;
    endShow();
    setTimeout(() => {
      const t = Date.now();
      for (const s of onAir) station.desk({ op: 'on_air', identity: s.identity, on: false }, t);
      station.setMode('auto', t);
      push();
    }, wait + 1000);
    return json(200, { view: station.view(now), off_air: onAir.length, automation_in_ms: wait + 1000 });
  }
  if (method === 'POST' && path === '/api/v1/playout/mode') {
    const b = (await body(init)) as { mode?: unknown };
    if (b.mode !== 'live' && b.mode !== 'auto') return json(422, { error: 'mode is `live` or `auto`' });
    station.setMode(b.mode, now);
    push();
    return json(200, station.view(now));
  }
  const run = method === 'POST' ? commands[path] : undefined;
  if (run) {
    try {
      run((await body(init)) as never, now);
      push();
      return json(200, station.view(now));
    } catch (e) {
      if (e instanceof Refusal) return json(e.status, { error: e.message });
      return json(400, { error: e instanceof Error ? e.message : String(e) });
    }
  }
  return json(503, { error: path.startsWith('/api/v1/chat/') ? NO_CHAT : NOT_HERE });
};
