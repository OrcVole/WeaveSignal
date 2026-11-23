<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The rehearsal desk (1.3.0, step R13; rehearsal-mode-build-plan.md, screens 2 and 3): the
  // Desk tab while its studio is the rehearsal studio. It keeps desk B's three columns so a DJ
  // finds everything where the live desk has it: time and the session on the left, the people
  // in the middle under the lamp, the rehearsed show's carts on the right, health along the
  // foot. What differs is what must: the lamp is REHEARSAL, hollow and dashed, never red and
  // never ON AIR or OPEN (ADR-0062); a person is In the mix or Not in the mix; there is no
  // delay, no Dump, no handover, no picture and no playout, because this desk has no way to
  // the air (build plan section 1.7).
  //
  // It says each state it can be in: not read yet, off at this station, the rehearsal desk not
  // answering, no session (Open the rehearsal studio for…), a session this window drives, and
  // another show's session, which it shows and cannot drive. Every press goes to
  // `/api/v1/rehearsal/*` (`lib/rehearsalDesk.svelte.ts`); nothing here calls the live desk.
  // Panel names: `rehearsal.stage`, `rehearsal.lamp`, `rehearsal.tile.N`, `rehearsal.waiting`,
  // `rehearsal.clock`, `rehearsal.session`, `rehearsal.carts`, `rehearsal.toast`,
  // `rehearsal.status` and the dialog `rehearsal.move` (ADR-0070).
  import { onMount } from 'svelte';
  import Tile from './Tile.svelte';
  import Invite from './Invite.svelte';
  import StudioChoice from './StudioChoice.svelte';
  import StateChip from '../../../../packages/ui/StateChip.svelte';
  import { bankAt, bankOwner, cartAt } from '../../../../packages/ui/carts.ts';
  import { account } from '../lib/account.svelte.ts';
  import { dur, firstName, hm, hms } from '../lib/format.ts';
  import { presentPeople, roomFull } from '../lib/rehearsal.ts';
  import { clearRehearsalError, endRehearsalNow, openRehearsalFor, rehearsal, rehearsalPhase } from '../lib/rehearsal.svelte.ts';
  import { MOVE, REHEARSAL_CHIP, REHEARSAL_LAMP_WHY, REHEARSAL_OFF_WHY, asSource, moveRows, stageSub, windowLine } from '../lib/rehearsalDesk.ts';
  import { choiceShown, mayDrive, mayOpen, moveNow, rdesk, runRehearsal, settleCatalogue, settleSlots, switchDeskStudio } from '../lib/rehearsalDesk.svelte.ts';
  import { stationNow, studio } from '../lib/studio.svelte.ts';
  import { loadShows, mine, setup } from '../lib/wizard.svelte.ts';

  const v = $derived(rehearsal.view);
  const phase = $derived(rehearsalPhase());
  const session = $derived(v?.session ?? null);
  const now = $derived(stationNow());
  const tenth = $derived(Math.floor((now % 1000) / 100));
  /** This window may drive the open session; else it shows it. */
  const drives = $derived(mayDrive());
  const offWhy = $derived(session && !drives ? (rdesk.refused ?? `${session.opened_by || 'Someone else'} is rehearsing ${session.show_name}. You can watch; only its presenter, a producer or an admin drives it.`) : null);
  const deskDown = $derived(!!v && v.enabled && !v.desk.linked);

  // The strips and the rehearsed show's carts follow the station's view.
  $effect(() => {
    void [v?.session?.id, v?.people.length, v?.people.map((p) => p.identity).join(' ')];
    settleSlots();
    void settleCatalogue(studio.view?.carts?.version ?? null);
  });
  onMount(() => {
    if (!setup.loaded && !setup.shows.length) void loadShows();
  });

  /** Everyone the station lists, each on their strip; the stage keeps strip order. */
  const strips = $derived(
    (v?.people ?? [])
      .map((p, i) => ({ p, slot: rdesk.slots[p.identity] ?? 0, order: rdesk.slots[p.identity] ?? 100 + i }))
      .sort((a, b) => a.order - b.order),
  );
  const inMix = $derived(strips.filter((s) => s.p.on));
  const waiting = $derived(strips.filter((s) => !s.p.on));
  const here = $derived(presentPeople(v));
  const full = $derived(roomFull(v));
  const win = $derived(windowLine(v?.window, now));

  // ---- no session: Open the rehearsal studio for… ----
  /** The shows this account may rehearse, its own first. */
  const shows = $derived(setup.shows.filter((s) => mayOpen(s.slug)).sort((a, b) => Number(b.slug === mine()?.slug) - Number(a.slug === mine()?.slug) || a.name.localeCompare(b.name)));
  let picked = $state('');
  const show = $derived(shows.find((s) => s.slug === picked) ?? shows[0] ?? null);
  async function open(e: SubmitEvent) {
    e.preventDefault();
    if (show && !rehearsal.busy) await openRehearsalFor(show.slug);
  }

  // ---- end: two presses, as every press that puts people out of a room ----
  let endArmed = $state(0);
  async function end() {
    if (now - endArmed > 4000) return void (endArmed = now);
    endArmed = 0;
    await endRehearsalNow();
  }

  // ---- the move dialog (`rehearsal.move`) ----
  let moveEl = $state<HTMLDialogElement | null>(null);
  let moving = $state(false);
  let moveWhy = $state('');
  const rows = $derived(moveRows(v));
  $effect(() => {
    if (rdesk.moveAsk && moveEl && !moveEl.open) {
      moveWhy = '';
      moveEl.showModal();
    } else if (!rdesk.moveAsk && moveEl?.open) moveEl.close();
  });
  async function move(e: SubmitEvent) {
    e.preventDefault();
    if (moving || !rows.length) return;
    moving = true;
    const r = await moveNow();
    moving = false;
    if (!r.ok) moveWhy = r.why;
  }

  // ---- the cart glance: the rehearsed show's current bank, F1 to F6 ----
  const bank = $derived(bankAt(rdesk.catalogue, studio.bank));
  const glance = $derived(
    [1, 2, 3, 4, 5, 6].map((n) => {
      const cart = cartAt(rdesk.catalogue, studio.bank, n);
      const s = cart ? (v?.carts_sounding.filter((x) => x.cart === cart.id).at(-1) ?? null) : null;
      return { n, cart, s };
    }),
  );

  // ---- the toast slot: the station's refusal first, then this window's own note ----
  const toast = $derived.by(() => {
    // A refusal that made the screen read-only is said in the session's panel, once.
    if (rehearsal.error && !rdesk.moveAsk && rehearsal.error !== rdesk.refused) return { tone: rehearsal.errorStatus === 403 ? 'plain' : 'warning', glyph: 'g-fault', title: rehearsal.errorStatus === null ? 'The station did not answer' : 'The station said no', msg: rehearsal.error, dismiss: clearRehearsalError };
    if (studio.note) return { tone: studio.note.tone, glyph: 'g-good', title: studio.note.title, msg: studio.note.message, dismiss: null };
    return null;
  });
