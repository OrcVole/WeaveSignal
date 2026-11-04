<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // One source as a tile (desk B, ADR-0061): a fixed head (slot, name, role, one state chip),
  // levels (the R128 meter with its ticks, ported from desk A, and the fader), the four keyed
  // buttons at one offset in every state, then the lines and notes. The keys belong to the
  // slot, not the place on screen (desk.md section 3, rule 4).
  import type { Source } from '../lib/api.ts';
  import { CHIP, isOpen, listenersHear, sourceChip, type Chip, type StationMode } from '../../../../packages/ui/states.ts';
  import StateChip from '../../../../packages/ui/StateChip.svelte';
  import { slotKeys } from '../../../../packages/ui/keymap.ts';
  import { dur, meterPct, signed } from '../lib/format.ts';
  import { run } from '../lib/actions.ts';
  import { studio } from '../lib/studio.svelte.ts';

  interface Props {
    /** The source, or null for slot 6, playout. */
    source: Source | null;
    slot: number;
    playoutUp: boolean;
    playoutForMs: number | null;
    /** The station's mode: in auto mode an open fader says OPEN (ADR-0183). */
    mode?: StationMode | null;
  }
  let { source, slot, playoutUp, playoutForMs, mode = null }: Props = $props();

  const keys = $derived(slotKeys(slot));
  const isPlayout = $derived(source === null);
  const ready = $derived(!!source?.caller?.sent && !source.caller.taken);
  const chip: Chip = $derived(
    source
      ? sourceChip({ on_air: source.on_air, mute: source.mute, cue: source.cue, ready }, playoutUp, mode)
      : playoutUp
        ? CHIP.standby
        : CHIP.playoutDown,
  );
  const open = $derived(!!source && isOpen(source));
  /** Listeners hear the room: playout is up and automation does not hold the air (ADR-0183). */
  const heard = $derived(listenersHear(playoutUp, mode));
  const talking = $derived(!!source && studio.talking.includes(source.identity));
  const ROLE: Record<string, [string, string]> = {
    presenter: ['g-mic', 'Presenter'],
    dj: ['g-disc', 'Remote DJ'],
    guest: ['g-browser', 'Guest'],
    caller: ['g-phone', 'Caller'],
  };
  const role = $derived(source ? (ROLE[source.role] ?? ['g-standby', source.role]) : ['g-playout', 'Liquidsoap 2.5']);
  const roleLine = $derived(
    source?.role === 'caller' ? `Caller · ${source.caller?.sent ? 'sent by the screener' : 'from the screener'}` : role[1],
  );
  const name = $derived(source ? source.caller?.name_on_air || source.name || source.identity : 'Playout');
  const hears = $derived(
    !source
      ? 'Takes the desk into the programme'
      : source.role === 'presenter' || source.role === 'dj'
        ? 'Hears the programme'
        : 'Own mix-minus',
  );
  const lufs = $derived(source?.lufs ?? null);
  const peak = $derived(source?.lufs_peak ?? null);
  const over = $derived(lufs !== null && lufs > -22);
  const away = $derived(source && !source.present ? (source.away_ms ?? 0) : null);
  const degraded = $derived(!!source && (!source.present || !source.linked));
  const armedDrop = $derived(studio.armed?.what === 'drop' && slot === 4);
  // End pass (ADR-0119) is for the people who joined with an invite: a presenter, a remote DJ
  // or a guest. A caller has Drop, which ends the pass too; playout has no pass. Two presses,
  // as Drop, and the arm is this tile's own: it carries the identity it was pressed for.
  const invited = $derived(source?.role === 'presenter' || source?.role === 'dj' || source?.role === 'guest');
  const armedEnd = $derived(!!source && studio.armed?.what === 'end-pass' && studio.armed.identity === source.identity);
  const TICKS = [-40, -30, -23, -18, -10];
</script>

<section
  class="tile"
  class:is-onair={open && heard}
  class:is-open={open && !heard}
  data-panel="desk.tile.{slot}"
  aria-label="Source {slot}, {name}"
