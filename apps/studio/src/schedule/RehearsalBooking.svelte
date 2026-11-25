<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The booking editor (`rehearsal.booking`; 1.3.0, issue 24; rehearsal-mode-build-plan.md
  // section 2.3), in the Schedule tab's editor column while the Studios view is open. It holds
  // whatever is selected in the Rehearsal column:
  //
  // - the booking being made (Book…, or a drag across the column): the show, the day and the
  //   two times, and **Book the studio**. Nothing is at the station until then, and the block
  //   on the grid follows the fields as they change, as the fields follow the block when it is
  //   dragged;
  // - a booking still to come: the same fields, **Save booking** and **Delete booking**. One
  //   under way keeps its start, as the station has it, so only its end and its show change;
  // - a refusal: the station's own sentence, under the field it names; when another booking is
  //   in the way (409), which one and when, with **Show it**. An overlap the week on screen
  //   already shows is said before the station is asked, and the station still decides;
  // - what is only to read, and why: a booking that is over, a booking for a show this window
  //   has no rights over, and a session, the studio as it was used or is being used.
  //
  // A booking holds the studio for its show and airs nothing; a session is run from the Desk
  // tab. So nothing here is a lamp, nothing is red, and no word of it is ON AIR or OPEN
  // (ADR-0062). The rules are `studios.ts`; the station stays the authority.
  import TimeField from './TimeField.svelte';
  import Field from './Field.svelte';
  import { noteUnsaved } from '../lib/schedule.svelte.ts';
  import { hm, lengthWords, whenWords, type Block, type Rehearsal } from './model.ts';
  import { addDays } from './rule/form.ts';
  import { airsAfterWords, bookingAsk, bookingChange, fieldsOf, isoDate, readOnlyWhy, sessionWords, sharesWith, stretchOf, type BookingFields, type Refusal } from './studios.ts';
  import type { BookingRequest } from '../lib/api.ts';
  import type { Show } from '../lib/schedule.ts';

  let {
    item,
    shows,
    zone,
    now,
    may,
    taken = [],
    blocks = [],
    refused: firstRefusal = null,
    onDraft,
    onSave,
    onDelete,
    onShows,
    onShowIt,
    onDesk,
    deskWhy = null,
  }: {
    /** What is selected in the Rehearsal column; `draft` is the booking being made. */
    item: Rehearsal & { draft?: boolean };
    /** The shows this window may book for. */
    shows: readonly Pick<Show, 'slug' | 'name' | 'colour_slot'>[];
    zone: string;
    /** The station's clock, to the minute. */
    now: number;
    /** This window has rights over the booking's show. */
    may: boolean;
    /** The Rehearsal column as the week on screen has it, for an overlap seen before the save. */
    taken?: readonly Rehearsal[];
    /** The week on screen, for when the booked show next goes on air. */
    blocks?: readonly Block[];
    /** A refusal to open with (a test's). */
    refused?: Refusal | null;
    /** The booking being made changed in its fields: the grid's block follows. */
    onDraft?: (d: { show: string; start: number; end: number }) => void;
    /** Save: the whole booking, and what changed of one being changed. Null when taken. */
    onSave: (request: BookingRequest, patch: Partial<BookingRequest>) => Promise<Refusal | null>;
    onDelete: () => Promise<Refusal | null>;
    /** ‹ Shows. */
    onShows: () => void;
    /** Show it: bring the booking in the way into view, on its own day. */
    onShowIt?: (id: string, ms: number) => void;
    /** Go to the rehearsal desk (1.3.2), on the pane of a session that is open: the Desk tab,
     *  showing the rehearsal studio. Left out, the pane only says where the desk is. */
    onDesk?: () => void | Promise<void>;
    /** Why the rehearsal desk cannot be gone to from this window now, or null when it can. */
    deskWhy?: string | null;
  } = $props();

  /** Go to the rehearsal desk: held keys are let go on the way, so the press may take a moment. */
  let going = $state(false);
  async function toDesk() {
    if (!onDesk || deskWhy || going) return;
    going = true;
    try {
      await onDesk();
    } finally {
      going = false;
    }
  }

  const isNew = $derived(!!item.draft);
  const change = $derived(isNew ? 'all' : bookingChange(item, now, may));
  const readOnly = $derived(isNew ? null : readOnlyWhy(item, now, may));
  const underWay = $derived(change === 'end');

  // ---- the fields ----
  // svelte-ignore state_referenced_locally
  let f = $state<BookingFields>(fieldsOf(item.slug, item.start, item.end, zone));
  /** What the fields last agreed with the block about: the block dragged on the grid differs
   *  from it and the fields follow; a field half typed does not, and is left alone. */
  // svelte-ignore state_referenced_locally
  let agreed = { show: item.slug, start: item.start, end: item.end };
  $effect(() => {
    const [show, start, end] = [item.slug, item.start, item.end];
    if (show === agreed.show && start === agreed.start && end === agreed.end) return;
    agreed = { show, start, end };
    f = fieldsOf(show, start, end, zone);
  });

  /** A field of the booking being made was changed by hand. */
  let touched = $state(false);
  // svelte-ignore state_referenced_locally
  let refusal = $state<Refusal | null>(firstRefusal);
  let problem = $state<{ message: string; about: 'show' | 'from' | 'until' } | null>(null);
  let busy = $state(false);

  /** A field changed: what was refused is no longer what is on screen, and the booking being
   *  made is drawn where its fields now put it. */
  function fieldChanged() {
    refusal = null;
    problem = null;
    touched = true;
    if (!isNew) return;
    const s = stretchOf(f, zone);
    if (!s || s.end <= s.start) return;
    agreed = { show: f.show, ...s };
    onDraft?.(agreed);
  }

  const stretch = $derived(stretchOf(f, zone));
  const start = $derived(underWay ? item.start : (stretch?.start ?? item.start));
  const end = $derived(stretch?.end ?? item.end);
  const overnight = $derived(!!stretch && isoDate(start, zone) !== isoDate(end, zone) && hm(end, zone) !== '00:00');
  const was = $derived(isNew ? null : item);
  const dirty = $derived.by(() => {
    if (readOnly) return false;
    if (isNew) return touched;
    const o = fieldsOf(item.slug, item.start, item.end, zone);
    return f.show !== o.show || f.date !== o.date || f.from !== o.from || f.until !== o.until;
  });
  // Esc on the tab closes this editor, but never over changes that are not saved (closeOpen).
  const me = {};
  $effect(() => {
    noteUnsaved(me, dirty ? 'The booking' : null);
    return () => noteUnsaved(me, null);
  });

  const showName = $derived(shows.find((s) => s.slug === f.show)?.name ?? item.show);
  const colour = $derived(shows.find((s) => s.slug === f.show)?.colour_slot ?? item.colour);
  /** A booking the week on screen already shows in the way, said before the station is asked. */
  const inTheWay = $derived(readOnly || !stretch || end <= start ? null : sharesWith(taken, start, end, isNew ? null : item.id));
  const airs = $derived(readOnly || !f.show ? '' : airsAfterWords(f.show, showName, end, blocks, zone));
  const about = $derived(problem?.about ?? refusal?.about ?? null);

  async function save() {
    if (busy || readOnly) return;
    const ask = bookingAsk(f, zone, now, was);
    refusal = null;
    if ('problem' in ask) return void (problem = { message: ask.problem, about: ask.about });
    problem = null;
    if ('same' in ask) return;
    busy = true;
    try {
      refusal = await onSave(ask.request, ask.patch);
    } finally {
      busy = false;
    }
  }

  function discard() {
    if (isNew) return onShows();
    f = fieldsOf(item.slug, item.start, item.end, zone);
    refusal = null;
    problem = null;
    touched = false;
  }

  async function remove() {
    if (busy) return;
    busy = true;
    try {
      refusal = await onDelete();
    } finally {
      busy = false;
    }
  }

  // A booking just begun takes the focus to its first field: Book… is at the head of the grid,
  // and the keyboard's way from there to here would be through every block of the day. A
  // booking or a session that was pressed leaves the focus where it was, as an episode does.
  let showField = $state<HTMLSelectElement | null>(null);
  $effect(() => {
    if (isNew) showField?.focus({ preventScroll: true });
  });

  const title = $derived(
    isNew ? 'Book the rehearsal studio' : item.kind === 'session' ? `${item.show} · ${item.open ? 'rehearsing now' : 'rehearsed'}` : `${item.show} · ${item.past ? 'was booked' : 'rehearsal booked'}`,
  );
  const crumb = $derived(isNew ? 'a new booking' : item.kind === 'session' ? 'a rehearsal' : 'a booking');
  const whenLine = $derived(item.kind === 'session' && item.open ? `${whenWords(item.start, item.start, zone).replace(/–.*$/, '')}–now` : whenWords(start, end, zone));
</script>

<form class="bk" novalidate onsubmit={(e) => { e.preventDefault(); void save(); }}>
  <div class="head">
    <div class="crumbs"><button type="button" class="back" title="Back to the list of shows (Esc)" aria-keyshortcuts="Escape" onclick={onShows}>‹ Shows</button> › <b>Rehearsal studio</b> › {crumb}</div>
    <h2 data-panel-title>{title}</h2>
    <div class="when">
      <span class="num">{whenLine}</span>
      {#if readOnly}<span class="state hollow hollow-standby sm"><svg aria-hidden="true"><use href="#g-lock" /></svg>To read</span>{/if}
    </div>
  </div>

  <div class="fields">
    {#if refusal}
      <div class="problem serious" role="alert" data-testid="booking-refused">
        <svg aria-hidden="true"><use href="#g-fault" /></svg>
        <div class="said">
          <span><b>Not saved.</b> {refusal.clash ? refusal.clash.words : refusal.message}</span>
          {#if refusal.clash}
            <span>Two bookings cannot share the studio. Choose another time, or change that booking first.</span>
            {#if onShowIt}<button type="button" class="act2" onclick={() => refusal?.clash && onShowIt(refusal.clash.id, refusal.clash.start)}>Show it</button>{/if}
          {/if}
        </div>
      </div>
    {:else if problem}
      <div class="problem serious" role="alert" data-testid="booking-problem">
        <svg aria-hidden="true"><use href="#g-fault" /></svg>
        <span>{problem.message}</span>
      </div>
    {:else if inTheWay}
      <div class="problem" role="status" data-testid="booking-shares">
        <svg aria-hidden="true"><use href="#g-fault" /></svg>
        <div class="said">
          <span><b>{inTheWay.show} has the studio</b> from {hm(inTheWay.start, zone)} to {hm(inTheWay.end, zone)}. The station refuses two bookings at one time.</span>
          {#if onShowIt}<button type="button" class="act2" onclick={() => onShowIt(inTheWay.id, inTheWay.start)}>Show it</button>{/if}
        </div>
      </div>
    {/if}

    {#if readOnly}
      <div class="frozen-note" data-testid="booking-read-only"><svg aria-hidden="true"><use href="#g-lock" /></svg><span>{readOnly}</span></div>
      <Field label="Show"><i class="sq" style:--c={item.colour ? `var(--cart-${item.colour})` : 'var(--deemph)'}></i>{item.show}</Field>
      <Field label="When"><span class="num">{whenLine}</span>{#if !(item.kind === 'session' && item.open)}<span class="len">{lengthWords(item.end - item.start)}</span>{/if}</Field>
      {#if item.kind === 'session'}
        <Field label={item.open ? 'Under way' : 'As it happened'} multi>{sessionWords(item, zone)}</Field>
      {:else if item.by}
        <Field label="Booked by">{item.by}</Field>
      {/if}
      <div class="nothing flat">
        {#if item.kind === 'session' && item.open}
          {#if onDesk}
            <p>The rehearsal studio is open for <b>{item.show}</b>. Nothing in it reaches the air. Its desk is where you take part, and where everyone is moved to the live studio.</p>
          {:else}
            <p>The rehearsal studio is open for <b>{item.show}</b>. Nothing in it reaches the air. To take part, or to move everyone to the live studio, open the <b>Desk</b> tab and choose the rehearsal studio there.</p>
          {/if}
        {:else if item.kind === 'session'}
          <p>This is the rehearsal studio as it was used. Nothing in it reached the air.</p>
        {:else}
          <p>A booking holds the rehearsal studio for its show: while it runs, nobody opens the studio for another show. It airs nothing.</p>
        {/if}
      </div>
    {:else}
      <Field label="Show" plain hint={shows.length > 1 ? 'The show the studio is held for. While the booking runs, nobody opens the studio for another show.' : 'While the booking runs, nobody opens the studio for another show.'}>
        <div class="row showpick">
          <i class="sq" style:--c={colour ? `var(--cart-${colour})` : 'var(--deemph)'}></i>
          <select bind:this={showField} bind:value={f.show} aria-label="The show the rehearsal studio is held for" aria-invalid={about === 'show' ? true : undefined} onchange={fieldChanged}>
            {#if !shows.some((s) => s.slug === f.show)}<option value={f.show}>{f.show ? item.show : 'Choose a show'}</option>{/if}
            {#each shows as s (s.slug)}<option value={s.slug}>{s.name}</option>{/each}
          </select>
        </div>
      </Field>
      <Field label="Day" plain hint={underWay ? 'This booking is under way: its day and its start stay as they are.' : ''}>
        <div class="row move">
          <input class="date" type="date" bind:value={f.date} min={isoDate(now, zone)} disabled={underWay} aria-label="The day, station time" aria-invalid={about === 'from' && !underWay ? true : undefined} onchange={fieldChanged} />
        </div>
      </Field>
      <Field label="From, until" plain hint={stretch && end > start ? `${lengthWords(end - start)}${overnight ? ', until the next day' : ''}. Station time.` : 'Station time.'}>
        <div class="row move">
          <TimeField bind:value={f.from} label="From, station time" disabled={underWay} invalid={about === 'from'} onchange={fieldChanged} oncarry={(d) => { f.date = addDays(f.date, d); fieldChanged(); }} />
          <span class="to">until</span>
          <TimeField bind:value={f.until} label="Until, station time" invalid={about === 'until'} onchange={fieldChanged} />
        </div>
      </Field>
      <div class="nothing flat">
        {#if airs}<p data-testid="booking-airs"><b>{airs}</b></p>{/if}
        <p>A booking only holds the studio. When the time comes, whoever presents <b>{showName || 'the show'}</b> opens the rehearsal studio from the <b>Desk</b> tab. Nothing in it reaches the air.</p>
      </div>
    {/if}
  </div>

  <div class="foot">
    {#if readOnly && item.kind === 'session' && item.open && onDesk}
      <!-- The one thing to do with a rehearsal under way: go to it. The Desk tab opens on the
           rehearsal studio; nothing is opened, moved or put on air by going there. -->
      <div class="r">
        <button type="button" class="btn primary" data-testid="to-rehearsal-desk" aria-disabled={deskWhy ? 'true' : undefined} aria-describedby="to-desk-note" disabled={going} aria-busy={going || undefined} onclick={toDesk}>Go to the rehearsal desk</button>
      </div>
      <p class="caption" id="to-desk-note">{deskWhy ?? 'Opens the Desk tab on the rehearsal studio. Live studio, at the head of that tab, is the way back to the live desk.'}</p>
    {:else if readOnly}
      <p class="caption">{item.kind === 'session' ? 'Sessions are drawn as they happened: they are not edited.' : 'Only a booking still to come, or under way, can be changed.'}</p>
    {:else}
      <div class="r">
        {#if !isNew}<button type="button" class="btn del" disabled={busy} onclick={remove} title="Free the rehearsal studio from this booking">Delete booking</button>{/if}
        <button type="button" class="btn quiet" disabled={busy || (!isNew && !dirty)} onclick={discard} title={isNew ? 'Leave without booking' : 'Put the fields back as the booking is'}>Discard</button>
        <button type="submit" class="btn primary" disabled={busy || (!isNew && !dirty)}>{isNew ? 'Book the studio' : 'Save booking'}</button>
      </div>
      <p class="caption">{isNew ? 'Not booked until Book the studio is pressed.' : underWay ? 'Under way: shorten or lengthen it, give it to another show, or delete it to free the studio.' : 'Drag the block in the Rehearsal column to move it, or its lower edge to change its length.'}</p>
    {/if}
  </div>
</form>
