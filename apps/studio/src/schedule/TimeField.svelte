<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A schedule time with its two arrows (the operator's 1.0.5 click-through): the studio's own
  // webview on Linux, WebKitGTK, draws a time field with no arrows, so a time could only be
  // typed. Every time the schedule asks for is this field: an episode's Move…, a date's Move…
  // in a show's dates and in the rule dialog, and the rule's from and until.
  //
  // A press moves the time to the quarter-hour before or after it (`time.ts`). The field is
  // still typed in for an odd minute, and ↑ ↓ in it stay the browser's own, a segment at a
  // time. The arrows are real buttons with their names, but not stops for Tab: the keys already
  // step the field, and two more stops on every time would stand between a time and its Move.
  // A press leaves the focus in the field, so ⏎ and Esc go on to mean what the row says.
  import { stepTime, stepWords } from './time.ts';

  let {
    value = $bindable(''),
    id,
    label,
    invalid = false,
    describedby,
    disabled = false,
    onchange,
    oncarry,
  }: {
    /** `HH:MM`, or empty while the field is not a time. */
    value?: string;
    id?: string;
    /** The field's name where no `<label for>` gives it one. */
    label?: string;
    invalid?: boolean;
    describedby?: string;
    disabled?: boolean;
    /** The time was typed or stepped: for a host that keeps the value itself. */
    onchange?: (value: string) => void;
    /** A step went past midnight: the day after (1) or before (-1), for a date beside it. */
    oncarry?: (days: 1 | -1) => void;
  } = $props();

  let field = $state<HTMLInputElement | null>(null);

  function step(dir: 1 | -1) {
    const next = stepTime(value, dir);
    value = next.time;
    if (next.carry) oncarry?.(next.carry);
    onchange?.(next.time);
    if (document.activeElement !== field) field?.focus();
  }
</script>

{#snippet arrow(dir: 1 | -1)}
  <!-- the press does not take the focus out of the field -->
  <button type="button" class={dir > 0 ? 'later' : 'earlier'} tabindex="-1" {disabled} aria-label={stepWords(dir)} title={stepWords(dir)} onmousedown={(e) => e.preventDefault()} onclick={() => step(dir)}>
    <svg aria-hidden="true"><use href="#g-up" /></svg>
  </button>
{/snippet}

<span class="timefield">
  <input
    bind:this={field}
    {id}
    class="time"
    type="time"
    bind:value
    {disabled}
    aria-label={label}
    aria-invalid={invalid ? true : undefined}
    aria-describedby={describedby}
    onchange={(e) => onchange?.((e.currentTarget as HTMLInputElement).value)}
  />
  <span class="steps">{@render arrow(-1)}{@render arrow(1)}</span>
</span>