>
  <div class="thumb"><b>{source?.role === 'caller' ? 'name card' : 'camera'}</b><span>no picture in the tile yet</span></div>
  <div class="head">
    <span class="slot num">{slot}</span>
    <div>
      <div class="name" data-panel-title>{name}</div>
      <div class="role">
        <svg aria-hidden="true"><use href="#{role[0]}" /></svg>{roleLine}{#if source}<span class="lg-only">
            · {source.identity}</span
          >{/if}
      </div>
    </div>
    <div class="chips">
      <StateChip {chip} />
      {#if talking}<StateChip chip={CHIP.talk} />{/if}
      {#if source?.role === 'caller'}
        <button
          type="button"
          class="btn quiet"
          class:armed={armedDrop}
          onclick={() => run(armedDrop ? { type: 'drop' } : { type: 'arm', what: 'drop' })}
          >{armedDrop ? 'Press again' : 'Drop'} <kbd>⌃D</kbd><kbd>⌃D</kbd></button
        >
      {:else if source && invited}
        <button
          type="button"
          class="btn quiet"
          class:armed={armedEnd}
          aria-label={armedEnd ? `Press again to end pass for ${name}` : `End pass for ${name}`}
          onclick={() =>
            run(armedEnd ? { type: 'end-pass', identity: source.identity } : { type: 'arm', what: 'end-pass', identity: source.identity })}
          >{armedEnd ? 'Press again' : 'End pass'}</button
        >
      {/if}
    </div>
  </div>

  <div class="levels">
    <div class="hmeter-wrap">
      <div
        class="hmeter"
        role="img"
        aria-label={lufs === null ? 'No level' : `Short-term ${signed(lufs)} LUFS, target −23`}
      >
        <div class="fill" style:width="{meterPct(lufs)}%"></div>
        {#if over && lufs !== null}<div
            class="over"
            style:left="{meterPct(-23)}%"
            style:width="{meterPct(lufs) - meterPct(-23)}%"
          ></div>{/if}
        {#if peak !== null}<div class="peak" style:left="{meterPct(peak)}%"></div>{/if}
        <div class="target" style:left="{meterPct(-23)}%"></div>
      </div>
      <div class="hscale" aria-hidden="true">
        {#each TICKS as t (t)}<span class:t={t === -23} style:left="{meterPct(t)}%">{signed(t).replace('.0', '')}</span>{/each}
      </div>
    </div>
    <span class="rd" class:over><b>{lufs === null ? '—' : signed(lufs)}</b> LUFS</span>
    <div class="hfader" role="img" aria-label="Fader at 0.0 dB; the desk has no fader yet">
      <div class="slot"></div>
      <div class="z" style:left="80%"></div>
      <div class="knob" style:left="80%"></div>
    </div>
    <span class="rd">fader <b>0.0</b> dB</span>
  </div>

  <div class="controls">
    <button
      type="button"
      class="btn"
      class:lit-onair={!!source?.on_air && heard}
      class:lit-open={!!source?.on_air && !heard}
      aria-pressed={!!source?.on_air}
      aria-disabled={isPlayout}
      onclick={() => run({ type: 'on', slot })}
      ><span><svg aria-hidden="true"><use href={heard ? '#g-onair' : '#g-open'} /></svg> On</span><kbd>{keys.on}</kbd></button
    >
    <button
      type="button"
      class="btn"
      class:lit-cue={!!source?.cue}
      aria-pressed={!!source?.cue}
      aria-disabled={isPlayout}
      onclick={() => run({ type: 'cue', slot })}
      ><span><svg aria-hidden="true"><use href="#g-cue" /></svg> Cue</span><kbd>{keys.cue}</kbd></button
    >
    <button
      type="button"
      class="btn"
      class:lit-talk={talking}
      aria-pressed={talking}
      aria-disabled={isPlayout}
      aria-label="Talk to {name}, held"
      onpointerdown={() => run({ type: 'talk', slot, held: true })}
      onpointerup={() => run({ type: 'talk', slot, held: false })}
      onpointerleave={() => talking && run({ type: 'talk', slot, held: false })}
      ><span><svg aria-hidden="true"><use href="#g-talk" /></svg> Talk</span><kbd>{keys.talk}</kbd></button
    >
    <button
      type="button"
      class="btn"
      class:lit-off={!!source?.mute}
      aria-pressed={!!source?.mute}
      aria-disabled={isPlayout}
      onclick={() => run({ type: 'mute', slot })}
      ><span><svg aria-hidden="true"><use href="#g-muted" /></svg> Mute</span><kbd>{keys.mute}</kbd></button
    >
  </div>

  <div class="lines">
    <span class="mm"><svg aria-hidden="true"><use href="#g-good" /></svg><span>{hears}</span></span>
    {#if isPlayout}
      {#if playoutUp}
        <span><svg class="g-good" aria-hidden="true"><use href="#g-good" /></svg><span
            >SRT connected{playoutForMs !== null ? ` ${dur(playoutForMs)}` : ''}</span
          ></span
        >
      {:else}
        <span
          ><svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg><span
            >{playoutForMs === null ? 'Playout has not connected' : `SRT link lost ${dur(playoutForMs)} ago`} · room alive</span
          ></span
        >
      {/if}
    {:else if degraded}
      <span class="st"
        ><StateChip chip={CHIP.linkDegraded} /><span class="lg-only"
          >{away !== null ? `left the room ${dur(away)} ago · kept 30 s` : 'no audio reaching the desk'}</span
        ></span
      >
    {:else}
      <span><svg class="g-good" aria-hidden="true"><use href="#g-link" /></svg><span>Linked · in the room</span></span>
    {/if}
  </div>

  <div class="rest">
    {#if source?.caller && (source.caller.line || source.caller.note)}
      <div class="note">{[source.caller.line, source.caller.note].filter(Boolean).join(' · ')}</div>
    {/if}
    {#if degraded && away !== null}
      <div class="extra">Left the room {dur(away)} ago; their place on the desk is kept for 30 s.</div>
    {/if}
    {#if isPlayout && !playoutUp}
      <div class="extra">Listeners hear nothing of the room until playout is back. Restart it ⌃R (not wired yet).</div>
    {/if}
  </div>
</section>
