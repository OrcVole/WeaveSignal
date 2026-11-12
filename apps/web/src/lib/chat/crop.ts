// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The avatar crop, as pure functions (ADR-0235: "permit the avatar to be centred/zoomed to get a
// good fit"). A crop is a square of the picture: where its middle is, in the picture's own
// pixels, and how far it is zoomed in. At zoom 1 the square is as large as the picture allows
// (its shorter side); it never leaves the picture, so nothing empty is ever sent. No DOM here,
// so it is tested on its own (src/crop.test.ts).

export interface Crop {
  /** The square's middle, in the picture's pixels. */
  cx: number;
  cy: number;
  /** 1 shows the whole shorter side; MAX_ZOOM the closest. */
  zoom: number;
}

export interface Size {
  width: number;
  height: number;
}

export const MIN_ZOOM = 1;
export const MAX_ZOOM = 4;
/** One press of + or −, or one notch of the slider. */
export const ZOOM_STEP = 0.1;
/** The side of the square the page sends: the station keeps 128, so twice that is plenty. */
export const SEND_SIDE = 256;
/** The ground a transparent picture is flattened on (the station's own for an avatar). */
export const GROUND = '#e9eaee';

const clampTo = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** The square's side in the picture's pixels. */
export function side(img: Size, zoom: number): number {
  return Math.min(img.width, img.height) / clampTo(zoom, MIN_ZOOM, MAX_ZOOM);
}

/** A crop kept inside the picture and inside the zoom range. */
export function clamp(c: Crop, img: Size): Crop {
  const zoom = clampTo(c.zoom, MIN_ZOOM, MAX_ZOOM);
  const half = side(img, zoom) / 2;
  return { zoom, cx: clampTo(c.cx, half, img.width - half), cy: clampTo(c.cy, half, img.height - half) };
}

/** The whole shorter side, centred: where every picture starts, and what "Centre" returns to. */
export function centred(img: Size): Crop {
  return { cx: img.width / 2, cy: img.height / 2, zoom: 1 };
}

/** Zoom to `zoom`, keeping the same point of the picture in the middle. */
export function zoomTo(c: Crop, img: Size, zoom: number): Crop {
  return clamp({ ...c, zoom }, img);
}

/** Move the picture under the square by a drag of `dx`, `dy` CSS pixels on a stage `stage`
 *  pixels wide: the picture follows the pointer, so the square's middle goes the other way. */
export function dragBy(c: Crop, img: Size, dx: number, dy: number, stage: number): Crop {
  const k = side(img, c.zoom) / stage;
  return clamp({ ...c, cx: c.cx - dx * k, cy: c.cy - dy * k }, img);
}

/** Move the square by a share of its own side (an arrow key: 0.05; with Shift: 0.2). */
export function nudge(c: Crop, img: Size, right: number, down: number): Crop {
  const s = side(img, c.zoom);
  return clamp({ ...c, cx: c.cx + right * s, cy: c.cy + down * s }, img);
}

/** The rectangle of the picture the square covers: what `drawImage` takes as its source. */
export function source(c: Crop, img: Size): { sx: number; sy: number; s: number } {
  const k = clamp(c, img);
  const s = side(img, k.zoom);
  return { sx: k.cx - s / 2, sy: k.cy - s / 2, s };
}

/** What a screen reader hears after a move or a zoom: "Zoom 150%. Showing the middle." */
export function describe(c: Crop, img: Size): string {
  const k = clamp(c, img);
  const half = side(img, k.zoom) / 2;
  const pos = (v: number, len: number, lo: string, hi: string) => {
    const room = len - 2 * half;
    if (room < 1) return '';
    const t = (v - half) / room;
    return t < 0.34 ? lo : t > 0.66 ? hi : '';
  };
  const v = pos(k.cy, img.height, 'top', 'bottom');
  const h = pos(k.cx, img.width, 'left', 'right');
  const where = [v, h].filter(Boolean).join(' ') || 'middle';
  return `Zoom ${Math.round(k.zoom * 100)}%. Showing the ${where}.`;
}
