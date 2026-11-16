<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The station switcher (ADR-0285; docs/reference/design/station-switcher.md): the station's
  // name in the frame as the button that lists the stations this computer keeps, and the
  // Stations dialog under it. Panel names `studio.station` (the button), `studio.stations`
  // (the dialog) and `studio.stations.leave` (the question before leaving a station that is on
  // air), ADR-0070. In the packaged app only: in a browser the page's own address is the station.
  //
  // It is not a state chip and takes no live or status colour: a station is told apart by its
  // name and a badge of two letters. With one station the frame keeps the mark; from two, the
  // badge takes the mark's place.
  //
  // A dialog owns the keyboard (keymap rule 5): ↑ ↓ move between stations, ⏎ opens the focused
  // one (and on the leave question, switches), Esc closes (and on the question, stays). No key
  // opens it and no key switches stations by itself; a chord to open it is proposed and waits
  // for the operator (question 2 of the design note).
  import { onMount, tick } from 'svelte';
  import Mark from './Mark.svelte';
  import { hereName, hereRow, leaveNow, loadStations, rename, requestLeave, stations, openStations } from './stations.svelte.ts';
  import { badges, hostOf, stationName, versionTag, whoWords } from './stations.ts';

  let dialogEl = $state<HTMLDialogElement | null>(null);
  let buttonEl = $state<HTMLButtonElement | null>(null);
  /** The name being typed in Manage, for the row that is open. */
  let label = $state('');
  let saved = $state('');

  const rows = $derived(stations.list?.stations ?? []);
  const marks = $derived(badges(rows));
  const here = $derived(hereRow());
  const name = $derived(hereName());
  const many = $derived(rows.length > 1);
  const app = $derived(stations.list?.app_version ?? null);
  const ask = $derived(stations.ask);
  /** Where a yes to the question takes the window, in the button's words. */
  const askGo = $derived.by(() => {
    if (!ask) return '';
    if (ask.leave.kind === 'sign-out') return `Sign out of ${name}`;
    if (ask.leave.kind === 'remove') return `Remove ${name}`;
    const to = rows.find((r) => r.id === ask.leave.id);
    return `Switch to ${to ? stationName(to) : 'the other station'}`;
  });

  onMount(() => void loadStations());

  // The dialog follows `stations.open`, so the sign-in screen's Add can open it on the question.
  $effect(() => {
    const d = dialogEl;
    if (!d) return;
    if (stations.open && !d.open) {
      d.showModal();
      queueMicrotask(() => (d.querySelector<HTMLElement>('[data-first]') ?? d.querySelector<HTMLElement>('.go:not([aria-current])') ?? d.querySelector<HTMLElement>('[data-close]'))?.focus());
    } else if (!stations.open && d.open) d.close();
  });

  // The dialog's keys are read on the dialog, so focus must be inside it. Turning to the
  // question takes the pressed row away, and Stay takes the question away: each time, focus
  // goes to what the next key is for (Switch on the question; a row on the list).
  let asked = false;
  $effect(() => {
    const now = !!ask;
    if (now === asked || !stations.open) return void (asked = now);
    asked = now;
    void tick().then(() => {
      const d = dialogEl;
      if (!d?.open) return;
      (d.querySelector<HTMLElement>(now ? '[data-first]' : '.go:not([aria-current])') ?? d.querySelector<HTMLElement>('[data-close]'))?.focus();
    });
  });

  function open() {
    openStations();
  }

  function closed() {
    stations.open = false;
    stations.ask = null;
    stations.manage = null;
    stations.removing = null;
  }

  function manage(id: string) {
    stations.removing = null;
    stations.problem = '';
    saved = '';
    if (stations.manage === id) {
      stations.manage = null;
      return;
    }
    stations.manage = id;
    label = rows.find((r) => r.id === id)?.label ?? '';
  }

  async function saveLabel(e: SubmitEvent, id: string) {
    e.preventDefault();
    saved = (await rename(id, label)) ? (label.trim() ? 'Saved.' : 'The station’s own name is used.') : '';
  }

  /** Remove is asked in place; focus goes to the safe answer, so the question is read and a
   *  second ⏎ keeps the station. */
  function askRemove(id: string) {
    stations.removing = id;
    void tick().then(() => dialogEl?.querySelector<HTMLElement>('[data-keep]')?.focus());
  }

  // Back from Add a station: focus returns to the button the dialog was opened from.
  let wasAdding = false;
  $effect(() => {
    const now = stations.adding;
    if (wasAdding && !now) void tick().then(() => buttonEl?.focus());
    wasAdding = now;
  });

  function add() {
    stations.open = false;
    stations.adding = true;
  }

  function onKey(e: KeyboardEvent) {
    if (stations.busy) return;
    if (ask) {
      // A focused button takes its own ⏎ (Stay must stay).
      if (e.key === 'Enter' && !(e.target instanceof HTMLButtonElement)) {
        e.preventDefault();
        void leaveNow(ask.leave, ask.words.live);
      }
      return;
    }
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    if (e.target instanceof HTMLInputElement) return;
    const go = [...(dialogEl?.querySelectorAll<HTMLElement>('.stlist .go') ?? [])];
    if (!go.length) return;
    const i = go.indexOf(document.activeElement as HTMLElement);
    e.preventDefault();
    go[(i + (e.key === 'ArrowDown' ? 1 : go.length - 1)) % go.length].focus();
  }

  /** Esc on the question stays, and the list is back; anywhere else it closes the dialog. */
  function onCancel(e: Event) {
    if (stations.busy) return e.preventDefault();
    if (!ask) return;
    e.preventDefault();
    stations.ask = null;
  }
</script>

