<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Whether this window is Live or Preparing (ADR-0276), in the frame after who it runs as: one
  // pill with the word, and the way to switch. Drawn for a member of staff signed in as
  // themselves; the station key is always live and has no switch. It is not a state chip and
  // takes no live or status colour: "Live" here is the window's, never the red of ON AIR.
  // Panel names `studio.prepare` (the control) and `studio.prepare.menu` (its dialog), ADR-0070.
  //
  // The dialog follows the step dialog's grammar: the two choices, the one in force marked now;
  // picking the other changes nothing yet, it first says what will change, and the primary
  // button then names the switch. It is also the one ask before taking the desk while someone
  // else's show is on air. A dialog owns the keyboard (keymap rule 5): ↑ ↓ move between the two,
  // ⏎ confirms the switch, Esc keeps what is. No key opens it.
  import { account } from './account.svelte.ts';
  import { onAir, prepare, preparing, setPreparing } from './prepare.svelte.ts';
  import { PREPARE_LEDE, WINDOW_LINE, WINDOW_WORD, canPrepare, prepareButtons, prepareChanges, prepareLabel, prepareTitle, type WindowIs } from './prepare.ts';

  const BOTH: readonly WindowIs[] = ['live', 'preparing'];

  let dialogEl = $state<HTMLDialogElement | null>(null);
  const now = $derived<WindowIs>(preparing() ? 'preparing' : 'live');
  /** The choice picked in the list: the one in force until the other is chosen. */
  let picked = $state<WindowIs>('live');
  const moving = $derived(picked !== now);
  const changes = $derived(moving ? prepareChanges(picked, onAir()) : null);
  const buttons = $derived(prepareButtons(now));

  // The dialog follows `prepare.menu`, so the band's Switch to Live… opens the same one.
  $effect(() => {
    const d = dialogEl;
    if (!d) return;
    if (prepare.menu && !d.open) {
      picked = prepare.pick ?? now;
      prepare.pick = null;
      prepare.problem = '';
      d.showModal();
      queueMicrotask(() => d.querySelector<HTMLElement>('input[type=radio]:checked')?.focus());
    } else if (!prepare.menu && d.open) d.close();
  });

  async function move() {
    if (!moving || prepare.busy) return;
    if (!(await setPreparing(picked === 'preparing'))) return;
    prepare.menu = false;
    // The dialog would hand focus back to the pill, whose name plate then lies over the band
    // that has just appeared. Let it go: the keys are the surface's again, which after a switch
    // to Live is the point.
    queueMicrotask(() => (document.activeElement as HTMLElement | null)?.blur?.());
  }

  function onKey(e: KeyboardEvent) {
    // A focused button takes its own ⏎ (Keep preparing must keep preparing).
    if (e.target instanceof HTMLButtonElement) return;
    if (e.key === 'Enter') {
      e.preventDefault();
      return void move();
    }
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    e.preventDefault();
    picked = e.key === 'ArrowDown' ? 'preparing' : 'live';
    dialogEl?.querySelector<HTMLElement>(`input[data-window="${picked}"]`)?.focus();
  }
</script>

{#if canPrepare(account.me)}
  <span class="preparectl" data-panel="studio.prepare" role="group" aria-label="This window">
    <button
      type="button"
      class="stepbtn"
      class:is-preparing={now === 'preparing'}
      aria-haspopup="dialog"
      aria-expanded={prepare.menu}
      data-panel-title
      data-window={now}
      aria-label={prepareLabel(now)}
      title={prepareTitle(now)}
      onclick={() => (prepare.menu = true)}
      ><span class="word">{WINDOW_WORD[now]}</span><svg class="caret" viewBox="0 0 10 10" aria-hidden="true"
        ><path d="M2 3.5 5 6.5 8 3.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg
      ></button
    >
  </span>

  <dialog bind:this={dialogEl} class="stepmenu preparemenu" data-panel="studio.prepare.menu" aria-labelledby="pm-h" aria-describedby="pm-lede" onkeydown={onKey} onclose={() => (prepare.menu = false)}>
    <div><h2 id="pm-h" data-panel-title>This window</h2></div>
    <p class="lede" id="pm-lede">{PREPARE_LEDE}</p>
    <fieldset class="steplist windowlist">
      <legend class="sr-only">This window is</legend>
      {#each BOTH as w (w)}
        <label>
          <input type="radio" name="pm-window" value={w} bind:group={picked} data-window={w} />
          <b>{WINDOW_WORD[w]}</b><span class="what">{WINDOW_LINE[w]}</span>
          {#if now === w}<span class="nowtag">now</span>{:else}<span></span>{/if}
        </label>
      {/each}
    </fieldset>
    {#if changes}
      <div class="changes" class:people={changes.ask} role="status">
        <b>{changes.heading}</b>
        <ul>
          {#each changes.lines as line (line)}<li>{line}</li>{/each}
        </ul>
      </div>
    {/if}
    {#if prepare.problem}<p class="err" role="alert">{prepare.problem}</p>{/if}
    <div class="acts">
      {#if moving}
        <button type="button" class="btn quiet" onclick={() => (prepare.menu = false)} disabled={prepare.busy}>{buttons.keep} <kbd>Esc</kbd></button>
        <button type="button" class="btn primary" onclick={move} disabled={prepare.busy} aria-busy={prepare.busy || undefined}>{prepare.busy ? 'Switching…' : buttons.move} <kbd>⏎</kbd></button>
      {:else}
        <button type="button" class="btn" data-close onclick={() => (prepare.menu = false)}>Close <kbd>Esc</kbd></button>
      {/if}
    </div>
  </dialog>
{/if}
