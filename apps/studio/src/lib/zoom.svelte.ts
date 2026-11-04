// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The text size: the whole interface scales with Ctrl+plus, Ctrl+minus and Ctrl+0, or the
// frame's A− and A+ buttons. The level is remembered on this machine.

const KEY = 'weavesignal.zoom';
const MIN = 0.7;
const MAX = 2;
const STEP = 0.1;

function stored(): number {
  try {
    const n = Number(globalThis.localStorage?.getItem(KEY));
    return n >= MIN && n <= MAX ? n : 1;
  } catch {
    return 1;
  }
}

export const zoom = $state({ level: stored() });

export function setZoom(level: number): void {
  const n = Math.round(Math.min(MAX, Math.max(MIN, level)) * 10) / 10;
  zoom.level = n;
  const root = globalThis.document?.documentElement;
  if (root) root.style.setProperty('zoom', String(n));
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
  setZoom(zoom.level);
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
  return () => globalThis.removeEventListener('keydown', on, true);
}
