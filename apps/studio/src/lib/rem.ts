// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// One rem in the pixels `getBoundingClientRect` gives. The page is zoomed on <html> (the text
// size times the window's factor, zoom.svelte.ts), and a measured box and a rem are only
// comparable when both are measured the same way: a container query cannot do it, because it
// multiplies its rem by the zoom and measures the container without it (the 1.0.4 test run).

export function remPx(doc: Document = document): number {
  const p = doc.createElement('div');
  p.style.cssText = 'position:fixed;left:0;top:0;width:1rem;height:0;visibility:hidden;pointer-events:none';
  doc.body.append(p);
  const w = p.getBoundingClientRect().width;
  p.remove();
  return w || 16;
}
