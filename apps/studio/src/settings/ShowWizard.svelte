<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The DJ's show setup (ADR-0218, ADR-0219): three steps in a dialog over the surface.
  // About you (the on-air name and the takedown contact), Your show (its name, the links to
  // your channels, the usual day and time, whether it plays commercial music), Done (what
  // the answers now do). The answers become the show's own fields in the station, tied to
  // this account, so the consent words name the DJ while the show is on air (ADR-0214) and
  // new episodes take the usual slot. It opens by itself for a newly signed-in presenter
  // with no show of their own, and again from Settings. No email is sent or needed. Panel
  // name `studio.show-setup` (ADR-0070). A dialog owns the keyboard (keymap rule 5): only
  // the ⌃ chords reach the keymap while it is open.
  import { untrack } from 'svelte';
  import type { Me } from '../lib/api.ts';
  import { placeError } from '../lib/account.ts';
  import * as api from '../lib/schedule.ts';
  import type { Show, ShowView } from '../lib/schedule.ts';
  import {
    CONTACT_MAX,
    MUSIC_REMINDER,
    PRESENTER_MAX,
    SHOW_NAME_MAX,
    WEEKDAYS,
    aboutProblems,
    freeSlug,
    fromShow,
    nextColour,
    showProblems,
    slotLine,
    splitLinks,
    toShowInput,
    overnight,
    type Answers,
  } from '../lib/wizard.ts';

  let {
    me,
    shows,
    initial = null,
    step: firstStep = 1,
    onDone,
    onSkip,
    onClose,
  }: {
    me: Me;
    /** Every show, for the path check and the colour; the one tied to `me` is the one changed. */
    shows: readonly Show[];
    /** The show this account presents, when the wizard is run again. */
    initial?: Show | null;
    /** The step to open on (a test's). */
    step?: 1 | 2 | 3;
    /** The show was saved. */
    onDone: (saved: ShowView) => void;
    /** Skipped for now: not asked again on this machine. */
    onSkip: () => void;
    onClose: () => void;
  } = $props();

  const seed = untrack(() => ({ initial, step: firstStep }));
  let step = $state<1 | 2 | 3>(seed.step);
  let a = $state<Answers>(untrack(() => fromShow(seed.initial, me)));
  let errors = $state<{ presenter?: string; contact?: string; show?: string; links?: string; time?: string; form?: string }>({});
  let saving = $state(false);
  /** The past-midnight slot confirmed (ADR-0244); asked again when the times change. */
  let overnightOk = $state(false);
  let overnightFor = '';
  let saved = $state<ShowView | null>(null);
  let dialogEl = $state<HTMLDialogElement | null>(null);

  const again = $derived(seed.initial !== null);
  const steps = ['About you', 'Your show', 'Done'] as const;
  const musicNote = $derived(a.commercial_music ? MUSIC_REMINDER : '');
  const linkCount = $derived(splitLinks(a.links).length);

  $effect(() => {
    const key = `${a.from}-${a.until}`;
    if (key !== overnightFor) {
      overnightFor = key;
      overnightOk = false;
    }
  });

  function focusFirst(id: string) {
    queueMicrotask(() => document.getElementById(id)?.focus());
  }

  $effect(() => {
    const d = dialogEl;
    if (d && !d.open) {
      d.showModal();
      focusFirst(step === 1 ? 'sw-presenter' : 'sw-show');
    }
  });

  function next() {
    if (step === 1) {
      const p = aboutProblems(a);
      errors = p;
      if (p.presenter || p.contact) return focusFirst(p.presenter ? 'sw-presenter' : 'sw-contact');
      step = 2;
      focusFirst('sw-show');
    }
  }

  function back() {
    errors = {};
    step = 1;
    focusFirst('sw-presenter');
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (saving) return;
    const p = showProblems(a);
    if (!p.time && overnight(a) && !overnightOk) p.time = 'It ends before it starts, so it runs past midnight. Tick the box below if it does, or change the times.';
    errors = p;
    if (p.show || p.links || p.time) return focusFirst(p.show ? 'sw-show' : p.links ? 'sw-links' : p.time && overnight(a) ? 'sw-overnight' : 'sw-from');
    saving = true;
    try {
      // The show this account presents is changed in place; a new one takes a free path.
      const existing = seed.initial ? await api.show(seed.initial.slug) : null;
      const slug = existing ? existing.show.slug : freeSlug(a.show, shows.map((s) => s.slug));
      const v = await api.putShow(slug, toShowInput(a, me, existing, nextColour(shows.length)));
      saved = v;
      step = 3;
      focusFirst('sw-done');
    } catch (err) {
      const placed = placeError(err, ['name', 'presenter', 'contact', 'links', 'slot'] as const);
      const field = placed.field === 'name' ? 'show' : placed.field === 'slot' ? 'time' : placed.field;
      errors = field ? { [field]: placed.message } : { form: placed.message };
    } finally {
      saving = false;
    }
  }

  function skip() {
    onSkip();
    dialogEl?.close();
  }

  function finish() {
    if (saved) onDone(saved);
    dialogEl?.close();
  }

  function onCancel(e: Event) {
    // Esc: the dialog's own key. Leaving without saving is not a skip: it asks again next time.
    e.preventDefault();
    dialogEl?.close();
  }
