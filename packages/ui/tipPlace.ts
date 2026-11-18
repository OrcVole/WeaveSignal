// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Where a tooltip goes (StateChip's, and Tip's for a control or a word): one rule for both, so
// every tooltip in the studio lands the same way.
//
// - Under its trigger, its left edge on the trigger's, held inside the window.
// - Over the trigger when it would run off the bottom and there is room above.
// - Clear of `clear`, an element round the trigger, when one is given: under it, or over it. The
//   master lamp and the button under it are such a pair: the lamp's tooltip opens under the
//   button, not on it, and the button's goes over the lamp and the button together.
// - Beyond any other tooltip already showing in its way. In practice that is a panel's name
//   plate (ADR-0070), which a trigger that is its panel's title brings up at once, in the same
//   place: the tip's words begin where the plate ends, and both can be read. The way across is
//   the tip's own padding, so the pointer can still cross from the trigger to the tip.
//
// The tip is `position: fixed` with a vertical padding of `GAP` (its style says the same), and
// must be showing when this is called. A trigger with no size (not drawn) is left alone.

import { fixedScale } from './fixed.ts';

/** The gap between the trigger and the tip's words, in viewport pixels at no zoom. */
const GAP = 6;

/** The other tooltips showing now. Found in the page, not through the trigger's
 *  `aria-describedby`: a rendered attribute is written again when it changes, and a plate's id,
 *  added to it from outside, is then gone. */
function others(box: HTMLElement, win: Window): DOMRect[] {
  return [...box.ownerDocument.querySelectorAll<HTMLElement>('[role="tooltip"]')]
    .filter((el) => el !== box && el.getClientRects().length > 0 && win.getComputedStyle(el).visibility !== 'hidden')
    .map((el) => el.getBoundingClientRect());
}

export function placeTip(box: HTMLElement, trigger: HTMLElement, clear: HTMLElement | null = null): void {
  const win = trigger.ownerDocument.defaultView;
  if (!win) return;
  const t = trigger.getBoundingClientRect();
  if (!t.width && !t.height) return;
  const g = clear?.getBoundingClientRect();
  const under = Math.max(t.bottom, g?.bottom ?? t.bottom);
  const over = Math.min(t.top, g?.top ?? t.top);
  // Rects and the window are in viewport pixels; the tip's style is in CSS pixels, which the
  // studio's text size (zoom on <html>) scales: convert, or at 130 % the tip lands off the window.
  box.style.paddingTop = box.style.paddingBottom = '';
  const s = fixedScale(box);
  const vw = win.innerWidth;
  const vh = win.innerHeight;
  box.style.maxWidth = `min(24rem, ${(vw - 8) / s}px)`;
  let b = box.getBoundingClientRect();
  const below = !(under + b.height > vh - 4 && over - b.height >= 4);
  const left = Math.max(4, Math.min(t.left, vw - b.width - 4));
  let past = 0;
  for (const o of others(box, win)) {
    if (o.right <= left || o.left >= left + b.width) continue;
    if (below && o.bottom > under && o.top < under + b.height) past = Math.max(past, o.bottom - under);
    if (!below && o.top < over && o.bottom > over - b.height) past = Math.max(past, over - o.top);
  }
  if (past > 0) {
    box.style[below ? 'paddingTop' : 'paddingBottom'] = `${(past + GAP * s) / s}px`;
    b = box.getBoundingClientRect();
  }
  const top = below ? under : over - b.height;
  box.style.left = `${(left - b.left) / s}px`;
  box.style.top = `${(Math.max(0, top) - b.top) / s}px`;
}
