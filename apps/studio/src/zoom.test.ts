// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { describe, expect, it } from 'vitest';
import { windowFit } from './lib/zoom.svelte.ts';

describe('the window factor on top of the text size', () => {
  it('leaves a window up to 1920 by 1080 as designed', () => {
    expect(windowFit(1280, 720)).toBe(1);
    expect(windowFit(1920, 1080)).toBe(1);
    expect(windowFit(0, 0)).toBe(1);
  });
  it('grows with the smaller of width and height, to at most 2.5', () => {
    expect(windowFit(2560, 1440)).toBe(1.35);
    expect(windowFit(3840, 2160)).toBe(2);
    expect(windowFit(3840, 1080)).toBe(1);
    expect(windowFit(7680, 4320)).toBe(2.5);
  });
});
