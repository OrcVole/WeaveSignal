// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Where the chat's socket opens. On Cloudron the studio's own address is behind a login that a
// socket cannot answer (no Authorization header), so the socket goes to the public address; a
// Compose install keeps its own. The people are from the Scottish names list (ADR-0071).
import { afterEach, describe, expect, it, vi } from 'vitest';
import { socketOrigins, socketUrlOn } from './lib/chatSocket.ts';

const own = 'https://studio.kettle.example';
const page = 'http://localhost:5173';

describe('the chat socket’s address', () => {
  it('goes to the public address first when the station has one, its own after', () => {
    expect(socketOrigins({ chat: '', own, publicUrl: 'https://kettle.example/', page })).toEqual(['https://kettle.example', own]);
  });

  it('stays on the station’s own address with no public address', () => {
    for (const publicUrl of [null, undefined, '', '   ', 'kettle.example', 'ftp://kettle.example']) {
      expect(socketOrigins({ chat: '', own, publicUrl, page })).toEqual([own]);
    }
  });

  it('stays on one address when the public address is the station’s own', () => {
    expect(socketOrigins({ chat: '', own, publicUrl: `${own}/`, page })).toEqual([own]);
    expect(socketOrigins({ chat: '', own: `${own}/`, publicUrl: own, page })).toEqual([`${own}/`]);
  });

  it('uses the page’s own origin when the studio talks to it (a proxy), whatever the public address says', () => {
    expect(socketOrigins({ chat: '', own: '', publicUrl: 'https://kettle.example', page })).toEqual([page]);
  });

  it('lets ?chat= name the only address', () => {
    expect(socketOrigins({ chat: 'http://127.0.0.1:7311', own, publicUrl: 'https://kettle.example', page })).toEqual(['http://127.0.0.1:7311']);
  });

  it('makes a wss address from https and a ws address from http', () => {
    expect(socketUrlOn('https://kettle.example')).toBe('wss://kettle.example/api/v1/chat/socket');
    expect(socketUrlOn('http://127.0.0.1:7311/')).toBe('ws://127.0.0.1:7311/api/v1/chat/socket');
  });
});

describe('the socket the loop opens', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.resetModules();
    vi.doUnmock('./lib/api.ts');
    vi.doUnmock('./lib/schedule.svelte.ts');
  });

  /** Runs the chat's loop against a station whose own address is `own` and whose public address is
   *  `publicUrl`, with a socket that opens or not, and reports the addresses it tried. */
  async function run(publicUrl: string | null, opens: (url: string) => boolean, stopAt: number) {
    vi.useFakeTimers();
    vi.resetModules();
    const urls: string[] = [];
    const sent: string[] = [];
    class FakeSocket {
      static OPEN = 1;
      readyState = 0;
      onopen: (() => void) | null = null;
      onclose: ((e: { code: number; reason: string }) => void) | null = null;
      onmessage: ((e: { data: string }) => void) | null = null;
      constructor(readonly url: string) {
        urls.push(url);
        setTimeout(() => {
          if (opens(url)) {
            this.readyState = 1;
            this.onopen?.();
          } else {
            this.onclose?.({ code: 1006, reason: '' });
          }
        }, 10);
      }
      send(d: string) {
        sent.push(d);
      }
      close() {}
    }
    vi.stubGlobal('WebSocket', FakeSocket);
    vi.doMock('./lib/api.ts', async (orig) => ({
      ...(await orig<typeof import('./lib/api.ts')>()),
      base: () => own,
      netFetch: vi.fn(async () => new Response(JSON.stringify({ session: 'cs-1', me: { name: 'Eilidh' } }), { status: 200 })),
    }));
    vi.doMock('./lib/schedule.svelte.ts', () => ({ loadPublicAddress: vi.fn(async () => publicUrl) }));
    const { chat, startChat, stopChat } = await import('./lib/chat.svelte.ts');
    startChat();
    for (let i = 0; i < 400 && urls.length < stopAt; i++) await vi.advanceTimersByTimeAsync(200);
    await vi.advanceTimersByTimeAsync(50);
    const link = chat.link;
    stopChat();
    return { urls, sent, link };
  }

  it('opens on the public address and sends the chat session in the hello', async () => {
    const { urls, sent } = await run('https://kettle.example', () => true, 1);
    expect(urls[0]).toBe('wss://kettle.example/api/v1/chat/socket');
    expect(JSON.parse(sent[0])).toMatchObject({ t: 'hello', session: 'cs-1' });
  });

  it('opens on the station’s own address when it has no public one', async () => {
    const { urls } = await run(null, () => true, 1);
    expect(urls[0]).toBe('wss://studio.kettle.example/api/v1/chat/socket');
  });

  it('falls back to its own address when the public one never opens, and keeps the one that does', async () => {
    const { urls } = await run('https://kettle.example', (u) => u.includes('studio.'), 2);
    expect(urls.slice(0, 2)).toEqual(['wss://kettle.example/api/v1/chat/socket', 'wss://studio.kettle.example/api/v1/chat/socket']);
  });
});
