// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

/**
 * How many viewport pixels one CSS pixel of a `position: fixed` element's `left` or `top` moves it.
 *
 * The studio's text size (A− and A+ in the frame) is CSS `zoom` on `<html>`. Under it
 * `getBoundingClientRect` and `innerWidth` answer in viewport pixels, while a fixed element's
 * `left`, `top` and `max-width` are CSS pixels that the zoom then scales. A tip placed at a rect's
 * coordinates therefore lands zoom times too far right and down, and near the window's right edge
 * runs off it (WebKitGTK 4.1, measured 2026-11-11: at 130 % a chip at x 650 put its tip at 845).
 * Divide every length written to the element's style by this figure.
 *
 * Measured rather than read from the zoom, so it holds in any engine and with no zoom at all.
 * Leaves the element at left 0, top 0.
 */
export function fixedScale(el: HTMLElement): number {
  el.style.top = '0px';
  el.style.left = '100px';
  const at100 = el.getBoundingClientRect().left;
  el.style.left = '0px';
  const at0 = el.getBoundingClientRect().left;
  const s = (at100 - at0) / 100;
  return s > 0 && Number.isFinite(s) ? s : 1;
}
