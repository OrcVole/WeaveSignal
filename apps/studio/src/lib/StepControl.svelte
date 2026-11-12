<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The step control in the frame (ADR-0271, ADR-0274): the show's step as four pips and a
  // word, the way to change it, and Show everything. It sits beside the station's name, before
  // the tabs it governs, and never changes place. It is not a state chip and takes no live or
  // status colour. Panel names `studio.step` (the control) and `studio.step.menu` (its dialog),
  // ADR-0070.
  //
  // The dialog lists the four steps for the show named and marks the one in force. Picking
  // another changes nothing yet: it first says what will change at once, then the primary
  // button names the move. A dialog owns the keyboard (keymap rule 5): ↑ ↓ move between steps,
  // ⏎ confirms the move, Esc keeps the step. No key opens it; it is a Tab away from the tabs.
  //
  // Show everything is a look, not a switch: this window draws the whole studio and the station
  // stays at the show's step.
  import Pips from './Pips.svelte';
  import StepList from './StepList.svelte';
  import { setShowStep, type ShowStep } from './api.ts';
  import { account } from './account.svelte.ts';
  import { show as fetchShow } from './schedule.ts';
  import { setShowAll, studio } from './studio.svelte.ts';
  import { openSetup } from './wizard.svelte.ts';
  import { run } from './actions.ts';
  import { STEP_WORD, menuKey, stationStep, stepChanges, stepHeading, stepLabel, stepLede, stepTitle } from './steps.ts';

  let dialogEl = $state<HTMLDialogElement | null>(null);
  /** The step picked in the list: the one in force until another is chosen. */
  let picked = $state<ShowStep>(stationStep(studio.view));
  let busy = $state(false);
  /** The station's own sentence when it refuses the change. */
  let problem = $state('');
  /** The show's presenter by name, for the line that says who can change the step. */
  let presenter = $state('');

  const sv = $derived(studio.view?.step ?? null);
  const step = $derived(stationStep(studio.view));
  const word = $derived(STEP_WORD[step]);
  /** Between shows the step is kept from the show last on air: the dashed edge. */
  const away = $derived(!!sv?.show && !sv.on_air);
  /** Nothing to change: no show has ever been on air, or this account may not. */
  const locked = $derived(!sv?.show || !sv.may_change);
  /** What would unlock the choices, for the tooltip on each and the line under them. */
  const lockedWhy = $derived(
    !sv?.show
      ? 'Locked until a show has been on air: make a show with an episode on the Schedule tab (⌃5), and its step can be changed here.'
      : !sv.may_change
        ? `Locked: only ${presenter || 'the show’s presenter'}, a producer or an admin can change ${sv.show.name}’s step.`
        : '',
  );
  const moving = $derived(picked !== step && !locked);
  const changes = $derived(moving && sv?.show ? stepChanges(step, picked, studio.view, sv.show.name, sv.on_air) : null);
  /** Something is hidden, and it is not already shown. */
  const canShowAll = $derived(step !== 'full' && !studio.showAll);

  // The dialog follows `studio.stepMenu`, so the desk's own buttons (Move up a step…, a hidden
  // key's Step…) open the same one.
  $effect(() => {
    const d = dialogEl;
    if (!d) return;
    if (studio.stepMenu && !d.open) {
      picked = step;
      problem = '';
      d.showModal();
      queueMicrotask(() => (d.querySelector<HTMLElement>('input[type=radio]:checked:not(:disabled)') ?? d.querySelector<HTMLElement>('[data-close]'))?.focus());
      void whoPresents();
    } else if (!studio.stepMenu && d.open) d.close();
  });
  // Closed, the list follows the step, so it never opens on a stale pick.
  $effect(() => {
    if (!studio.stepMenu) picked = step;
  });

  /** Someone who may not change the step is told who can: the show's presenter by name. */
  async function whoPresents() {
    presenter = '';
    if (!sv?.show || sv.may_change) return;
    try {
      presenter = (await fetchShow(sv.show.slug)).show.presenter ?? '';
    } catch {
      // The line then says "the show's presenter".
    }
  }

  async function move() {
    if (!moving || busy || !sv?.show) return;
    busy = true;
    problem = '';
    try {
      const r = await setShowStep(sv.show.slug, picked);
      // The view's stream follows; this window need not wait for it.
      if (studio.view) studio.view = { ...studio.view, step: { ...r.station, may_change: true } };
      studio.stepMenu = false;
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    // A focused button takes its own ⏎ (Keep Solo must keep Solo).
    if (e.key === 'Enter' && e.target instanceof HTMLButtonElement) return;
    const k = menuKey(e.key, picked, step, locked);
    // Esc is the dialog's own: it closes, and the step is kept.
    if (!k || k.type === 'close') return;
    e.preventDefault();
    if (k.type === 'confirm') return void move();
    picked = k.step;
    dialogEl?.querySelector<HTMLElement>(`input[data-step="${k.step}"]`)?.focus();
  }

  function showAll() {
    setShowAll(true);
    studio.stepMenu = false;
  }

  function setUp() {
    studio.stepMenu = false;
    openSetup();
  }
</script>

<span class="stepctl" data-panel="studio.step" role="group" aria-label="Step">
  <button
    type="button"
    class="stepbtn"
    class:away
    aria-haspopup="dialog"
    aria-expanded={studio.stepMenu}
    data-panel-title
    data-step={step}
    aria-label={stepLabel(step)}
    title={stepTitle(sv)}
    onclick={() => (studio.stepMenu = true)}
    ><Pips {step} /><span class="word">{word}</span><svg class="caret" viewBox="0 0 10 10" aria-hidden="true"
      ><path d="M2 3.5 5 6.5 8 3.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg
    ></button
  >
  {#if canShowAll}
    <button type="button" class="showall" title="See the whole studio for now. Nothing is switched on." onclick={() => setShowAll(true)}>Show everything</button>
  {/if}
</span>

<dialog bind:this={dialogEl} class="stepmenu" data-panel="studio.step.menu" aria-labelledby="sm-h" aria-describedby="sm-lede" onkeydown={onKey} onclose={() => (studio.stepMenu = false)}>
  <div><h2 id="sm-h" data-panel-title>{stepHeading(sv)}</h2></div>
  <p class="lede" id="sm-lede">
    {#each stepLede(sv, presenter, studio.zone) as part, i (i)}{#if typeof part === 'string'}{part}{:else}<b>{part.b}</b>{/if}{/each}
  </p>
  <StepList bind:value={picked} name="sm-step" now={step} {locked} why={lockedWhy} />
  {#if changes}
    <div class="changes" class:people={changes.people} role="status">
      <b>{changes.heading}</b>
      <ul>
        {#each changes.lines as line, i (i)}
          <li>{#each line as part, j (j)}{#if typeof part === 'string'}{part}{:else}<b>{part.b}</b>{/if}{/each}</li>
        {/each}
      </ul>
      <span>{changes.last}</span>
    </div>
  {/if}
  {#if problem}<p class="err" role="alert">{problem}</p>{/if}
  <div class="acts">
    {#if moving}
      <button type="button" class="btn quiet" onclick={() => (studio.stepMenu = false)} disabled={busy}>Keep {word} <kbd>Esc</kbd></button>
      <button type="button" class="btn primary" onclick={move} disabled={busy} aria-busy={busy || undefined}>{busy ? 'Moving…' : `Move to ${STEP_WORD[picked]}`} <kbd>⏎</kbd></button>
    {:else}
      {#if !sv?.show && account.me?.kind === 'staff'}
        <button type="button" class="btn" onclick={setUp}>Set up your show…</button>
      {:else if !sv?.show}
        <!-- The station key has no show of its own to set up; the Schedule tab makes one (the operator's 1.0.2 run). -->
        <button type="button" class="btn" onclick={() => { studio.stepMenu = false; void run({ type: 'tab', tab: 'schedule' }); }}>Open the Schedule tab <kbd>⌃5</kbd></button>
      {/if}
      {#if canShowAll}
        <!-- For someone who may not change the step, the look is the one thing to do here. -->
        <button type="button" class="btn" class:quiet={!locked} onclick={showAll}>Show everything</button>
      {/if}
      <button type="button" class="btn" class:quiet={locked && canShowAll} data-close onclick={() => (studio.stepMenu = false)}>Close <kbd>Esc</kbd></button>
    {/if}
  </div>
</dialog>
