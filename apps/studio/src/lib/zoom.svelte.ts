// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The text size: the whole interface scales with Ctrl+plus, Ctrl+minus and Ctrl+0, or the
// frame's A− and A+ buttons. The level is remembered on this machine.
//
// On top of it, the interface grows with the window (the operator, 2026-11-12: "why isn't the
// GUI reactive?"). Every size in the studio is in CSS pixels or rems, so a larger window used to
// widen the panels while their text, keys and meters stayed laptop-sized. The page's zoom is now
// the text size times a window factor: 1 up to a 1920 by 1080 window (CSS pixels, so a screen the
// system already scales is not scaled twice), then the window's width or height over that,
// whichever is smaller, up to 2.5. A 2560 by 1440 window gets about 1.33, a 4K one 2. The layout
// at any size is then the one designed for 1920 by 1080, larger.

const KEY = 'weavesignal.zoom';
const MIN = 0.7;
const MAX = 2;
const STEP = 0.1;
const BASE_W = 1920;
const BASE_H = 1080;
const FIT_MAX = 2.5;

function stored(): number {
  try {
    const n = Number(globalThis.localStorage?.getItem(KEY));
    return n >= MIN && n <= MAX ? n : 1;
  } catch {
    return 1;
  }
}

/** `level` is the person's text size; `fit` the window's factor; the page's zoom is the two. */
export const zoom = $state({ level: stored(), fit: 1 });

/** The window's factor for a window this size, in CSS pixels. */
export function windowFit(width: number, height: number): number {
  if (!(width > 0 && height > 0)) return 1;
  const f = Math.min(width / BASE_W, height / BASE_H);
  return Math.round(Math.min(FIT_MAX, Math.max(1, f)) * 20) / 20;
}

// Under zoom on <html>, WebKitGTK and Chromium multiply `vh` and `vw` by the zoom: at 1.5 a
// dialog held to `100vh - 4rem` ended 465 px below a 962 px window, its foot (Make the show,
// Save) out of reach (the 1.0.4 test run; measured in WebKitGTK 4.1). So the window's own size,
// in the page's zoomed pixels, is published as `--win-w` and `--win-h`, and every size that
// means "the window" uses them, never `vh` or `vw`.
function apply(): void {
  const root = globalThis.document?.documentElement;
  if (!root) return;
  const z = Math.round(zoom.level * zoom.fit * 1000) / 1000;
  root.style.setProperty('zoom', String(z));
  const w = globalThis.innerWidth ?? 0;
  const h = globalThis.innerHeight ?? 0;
  if (w > 0 && h > 0) {
    root.style.setProperty('--win-w', `${w / z}px`);
    root.style.setProperty('--win-h', `${h / z}px`);
  }
}

export function setZoom(level: number): void {
  const n = Math.round(Math.min(MAX, Math.max(MIN, level)) * 10) / 10;
  zoom.level = n;
  apply();
  try {
    globalThis.localStorage?.setItem(KEY, String(n));
  } catch {
    /* not remembered */
  }
}

export const zoomIn = () => setZoom(zoom.level + STEP);
export const zoomOut = () => setZoom(zoom.level - STEP);
export const zoomReset = () => setZoom(1);

/** Apply the remembered level and listen for the chords; returns the listener's removal. */
export function installZoom(): () => void {
  const refit = () => {
    zoom.fit = windowFit(globalThis.innerWidth ?? 0, globalThis.innerHeight ?? 0);
    apply();
  };
  zoom.fit = windowFit(globalThis.innerWidth ?? 0, globalThis.innerHeight ?? 0);
  setZoom(zoom.level);
  globalThis.addEventListener('resize', refit);
  const on = (e: KeyboardEvent) => {
    if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
    if (e.key === '+' || e.key === '=') zoomIn();
    else if (e.key === '-' || e.key === '_') zoomOut();
    else if (e.key === '0') zoomReset();
    else return;
    e.preventDefault();
    e.stopPropagation();
  };
  globalThis.addEventListener('keydown', on, true);
  return () => {
    globalThis.removeEventListener('keydown', on, true);
    globalThis.removeEventListener('resize', refit);
  };
}