<button
  bind:this={buttonEl}
  type="button"
  class="stationbtn"
  data-panel="studio.station"
  data-panel-title
  aria-haspopup="dialog"
  aria-expanded={stations.open}
  aria-label={`Station: ${name}. Stations…`}
  title={`${name}${here?.label && here.name && here.name !== here.label ? ` (${here.name})` : ''}${here ? ` · ${hostOf(here.url)}` : ''} · Stations… (⌃⇧S)`}
  aria-keyshortcuts="Control+Shift+S"
  onclick={open}
  >{#if many && here}<span class="stbadge" aria-hidden="true">{marks[here.id]}</span>{:else}<Mark />{/if}<span class="name">{name}</span><svg class="caret" viewBox="0 0 10 10" aria-hidden="true"
    ><path d="M2 3.5 5 6.5 8 3.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg
  ></button
>

<dialog
  bind:this={dialogEl}
  class="stations"
  data-panel={ask ? 'studio.stations.leave' : 'studio.stations'}
  aria-labelledby="st-h"
  aria-describedby={ask ? 'st-d' : undefined}
  onkeydown={onKey}
  oncancel={onCancel}
  onclose={closed}
>
  {#if ask}
    <h2 id="st-h" data-panel-title>{ask.words.heading}</h2>
    <div class="stask" id="st-d">
      <b>{ask.words.lead}</b>
      <ul>
        {#each ask.words.lines as line, i (i)}<li>{line}</li>{/each}
      </ul>
    </div>
    <div class="acts">
      <button type="button" class="btn quiet" onclick={() => (stations.ask = null)} disabled={stations.busy}>Stay on {name} <kbd>Esc</kbd></button>
      <button type="button" class="btn primary" data-first onclick={() => leaveNow(ask.leave, ask.words.live)} disabled={stations.busy} aria-busy={stations.busy || undefined}
        >{stations.busy ? 'One moment…' : askGo} <kbd>⏎</kbd></button
      >
    </div>
  {:else}
    <h2 id="st-h" data-panel-title>Stations</h2>
    <p class="lede">Each station is a place of its own, with its own sign-in. This window works with one at a time.</p>
    <ul class="stlist">
      {#each rows as row (row.id)}
        {@const current = row.id === stations.list?.current}
        {@const opened = stations.manage === row.id}
        {@const ver = versionTag(app, row.version)}
        <li class="strow" class:here={current} class:open={opened}>
          <button
            type="button"
            class="go"
            aria-current={current ? 'true' : undefined}
            aria-disabled={current ? 'true' : undefined}
            disabled={stations.busy}
            title={current ? 'This window is on this station' : row.kept ? `Switch to ${stationName(row)}` : `Sign in to ${stationName(row)}`}
            onclick={() => requestLeave({ kind: 'switch', id: row.id })}
          >
            <span class="stbadge lg" class:out={!row.kept} aria-hidden="true">{marks[row.id]}</span>
            <span class="nm"
              ><span>{stationName(row)}</span>{#if row.label && row.name && row.name !== row.label}<span class="own">{row.name}</span>{/if}{#if current}<span class="heretag">This window</span>{/if}</span
            >
            <span class="sub">{hostOf(row.url)} · {whoWords(row)}{#if ver}{' · '}<span class="ver">{ver}</span>{/if}</span>
          </button>
          <button type="button" class="btn quiet sm manage" aria-expanded={opened} aria-controls={`stm-${row.id}`} onclick={() => manage(row.id)}
            >{opened ? 'Done' : 'Manage'}<span class="sr-only">{' '}{stationName(row)}</span></button
          >
          {#if opened}
            <div class="stmanage" id={`stm-${row.id}`}>
              {#if stations.removing === row.id}
                <div class="stask" role="group" aria-labelledby={`str-${row.id}`}>
                  <b id={`str-${row.id}`}>Remove {stationName(row)} from this computer?</b>
                  <span>Its sign-in here ends and its key is deleted from {row.kept === 'file' ? 'this computer' : 'the keychain'}. The station itself is not changed.</span>
                  <div class="acts">
                    <button type="button" class="btn quiet sm" data-keep onclick={() => (stations.removing = null)}>Keep it</button>
                    <button type="button" class="btn danger-hollow sm" onclick={() => requestLeave({ kind: 'remove', id: row.id })} disabled={stations.busy}>Remove</button>
                  </div>
                </div>
              {:else}
                <form class="field" onsubmit={(e) => saveLabel(e, row.id)} novalidate>
                  <label for={`stn-${row.id}`}>Name on this computer</label>
                  <div class="inrow">
                    <input id={`stn-${row.id}`} type="text" bind:value={label} maxlength="60" placeholder={row.name ?? hostOf(row.url)} autocomplete="off" spellcheck="false" />
                    <button type="submit" class="btn sm">Save</button>
                  </div>
                  <span class="hint" role="status">{saved || 'Only this computer shows it. Empty, the station’s own name is used.'}</span>
                </form>
                <div class="acts">
                  {#if row.kept}
                    <button type="button" class="btn sm" onclick={() => requestLeave({ kind: 'sign-out', id: row.id })} disabled={stations.busy}>Sign out</button>
                  {:else}
                    <button type="button" class="btn sm" onclick={() => requestLeave({ kind: 'switch', id: row.id })} disabled={stations.busy}>Sign in…</button>
                  {/if}
                  <button type="button" class="btn sm" onclick={() => askRemove(row.id)}>Remove…</button>
                </div>
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
    {#if stations.problem}<p class="err" role="alert">{stations.problem}</p>{/if}
    <div class="foot">
      <button type="button" class="btn" onclick={add} disabled={stations.busy}>Add a station…</button>
      <button type="button" class="btn quiet" data-close onclick={() => (stations.open = false)}>Close <kbd>Esc</kbd></button>
    </div>
  {/if}
</dialog>
