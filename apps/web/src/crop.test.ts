// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { describe as suite, expect, it } from 'vitest';
import { MAX_ZOOM, centred, clamp, describe, dragBy, nudge, side, source, zoomTo } from './lib/chat/crop.ts';

const wide = { width: 1200, height: 800 };
const tall = { width: 600, height: 900 };

suite('the avatar crop', () => {
  it('starts with the whole shorter side, centred', () => {
    const c = centred(wide);
    expect(source(c, wide)).toEqual({ sx: 200, sy: 0, s: 800 });
    expect(source(centred(tall), tall)).toEqual({ sx: 0, sy: 150, s: 600 });
  });

  it('never leaves the picture, however far it is dragged', () => {
    const c = dragBy(centred(wide), wide, 10_000, 10_000, 280);
    expect(source(c, wide)).toEqual({ sx: 0, sy: 0, s: 800 });
    const d = dragBy(centred(wide), wide, -10_000, -10_000, 280);
    expect(source(d, wide)).toEqual({ sx: 400, sy: 0, s: 800 });
  });

  it('follows the pointer: dragging right shows more of the left', () => {
    const c = dragBy(centred(wide), wide, 28, 0, 280);
    expect(source(c, wide).sx).toBeCloseTo(200 - 80);
  });

  it('zooms about its middle and stays within the range', () => {
    const c = zoomTo(centred(wide), wide, 2);
    expect(source(c, wide)).toEqual({ sx: 400, sy: 200, s: 400 });
    expect(zoomTo(c, wide, 99).zoom).toBe(MAX_ZOOM);
    expect(zoomTo(c, wide, 0).zoom).toBe(1);
    expect(side(wide, 4)).toBe(200);
  });

  it('zooming out near an edge pulls the square back inside', () => {
    const corner = dragBy(zoomTo(centred(wide), wide, 4), wide, 10_000, 10_000, 280);
    expect(source(corner, wide)).toEqual({ sx: 0, sy: 0, s: 200 });
    expect(source(zoomTo(corner, wide, 1), wide)).toEqual({ sx: 0, sy: 0, s: 800 });
  });

  it('an arrow key moves a share of the square', () => {
    const c = nudge(zoomTo(centred(wide), wide, 2), wide, 0.05, 0);
    expect(source(c, wide).sx).toBeCloseTo(420);
    expect(nudge(centred(tall), tall, 0, -1).cy).toBe(300);
  });

  it('a bad crop is repaired, not sent', () => {
    expect(clamp({ cx: -5, cy: 9e9, zoom: 0.2 }, tall)).toEqual({ cx: 300, cy: 600, zoom: 1 });
  });

  it('says where it is for a screen reader', () => {
    expect(describe(centred(wide), wide)).toBe('Zoom 100%. Showing the middle.');
    const c = dragBy(zoomTo(centred(wide), wide, 2), wide, 10_000, 10_000, 280);
    expect(describe(c, wide)).toBe('Zoom 200%. Showing the top left.');
  });
});
