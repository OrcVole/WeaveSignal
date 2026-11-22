<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The bank dialog (1.2.1): make a bank, or change its name and colour. Until this a show's own
  // banks were made through the station's API alone. A bank is the station's, shared by every
  // show, or one show's own; the dialog always says which it writes to, because a station tab in
  // a show's window is the wall of the show on air as well. A show's own bank can be removed
  // with its carts; the station's banks are emptied slot by slot instead. Panel name
  // `carts.bank-edit` (ADR-0070).
  //
  // Opened from the strip's button, or by the first cart added to a tab nobody has made a bank
  // for: then the cart dialog follows, with the file that was dropped still in hand.
  import { tick, untrack } from 'svelte';
  import type { Bank } from '../../../../packages/ui/carts.ts';
  import { cartEditor, cartShowName, mayChangeShow } from '../lib/cartEdit.svelte.ts';
  import { bankNameProblem, cartProblem, limitsOf, putBank, removeShowBank, type OwnerSlug } from '../lib/cartEdit.ts';
  import { reloadCatalogue, say, studio } from '../lib/studio.svelte.ts';
  import { setup } from '../lib/wizard.svelte.ts';
  import { dialogKeys } from '../schedule/rule/keys.ts';

  let dialogEl = $state<HTMLDialogElement | null>(null);
  let n = $state<number | null>(null);
  let owner = $state<OwnerSlug>(null);
  /** The bank as it was when the dialog opened; null: it is being made. */
  let orig = $state<Bank | null>(null);
  let name = $state('');
  let colour = $state(1);
  let busy = $state(false);
  let removing = $state(false);
  let errors = $state<{ name?: string; form?: string }>({});
  /** The cart that waits for the bank: its slot, and the file dropped for it. */
  let thenSlot = $state<number | null>(null);
  let thenFile = $state<File | null>(null);

  const cat = $derived(studio.catalogue);
  const show = $derived(studio.cartShow);
  const showName = $derived(cartShowName() ?? 'The show');
  const owned = (o: OwnerSlug, number: number) => cat?.banks.find((b) => b.number === number && (b.show ?? null) === o) ?? null;
  /** Making a bank on a tab nobody has made one for, in a window with a show this account may
   *  change: the dialog asks whose it is to be. */
  const choice = $derived(n !== null && !orig && show !== null && mayChangeShow() && !owned(null, n) && !owned(show, n));
  /** The station's bank of this number is here and the show has none: it can have its own. */
  const canOwn = $derived(n !== null && !!orig && owner === null && show !== null && mayChangeShow() && !owned(show, n));
  const whose = $derived(owner === null ? 'the station’s' : `${showName}’s own`);
  const held = $derived(n !== null && orig ? (cat?.carts.filter((c) => c.bank === n && (c.show ?? null) === owner).length ?? 0) : 0);

  $effect(() => {
    const ask = cartEditor.bank;
    if (!ask || !dialogEl) return;
    untrack(() => open(ask.n, ask.owner, ask.thenSlot, ask.file));
  });

  /** A show's bank takes its show's colour (ADR-0066); a station bank its own number's. */
  function defaultColour(o: OwnerSlug, number: number): number {
    const own = o === null ? null : (setup.shows.find((s) => s.slug === o)?.colour_slot ?? null);
    return own && own >= 1 && own <= 12 ? own : number;
  }

  function open(number: number, o: OwnerSlug, slot: number | null, file: File | null) {
    // The first cart on a tab with no bank: the show's own where the window has one to change.
    const first = slot !== null && !owned(null, number) && show !== null && mayChangeShow();
    n = number;
    owner = first ? show : o;
    orig = owned(owner, number);
    name = orig?.name ?? '';
    colour = orig?.colour_slot ?? defaultColour(owner, number);
    busy = false;
    removing = false;
    errors = {};
    thenSlot = slot;
    thenFile = file;
    if (!dialogEl?.open) dialogEl?.showModal();
    void tick().then(() => document.getElementById('be-name')?.focus());
  }

  function setOwner(o: OwnerSlug) {
    owner = o;
    if (n !== null && !orig) colour = defaultColour(o, n);
  }

  /** From the station's bank to the show's own of the same number: the dialog starts again. */
  function makeOwn() {
    if (n === null || show === null) return;
    open(n, show, null, null);
  }

  function leave() {
    if (busy) return;
    dialogEl?.close();
  }

  function onClose() {
    const tab = n;
    n = null;
    orig = null;
    cartEditor.bank = null;
    if (tab !== null) void tick().then(() => document.querySelector<HTMLElement>(`[data-panel="carts.bank.${tab}"]`)?.focus());
  }

  async function save() {
    if (busy || n === null || removing) return;
    const problem = bankNameProblem(name);
    errors = problem ? { name: problem } : {};
    if (problem) return document.getElementById('be-name')?.focus();
    busy = true;
    try {
      await putBank(owner, n, { name, colour_slot: colour });
      await reloadCatalogue();
      const made = !orig;
      const slot = thenSlot;
      const file = thenFile;
      const tab = n;
      busy = false;
      dialogEl?.close();
      if (made) say('good', 'Bank made', `${name.trim()} is ${whose} bank ${tab}${slot === null ? '. Its slots are empty: press a key to add a cart.' : '.'}`);
      // The cart that was waiting for the bank.
      if (slot !== null) cartEditor.slot = { bank: tab, slot, file };
    } catch (err) {
      errors = { form: cartProblem(err, limitsOf(cat), 'bank') };
      busy = false;
    }
  }

  async function remove() {
    if (busy || n === null || owner === null || !orig) return;
    busy = true;
    try {
      await removeShowBank(owner, n);
      await reloadCatalogue();
      say('good', 'Bank removed', `${orig.name} is gone with its ${held === 1 ? 'cart' : `${held} carts`}${owned(null, n) ? `; the station’s bank ${n} has its tab again.` : '.'}`);
      busy = false;
      dialogEl?.close();
    } catch (err) {
      errors = { form: cartProblem(err, limitsOf(cat), 'bank') };
      busy = false;
      removing = false;
    }
  }

  async function askRemove() {
    removing = true;
    await tick();
    document.getElementById('be-keep')?.focus();
  }

  function onKeys(e: KeyboardEvent) {
    if (!dialogEl) return;
    dialogKeys(e, dialogEl, leave, () => void save());
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<dialog
  bind:this={dialogEl}
  class="wz carts bankdlg"
  data-panel="carts.bank-edit"
  aria-labelledby="be-h"
  tabindex="-1"
  onkeydown={onKeys}
  oncancel={(e) => {
    e.preventDefault();
    leave();
  }}
  onclose={onClose}
>
  {#if n !== null}
    <div class="head">
      <h2 id="be-h" data-panel-title tabindex="-1">{orig ? `Edit bank ${n}` : `Make bank ${n}`}</h2>
      <p class="where" style="--bank-colour:var(--cart-{colour})">
        <kbd>⌃F{n}</kbd><span class="band" aria-hidden="true"></span><span data-testid="be-whose"
          >{#if choice}Whose is it to be?{:else if owner === null}The station’s bank, shared by every show.{:else}{showName}’s own bank: only its windows show it{#if owned(null, n)}, in place of the station’s bank {n}, {owned(null, n)?.name}{/if}.{/if}</span
        >
      </p>
      {#if thenSlot !== null}<p class="small">This tab has no bank yet. Make it, and the cart is added next.</p>{/if}
    </div>

    <form
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
      novalidate
      aria-busy={busy || undefined}
    >
      {#if choice && show}
        <fieldset class="field">
          <legend>Whose bank</legend>
          <div class="choices modes">
            <label>
              <input type="radio" name="be-owner" checked={owner === show} onchange={() => setOwner(show)} disabled={busy} />
              <span><b>{showName}’s own</b><span class="what">Only {showName}’s windows show it. Nobody else’s wall changes.</span></span>
            </label>
            <label>
              <input type="radio" name="be-owner" checked={owner === null} onchange={() => setOwner(null)} disabled={busy} />
              <span><b>The station’s</b><span class="what">Every show has it on this tab, the show on air included.</span></span>
            </label>
          </div>
        </fieldset>
      {/if}

      <div class="field">
        <label for="be-name">Name</label>
        <input
          id="be-name"
          type="text"
          bind:value={name}
          oninput={() => (errors = { ...errors, name: undefined })}
          maxlength="80"
          autocomplete="off"
          placeholder="Jingles"
          disabled={busy}
          aria-invalid={errors.name ? true : undefined}
          aria-describedby={errors.name ? 'be-name-err be-name-hint' : 'be-name-hint'}
        />
        <span id="be-name-hint" class="hint">Its tab shows it.</span>
        {#if errors.name}<p id="be-name-err" class="err">{errors.name}</p>{/if}
      </div>

      <div class="field">
        <span class="lab2" id="be-colour">Colour</span>
        <div class="swatches" role="group" aria-labelledby="be-colour">
          {#each Array.from({ length: 12 }, (_, i) => i + 1) as c (c)}
            <button type="button" aria-pressed={colour === c} aria-label="Colour {c}" style="--sw: var(--cart-{c})" disabled={busy} onclick={() => (colour = c)}><i></i>{c}</button>
          {/each}
        </div>
        <span class="hint">The band along the top of its tab and its carts. A show keeps one colour everywhere.</span>
      </div>

      {#if canOwn}
        <p class="note">
          <svg aria-hidden="true"><use href="#g-add" /></svg>
          <span
            >Changing this bank changes it for every show. {showName} can have a bank {n} of its own on this tab instead.
            <button type="button" class="btn sm" disabled={busy} onclick={makeOwn}>Make {showName}’s own bank {n}…</button></span
          >
        </p>
      {/if}

      {#if errors.form}<p class="err" role="alert" data-testid="be-problem"><b>Not done.</b> {errors.form}</p>{/if}

      {#if removing && orig}
        <div class="ask" role="group" aria-labelledby="be-ask">
          <b id="be-ask">Remove {orig.name}?</b>
          <span
            >{held === 0 ? 'It holds no carts.' : `Its ${held === 1 ? 'cart goes' : `${held} carts go`} with it, and their sounds are deleted from the station.`}
            {owned(null, n) ? `The station’s bank ${n} takes this tab again.` : ''} This cannot be undone.</span
          >
          <div class="acts">
            <button id="be-keep" type="button" class="btn primary" disabled={busy} onclick={() => (removing = false)}>Keep it</button>
            <button type="button" class="btn danger-hollow" disabled={busy} aria-busy={busy || undefined} onclick={remove}>Remove bank</button>
          </div>
        </div>
      {:else}
        <div class="acts">
          {#if orig && owner !== null}<button type="button" class="btn danger-hollow left" disabled={busy} onclick={askRemove}>Remove bank…</button>{/if}
          <button type="button" class="btn quiet" onclick={leave} disabled={busy}>Cancel <kbd>Esc</kbd></button>
          <button type="submit" class="btn primary" disabled={busy} aria-busy={busy || undefined}>{busy ? 'Saving…' : orig ? 'Save' : 'Make bank'} <kbd>⏎</kbd></button>
        </div>
      {/if}
    </form>
  {/if}
</dialog>
