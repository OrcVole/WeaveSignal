<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Which microphone (1.3.3), panel `join.microphone`: the name of the one in use, and a list
  // to choose another from when the browser has more than one. A browser names its microphones
  // only after the page has been given one, so this is drawn from then on. Before joining a
  // choice is taken at once (the level bar beside it shows the result). During a call nothing
  // changes until the person presses the button under the list (`confirm`): an arrow key or a
  // scroll wheel over the list must not change what the studio hears.
  import type { Mic } from './mic-devices.ts';

  let {
    list,
    using,
    label = 'Microphone',
    caps = false,
    confirm = false,
    disabled = false,
    busy = false,
    stopped = false,
    onchoose,
  }: {
    list: Mic[];
    /** The microphone in use, as the list names it. */
    using: Mic | null;
    label?: string;
    /** The label in the call panel's small capitals. */
    caps?: boolean;
    /** Ask for a press before changing (during a call). */
    confirm?: boolean;
    disabled?: boolean;
    /** A change is under way. */
    busy?: boolean;
    /** No microphone is sending: the list asks for one. */
    stopped?: boolean;
    onchoose: (id: string) => Promise<void> | void;
  } = $props();

  const uid = $props.id();
  let select = $state<HTMLSelectElement | undefined>();
  /** The microphone picked in the list and not yet in use. */
  let picked = $state<string | null>(null);

  // The one in use is always in the list, so the list can show it selected.
  const options = $derived(using && !stopped && !list.some((m) => m.id === using.id) ? [using, ...list] : list);
  const current = $derived(stopped ? '' : (using?.id ?? ''));
  const value = $derived(picked ?? current);
  const pending = $derived(picked && picked !== current ? (options.find((m) => m.id === picked) ?? null) : null);
  const choosable = $derived(options.length > 1 || (stopped && options.length > 0));

  // A microphone picked and then unplugged is no longer a choice.
  $effect(() => {
    if (picked && !options.some((m) => m.id === picked)) picked = null;
  });

  async function take(id: string) {
    await onchoose(id);
    picked = null;
    // The list shows the microphone in use, whether or not the change happened.
    if (select) select.value = current;
    // The button that was pressed goes with the pick: the keyboard stays on the list.
    if (confirm) select?.focus();
  }

  function changed() {
    const id = select?.value ?? '';
    // One change at a time: a second pick while the browser is still asking is not taken.
    if (busy) {
      if (select) select.value = value;
      return;
    }
    if (!id || id === current) {
      picked = null;
      return;
    }
    picked = id;
    if (!confirm) void take(id);
  }
</script>

<div class="choose" data-panel="join.microphone" data-mic={using?.label ?? ''}>
  {#if choosable}
    <label for="{uid}-mic" class={caps ? 'caps' : 'label'}>{label}</label>
    <select id="{uid}-mic" bind:this={select} {value} {disabled} onchange={changed}>
      {#if value === ''}<option value="" disabled>Choose a microphone</option>{/if}
      {#each options as m (m.id)}<option value={m.id}>{m.label}</option>{/each}
    </select>
    {#if confirm && pending}
      <button type="button" class="btn wide" disabled={busy || disabled} aria-busy={busy || undefined} aria-describedby="{uid}-wait" onclick={() => take(pending.id)}
        >{busy ? 'Changing…' : `Use ${pending.label}`}</button
      >
      <p id="{uid}-wait" class="small" role="status">Nothing changes until you press it. The studio then hears the new microphone at once.</p>
    {/if}
  {:else}
    <span class={caps ? 'caps' : 'label'}>{label}</span>
    <span class="one">{stopped ? 'None connected' : (using?.label ?? '')}</span>
  {/if}
</div>

<style>
  .choose {
    display: grid;
    gap: var(--s-1);
    min-width: 0;
  }
  .label {
    font-weight: 650;
  }
  select {
    min-height: var(--target-touch);
    padding: 0 var(--s-3);
    border-radius: var(--r-2);
    border: 1px solid var(--axis);
    background: var(--plane);
    color: var(--ink);
    font: inherit;
    font-size: var(--fs-16);
    width: 100%;
    min-width: 0;
    text-overflow: ellipsis;
  }
  select:disabled {
    color: var(--muted);
  }
  .one {
    overflow-wrap: anywhere;
  }
  .btn {
    margin-top: var(--s-1);
  }
</style>
