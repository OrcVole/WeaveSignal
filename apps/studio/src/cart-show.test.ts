// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A cart wall per show (1.0.2): when the show whose own banks a window works with changes (the
// next show takes the air, or the window switches between Live and Preparing), the window
// fetches that show's catalogue, and an answer for a show it has since left is not kept.
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from './lib/api.ts';
import { setCartShow, studio, takeView } from './lib/studio.svelte.ts';
import type { Catalogue } from '../../../packages/ui/carts.ts';
import type { View } from './lib/api.ts';

vi.mock('./lib/api.ts', async (orig) => ({ ...(await orig<typeof api>()), catalogue: vi.fn() }));

const cat = (show: string | null): Catalogue => ({ version: 4, show, banks: [], carts: [], loudness_lufs: -16 });
const view = {
  now_ms: 0,
  carts: { version: 4, desk: true, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
} as unknown as View;

beforeEach(() => {
  vi.mocked(api.catalogue).mockReset();
  studio.catalogue = null;
  studio.cartShow = null;
  studio.view = null;
});

describe('the window’s cart catalogue follows its show', () => {
  it('fetches the station’s, then the show’s when the show changes', async () => {
    vi.mocked(api.catalogue).mockImplementation(async (show) => cat(show ?? null));
    takeView(view);
    await vi.waitFor(() => expect(studio.catalogue?.show).toBe(null));
    setCartShow('steeped');
    expect(api.catalogue).toHaveBeenLastCalledWith('steeped');
    await vi.waitFor(() => expect(studio.catalogue?.show).toBe('steeped'));
    // The same show again asks for nothing.
    setCartShow('steeped');
    expect(api.catalogue).toHaveBeenCalledTimes(2);
  });

  it('drops an answer for a show the window has since left', async () => {
    let answer: (c: Catalogue) => void = () => {};
    vi.mocked(api.catalogue).mockImplementationOnce(() => new Promise((r) => (answer = r)));
    vi.mocked(api.catalogue).mockImplementation(async (show) => cat(show ?? null));
    studio.view = view;
    setCartShow('the-morning-kettle');
    setCartShow('steeped');
    await vi.waitFor(() => expect(studio.catalogue?.show).toBe('steeped'));
    answer(cat('the-morning-kettle'));
    await new Promise((r) => setTimeout(r, 0));
    expect(studio.catalogue?.show).toBe('steeped');
  });
});
