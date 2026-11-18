// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The Schedule toolbar's legend gives way before it wraps (issue 28). The toolbar is one line
// at 1440 by 900 with about 30 px to spare in a short week; a week across two months (28
// September – 4 October), a wider system typeface or one more button pushed the legend onto a
// second line and took a row from the calendar. The legend now takes the widest of three
// forms that still stands beside what is before it:
//
//   words    Live · Automation fills the gaps · A date from the show's rule · Conflict · Aired, frozen
//   short    Live · Automation · From the rule · Conflict · Aired
//   glyphs   the five glyphs alone
//
// The full words stay for a screen reader in every form, and are each item's tooltip in the
// shorter two. It is tried, not worked out: each form is set and the line looked at, so the
// typeface, the text size and whatever else the toolbar holds all count without being known.
// Where even the glyphs do not fit (a window far narrower than the studio's own), the legend
// takes a line of its own with its words, as it did.

export type LegendForm = 'words' | 'short' | 'glyphs';
const FORMS: readonly LegendForm[] = ['words', 'short', 'glyphs'];

/** A Svelte action on the legend: keep `data-fit` at the widest form that stays on the line of
 *  what is before it, and tell `said` when it changes. */
export function legendFit(el: HTMLElement, said?: (form: LegendForm) => void): { destroy: () => void } {
  const bar = el.parentElement;
  if (!bar || typeof ResizeObserver === 'undefined' || typeof MutationObserver === 'undefined') return { destroy() {} };
  /** What stands before the legend: the last thing drawn there. */
  const before = (): HTMLElement | null => {
    let p = el.previousElementSibling as HTMLElement | null;
    while (p && !p.offsetWidth) p = p.previousElementSibling as HTMLElement | null;
    return p;
  };
  const beside = (): boolean => {
    const p = before();
    return !p || el.offsetTop < p.offsetTop + p.offsetHeight;
  };
  let now: LegendForm | null = null;
  const fit = () => {
    let form: LegendForm = 'words';
    for (const f of FORMS) {
      el.dataset.fit = f;
      if (beside()) {
        form = f;
        break;
      }
    }
    el.dataset.fit = form;
    if (form !== now) said?.((now = form));
  };
  // The toolbar's width (the window, the text size) and everything before the legend (the
  // range's words, the zone, a button that comes and goes) each change the room it has. The
  // legend's own size is never watched: it changes with every form tried here.
  const size = new ResizeObserver(fit);
  const watch = () => {
    size.disconnect();
    size.observe(bar);
    for (const c of bar.children) if (c !== el) size.observe(c);
  };
  const content = new MutationObserver((records) => {
    if (records.every((r) => el.contains(r.target))) return;
    watch();
    fit();
  });
  content.observe(bar, { childList: true, subtree: true, characterData: true });
  watch();
  fit();
  // The typeface may arrive after the first layout.
  void document.fonts?.ready.then(fit);
  return {
    destroy() {
      size.disconnect();
      content.disconnect();
    },
  };
}
