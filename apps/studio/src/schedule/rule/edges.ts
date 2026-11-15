// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A column that scrolls inside a dialog says so at its edges. On a short window the questions
// of When it airs scroll by themselves beside the preview, and nothing showed that First on had
// gone out of sight above, or that Ends waited below. This marks the column with `data-more`
// (`above`, `below`, or both), and the style sheet fades that edge. A column that holds
// everything is left alone.

/** A Svelte action: keep `data-more` on a scrolling element in step with what is out of view. */
export function edges(el: HTMLElement): { destroy: () => void } {
  const set = () => {
    const more = el.scrollHeight - el.clientHeight > 1;
    const above = more && el.scrollTop > 1;
    const below = more && el.scrollTop + el.clientHeight < el.scrollHeight - 1;
    const now = [above ? 'above' : '', below ? 'below' : ''].filter(Boolean).join(' ');
    if (now) {
      if (el.dataset.more !== now) el.dataset.more = now;
    } else if (el.dataset.more !== undefined) delete el.dataset.more;
  };
  el.addEventListener('scroll', set, { passive: true });
  // The column's own size (the window, the text size) and what it holds (Ends appears, a date
  // is skipped) both change what is out of view.
  const size = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(set);
  size?.observe(el);
  const content = typeof MutationObserver === 'undefined' ? null : new MutationObserver(set);
  content?.observe(el, { childList: true, subtree: true, characterData: true });
  set();
  return {
    destroy() {
      el.removeEventListener('scroll', set);
      size?.disconnect();
      content?.disconnect();
    },
  };
}