</script>

<dialog bind:this={dialogEl} data-panel="studio.show-setup" aria-labelledby="sw-h" oncancel={onCancel} onclose={onClose}>
  <div class="head">
    <p class="crumbs" aria-hidden="true">
      {#each steps as s, i (s)}
        <span class:now={step === i + 1} class:done={step > i + 1}>{i + 1} {s}</span>
      {/each}
    </p>
    <h2 id="sw-h" data-panel-title>{again ? 'Your show' : 'Set up your show'}</h2>
    <p class="small"><span class="sr-only">Step {step} of 3: {steps[step - 1]}. </span>
      {#if step === 1}
        What callers hear about you. Two questions, then your show.
      {:else if step === 2}
        Your show in the station: its name, where people find you, and when it is on.
      {:else}
        {again ? 'Your show is up to date.' : 'That is everything the station needs.'}
      {/if}
    </p>
  </div>

  {#if step === 1}
    <form onsubmit={(e) => { e.preventDefault(); next(); }} novalidate>
      <div class="field">
        <label for="sw-presenter">Your on-air name</label>
        <input
          id="sw-presenter"
          type="text"
          bind:value={a.presenter}
          maxlength={PRESENTER_MAX}
          autocomplete="nickname"
          aria-invalid={errors.presenter ? true : undefined}
          aria-describedby={errors.presenter ? 'sw-presenter-err sw-presenter-hint' : 'sw-presenter-hint'}
        />
        <span id="sw-presenter-hint" class="hint">The name callers hear and the schedule shows.</span>
        {#if errors.presenter}<p id="sw-presenter-err" class="err">{errors.presenter}</p>{/if}
      </div>
      <div class="field">
        <label for="sw-contact">Takedown contact</label>
        <input
          id="sw-contact"
          type="text"
          bind:value={a.contact}
          maxlength={CONTACT_MAX}
          autocomplete="off"
          autocapitalize="none"
          spellcheck="false"
          placeholder="an email address or a link"
          aria-invalid={errors.contact ? true : undefined}
          aria-describedby={errors.contact ? 'sw-contact-err sw-contact-hint' : 'sw-contact-hint'}
        />
        <span id="sw-contact-hint" class="hint">
          Where a caller asks for their call to be taken down. While your show is on air the consent words give it first, then
          the station’s own as the host. Leave it empty and they give the station’s alone.
        </span>
        {#if errors.contact}<p id="sw-contact-err" class="err">{errors.contact}</p>{/if}
      </div>
      <div class="acts">
        {#if !again}<button type="button" class="btn quiet" onclick={skip}>Skip for now</button>{/if}
        <button type="submit" class="btn primary">Next: your show</button>
      </div>
    </form>
  {:else if step === 2}
    <form onsubmit={save} novalidate>
      <div class="field">
        <label for="sw-show">Show name</label>
        <input
          id="sw-show"
          type="text"
          bind:value={a.show}
          maxlength={SHOW_NAME_MAX}
          autocomplete="off"
          aria-invalid={errors.show ? true : undefined}
          aria-describedby={errors.show ? 'sw-show-err' : undefined}
        />
        {#if errors.show}<p id="sw-show-err" class="err">{errors.show}</p>{/if}
      </div>
      <div class="field">
        <label for="sw-links">Links to your channels</label>
        <textarea
          id="sw-links"
          bind:value={a.links}
          rows="3"
          autocapitalize="none"
          spellcheck="false"
          placeholder={'https://rumble.com/c/…\nhttps://odysee.com/@…'}
          aria-invalid={errors.links ? true : undefined}
          aria-describedby={errors.links ? 'sw-links-err sw-links-hint' : 'sw-links-hint'}
        ></textarea>
        <span id="sw-links-hint" class="hint">One per line: Rumble, Odysee, AzuraCast, wherever your audience already is.{#if linkCount}<span class="count">{linkCount} {linkCount === 1 ? 'link' : 'links'}.</span>{/if}</span>
        {#if errors.links}<p id="sw-links-err" class="err">{errors.links}</p>{/if}
      </div>
      <fieldset class="field when" aria-describedby={errors.time ? 'sw-time-err sw-time-hint' : 'sw-time-hint'}>
        <legend>Usual day and time</legend>
        <div class="row">
          <select id="sw-weekday" bind:value={a.weekday} aria-label="Weekday">
            {#each WEEKDAYS as d, i (d)}<option value={i + 1}>{d}</option>{/each}
          </select>
          <label class="t"><span aria-hidden="true">from</span><input id="sw-from" type="time" bind:value={a.from} aria-label="Start, station time" aria-invalid={errors.time ? true : undefined} /></label>
          <label class="t"><span aria-hidden="true">until</span><input id="sw-until" type="time" bind:value={a.until} aria-label="End, station time" aria-invalid={errors.time ? true : undefined} /></label>
        </div>
        <span id="sw-time-hint" class="hint">Station time. {slotLine(a)}. New episodes start from it; leave the times empty if the show has no usual slot yet.</span>
        {#if errors.time}<p id="sw-time-err" class="err">{errors.time}</p>{/if}
        {#if overnight(a)}
          <label class="check-inline"><input id="sw-overnight" type="checkbox" bind:checked={overnightOk} /> Yes, it runs past midnight</label>
        {/if}
      </fieldset>
      <div class="check">
        <input id="sw-music" type="checkbox" bind:checked={a.commercial_music} aria-describedby="sw-music-hint" />
        <label for="sw-music">The show plays commercial music</label>
        <p id="sw-music-hint" class="hint" role={a.commercial_music ? 'note' : undefined}>{musicNote || 'Released records, rather than your own or licence-free music.'}</p>
      </div>
      {#if errors.form}<p class="err" role="alert">{errors.form}</p>{/if}
      <div class="acts">
        <button type="button" class="btn quiet" onclick={back} disabled={saving}>Back</button>
        <button type="submit" class="btn primary" disabled={saving} aria-busy={saving || undefined}>{saving ? 'Saving…' : again ? 'Save changes' : 'Save my show'}</button>
      </div>
    </form>
  {:else}
    <div class="done" id="sw-done" tabindex="-1">
      <p class="lead"><svg class="g-good" aria-hidden="true"><use href="#g-good" /></svg><strong>{saved?.show.name ?? a.show}</strong> is set up as your show.</p>
      <ul>
        <li>While it is on air, callers agree to be recorded under your name{a.contact.trim() ? ', with your takedown contact first and the station’s as the host' : ', with the station’s takedown contact'}.</li>
        <li>{slotLine(a) === 'No usual time yet' ? 'It has no usual time yet; set one from the Schedule tab when you know it.' : `New episodes start on ${slotLine(a)}.`}</li>
        {#if a.commercial_music}<li>{MUSIC_REMINDER}</li>{/if}
        <li>Change any of this from Settings, under <b>Your show</b>, or on the Schedule tab.</li>
      </ul>
      <div class="acts">
        <button type="button" class="btn primary" onclick={finish}>Done</button>
      </div>
    </div>
  {/if}
</dialog>

<style>
  .check-inline { display: flex; align-items: center; gap: var(--s-2); margin-top: var(--s-2); font-size: var(--fs-14); }
  dialog {
    width: min(32rem, calc(100vw - 2 * var(--s-4)));
    margin: 3rem auto auto;
    background: var(--surface);
    color: var(--ink);
    border: 1px solid var(--axis);
    border-radius: var(--r-3);
    box-shadow: var(--e-3);
    padding: var(--s-5);
    display: none;
    gap: var(--s-4);
  }
  dialog[open] {
    display: grid;
  }
  dialog::backdrop {
    background: var(--scrim);
  }
  .head {
    display: grid;
    gap: var(--s-1);
  }
  .crumbs {
    display: flex;
    gap: var(--s-3);
    font-size: var(--fs-12);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
    color: var(--muted);
    font-weight: 600;
  }
  .crumbs .now {
    color: var(--ink);
    border-bottom: 2px solid var(--accent);
  }
  .crumbs .done {
    color: var(--ink-2);
  }
  h2 {
    font-size: var(--fs-20);
  }
  .small {
    line-height: var(--lh-body);
    color: var(--ink-2);
  }
  form,
  .done {
    display: grid;
    gap: var(--s-3);
  }
  .field {
    display: grid;
    gap: var(--s-1);
    border: 0;
    padding: 0;
    margin: 0;
    min-width: 0;
  }
  label,
  legend {
    font-weight: 600;
    padding: 0;
  }
  input,
  select,
  textarea {
    font: inherit;
    color: var(--ink);
    background: var(--plane);
    border: 1px solid var(--muted);
    border-radius: var(--r-2);
    min-height: 2.25rem;
    padding: 0 var(--s-2);
    width: 100%;
    color-scheme: dark;
  }
  textarea {
    padding: var(--s-1) var(--s-2);
    line-height: var(--lh-body);
    resize: vertical;
    font-family: var(--font-mono);
    font-size: var(--fs-13);
  }
  :global(:root[data-theme='light']) input,
  :global(:root[data-theme='light']) select,
  :global(:root[data-theme='light']) textarea {
    color-scheme: light;
  }
  input[aria-invalid='true'],
  textarea[aria-invalid='true'] {
    border-color: var(--status-critical);
    border-width: 2px;
  }
  .row {
    display: flex;
    gap: var(--s-2);
    align-items: center;
    flex-wrap: wrap;
  }
  .row select {
    width: auto;
    flex: 1 1 8rem;
    min-width: 0;
  }
  .count {
    color: var(--ink-2);
    margin-left: 0.4em;
  }
  .t {
    display: inline-flex;
    align-items: center;
    gap: var(--s-1);
    font-weight: 400;
    color: var(--ink-2);
  }
  .t input {
    width: 6.5rem;
  }
  .check {
    display: grid;
    grid-template-columns: auto 1fr;
    column-gap: var(--s-2);
    align-items: center;
  }
  .check input {
    width: 1.1rem;
    height: 1.1rem;
    min-height: 0;
    margin: 0;
    accent-color: var(--accent);
  }
  .check .hint {
    grid-column: 2;
  }
  .hint {
    display: block;
    font-size: var(--fs-13);
    color: var(--muted);
    line-height: var(--lh-body);
  }
  .err {
    color: var(--ink);
    font-size: var(--fs-13);
    line-height: var(--lh-body);
    border-left: 3px solid var(--status-critical);
    padding-left: var(--s-2);
  }
  .acts {
    display: flex;
    gap: var(--s-2);
    justify-content: flex-end;
    flex-wrap: wrap;
    margin-top: var(--s-1);
  }
  .lead {
    display: flex;
    gap: var(--s-2);
    align-items: center;
    line-height: var(--lh-body);
  }
  .lead svg {
    width: 1.25rem;
    height: 1.25rem;
    flex: none;
  }
  .done ul {
    margin: 0;
    padding-left: 1.25rem;
    display: grid;
    gap: var(--s-1);
    line-height: var(--lh-body);
    color: var(--ink-2);
  }
  .done:focus {
    outline: none;
  }
</style>
