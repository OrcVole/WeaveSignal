// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A station without the chat service (issue 14): its chat routes answer 404, and the screener
// says the station has no chat rather than a raw error, checking again now and then.
import { describe, expect, it, vi } from 'vitest';
import * as api from './lib/api.ts';

vi.mock('./lib/api.ts', async (orig) => ({
  ...(await orig<typeof api>()),
  netFetch: vi.fn(async () => new Response(JSON.stringify({}), { status: 404, statusText: 'Not Found' })),
}));

describe('a station with no chat', () => {
  it('reads as none, not as a lost link, and says nothing raw', async () => {
    vi.useFakeTimers();
    const { chat, startChat, NO_CHAT_RETRY_MS } = await import('./lib/chat.svelte.ts');
    startChat();
    // It waits a few seconds for the signed-in name before asking for a session.
    for (let i = 0; i < 100 && chat.link !== 'none'; i++) await vi.advanceTimersByTimeAsync(200);
    expect(chat.link).toBe('none');
    expect(chat.why).toBe('');
    expect(chat.retryAt - Date.now()).toBeGreaterThan(NO_CHAT_RETRY_MS - 1_000);
    vi.useRealTimers();
  });
});
