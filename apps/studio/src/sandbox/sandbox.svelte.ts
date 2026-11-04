// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The sandbox's link (ADR-0212): `sandboxFetch` answers the studio app's requests in the page,
// from the simulated station, as the control plane would; the clock drives the station and the
// sound. Only a sandbox build reaches this module (`api.ts` imports it behind the build's mode).

import type { Catalogue } from '../../../../packages/ui/carts.ts';
import type { Me } from '../lib/api.ts';
import { catalogueFrom, Refusal, Station, type PackCart } from './station.ts';
import { Sound, type Hear } from './sound.ts';
import pack from '../../../../assets/carts-demo/carts.json';

const catalogue: Catalogue = catalogueFrom(pack as { banks: Record<string, string>; carts: PackCart[] });
export const station = new Station(Date.now(), catalogue);
// For the browser test (tests/sandbox/drive.mjs) and the curious: the simulated station.
(globalThis as { __kettle?: Station }).__kettle = station;

/** What the guide shows: whether the sound has started, and which path is heard. */
export const sandbox = $state({
  started: false,
  loading: false,
  hear: 'studio' as Hear,
  /** Moves with the station, so the guide's steps follow it. */
  version: 0,
  error: '' as string,
});

let sound: Sound | null = null;

/** The visitor's first press: the sound may start only from a gesture. */
export async function begin(): Promise<void> {
  if (sandbox.started || sandbox.loading) return;
  sandbox.loading = true;
  try {
    sound ??= new Sound();
    await sound.resume();
    await sound.preload(catalogue);
    station.onLine = (sp) => void sound?.say(sp, station);
    sandbox.started = true;
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

export function pullPlayout(): void {
  station.setPlayout(false, Date.now());
  setTimeout(() => station.setPlayout(true, Date.now()), 15_000);
  push();
}

export function endShow(): void {
  station.endShow(Date.now());
  push();
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

const ME: Me = {
  kind: 'staff',
  role: 'admin',
  account: { id: 'eilidh', username: 'eilidh', name: 'Eilidh Fraser-MacLeod', role: 'admin', disabled_ms: null, created_ms: Date.UTC(2026, 9, 1), has_email: false },
};

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
  if (method === 'GET' && path === '/api/v1/carts') return json(200, catalogue);
  if (method === 'GET' && path === '/api/v1/sessions/me') return json(200, ME);
  const commands: Record<string, (b: never, now: number) => void> = {
    '/api/v1/desk/commands': (b, n) => station.desk(b, n),
    '/api/v1/screener/commands': (b, n) => station.screener(b, n),
    '/api/v1/carts/commands': (b, n) => station.carts(b, n),
    '/api/v1/chat/modes': (b, n) => station.chat(b, n),
    '/api/v1/passes/end': (b, n) => station.endPass(b, n),
  };
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
