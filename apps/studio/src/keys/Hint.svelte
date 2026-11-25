<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A hint beside a field's name (keyed shows, 1.3.2; the operator's click-through of
  // 2026-11-25: "inline tool tips … would help"): a small ? that says, in a tooltip, what the
  // field is for. The tooltip is the studio's one (Tip.svelte): on hover, on the keyboard's
  // focus, hidden by Esc. A press shows it too, since this button does nothing else and a touch
  // has no hover.
  import type { Snippet } from 'svelte';
  import Tip from '../../../../packages/ui/Tip.svelte';

  let { id, about, children }: { /** The tooltip's id: unique on the page. */ id: string; /** What it is about, for its name: "the kind of key". */ about: string; children: Snippet } = $props();
  let el = $state<HTMLButtonElement | null>(null);

  /** Esc hides a hint that is showing and goes no further: the hints stand in dialogs, where
   *  the same key would close the dialog and lose what is typed. With no hint showing the key
   *  is the dialog's as ever. (Caught before the tooltip's own listener hides it.) */
  function esc(e: KeyboardEvent) {
    if (e.key !== 'Escape' || !el?.ownerDocument.getElementById(id)?.classList.contains('show')) return;
    e.preventDefault();
  }
</script>

<button type="button" class="hintbtn" bind:this={el} aria-label="About {about}" aria-describedby={id} data-hint={id} onkeydowncapture={esc} onclick={() => el?.dispatchEvent(new MouseEvent('mouseenter'))}>?</button>
<Tip {id} anchor={el} delay={150}>{@render children()}</Tip>
