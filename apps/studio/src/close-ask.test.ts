// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Closing the window while the bridge runs (Phase 17g): the question, keyboard first, and the
// commands its two answers send to Rust (which decides when to ask: src-tauri/src/bridge.rs).

import { describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import CloseAsk from './lib/CloseAsk.svelte';
import { CLOSE_ASKED, CLOSE_WORDS, keepOpen, onCloseAsked, stopAndClose } from './lib/closeAsk.ts';
import { PANELS } from '../../../packages/ui/panels.ts';

const invoked: string[] = [];
let heard: ((e: unknown) => void) | null = null;
vi.mock('@tauri-apps/api/core', () => ({ invoke: async (cmd: string) => void invoked.push(cmd) }));
vi.mock('@tauri-apps/api/event', () => ({
  listen: async (name: string, f: (e: unknown) => void) => {
    expect(name).toBe('close-asked');
    heard = f;
    return () => (heard = null);
  },
}));

describe('closing the window while the bridge runs', () => {
  it('asks plainly, with staying first and in focus', () => {
    const { body } = render(CloseAsk, { props: { open: true } });
    expect(body).toContain('data-panel="studio.close-ask"');
    expect(body).toContain(CLOSE_WORDS.title);
    expect(body).toContain('your source leaves the desk');
    const keep = body.indexOf('data-testid="close-keep"');
    const stop = body.indexOf('data-testid="close-stop"');
    expect(keep).toBeGreaterThan(-1);
    expect(stop).toBeGreaterThan(keep);
    expect(body.slice(body.lastIndexOf('<button', keep), stop)).toContain('autofocus');
    expect(body).toContain(CLOSE_WORDS.keep);
    expect(body).toContain(CLOSE_WORDS.stop);
    // Both are buttons a keyboard reaches; nothing is drawn unless asked.
    expect(body.match(/<button type="button"/g)?.length).toBe(2);
    expect(render(CloseAsk).body).not.toContain('studio.close-ask');
  });

  it('answers Rust with one command each, and hears its question', async () => {
    await keepOpen();
    await stopAndClose();
    expect(invoked).toEqual(['window_close_keep', 'window_close_stop']);
    let asked = 0;
    const stop = await onCloseAsked(() => asked++);
    heard?.({});
    expect(asked).toBe(1);
    stop();
    expect(heard).toBeNull();
    expect(CLOSE_ASKED).toBe('close-asked');
  });

  it('has its panel name in the registry', () => {
    expect(PANELS['studio.close-ask']).toMatch(/bridge/);
  });
});
