<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A state chip or the master lamp, with the sentence the design gives its word as an
  // accessible tooltip (design system section 3, "The tooltip on each word").
  //
  // The tip is a role="tooltip" sibling named by aria-describedby, so the chip's own accessible
  // name stays the word. The chip takes a tab stop and shows the tip on hover and on keyboard
  // focus; Esc hides it without being consumed (the same contract as the panel name plate,
  // ADR-0070), and pointing at the tip itself keeps it open so it can be read at leisure.
  // It is placed by the rule every tooltip shares (tipPlace.ts): under the chip, or over it when
  // there is no room below, never over the chip itself, and beyond a panel's name plate. It
  // does not animate its position, and its fade takes the token --t-state, which is 0 under
  // prefers-reduced-motion. A word with no approved sentence (READY, SCREENING, the surface
  // words) is drawn as a plain chip with no tab stop.
  import { tipFor, type Chip } from './states.ts';
  import { placeTip } from './tipPlace.ts';

  interface Props {
    chip: Chip;
    /** What the surface prints in place of the chip's word (a figure, "Hearing the desk"); the
     *  tip is the chip's own. */
    word?: string;
    /** The small chip of the screener's rows and status bar. */
    sm?: boolean;
    /** Draw the master lamp (`.lamp`) instead of a chip (`.state`). */
    lamp?: boolean;
    /** Take a tab stop; false inside a listbox row, which is navigated by arrow keys. */
    focusable?: boolean;
    /** Whether the pointer can rest on the tip; false inside a button or a listbox row, where a
     *  click on the tip would press the row. */
    hoverable?: boolean;
    /** A sentence more, under the word's own: what the surface can add to the approved one (the
     *  AUTO lamp says what automation plays). It does not replace the word's sentence. */
    more?: string;
    /** A tip of the surface's own in place of the word's, for the one place where the word's
     *  sentence would be untrue (a lone cart is ON AIR; the word's tip speaks of a room). */
    tip?: string;
    /** What the tip must stay clear of besides the chip: an element round it, whose control the
     *  tip would otherwise cover (the button under the master lamp). The tip goes under it, or
     *  over it when there is no room below. */
    clear?: HTMLElement | null;
    class?: string;
    [attr: string]: unknown;
  }
  let { chip, word, sm = false, lamp = false, focusable = true, hoverable = true, more, tip: ownTip, clear = null, class: extra = '', ...rest }: Props = $props();

  const tip = $derived(ownTip ?? tipFor(chip));
  const uid = $props.id();
  const tipId = $derived(`st-tip-${uid}`);
  const shown = $derived(word ?? chip.word);
  const classes = $derived(
    [lamp ? 'lamp' : `state ${chip.cls}`, sm ? 'sm' : '', extra, tip ? 'st-trigger' : ''].filter(Boolean).join(' '),
  );

  let trigger = $state<HTMLElement>();
  let box = $state<HTMLElement>();
  let leave = 0;
  const win = $derived(trigger?.ownerDocument.defaultView ?? null);

  function show() {
    if (!tip || !box || !trigger) return;
    win?.clearTimeout(leave);
    box.classList.add('show');
    placeTip(box, trigger, clear);
    // A chip that is its panel's title shows the panel's name plate too, which may be drawn
    // after this: look again once it is, so the tip's words begin beyond it.
    const [b, t] = [box, trigger];
    win?.requestAnimationFrame(() => b.classList.contains('show') && placeTip(b, t, clear));
  }
  function hide() {
    box?.classList.remove('show');
  }
  // A short grace lets the pointer cross from the chip to the tip.
  function later() {
    win?.clearTimeout(leave);
    leave = (win ?? window).setTimeout(hide, 120);
  }
  function onkeydown(e: KeyboardEvent) {
    // Dismiss only; the key still reaches whatever handles it.
    if (e.key === 'Escape' && box?.classList.contains('show')) hide();
  }
  // A scroll or resize moves the chip from under a fixed tip.
  $effect(() => {
    if (!tip || !win) return;
    const away = () => hide();
    win.addEventListener('scroll', away, { passive: true, capture: true });
    win.addEventListener('resize', away);
    return () => {
      win.removeEventListener('scroll', away, { capture: true });
      win.removeEventListener('resize', away);
      win.clearTimeout(leave);
    };
  });
</script>

<!-- A tab stop on a word, so the keyboard reaches its tooltip (as the panel name plate's titles do). -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<span
  bind:this={trigger}
  class={classes}
  aria-describedby={tip ? tipId : undefined}
  tabindex={tip && focusable ? 0 : undefined}
  onmouseenter={show}
  onmouseleave={later}
  onfocus={show}
  onblur={hide}
  {onkeydown}
  {...rest}
>
  <svg class={chip.glyphCls} aria-hidden="true"><use href="#{chip.glyph}" /></svg>{shown}
</span>
{#if tip}
  <span bind:this={box} id={tipId} class="st-tip" class:ghost={!hoverable} role="tooltip" onmouseenter={show} onmouseleave={later}
    ><span class="st-tip-text">{tip}{#if more}<span class="st-tip-more">{more}</span>{/if}</span></span
  >
{/if}

<style>
  /* A tip is fixed, so no overflow or grid cell clips it; it is hidden with visibility, which
     keeps it in the accessibility tree for aria-describedby. */
  .st-tip {
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
  .st-tip.ghost {
    pointer-events: none;
  }
  .st-tip:global(.show) {
    visibility: visible;
    opacity: 1;
  }
  .st-tip-text {
    display: block;
    padding: var(--s-2, 0.5rem) var(--s-3, 0.75rem);
    border-radius: var(--r-1, 4px);
    background: var(--surface-3, #292b3d);
    border: 1px solid var(--axis, #3b3f57);
    color: var(--ink, #c0caf5);
    box-shadow: var(--e-2, 0 2px 6px rgba(0, 0, 0, 0.35));
  }
  .st-tip-more {
    display: block;
    margin-top: var(--s-1, 0.25rem);
  }
</style>
