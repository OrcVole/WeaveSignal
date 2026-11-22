<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The cart dialog (1.2.1; cart-wall.md section 6, "editing a cart", built): add a cart to an
  // empty slot, or change the one a slot holds. A sound is chosen or dropped; the station's media
  // worker reads it, measures it and levels it, and its sentence is shown when it refuses one.
  // A cart's name, second line, mode, ramp, credit and key are changed without sending its sound
  // again; replacing the sound and removing the cart each ask first. Panel name `carts.edit`
  // (ADR-0070).
  //
  // A modal dialog owns the keyboard (keymap rule 5): ⏎ saves, the first Esc leaves a text field
  // and the second closes. While a file is on its way Cancel stops waiting, so a presenter on air
  // is never held here: the wall's keys are theirs again at once.
  import { tick, untrack } from 'svelte';
  import { cartKey } from '../../../../packages/ui/keymap.ts';
  import { bankAt, cartAt, type Cart, type CartMode } from '../../../../packages/ui/carts.ts';
  import { cartEditor, cartShowName } from '../lib/cartEdit.svelte.ts';
  import {
    ACCEPT,
    MODE_CHOICES,
    addedWords,
    bankWords,
    cartProblem,
    changeOf,
    emptySlots,
    fileProblem,
    labelFromFile,
    labelProblem,
    limitsOf,
    patchCart,
    putCart,
    rampMs,
    rampText,
    removeCart,
    sizeWords,
    soundLine,
    takesLine,
  } from '../lib/cartEdit.ts';
  import { listenForDrops } from '../lib/cartDrop.ts';
  import { sandboxed } from '../lib/api.ts';
  import { signed } from '../lib/format.ts';
  import { reloadCatalogue, say, studio } from '../lib/studio.svelte.ts';
  import { dialogKeys } from '../schedule/rule/keys.ts';

  type Phase = '' | 'sending' | 'saving' | 'removing';

  let dialogEl = $state<HTMLDialogElement | null>(null);
  let input = $state<HTMLInputElement | null>(null);

  /** The slot this dialog is open on, and the cart it held when it opened (null: adding). */
  let at = $state<{ bank: number; slot: number } | null>(null);
  let orig = $state<Cart | null>(null);
  let file = $state<File | null>(null);
  let label = $state('');
  /** The name was typed, so another file chosen does not write over it. */
  let named = $state(false);
  let detail = $state('');
  let mode = $state<CartMode>('one_shot');
  let ramp = $state('');
  let licence = $state('');
  let slot = $state(1);
  let phase = $state<Phase>('');
  let errors = $state<{ file?: string; label?: string; ramp?: string; form?: string }>({});
  let removing = $state(false);
  let over = $state(false);
  let stop: AbortController | null = null;

  const busy = $derived(phase !== '');
  const cat = $derived(studio.catalogue);
  const limits = $derived(limitsOf(cat));
  const bank = $derived(at ? bankAt(cat, at.bank) : null);
  const owner = $derived(bank?.show ?? null);
  const where = $derived(at ? bankWords(bank, at.bank, cartShowName()) : '');
  const missing = $derived(!!orig && (orig.missing || !orig.file || (studio.view?.carts.missing.includes(orig.id) ?? false)));
  /** The keys this cart can move to: where it is, and every empty slot of its bank. */
  /* A set: once the cart has moved or gone, the wall read again lists its old slot as empty too. */
  const keys = $derived(at && orig ? [...new Set([orig.slot, ...emptySlots(cat, at.bank)])].sort((a, b) => a - b) : []);
  const target = $derived(limits.loudnessLufs === null ? '' : `${signed(limits.loudnessLufs)} LUFS`);

  const working = $derived(
    phase === 'sending' && file
      ? `Sending “${file.name}” (${sizeWords(file.size)}). The station reads it, measures it and levels it${target ? ` to ${target}` : ''}; a long sound takes a few seconds.`
      : phase === 'saving'
        ? 'Saving…'
        : phase === 'removing'
          ? 'Removing…'
          : '',
  );

  // The wall asks; the dialog opens on what the slot holds now.
  $effect(() => {
    const ask = cartEditor.slot;
    if (!ask || !dialogEl) return;
    untrack(() => open(ask.bank, ask.slot, ask.file));
  });

  function open(b: number, s: number, dropped: File | null) {
    const cart = cartAt(studio.catalogue, b, s);
    at = { bank: b, slot: s };
    orig = cart;
    file = null;
    label = cart?.label ?? '';
    named = !!cart;
    detail = cart?.detail ?? '';
    mode = cart?.mode ?? 'one_shot';
    ramp = rampText(cart?.ramp_ms);
    licence = cart?.licence ?? '';
    slot = s;
    phase = '';
    errors = {};
    removing = false;
    over = false;
    if (dropped) take(dropped);
    if (!dialogEl?.open) dialogEl?.showModal();
    // A cart to change starts on its name; a slot to fill starts on the chooser.
    void tick().then(() => (cart || file ? document.getElementById('ce-label') : document.getElementById('ce-choose'))?.focus());
  }

  /** A file chosen or dropped: checked before it is kept, and its name offered as the cart's. */
  function take(f: File) {
    const problem = fileProblem(f, limits);
    if (problem) {
      errors = { ...errors, file: problem };
      return;
    }
    file = f;
    errors = { ...errors, file: undefined, form: undefined };
    if (!named) label = labelFromFile(f.name);
  }

  function choose(e: Event) {
    const f = (e.currentTarget as HTMLInputElement).files?.[0];
    if (f) take(f);
    if (input) input.value = '';
  }

  // A drop anywhere in the window is a drop on the dialog, which is all there is to drop on
  // while it is open.
  $effect(() => {
    if (!at) return;
    return listenForDrops(
      (e) => {
        if (e.kind !== 'drop') {
          over = e.kind === 'over' && !untrack(() => busy);
          return;
        }
        over = false;
        if (untrack(() => busy)) return;
        if (e.problem) errors = { ...errors, file: e.problem };
        else if (e.file) take(e.file);
      },
      () => untrack(() => limits.maxMb),
    );
  });

  /** Close: asked by Cancel, the second Esc and a finished save. While a file is on its way it
   *  stops waiting for the station instead. */
  function leave() {
    if (phase === 'sending' && stop) {
      stop.abort();
      return;
    }
    if (busy) return;
    dialogEl?.close();
  }

  function onClose() {
    const back = at ? slot : null;
    at = null;
    orig = null;
    file = null;
    cartEditor.slot = null;
    // The focus goes back to the pad, wherever the press came from: its key fires it again.
    if (back !== null) void tick().then(() => document.querySelector<HTMLElement>(`[data-cart-slot="${back}"]`)?.focus());
  }

  async function save() {
    // While Remove asks, ⏎ is its Keep it: nothing is saved behind the question.
    if (busy || !at || removing) return;
    const duration = file ? null : (orig?.duration_ms ?? null);
    const r = rampMs(ramp, duration);
    const next: typeof errors = {};
    if (!orig && !file) next.file = 'Choose the sound first: a cart is a sound on a key.';
    const lp = labelProblem(label);
    if (lp) next.label = lp;
    if (r === 'bad') next.ramp = 'A ramp is a number of seconds, such as 12 or 4.5. Leave it empty for none.';
    if (r === 'long') next.ramp = 'The ramp is longer than the cart. A ramp ends before its cart does.';
    errors = next;
    if (next.file) return document.getElementById('ce-choose')?.focus();
    if (next.label) return document.getElementById('ce-label')?.focus();
    if (next.ramp || r === 'bad' || r === 'long') return document.getElementById('ce-ramp')?.focus();
    const words = { label, detail, mode, ramp_ms: r, licence };
    try {
      if (file) {
        phase = 'sending';
        stop = new AbortController();
        const made = await putCart(owner, at.bank, orig?.slot ?? at.slot, { ...words, origin: file.name }, file, stop.signal);
        // Its sound replaced and its key changed in one save: the new cart moves.
        const moved = orig && slot !== orig.slot ? await patchCart(owner, at.bank, orig.slot, { slot }) : made;
        const said = addedWords(moved, !!orig, limits.loudnessLufs);
        say('good', said.title, said.message, 9000);
      } else if (orig) {
        const change = changeOf(orig, words, slot);
        if (Object.keys(change).length) {
          phase = 'saving';
          const saved = await patchCart(owner, at.bank, orig.slot, change);
          say('good', 'Cart saved', `“${saved.label}” is on ${cartKey(saved.slot)}.`);
        }
      }
      await reloadCatalogue();
      phase = '';
      dialogEl?.close();
    } catch (err) {
      const stopped = err instanceof DOMException && err.name === 'AbortError';
      errors = {
        form: stopped ? 'Stopped. If the station already had the whole file, the cart appears on its key in a moment; otherwise nothing has changed.' : cartProblem(err, limits, file ? 'send' : 'change'),
      };
      phase = '';
      // What the station holds may have moved under the dialog (a slot filled from another window).
      void reloadCatalogue();
      await tick();
      document.getElementById('ce-problem')?.focus();
    } finally {
      stop = null;
    }
  }

  async function remove() {
    if (busy || !at || !orig) return;
    phase = 'removing';
    try {
      await removeCart(owner, at.bank, orig.slot);
      say('good', 'Cart removed', `“${orig.label}” is off ${cartKey(orig.slot)}, and its sound is deleted from the station.`);
      await reloadCatalogue();
      phase = '';
      slot = orig.slot;
      dialogEl?.close();
    } catch (err) {
      errors = { form: cartProblem(err, limits, 'remove') };
      phase = '';
      removing = false;
    }
  }

  async function askRemove() {
    removing = true;
    await tick();
    // Keep it has the focus: ⏎ by reflex removes nothing.
    document.getElementById('ce-keep')?.focus();
  }

  function onKeys(e: KeyboardEvent) {
    if (!dialogEl) return;
    dialogKeys(e, dialogEl, leave, () => void save());
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<dialog
  bind:this={dialogEl}
  class="wz mid carts"
  class:over
  data-panel="carts.edit"
  aria-labelledby="ce-h"
  tabindex="-1"
  onkeydown={onKeys}
  oncancel={(e) => {
    e.preventDefault();
    leave();
  }}
  onclose={onClose}
>
  {#if at}
    <div class="head">
      <h2 id="ce-h" data-panel-title tabindex="-1">{orig ? 'Edit cart' : 'Add a cart'}</h2>
      <p class="where" style="--bank-colour:{bank ? `var(--cart-${bank.colour_slot})` : 'var(--deemph)'}">
        <kbd>{cartKey(orig?.slot ?? at.slot)}</kbd><span class="band" aria-hidden="true"></span><span>in {where}</span>
      </p>
    </div>

    <form
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
      novalidate
      aria-busy={busy || undefined}
    >
      <div class="field">
        <span class="lab2" id="ce-sound">Sound</span>
        {#if orig}
          <p class="has" data-testid="ce-has">
            {#if missing}<span class="state hollow hollow-critical sm"><svg class="g-critical" aria-hidden="true"><use href="#g-fault" /></svg>File missing</span>{/if}
            <span class="num">{soundLine(orig, limits.loudnessLufs)}</span>{#if orig.origin}<span class="from"> · from {orig.origin}</span>{/if}
          </p>
          {#if missing && !file}<p class="hint ink">Its sound is gone from the station, so its key does nothing. Choose the sound again to put it back.</p>{/if}
        {/if}
        <div class="pickfile" class:over class:chosen={!!file}>
          {#if file}
            <svg class="g" aria-hidden="true"><use href="#g-tick" /></svg>
            <span class="picked" data-testid="ce-file"
              ><b>{file.name}</b><span class="num"> · {sizeWords(file.size)}</span>{#if orig}<span class="swap"> · replaces the sound when you save</span>{/if}</span
            >
            <button id="ce-choose" type="button" class="btn sm" disabled={busy} onclick={() => input?.click()}>Choose another…</button>
            {#if orig}<button type="button" class="btn quiet sm" disabled={busy} onclick={() => (file = null)}>Keep the old sound</button>{/if}
          {:else}
            <svg class="g" aria-hidden="true"><use href="#g-drop" /></svg>
            <button id="ce-choose" type="button" class="btn {orig ? '' : 'primary'}" disabled={busy} aria-describedby={errors.file ? 'ce-file-err ce-takes ce-dropline' : 'ce-takes ce-dropline'} onclick={() => input?.click()}
              >{orig ? 'Replace the sound…' : 'Choose a sound…'}</button
            >
            <span id="ce-dropline" class="dropline">{over ? 'Let go to use this sound.' : 'or drop one anywhere on this window'}</span>
          {/if}
          <span id="ce-takes" class="hint takes">{takesLine(limits)}.{#if target}{' '}The station measures it, so every cart plays at {target}.{/if}</span>
        </div>
        {#if errors.file}<p id="ce-file-err" class="err" role="alert" data-testid="ce-file-err">{errors.file}</p>{/if}
        {#if sandboxed()}<p class="hint sandbox">In the sandbox the sound stays in this page: nothing is sent anywhere, and it is gone when the page is reloaded.</p>{/if}
        <input bind:this={input} type="file" accept={ACCEPT} aria-label={orig ? 'Choose the sound that replaces this cart’s' : 'Choose the cart’s sound'} onchange={choose} disabled={busy} hidden />
      </div>

      <div class="field">
        <label for="ce-label">Name</label>
        <input
          id="ce-label"
          type="text"
          bind:value={label}
          oninput={() => {
            named = true;
            errors = { ...errors, label: undefined };
          }}
          maxlength="120"
          autocomplete="off"
          disabled={busy}
          aria-invalid={errors.label ? true : undefined}
          aria-describedby={errors.label ? 'ce-label-err ce-label-hint' : 'ce-label-hint'}
        />
        <span id="ce-label-hint" class="hint">As the pad shows it: two lines at the most.</span>
        {#if errors.label}<p id="ce-label-err" class="err">{errors.label}</p>{/if}
      </div>

      <fieldset class="field">
        <legend>Plays as</legend>
        <div class="choices modes">
          {#each MODE_CHOICES as m (m.mode)}
            <label>
              <input type="radio" name="ce-mode" value={m.mode} bind:group={mode} disabled={busy} />
              <span><b>{m.name}</b><span class="what">{m.what}</span></span>
            </label>
          {/each}
        </div>
      </fieldset>

      <div class="two">
        <div class="field">
          <label for="ce-detail">Second line <span class="opt">optional</span></label>
          <input id="ce-detail" type="text" bind:value={detail} maxlength="120" autocomplete="off" disabled={busy} aria-describedby="ce-detail-hint" />
          <span id="ce-detail-hint" class="hint">An intro’s artist, say.</span>
        </div>
        <div class="field">
          <label for="ce-licence">Credit or licence <span class="opt">optional</span></label>
          <input id="ce-licence" type="text" bind:value={licence} maxlength="80" autocomplete="off" disabled={busy} aria-describedby="ce-licence-hint" />
          <span id="ce-licence-hint" class="hint">Who made it, or what lets you play it.</span>
        </div>
      </div>

      <div class="two">
        <div class="field">
          <label for="ce-ramp">Ramp to the vocal <span class="opt">optional</span></label>
          <span class="t"
            ><input id="ce-ramp" type="text" inputmode="decimal" class="n" bind:value={ramp} oninput={() => (errors = { ...errors, ramp: undefined })} autocomplete="off" disabled={busy} aria-invalid={errors.ramp ? true : undefined} aria-describedby={errors.ramp ? 'ce-ramp-err ce-ramp-hint' : 'ce-ramp-hint'} /> seconds</span
          >
          <span id="ce-ramp-hint" class="hint">For an intro: the pad says how long before the singing starts.</span>
          {#if errors.ramp}<p id="ce-ramp-err" class="err">{errors.ramp}</p>{/if}
        </div>
        {#if orig}
          <div class="field">
            <label for="ce-key">Key</label>
            <select id="ce-key" bind:value={slot} disabled={busy || keys.length < 2} aria-describedby="ce-key-hint">
              {#each keys as k (k)}<option value={k}>{cartKey(k)}{k === orig.slot ? ' (where it is)' : ''}</option>{/each}
            </select>
            <span id="ce-key-hint" class="hint">Its place on the wall is its key. Only this bank’s empty slots are offered.</span>
          </div>
        {/if}
      </div>

      {#if working}
        <div class="working" role="status" aria-live="polite" data-testid="ce-working"><span class="bar" aria-hidden="true"><i></i></span><span>{working}</span></div>
      {/if}
      {#if errors.form}<p id="ce-problem" class="err" role="alert" tabindex="-1" data-testid="ce-problem"><b>Not done.</b> {errors.form}</p>{/if}

      {#if removing && orig}
        <div class="ask" role="group" aria-labelledby="ce-ask">
          <b id="ce-ask">Remove “{orig.label}” from {cartKey(orig.slot)}?</b>
          <span>The cart goes from {where}, and its sound is deleted from the station. This cannot be undone.</span>
          <div class="acts">
            <button id="ce-keep" type="button" class="btn primary" disabled={busy} onclick={() => (removing = false)}>Keep it</button>
            <button type="button" class="btn danger-hollow" disabled={busy} aria-busy={phase === 'removing' || undefined} onclick={remove}>Remove cart</button>
          </div>
        </div>
      {:else}
        <div class="acts">
          {#if orig}<button type="button" class="btn danger-hollow left" disabled={busy} onclick={askRemove}>Remove cart…</button>{/if}
          <button type="button" class="btn quiet" onclick={leave} disabled={busy && phase !== 'sending'}>{phase === 'sending' ? 'Stop' : 'Cancel'} <kbd>Esc</kbd></button>
          <button type="submit" class="btn primary" disabled={busy} aria-busy={busy || undefined}>{phase === 'sending' ? 'Sending…' : orig ? 'Save' : 'Add cart'} <kbd>⏎</kbd></button>
        </div>
      {/if}
    </form>
  {/if}
</dialog>
