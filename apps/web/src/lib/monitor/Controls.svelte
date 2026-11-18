<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // `monitor.controls`: only what this host may do now (ADR-0280; the design's sections 3.3,
  // 3.5, 3.7, 3.8 and 6). The station says what may be pressed (`controls.*.may`) and is asked
  // again at every press; a refusal is printed in its own words. No single key does anything
  // here: the buttons are reached by Tab and pressed with Enter or Space, Dump is two presses
  // within 3 s, End my show asks once, and a key held down never counts as a second press.
  import { tick } from 'svelte';
  import { CHIP } from '../../../../../packages/ui/states.ts';
  import type { MonitorView, Press } from './api.ts';
  import type { MonitorLink } from './link.svelte.ts';
  import { dumpedWords, first, nothingToPress } from './words.ts';

  let { link, view, known, now, zone }: { link: MonitorLink; view: MonitorView; known: boolean; now: number; zone: string } = $props();

  const c = $derived(view.controls);
  const next = $derived(view.next_host ? first(view.next_host.name) : 'the next host');
  const cued = $derived(view.next_host?.cued ?? false);
  // Dump stays in sight, off, with the station's reason, for a host who holds the air or has
  // just lost it: below Callers, or while their link is gone.
  const dumpOff = $derived(!c.dump.may && !!c.dump.why_not && (view.state === 'on_air' || view.state === 'open' || view.state === 'link_lost'));
  const any = $derived(c.go.may || c.not_yet.may || c.cue.may || c.bring.may || c.hand_over.may || c.end_show.may || c.dump.may);

  /** The press on its way to the station. */
  let busy = $state<string | null>(null);
  /** The station's refusal of the last press, or that it did not answer. */
  let problem = $state<string | null>(null);
  let section = $state<HTMLElement | undefined>();

  async function send(what: string, route: Press, body?: Record<string, unknown>) {
    if (busy || !known) return null;
    busy = what;
    problem = null;
    const r = await link.press(route, body);
    busy = null;
    // A button that has gone with its press takes the focus with it: keep it in the controls.
    await tick();
    if (section && (document.activeElement === document.body || document.activeElement === null)) section.focus();
    if (r.ok) return r.done;
    if ('refused' in r) problem = r.refused;
    else if ('unanswered' in r) problem = 'The station did not answer. Look at the lamp before you press again.';
    return null;
  }

  // Dump: two presses within 3 s, as everywhere (keymap rule 4).
  let armed = $state(false);
  let armTimer: ReturnType<typeof setTimeout> | undefined;
  let dumped = $state<{ lost_ms: number | null } | null>(null);
  let dumpedTimer: ReturnType<typeof setTimeout> | undefined;
  function disarm() {
    clearTimeout(armTimer);
    armed = false;
  }
  async function dump() {
    if (!c.dump.may || busy || !known) return;
    if (!armed) {
      armed = true;
      clearTimeout(armTimer);
      armTimer = setTimeout(() => (armed = false), 3000);
      return;
    }
    disarm();
    const done = await send('dump', 'dump');
    if (!done) return;
    dumped = { lost_ms: done.lost_ms };
    clearTimeout(dumpedTimer);
    dumpedTimer = setTimeout(() => (dumped = null), 30_000);
  }

  // End my show asks once (the design's section 3.8).
  let dialog = $state<HTMLDialogElement | undefined>();
  function ask() {
    if (!c.end_show.may || busy || !known) return;
    dialog?.showModal();
  }
  async function end() {
    dialog?.close();
    await send('end', 'end-show');
  }

  // What was armed does not outlive the right to press it.
  $effect(() => {
    if (armed && (!c.dump.may || !known)) disarm();
  });
  $effect(() => () => {
    clearTimeout(armTimer);
    clearTimeout(dumpedTimer);
  });

  /** A key held down repeats; a repeat is not a press. */
  function noRepeat(e: KeyboardEvent) {
    if (e.repeat && (e.key === 'Enter' || e.key === ' ')) e.preventDefault();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="card ctl" data-panel="monitor.controls" aria-labelledby="ctl-h" tabindex="-1" bind:this={section} onkeydown={noRepeat}>
  <h2 id="ctl-h" class="sr-only">Controls</h2>
  {#if !known}
    <p class="small">The controls come back when this page reaches the station again.</p>
  {:else}
    {#if !any}
      <p class="small">{nothingToPress(view, now, zone)}</p>
    {/if}
    {#if c.go.may}
      <button type="button" class="btn wide primary" data-act="go" aria-busy={busy === 'go' || undefined} onclick={() => send('go', 'go')}>Go on air now</button>
    {/if}
    {#if c.not_yet.may}
      <button type="button" class="btn wide" data-act="not-yet" aria-busy={busy === 'not-yet' || undefined} onclick={() => send('not-yet', 'not-yet')}>Not yet</button>
    {/if}
    {#if c.cue.may}
      <button type="button" class="btn wide" class:lit-cue={cued} data-act="cue" aria-pressed={cued} aria-busy={busy === 'cue' || undefined} onclick={() => send('cue', 'cue', { on: !cued })}>
        <svg aria-hidden="true"><use href="#g-cue" /></svg>Cue {next}
      </button>
    {/if}
    {#if c.bring.may}
      <button type="button" class="btn wide primary" data-act="bring" aria-busy={busy === 'bring' || undefined} onclick={() => send('bring', 'handover', { step: 'bring' })}>Bring {next} on air</button>
    {/if}
    {#if c.hand_over.may}
      <button type="button" class="btn wide primary" data-act="hand-over" aria-busy={busy === 'hand-over' || undefined} onclick={() => send('hand-over', 'handover', { step: 'hand_over' })}>Hand over to {next}</button>
    {/if}
    {#if c.end_show.may}
      <button type="button" class="btn wide" data-act="end" aria-haspopup="dialog" aria-busy={busy === 'end' || undefined} onclick={ask}>End my show</button>
    {/if}
    {#if c.bring.may || c.hand_over.may}
      <p class="small">You and {next} sound together until you hand over. Fade your own music. Do not try to match beats across the link.</p>
    {/if}
    {#if problem}<p class="problem" role="alert" data-says="refusal">{problem}</p>{/if}
    {#if c.dump.may}
      <button type="button" class="btn wide danger-hollow dump" class:armed data-act="dump" aria-busy={busy === 'dump' || undefined} aria-describedby="dump-says" onclick={dump}>{armed ? 'Press again' : 'Dump'}</button>
      <p id="dump-says" class="small" role="status">{armed ? 'Press again within 3 s to dump.' : 'Cuts the last few seconds from what listeners hear. Two presses.'}</p>
    {:else if dumpOff}
      <button type="button" class="btn wide danger-hollow dump" data-act="dump" aria-disabled="true" aria-describedby="dump-says">Dump</button>
      <p id="dump-says" class="small">{c.dump.why_not}</p>
    {/if}
    {#if dumped}
      <p class="dumpsay" role="status" data-says="dumped">
        <span class="state {CHIP.dumped.cls}"><svg class={CHIP.dumped.glyphCls} aria-hidden="true"><use href="#{CHIP.dumped.glyph}" /></svg>{CHIP.dumped.word}</span>
        <span>{dumpedWords(dumped.lost_ms)}</span>
      </p>
    {/if}
  {/if}

  {#if known && c.end_show.may}
    <!-- The question is there only while the show is this host's to end: taken off air, or the
         station lost, it goes with the right to answer it. -->
    <dialog bind:this={dialog} aria-labelledby="end-h" data-panel="monitor.end">
      <h2 id="end-h">End {view.show_name} now?</h2>
      {#if c.end_show_says}<p>{c.end_show_says}</p>{/if}
      <p class="small">Ending it here is kinder to listeners than disconnecting your software, which leaves them a few seconds of silence first.</p>
      <div class="acts">
        <!-- svelte-ignore a11y_autofocus -->
        <button type="button" class="btn primary" data-act="end-yes" autofocus onclick={end}>End my show</button>
        <button type="button" class="btn" data-act="end-no" onclick={() => dialog?.close()}>Keep going</button>
      </div>
    </dialog>
  {/if}
</section>
