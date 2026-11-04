<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The station's chat terms as it wrote them, with who answers for the chat and how to reach
  // them (ADR-0032). Empty terms are said plainly, never filled in for the station.
  import type { ChatInfo } from '../../../../../packages/ui/chat.ts';

  let { info, id }: { info: ChatInfo | null; id?: string } = $props();

  const email = $derived(/^[^\s@]+@[^\s@]+$/u.test(info?.contact.trim() ?? ''));
</script>

<div class="terms" {id}>
  {#if !info}
    <p class="small">The chat terms could not load. Try again in a moment.</p>
  {:else}
    {#if info.terms.trim()}
      <!-- A scrolling box must take keyboard focus, or a keyboard user cannot read long terms. -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div class="words" role="region" aria-label="Chat terms" tabindex="0" data-version={info.terms_version}>{info.terms}</div>
    {:else}
      <p class="small">{info.station || 'The station'} has not published its chat terms yet.</p>
    {/if}
    <dl class="small">
      {#if info.accountable_person}<dt>Who answers for the chat</dt><dd>{info.accountable_person}</dd>{/if}
      {#if info.contact}<dt>Contact</dt><dd>{#if email}<a href="mailto:{info.contact.trim()}">{info.contact}</a>{:else}{info.contact}{/if}</dd>{/if}
    </dl>
  {/if}
</div>

<style>
  .terms {
    display: grid;
    gap: var(--s-2);
  }
  .words {
    max-height: 12rem;
    overflow-y: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-size: var(--fs-14);
    line-height: var(--lh-label);
    padding: var(--s-2) var(--s-3);
    border: 1px solid var(--hairline);
    border-radius: var(--r-2);
    background: var(--surface-2);
  }
  dl {
    display: grid;
    gap: 2px;
    margin: 0;
  }
  dd + dt {
    margin-top: var(--s-1);
  }
  dt {
    font-weight: 650;
    color: var(--ink);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
</style>
