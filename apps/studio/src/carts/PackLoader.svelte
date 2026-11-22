<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Load the default pack (1.2.1). A station loads the sounds WeaveSignal ships once, when it is
  // new; a station that already has banks never got a later release's. Here a person picks one
  // of the pack's banks (or all of them) and the station bank it goes into. The dialog says what
  // will happen before anything is asked: an empty bank is loaded directly; in a bank that
  // holds carts only the default sounds nobody changed are replaced, and every cart a person
  // added or changed stays. The button names it. Panel name `carts.pack` (ADR-0070).
  import { tick, untrack } from 'svelte';
  import { cartEditor } from '../lib/cartEdit.svelte.ts';
  import { cartProblem, limitsOf, loadPack, loadWords, packPlan, packView, planAsks, planWords, type PackPlan, type PackView } from '../lib/cartEdit.ts';
  import { say, studio } from '../lib/studio.svelte.ts';
  import { dialogKeys } from '../schedule/rule/keys.ts';

  /** `all`: every bank of the pack, each into the station's bank of its number. */
  type Pick = number | 'all';

  let dialogEl = $state<HTMLDialogElement | null>(null);
  let open_ = $state(false);
  let view = $state<PackView | null>(null);
  let pick = $state<Pick>(1);
  let into = $state(1);
  /** The station bank was chosen by hand, so picking another of the pack's banks leaves it. */
  let chose = $state(false);
  let busy = $state(false);
  let problem = $state('');

  const cat = $derived(studio.catalogue);
  const sounds = $derived(view?.banks.reduce((n, b) => n + b.carts.length, 0) ?? 0);
  const stationBank = (n: number) => cat?.banks.find((b) => (b.show ?? null) === null && b.number === n) ?? null;
  const held = (n: number) => cat?.carts.filter((c) => (c.show ?? null) === null && c.bank === n).length ?? 0;
  /** The loads asked for, and what each will do. */
  const plans = $derived.by((): PackPlan[] => {
    if (!view?.available) return [];
    const loads = pick === 'all' ? view.banks.filter((b) => b.number >= 1 && b.number <= 5).map((b) => ({ bank: b.number, into: b.number })) : [{ bank: pick, into }];
    return loads.map((l) => packPlan(view!, cat, l.bank, l.into)).filter((p): p is PackPlan => p !== null);
  });
  const loading = $derived(!!view?.loading || !!studio.view?.carts.importing);
  /** The show whose own bank has the tab the load goes under, so the person looks in the right place. */
  const under = $derived(plans.filter((p) => cat?.banks.some((b) => b.show && b.number === p.into)).map((p) => p.into));

  $effect(() => {
    const tab = cartEditor.pack;
    if (tab === null || !dialogEl) return;
    untrack(() => void open(tab));
  });

  async function open(tab: number) {
    open_ = true;
    view = null;
    problem = '';
    busy = false;
    chose = false;
    into = tab;
    pick = tab;
    if (!dialogEl?.open) dialogEl?.showModal();
    try {
      const v = await packView();
      view = v;
      // The pack's bank of the tab's number where it has one; else its first.
      if (!v.banks.some((b) => b.number === tab)) pick = v.banks[0]?.number ?? 1;
    } catch (err) {
      view = { available: false, problem: cartProblem(err, limitsOf(cat), 'bank'), loading: false, banks: [], untouched: [] };
    }
    await tick();
    (dialogEl?.querySelector<HTMLElement>('input[name="pk-bank"]:checked') ?? document.getElementById('pk-close'))?.focus();
  }

  function setPick(p: Pick) {
    pick = p;
    if (p !== 'all' && !chose) into = Math.min(5, Math.max(1, p));
  }

  function leave() {
    if (busy) return;
    dialogEl?.close();
  }

  function onClose() {
    const tab = cartEditor.pack;
    open_ = false;
    view = null;
    cartEditor.pack = null;
    if (tab !== null) void tick().then(() => document.querySelector<HTMLElement>(`[data-panel="carts.bank.${tab}"]`)?.focus());
  }

  async function load() {
    if (busy || !plans.length || loading) return;
    busy = true;
    problem = '';
    try {
      const started = await loadPack(
        plans.map((p) => ({ bank: p.bank, into: p.into })),
        plans.some(planAsks),
      );
      say('good', 'Loading the default pack', `${started.loading === 1 ? '1 sound goes' : `${started.loading} sounds go`} through the media worker one by one; the strip counts them. Nothing of yours is touched.`, 9000);
      busy = false;
      dialogEl?.close();
    } catch (err) {
      problem = cartProblem(err, limitsOf(cat), 'bank');
      busy = false;
      // What the wall holds may have moved under the dialog: read the pack again.
      void packView()
        .then((v) => (view = v))
        .catch(() => {});
    }
  }

  function onKeys(e: KeyboardEvent) {
    if (!dialogEl) return;
    dialogKeys(e, dialogEl, leave, () => void load());
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<dialog
  bind:this={dialogEl}
  class="wz mid carts packdlg"
  data-panel="carts.pack"
  aria-labelledby="pk-h"
  tabindex="-1"
  onkeydown={onKeys}
  oncancel={(e) => {
    e.preventDefault();
    leave();
  }}
  onclose={onClose}
>
  {#if open_}
    <div class="head">
      <h2 id="pk-h" data-panel-title tabindex="-1">Load the default pack</h2>
      <p class="small">
        {#if view?.available}
          The sounds WeaveSignal ships: <span class="num">{sounds}</span> in <span class="num">{view.banks.length}</span> banks. A station loads them once, when it is new. Load them again here to get a newer
          release’s, or to bring a bank back.
        {:else if view}
          The sounds WeaveSignal ships, loaded into a new station’s banks.
        {:else}
          Reading the pack…
        {/if}
      </p>
    </div>

    {#if view && !view.available}
      <p class="err" role="alert" data-testid="pk-none"><b>There is no pack to load here.</b> {view.problem ?? 'This station has no default pack.'}</p>
      <div class="acts">
        <button id="pk-close" type="button" class="btn" onclick={leave}>Close <kbd>Esc</kbd></button>
      </div>
    {:else if view}
      <form
        onsubmit={(e) => {
          e.preventDefault();
          void load();
        }}
        novalidate
        aria-busy={busy || undefined}
      >
        <fieldset class="field">
          <legend>Which of its banks</legend>
          <div class="choices">
            {#each view.banks as b (b.number)}
              <label>
                <input type="radio" name="pk-bank" checked={pick === b.number} onchange={() => setPick(b.number)} disabled={busy} />
                <span>{b.name} <span class="count num">{b.carts.length} {b.carts.length === 1 ? 'sound' : 'sounds'}</span></span>
              </label>
            {/each}
            {#if view.banks.length > 1}
              <label>
                <input type="radio" name="pk-bank" checked={pick === 'all'} onchange={() => setPick('all')} disabled={busy} />
                <span>All {view.banks.length}, each into the station’s bank of its own number</span>
              </label>
            {/if}
          </div>
        </fieldset>

        {#if pick !== 'all'}
          <div class="field">
            <label for="pk-into">Into</label>
            <select id="pk-into" bind:value={into} onchange={() => (chose = true)} disabled={busy} aria-describedby="pk-into-hint">
              {#each [1, 2, 3, 4, 5] as n (n)}
                {@const sb = stationBank(n)}
                <option value={n}>The station’s bank {n}{sb ? `, ${sb.name}` : ''} · {sb ? (held(n) ? `${held(n)} ${held(n) === 1 ? 'cart' : 'carts'}` : 'empty') : 'not made'}</option>
              {/each}
            </select>
            <span id="pk-into-hint" class="hint">A sound keeps the key the pack gives it.</span>
          </div>
        {/if}

        <div class="field">
          <span class="lab2" id="pk-what">What happens</span>
          <ul class="happens" aria-labelledby="pk-what" data-testid="pk-plan">
            {#each plans as p (p.into)}
              <li class:asks={planAsks(p)}>{planWords(p, stationBank(p.into)?.name ?? null).join(' ')}</li>
            {/each}
          </ul>
          <span class="hint">A cart you added, renamed, moved or changed in any way is yours: no load touches it.</span>
          {#if under.length}
            <p class="note">
              <svg aria-hidden="true"><use href="#g-standby" /></svg>
              <span>In this window the show’s own bank has tab {under.join(' and ')}: the station’s bank is under it, and other shows’ windows show it.</span>
            </p>
          {/if}
        </div>

        {#if loading && !busy}<p class="elsewhere" role="status">A pack is loading now. Wait until the strip has finished counting.</p>{/if}
        {#if problem}<p class="err" role="alert" data-testid="pk-problem"><b>Not done.</b> {problem}</p>{/if}

        <div class="acts">
          <button type="button" class="btn quiet" onclick={leave} disabled={busy}>Cancel <kbd>Esc</kbd></button>
          <button id="pk-load" type="submit" class="btn primary" disabled={busy || loading || !plans.length} aria-busy={busy || undefined}>{busy ? 'Starting…' : loadWords(plans)} <kbd>⏎</kbd></button>
        </div>
      </form>
    {/if}
  {/if}
</dialog>
