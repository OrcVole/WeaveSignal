<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Open the player and Copy link (1.3.1; K1 and K3 of the first-sound run). The studio app
  // makes no sound: the player is a web page, at the station's public address. The two buttons
  // sit in the Desk's Listeners panel and in Settings; the rule for the address is lib/player.ts.
  // Never drawn in the sandbox, which has no station of its own to listen to.
  //
  // With no address to write the buttons stay on the page, `aria-disabled`, and the reason is
  // drawn beside them as text (a disabled control's tooltip is not read by everyone).
  import { onDestroy, onMount } from 'svelte';
  import Tip from '../../../../packages/ui/Tip.svelte';
  import { base, openInBrowser, sandboxed } from './api.ts';
  import { PLAYER, playerLink } from './player.ts';
  import { markPlayerUsed } from './playerUsed.svelte.ts';
  import { loadPublicAddress, sched } from './schedule.svelte.ts';

  /** `detail`: also say the address and, where it is the window's own, what that means (Settings). */
  let { id = 'player', detail = false }: { id?: string; detail?: boolean } = $props();

  /** The asking has finished, whatever it answered. */
  let asked = $state(false);
  /** '' until a button is pressed; then what happened, announced. */
  let said = $state('');
  /** The Open the player button, for its tooltip. */
  let openEl = $state<HTMLButtonElement | null>(null);
  /** A good answer is cleared after this long; a failure stays until the next press (design review F1). */
  const CLEAR_MS = 6000;
  let clearing: ReturnType<typeof setTimeout> | undefined;
  function say(words: string, good: boolean) {
    clearTimeout(clearing);
    said = words;
    if (good) clearing = setTimeout(() => (said = ''), CLEAR_MS);
  }
  onDestroy(() => clearTimeout(clearing));

  const link = $derived(playerLink({ publicUrl: sched.publicUrl, asked, own: base() || globalThis.location?.origin || '' }));
  const ready = $derived(link.state === 'ready');

  onMount(() => {
    if (sandboxed()) return;
    void loadPublicAddress().finally(() => (asked = true));
  });

  async function open() {
    if (!link.url) return;
    const ok = await openInBrowser(link.url);
    if (ok) markPlayerUsed();
    say(ok ? PLAYER.opened : PLAYER.notOpened, ok);
  }

  async function copy() {
    if (!link.url) return;
    try {
      await navigator.clipboard.writeText(link.url);
      markPlayerUsed();
      say(link.own ? PLAYER.copiedOwn : PLAYER.copied, true);
    } catch {
      say(PLAYER.notCopied, false);
      document.getElementById(`${id}-link`)?.focus();
    }
  }
</script>

{#if !sandboxed()}
  <div class="player-link" data-testid="player-link">
    {#if detail && link.url}<p class="addr"><span class="small">Address</span> <code>{link.url}</code></p>{/if}
    <div class="row">
      <button type="button" class="btn sm" id="{id}-open" bind:this={openEl} aria-disabled={!ready} aria-describedby={ready ? `${id}-tip` : `${id}-why ${id}-tip`} onclick={() => ready && open()}>{PLAYER.open}</button>
      <Tip id="{id}-tip" anchor={openEl} delay={0}>{PLAYER.openTip}</Tip>
      <button type="button" class="btn sm quiet" id="{id}-copy" aria-disabled={!ready} aria-describedby={ready ? undefined : `${id}-why`} onclick={() => ready && copy()}>{PLAYER.copy}</button>
    </div>
    {#if !ready}<p class="why small" id="{id}-why" role="status">{link.why}</p>{/if}
    {#if detail && ready && link.own}<p class="hint small">{PLAYER.own}</p>{/if}
    {#if link.url && said === PLAYER.notCopied}
      <!-- Copying was refused (a webview may): the link is in a box to copy by hand. -->
      <input class="box" id="{id}-link" readonly value={link.url} aria-label="The player’s address" onfocus={(e) => e.currentTarget.select()} />
    {/if}
    <p class="said small" role="status" aria-live="polite">{said}</p>
  </div>
{/if}

<style>
  .player-link { display: grid; gap: var(--s-1); min-width: 0; }
  .row { display: flex; gap: var(--s-2); flex-wrap: wrap; }
  .btn[aria-disabled='true'] { opacity: 0.55; cursor: not-allowed; }
  .addr { display: flex; gap: var(--s-2); align-items: baseline; flex-wrap: wrap; min-width: 0; margin: 0; }
  .addr code { font-family: var(--font-mono); font-size: var(--fs-13); overflow-wrap: anywhere; }
  .why, .said, .hint { margin: 0; line-height: var(--lh-label); max-width: 60rem; }
  /* Kept in the page when empty, so the live region is there before its words arrive and they are read (design review F7). */
  .said:empty { height: 0; margin-top: calc(-1 * var(--s-1)); }
  .box { font-family: var(--font-mono); font-size: var(--fs-12); width: 100%; min-width: 0; }
</style>
