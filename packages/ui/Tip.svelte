<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A tooltip for a control or a word that is not a state chip (a chip's own is StateChip's): the
  // button under the lamp says what automation is, the frame's version says whose it is. It
  // keeps StateChip's contract, so the two read as one thing: a role="tooltip" element the
  // trigger names with aria-describedby, always in the page (hidden with visibility, so the
  // description is there for a screen reader), shown on hover and on keyboard focus, hidden by
  // Esc without taking the key, and open while the pointer rests on it. It is fixed, and placed
  // by the rule every tooltip shares (tipPlace.ts): under the trigger, or over it when there is
  // no room below, never over the trigger, clear of `clear` (the lamp over the button), and
  // beyond the panel's name plate when the trigger is its panel's title.
  //
  // Two things differ from a chip's, because the trigger may be a button: the pointer must rest
  // for `delay` before the tip shows, and a press hides it, so a button that is pressed far more
  // often than it is read does not flash its words; and focus shows it only when the keyboard
  // brought the focus (`:focus-visible`), so it does not stay up after a click.
  import type { Snippet } from 'svelte';
  import { placeTip } from './tipPlace.ts';

  interface Props {
    /** The id the trigger names in `aria-describedby`. */
    id: string;
    /** The trigger; null until it is drawn. */
    anchor: HTMLElement | null | undefined;
    /** What a tip placed over the trigger must also stay clear of: an element round the trigger. */
    clear?: HTMLElement | null;
    /** How long the pointer rests before the tip shows, in ms. */
    delay?: number;
    children: Snippet;
  }
  let { id, anchor, clear: group = null, delay = 350, children }: Props = $props();

  let box = $state<HTMLElement>();
  let wait = 0;
  let leave = 0;

  $effect(() => {
    const a = anchor;
    const win = a?.ownerDocument.defaultView;
    if (!a || !win) return;
    const clear = () => {
      win.clearTimeout(wait);
      win.clearTimeout(leave);
    };
    const show = () => {
      clear();
      const t = a.getBoundingClientRect();
      if (!box || (!t.width && !t.height)) return;
      box.classList.add('show');
      placeTip(box, a, group);
      // On the keyboard's focus the name plate may be drawn after this: look again once it is.
      win.requestAnimationFrame(() => box?.classList.contains('show') && placeTip(box, a, group));
    };
    const hide = () => {
      clear();
      box?.classList.remove('show');
    };
    const enter = () => {
      clear();
      wait = win.setTimeout(show, delay);
    };
    // A short grace lets the pointer cross from the trigger to the tip.
    const later = () => {
      clear();
      leave = win.setTimeout(hide, 120);
    };
    const focus = () => {
      if (a.matches(':focus-visible')) show();
    };
    // Dismiss only; the key still reaches whatever handles it.
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape') hide();
    };
    const tip = box;
    a.addEventListener('mouseenter', enter);
    a.addEventListener('mouseleave', later);
    a.addEventListener('focus', focus);
    a.addEventListener('blur', hide);
    a.addEventListener('pointerdown', hide);
    a.addEventListener('keydown', key);
    tip?.addEventListener('mouseenter', clear);
    tip?.addEventListener('mouseleave', later);
    // A scroll or resize moves the trigger from under a fixed tip.
    win.addEventListener('scroll', hide, { passive: true, capture: true });
    win.addEventListener('resize', hide);
    return () => {
      hide();
      a.removeEventListener('mouseenter', enter);
      a.removeEventListener('mouseleave', later);
      a.removeEventListener('focus', focus);
      a.removeEventListener('blur', hide);
      a.removeEventListener('pointerdown', hide);
      a.removeEventListener('keydown', key);
      tip?.removeEventListener('mouseenter', clear);
      tip?.removeEventListener('mouseleave', later);
      win.removeEventListener('scroll', hide, { capture: true });
      win.removeEventListener('resize', hide);
    };
  });
</script>

<span bind:this={box} {id} class="tip" role="tooltip"><span class="tip-text">{@render children()}</span></span>

<style>
  /* StateChip's tip, to the pixel: one look for every tooltip in the studio. */
  .tip {
    position: fixed;
    z-index: 70;
    left: 0;
    top: 0;
    visibility: hidden;
    opacity: 0;
    transition: opacity var(--t-state, 160ms) var(--ease-out, ease-out);
    /* the gap is padding, so the pointer can cross it without leaving the tip */
    padding: 6px 0;
    max-width: min(24rem, calc(100vw - 8px));
    width: max-content;
    box-sizing: border-box;
    text-transform: none;
    letter-spacing: 0;
    font-family: var(--font-ui, system-ui, sans-serif);
    font-size: var(--fs-13, 0.8125rem);
    font-weight: 400;
    line-height: 1.4;
    white-space: normal;
    text-align: left;
    cursor: default;
  }
  .tip:global(.show) {
    visibility: visible;
    opacity: 1;
  }
  .tip-text {
    display: grid;
    gap: var(--s-1, 0.25rem);
    padding: var(--s-2, 0.5rem) var(--s-3, 0.75rem);
    border-radius: var(--r-1, 4px);
    background: var(--surface-3, #292b3d);
    border: 1px solid var(--axis, #3b3f57);
    color: var(--ink, #c0caf5);
    box-shadow: var(--e-2, 0 2px 6px rgba(0, 0, 0, 0.35));
  }
  .tip-text :global(b) {
    font-weight: 600;
  }
</style>