</script>

<div class="desk rehearsal" data-studio="rehearsal">
  <div class="cols">
    <!-- left: time, and the session -->
    <div class="col left" class:has-choice={choiceShown()}>
      {#if choiceShown()}<StudioChoice />{/if}
      <section class="panel clock" data-panel="rehearsal.clock" aria-label="Studio clock">
        <div class="time num" data-panel-title>{hms(now)}<small>.{tenth}</small></div>
        {#if win}
          <!-- The handover window of the rehearsed show (build plan section 1.6): when it is on
               air, and that moving is a press. Nobody is moved by the clock. -->
          <div class="post says" class:due={win.due} data-testid="rehearsal-window"><span class="name">{win.text}</span></div>
        {:else if session}
          <div class="post says" data-testid="rehearsal-window"><span class="name">{session.show_name} is not next on air yet</span><span class="why">this line says when to move, once its handover window opens</span></div>
        {:else}
          <div class="post"><span class="name">No rehearsal open</span></div>
        {/if}
        <div class="listen">
          <svg aria-hidden="true" style="width:12px;height:12px;vertical-align:-1px"><use href="#g-lock" /></svg>
          Nothing here reaches listeners
        </div>
      </section>

      <section class="panel session" data-panel="rehearsal.session" aria-label="The rehearsal">
        <div class="ph"><h2>The rehearsal</h2><span class="spacer"></span>{#if session}<span class="small">since {hm(session.opened_at_ms)}</span>{/if}</div>
        {#if session}
          <p class="who"><b>{session.show_name}</b><span>opened by {session.opened_by || 'the station key'} · {here.length} of {v?.max_people ?? 6} here</span></p>
          {#if offWhy}
            <p class="readonly" data-testid="rehearsal-readonly" role="status"><svg aria-hidden="true"><use href="#g-lock" /></svg><span>{offWhy}</span></p>
          {:else}
            <div class="acts">
              <button type="button" class="btn primary" data-testid="rehearsal-move" aria-haspopup="dialog" disabled={rehearsal.busy} onclick={() => (rdesk.moveAsk = true)}>{MOVE.button}</button>
              <button type="button" class="btn quiet outline" class:armed={now - endArmed <= 4000} data-testid="rehearsal-end" disabled={rehearsal.busy} title="Ends the rehearsal: everyone leaves the rehearsal studio and its links stop working. Nobody is moved." onclick={end}
                >{now - endArmed <= 4000 ? 'Press again to end' : 'End rehearsal'}</button
              >
            </div>
            <p class="small">Moving takes everyone to the live studio standing by. Carts, the log and the show’s step are the show’s already: there is nothing else to carry.</p>
          {/if}
        {:else}
          <p class="small">One show rehearses at a time. A rehearsal has its own room and its own desk, and no way to the air.</p>
        {/if}
      </section>
    </div>

    <!-- centre: the rehearsal stage -->
    <main class="stage" data-panel="rehearsal.stage" aria-label="The rehearsal studio">
      <header class="stage-head">
        <div class="air">
          <StateChip lamp chip={REHEARSAL_CHIP.lamp} class="rehearsal" data-panel="rehearsal.lamp" data-state="rehearsal" role="status" title={REHEARSAL_LAMP_WHY} />
        </div>
        <div class="title">
          <h1 data-panel-title>{session ? session.show_name : 'Rehearsal studio'}</h1>
          {#if phase === 'off'}
            <div class="sub">Off at this station</div>
          {:else if deskDown}
            <div class="sub" data-testid="rehearsal-desk-down"><b>The rehearsal desk is starting again</b>{v?.desk.error ? ` · ${v.desk.error}` : ''} · the live studio is not affected</div>
          {:else if session}
            <div class="sub"><b>Rehearsing</b> · {stageSub(v)}</div>
          {:else if phase === 'closed'}
            <div class="sub">Nobody is rehearsing · nothing here reaches listeners</div>
          {/if}
        </div>
        <span></span>
        <div class="acts">
          {#if !choiceShown()}<button type="button" class="btn quiet" data-testid="to-live" title="Back to the live desk. The rehearsal goes on." onclick={() => switchDeskStudio('live')}>Live studio</button>{/if}
        </div>
      </header>

      <div class="stage-body">
        <div class="tiles">
          {#if phase === 'unknown'}
            <div class="empty" data-testid="rehearsal-empty"><b>Reading the rehearsal studio…</b><span>{rehearsal.linkWhy || 'Waiting for the station.'}</span></div>
          {:else if phase === 'absent'}
            <div class="empty" data-testid="rehearsal-empty"><b>This station has no rehearsal studio</b><span>It is older than 1.3.0. The live studio works as ever.</span></div>
          {:else if phase === 'off'}
            <div class="empty" data-testid="rehearsal-empty"><b>The rehearsal studio is off at this station</b><span>{REHEARSAL_OFF_WHY}</span></div>
          {:else if !session}
            <!-- No session (screen 2): Open the rehearsal studio for…, with a choice of show. -->
            <form class="empty open" data-testid="rehearsal-open" onsubmit={open}>
              <b>Open the rehearsal studio for…</b>
              {#if deskDown}<span class="warn" role="status">The rehearsal desk is not answering yet. You can open the studio; people hear each other once the desk is back.</span>{/if}
              {#if shows.length}
                <label class="pick"
                  ><span class="sr-only">The show to rehearse</span>
                  <select bind:value={picked} aria-label="The show to rehearse">
                    {#each shows as s (s.slug)}<option value={s.slug}>{s.name}</option>{/each}
                  </select></label
                >
                <span>Run through {show?.name ?? 'the show'} with your guests and its carts. Nothing reaches listeners; when it is time, one press moves everyone to the live studio, standing by.</span>
                <button type="submit" class="btn primary" disabled={rehearsal.busy} aria-busy={rehearsal.busy || undefined}>{rehearsal.busy ? 'Opening…' : `Open for ${show?.name ?? 'the show'}`}</button>
              {:else if setup.shows.length}
                <span>{account.me?.role === 'screener' || account.me?.role === 'presenter' ? 'No show is tied to your account, so there is none for you to rehearse. A producer or an admin can open the rehearsal studio for any show.' : 'There is no show to rehearse yet.'}</span>
              {:else}
                <span>{setup.error ? `The shows could not be read: ${setup.error}` : 'There is no show to rehearse yet. Make one on the Schedule tab.'}</span>
              {/if}
            </form>
          {:else}
            {#each inMix as s (s.p.identity)}
              <Tile context="rehearsal" source={asSource(s.p, s.slot)} slot={s.slot} playoutUp={false} playoutForMs={null} off={offWhy} />
            {:else}
              <div class="empty" data-testid="rehearsal-empty">
                {#if waiting.length}
                  <b>Nobody is in the mix</b>
                  <span>{waiting.length === 1 ? `${firstName(waiting[0].p.name, 'One person')} is here` : `${waiting.length} people are here`}, under <b>Not in the mix</b>, below. Press their number to put them in: the others then hear them.</span>
                {:else}
                  <b>Nobody is here yet</b>
                  <span>People appear when they open a link to the rehearsal: invite your guests, and yourself as a presenter, from <b>Invite someone…</b> below. A link to the rehearsal never leads to the air.</span>
                {/if}
              </div>
            {/each}
          {/if}
        </div>

        {#if session}
          <div class="waiting" data-panel="rehearsal.waiting">
            <div class="wh">
              <span data-panel-title>Not in the mix</span><span class="rule">here, and not heard by the others · their keys never change · cue and talk work as at the live desk</span>
              {#if !offWhy}{#if full}<span class="offtag" title="The rehearsal studio takes {v?.max_people} people at once.">full</span>{:else}{#if account.me?.kind === 'staff'}<Invite rehearsal own />{/if}<Invite rehearsal />{/if}{/if}
            </div>
            {#each waiting as s (s.p.identity)}
              <Tile context="rehearsal" source={asSource(s.p, s.slot)} slot={s.slot} playoutUp={false} playoutForMs={null} off={offWhy} />
            {/each}
          </div>
        {/if}
      </div>
    </main>

    <!-- right: the rehearsed show's carts, and what the station says -->
    <div class="col right three">
      <section class="panel" data-panel="rehearsal.carts" aria-label="Carts">
        <div class="ph">
          <h2>Carts{#if bank}&nbsp;· {bank.name}{/if}</h2>
          <span class="spacer"></span><span class="small">{#if session}bank {studio.bank}{#if bank}, {bankOwner(bank)}{/if} · <kbd>F1</kbd>–<kbd>F6</kbd>{/if}</span>
        </div>
        {#if !session}
          <p class="small">The rehearsed show’s carts play here, into the rehearsal only. They are the same carts the live desk fires.</p>
        {:else}
          <div class="carts" style="--cart-colour:{bank ? `var(--cart-${bank.colour_slot})` : 'var(--deemph)'}">
            {#each glance as { n, cart, s } (n)}
              {#if !cart}
                <button type="button" class="cart empty" aria-label="F{n}, empty slot" disabled={!!offWhy} title={offWhy ?? undefined} onclick={() => runRehearsal({ type: 'cart', n })}><span class="label">Empty</span><span class="dur num">no cart</span><kbd>F{n}</kbd></button>
              {:else}
                <button
                  type="button"
                  class="cart"
                  class:playing={!!s && !s.cue}
                  class:cued={!!s?.cue}
                  class:missing={cart.missing || !cart.file}
                  aria-label="F{n}, {cart.label}{s ? ', playing in the rehearsal' : `, ${dur(cart.duration_ms + 999)}`}"
                  disabled={!!offWhy}
                  title={offWhy ?? 'Plays into the rehearsal only. A play here is never in the music report.'}
                  onclick={() => runRehearsal({ type: 'cart', n })}
                  ><span class="label">{cart.label}</span><span class="dur num">{#if cart.missing || !cart.file}file missing{:else if s}<b>{s.loop || s.remaining_ms == null ? 'loop' : `−${dur(s.remaining_ms + 999)}`}</b>{:else}{dur(cart.duration_ms + 999)}{/if}</span><kbd>F{n}</kbd></button
                >
              {/if}
            {/each}
          </div>
          <p class="small">Into the rehearsal only: a play here is never in the music report.</p>
        {/if}
      </section>

      <div class="slot">
        {#if toast}
          <div class="toast {toast.tone}" data-panel="rehearsal.toast" role={toast.tone === 'good' || toast.tone === 'plain' ? 'status' : 'alert'}>
            <div class="stripe"></div>
            <div class="body">
              <svg aria-hidden="true"><use href="#{toast.glyph}" /></svg><span class="title" data-panel-title>{toast.title}</span>
              <span class="msg" title={toast.msg}>{toast.msg}</span>
              {#if toast.dismiss}<span class="acts"><button type="button" class="btn" onclick={toast.dismiss}>Dismiss</button></span>{/if}
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>

  <footer class="statusbar" data-panel="rehearsal.status" aria-label="Rehearsal studio status">
    <span class="item" data-panel-title
      ><svg class={v?.desk.linked ? 'g-good' : 'g-serious'} aria-hidden="true"><use href={v?.desk.linked ? '#g-good' : '#g-fault'} /></svg>Rehearsal desk <b>{phase === 'off' ? 'off' : v?.desk.linked ? 'answering' : 'not answering'}</b></span
    >
    {#if session}<span class="item"><svg aria-hidden="true"><use href="#g-crowd" /></svg>Rehearsal room <b>{here.length} of {v?.max_people ?? 6}</b></span>{/if}
    {#if rehearsal.link !== 'live' && phase !== 'absent'}<span class="item"><svg class="g-warning" aria-hidden="true"><use href="#g-link-degraded" /></svg>Not following the station{rehearsal.linkWhy ? `: ${rehearsal.linkWhy}` : ''} · what shows may be out of date</span>{/if}
    <span class="sep"></span>
    <span class="item"><svg aria-hidden="true"><use href="#g-lock" /></svg>No way to the air from here</span>
  </footer>
</div>

<!-- The move (screen 3): who moves, each with their opening. ⏎ is Move, Esc is Stay here; the
     move has no chord of its own. A dialog owns the keyboard (keymap rule 5). -->
<dialog bind:this={moveEl} data-panel="rehearsal.move" aria-labelledby="mv-h" onclose={() => (rdesk.moveAsk = false)}>
  <form onsubmit={move}>
    <h2 id="mv-h" data-panel-title>{MOVE.title(session?.show_name ?? 'the show')}</h2>
    <p class="lede">{MOVE.lede}</p>
    {#if rows.length}
      <ul class="movers" aria-label="Who moves, and what the handover does with each">
        {#each rows as r (r.identity)}
          <li data-opening={r.kind}>
            <span class="who"><b>{r.name}</b><span>{r.role}</span></span>
            <span class="op"><svg aria-hidden="true"><use href={r.kind === 'on' ? '#g-good' : r.kind === 'muted' ? '#g-muted' : '#g-standby'} /></svg>{r.opening}</span>
          </li>
        {/each}
      </ul>
      <p class="then">{MOVE.then}{v?.window && session ? ` ${session.show_name} is on air at ${hm(v.window.start_ms)}.` : ''}</p>
    {:else}
      <p class="then">{MOVE.nobody}</p>
    {/if}
    {#if moveWhy}<p class="err" role="alert">{moveWhy}</p>{/if}
    <div class="acts">
      <button type="button" class="btn quiet" onclick={() => (rdesk.moveAsk = false)}>{MOVE.stay} <kbd>Esc</kbd></button>
      <!-- svelte-ignore a11y_autofocus -->
      <button type="submit" class="btn primary" autofocus disabled={moving || !rows.length} aria-busy={moving || undefined}>{moving ? 'Moving…' : MOVE.confirm} <kbd>⏎</kbd></button>
    </div>
  </form>
</dialog>

<style>
  .session .who {
    display: grid;
    gap: 2px;
    margin: 0;
  }
  .session .who span,
  .session .small {
    font-size: var(--fs-13);
    color: var(--ink-2);
    line-height: var(--lh-label);
  }
  .session .acts {
    display: grid;
    gap: var(--s-2);
  }
  .session .acts .btn {
    justify-content: center;
    white-space: normal;
    text-align: center;
  }
  .readonly {
    display: flex;
    gap: var(--s-2);
    align-items: flex-start;
    margin: 0;
    padding: var(--s-2);
    border: 1px dashed var(--axis);
    border-radius: var(--r-2);
    font-size: var(--fs-13);
    color: var(--ink);
    line-height: var(--lh-label);
  }
  .readonly svg {
    width: 14px;
    height: 14px;
    flex: none;
    margin-top: 2px;
  }
  form.open {
    justify-items: center;
  }
  form.open select {
    font: inherit;
    font-weight: 600;
    color: var(--ink);
    background: var(--plane);
    border: 1px solid var(--muted);
    border-radius: var(--r-2);
    min-height: 2.25rem;
    padding: 0 var(--s-2);
    max-width: 100%;
  }
  form.open .warn {
    border-left: 3px solid var(--status-serious);
    padding-left: var(--s-2);
    text-align: start;
    color: var(--ink);
  }
  dialog {
    width: min(34rem, calc(var(--win-w, 100vw) - 2 * var(--s-4)));
    max-height: calc(var(--win-h, 100vh) - 2 * var(--s-4));
    overflow-y: auto;
    background: var(--surface);
    color: var(--ink);
    border: 1px solid var(--axis);
    border-radius: var(--r-3);
    box-shadow: var(--e-3);
    padding: var(--s-5);
    font-size: var(--fs-14);
  }
  dialog::backdrop {
    background: var(--scrim);
  }
  dialog form {
    display: grid;
    gap: var(--s-3);
  }
  dialog h2 {
    font-size: var(--fs-20);
    overflow-wrap: anywhere;
  }
  .lede {
    font-weight: 600;
    line-height: var(--lh-body);
  }
  .movers {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    border: 1px solid var(--hairline);
    border-radius: var(--r-2);
  }
  .movers li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--s-3);
    align-items: center;
    padding: var(--s-2) var(--s-3);
  }
  .movers li + li {
    border-top: 1px solid var(--hairline);
  }
  .movers .who {
    display: grid;
    min-width: 0;
  }
  .movers .who b {
    overflow-wrap: anywhere;
  }
  .movers .who span {
    font-size: var(--fs-13);
    color: var(--ink-2);
  }
  .movers .op {
    display: inline-flex;
    align-items: center;
    gap: var(--s-1);
    font-size: var(--fs-13);
    color: var(--ink);
  }
  .movers .op svg {
    width: 14px;
    height: 14px;
    flex: none;
    color: var(--muted);
  }
  .movers li[data-opening='on'] .op svg {
    color: var(--accent);
  }
  .then {
    color: var(--ink-2);
    line-height: var(--lh-body);
    font-size: var(--fs-13);
  }
  .err {
    color: var(--ink);
    font-size: var(--fs-13);
    line-height: var(--lh-body);
    border-left: 3px solid var(--status-critical);
    padding-left: var(--s-2);
  }
  dialog .acts {
    display: flex;
    gap: var(--s-2);
    justify-content: flex-end;
    flex-wrap: wrap;
    margin-top: var(--s-1);
  }
</style>
