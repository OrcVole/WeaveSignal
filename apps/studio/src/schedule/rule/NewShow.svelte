<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // New show (`calendar.new-show`; ADR-0278, new-show.md section 1): a dialog over the
  // Schedule tab in three parts. The show (its name, its colour with the next free bank slot
  // picked, its step with Solo picked), When it airs (`calendar.rule`), and Done. Nothing is
  // saved until Make the show; the station then saves the show, makes series 1 and drafts the
  // first episode in one transaction, so there is no half-made show. They are "parts": "step"
  // keeps its one meaning (ADR-0271). The dialog owns the keyboard (keymap rule 5).
  //
  // Part 1 also asks where the show comes from (`Comes from`, ADR-0280), on a station of 1.1 for
  // an admin or a producer: the station's room (the default) or the host's own studio. A show's
  // source follows from its keys, and the show does not exist until it is made, so the pick is
  // an intention: Done then offers Set up a host…, which makes the key (the same dialogs as the
  // show editor's) and the show comes from the host's own studio once there is one.
  import { untrack } from 'svelte';
  import * as api from '../../lib/api.ts';
  import type { IsoDate, RulePreview, ShowSaved, ShowStep } from '../../lib/api.ts';
  import { placeError } from '../../lib/account.ts';
  import * as sched from '../../lib/schedule.ts';
  import type { Show } from '../../lib/schedule.ts';
  import StepList from '../../lib/StepList.svelte';
  import { SHOW_NAME_MAX, freeSlug, startsAt } from '../../lib/wizard.ts';
  import { slugOf } from '../model.ts';
  import RulePart from './RulePart.svelte';
  import { dateWords, newForm, newShowInput, remembersWhole, setFirst, type RuleForm } from './form.ts';
  import { dialogKeys } from './keys.ts';
  import { firstState, firstWords, madeInAll, ruleStatus } from './view.ts';
  import { account } from '../../lib/account.svelte.ts';
  import { mayMakeFirstKey } from '../../lib/account.ts';
  import { routeAbsent } from '../../lib/hosts.ts';
  import HostKeyFlow from '../../settings/HostKeyFlow.svelte';

  let {
    shows,
    zone,
    today,
    zoneSet = true,
    canChooseZone = false,
    onzone,
    rules = true,
    at = null,
    part: firstPart = 1,
    name: firstName = '',
    form: firstForm = null,
    preview = null,
    saved: firstSaved = null,
    live = true,
    hosts: seedHosts = null,
    comes: firstComes = 'room',
    onMade,
    onOpen,
    onDates,
    onClose,
  }: {
    /** Every show, for the path and the next free colour. */
    shows: readonly Show[];
    zone: string;
    /** The station's date today. */
    today: IsoDate;
    zoneSet?: boolean;
    canChooseZone?: boolean;
    onzone?: () => void;
    /** The station stores rules (1.0.4 or later). */
    rules?: boolean;
    /** The block selected in the week: its day and hour start First on and from. */
    at?: { date: IsoDate; from: string } | null;
    /** The part to open on, the name, the answers, the preview and the saved show (a test's). */
    part?: 1 | 2 | 3;
    name?: string;
    form?: RuleForm | null;
    preview?: RulePreview | null;
    saved?: ShowSaved | null;
    live?: boolean;
    /** Whether Comes from is offered, a test's: left out, the station is asked (a station before
     *  1.1 has no sources, and the sandbox has no hosts). */
    hosts?: boolean | null;
    /** Where the show comes from to start with, a test's. */
    comes?: 'room' | 'push';
    /** The show was made: the week is fetched again. */
    onMade: (saved: ShowSaved) => void | Promise<void>;
    /** Open episode 1: the week moves to it and selects it. */
    onOpen: (saved: ShowSaved) => void;
    /** See its dates: the show's pane, at its Dates (week-clashes.md section 3). */
    onDates?: (saved: ShowSaved) => void;
    onClose: () => void;
  } = $props();

  const PARTS = ['The show', 'When it airs', 'Done'] as const;
  const seed = untrack(() => ({ firstPart, firstName, firstForm, firstSaved, at, today, shows, preview, hosts: seedHosts, comes: firstComes }));

  /** The next free bank slot: the first of the twelve no show holds, else round again. */
  function nextFree(all: readonly Show[]): number {
    for (let n = 1; n <= 12; n++) if (!all.some((s) => s.colour_slot === n)) return n;
    return (all.length % 12) + 1;
  }
  const until = (from: string) => {
    const [h, m] = from.split(':').map(Number);
    return `${String((h + 1) % 24).padStart(2, '0')}:${String(m).padStart(2, '0')}`;
  };
  function firstAnswers(): RuleForm {
    if (seed.firstForm) return seed.firstForm;
    const f = seed.at ? newForm(seed.at.date, seed.at.from, until(seed.at.from)) : newForm(seed.today);
    return { ...f, mode: remembersWhole(account.me?.account?.id) ? 'full' : 'list' };
  }

  let part = $state<1 | 2 | 3>(seed.firstPart);
  let name = $state(seed.firstName);
  const free = nextFree(seed.shows);
  let colour = $state(free);
  let step = $state<ShowStep>('solo');
  /** Comes from: where the show's sound arrives (ADR-0280). */
  let comes = $state<'room' | 'push'>(seed.comes);
  /** Whether the station can take a host's own studio, and this window may make the key. */
  let canHost = $state(seed.hosts ?? false);
  $effect(() => {
    if (seed.hosts !== null || !account.loaded) return;
    if (api.sandboxed() || !mayMakeFirstKey(account.me)) return void (canHost = false);
    let gone = false;
    api.listSources().then(
      () => !gone && (canHost = true),
      (e) => !gone && (canHost = !routeAbsent(e) && (e as { status?: number } | null)?.status !== 403),
    );
    return () => (gone = true);
  });
  /** The key's dialogs, which Done opens. */
  let flow = $state<HostKeyFlow | null>(null);
  /** A key was made for the new show. */
  let keyed = $state(false);
  /** A host sending to a show with callers wants SRT, whose timing the callers' return needs
   *  (hosts-in-their-own-studios.md section 4.1); Icecast is the simpler start. */
  const hostWay = $derived<'srt' | 'icecast'>(step === 'callers' || step === 'full' ? 'srt' : 'icecast');
  let form = $state<RuleForm>(firstAnswers());
  /** The block First on and from were taken from, until the person changes the date or asks
   *  for today: it is said under First on, because a date nobody typed is easy to miss (the
   *  1.0.4 test run made shows on the selected block's day without seeing it). */
  let seeded = $state(seed.firstForm ? null : seed.at);
  const seededNote = $derived.by(() => {
    if (!seeded || form.first !== seeded.date) return null;
    const timed = form.from === seeded.from && form.until === until(seeded.from);
    const day = seeded.date === today ? 'today' : dateWords(seeded.date, { weekday: 'long' });
    if (seeded.date === today && !timed) return null;
    return {
      words: timed ? `${day}, ${seeded.from} to ${until(seeded.from)}` : day,
      clear: seeded.date === today ? `Use ${newForm(today).from} instead` : 'Use today instead',
    };
  });
  /** Use today instead: First on goes back to today, and the times to the usual hour if they
   *  are still the block's. Everything else answered is kept. */
  function unseed() {
    const s = seeded;
    if (!s) return;
    seeded = null;
    const usual = newForm(today);
    const timed = form.from === s.from && form.until === until(s.from);
    form = { ...setFirst(form, today), ...(timed ? { from: usual.from, until: usual.until, overnightOk: false } : {}) };
    focus('ns-first');
  }
  let pst = $state(firstState(seed.preview));
  const status = $derived(ruleStatus(form, pst, rules));
  let errors = $state<{ name?: string; form?: string }>({});
  let saving = $state(false);
  let saved = $state<ShowSaved | null>(seed.firstSaved);
  /** What the preview said the rule gives in all, kept for Done's *Draft all 10*. */
  let total = $state<number | null>(null);
  let draftedAll = $state(false);
  let drafting = $state(false);
  let discarding = $state(false);
  let dialogEl = $state<HTMLDialogElement | null>(null);
  let confirmEl = $state<HTMLDialogElement | null>(null);

  const initialForm = JSON.stringify(untrack(() => form));
  const dirty = $derived(!saved && (name.trim() !== '' || part > 1 || JSON.stringify(form) !== initialForm));
  const starts = $derived(startsAt(step));
  const firstEp = $derived(saved?.drafted?.episode ?? null);
  const firstWhen = $derived(firstEp?.air_at ? firstWords({ start: firstEp.air_at, minutes: firstEp.minutes ?? saved?.show.rule?.minutes ?? 60 }, zone, today) : '');
  const madeRule = $derived(saved ? (saved.show.rule ?? null) : null);
  const hasDates = $derived(!!saved && (!!madeRule || !!saved.show.slot));
  const rest = $derived(madeRule && madeRule.repeat.kind !== 'once' && total !== null && total > 1 ? total : null);

  const focus = (id: string) => queueMicrotask(() => document.getElementById(id)?.focus());

  $effect(() => {
    const d = dialogEl;
    if (d && !d.open) {
      d.showModal();
      focus(part === 1 ? 'ns-name' : part === 2 ? 'ns-first' : 'ns-h');
    }
  });
  $effect(() => {
    const c = confirmEl;
    if (c && !c.open) c.showModal();
  });

  function nameProblem(): string {
    const n = name.trim();
    if (!n) return 'Give the show a name.';
    if (n.length > SHOW_NAME_MAX) return `At most ${SHOW_NAME_MAX} characters.`;
    if (!slugOf(n)) return 'The name needs at least one letter or digit.';
    return '';
  }

  function next(e?: Event) {
    e?.preventDefault();
    const p = nameProblem();
    errors = p ? { name: p } : {};
    if (p) return focus('ns-name');
    part = 2;
    focus('ns-first');
  }

  function back() {
    errors = {};
    part = 1;
    focus('ns-name');
  }

  /** Make the show: with its dates and the first episode drafted, or with no dates yet. */
  async function make(withDates: boolean) {
    if (saving || (withDates && (status.held || !status.rule))) return;
    saving = true;
    errors = {};
    try {
      const rule = withDates ? status.rule : null;
      total = withDates ? madeInAll(rule, status.answer) : null;
      const v = await api.saveShow(freeSlug(name, shows.map((s) => s.slug)), newShowInput({ name, colour, step }, rule, rules));
      saved = { ...v, show: sched.asShow(v.show) };
      part = 3;
      focus('ns-h');
      await onMade(saved);
    } catch (err) {
      const placed = placeError(err, ['name'] as const);
      if (placed.field === 'name') {
        // The name does not fit the cards: the station's own words, with part 1 shown again.
        part = 1;
        errors = { name: placed.message };
        focus('ns-name');
      } else {
        errors = { form: placed.message };
      }
    } finally {
      saving = false;
    }
  }

  /** Draft the rest, as Draft the season does. */
  async function draftAll() {
    if (!saved || rest === null || drafting) return;
    drafting = true;
    errors = {};
    try {
      await sched.createEpisodes(saved.show.slug, saved.series.at(-1)?.number ?? 1, { count: rest - (firstEp ? 1 : 0) });
      draftedAll = true;
      await onMade(saved);
    } catch (err) {
      errors = { form: err instanceof Error ? err.message : String(err) };
    } finally {
      drafting = false;
    }
  }

  /** Close, Cancel or the second Esc: anything typed is asked about once. */
  function leave() {
    if (dirty) discarding = true;
    else dialogEl?.close();
  }
  function keepEditing() {
    discarding = false;
    focus(part === 1 ? 'ns-name' : 'ns-first');
  }

  function keys(e: KeyboardEvent) {
    if (!dialogEl || discarding) return;
    dialogKeys(e, dialogEl, leave, () => {
      if (part === 1) next();
      else if (part === 2) void make(true);
      else if (saved && firstEp) openFirst();
    });
  }

  function openFirst() {
    if (saved) onOpen(saved);
    dialogEl?.close();
  }

  /** Done's Set up a host…: the setup for the show just made, with SRT offered for a show with callers. */
  function setUpHost() {
    if (saved) flow?.setup({ show: saved.show.slug, fixed: true, hint: hostWay });
  }

  function seeDates() {
    if (saved) onDates?.(saved);
    dialogEl?.close();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<dialog bind:this={dialogEl} class="wz" class:wide={part === 2 && rules} class:mid={part === 2 && !rules} data-panel="calendar.new-show" aria-labelledby="ns-h" tabindex="-1" onkeydown={keys} oncancel={(e) => { e.preventDefault(); leave(); }} onclose={onClose}>
  <div class="head">
    <p class="crumbs" aria-hidden="true">
      {#each PARTS as p, i (p)}<span class:now={part === i + 1} class:done={part > i + 1}>{i + 1} {p}</span>{/each}
    </p>
    <h2 id="ns-h" data-panel-title tabindex="-1">New show{#if part === 2 && name.trim()}{' '}<span class="of">· {name.trim()}</span>{/if}</h2>
    <p class="small"><span class="sr-only">Part {part} of 3: {PARTS[part - 1]}. </span>
      {#if part === 1}Name it and pick how much of the studio it starts with. The rest can wait.
      {:else if part === 2}Give one date and time, then say whether it comes back.
      {:else}Made, and on the schedule.{/if}
    </p>
  </div>

  {#if part === 1}
    <form onsubmit={next} novalidate>
      <div class="field">
        <label for="ns-name">Name</label>
        <input id="ns-name" type="text" bind:value={name} maxlength={SHOW_NAME_MAX} autocomplete="off" aria-invalid={errors.name ? true : undefined} aria-describedby={errors.name ? 'ns-name-err ns-name-hint' : 'ns-name-hint'} />
        <span id="ns-name-hint" class="hint">Up to three lines on the card at its smallest size.</span>
        {#if errors.name}<p id="ns-name-err" class="err" role="alert">{errors.name}</p>{/if}
      </div>
      <div class="field">
        <span class="lab2" id="ns-colour">Colour</span>
        <div class="swatches" role="group" aria-labelledby="ns-colour" aria-describedby="ns-colour-hint">
          {#each Array.from({ length: 12 }, (_, i) => i + 1) as n (n)}
            <button type="button" aria-pressed={colour === n} aria-label="Bank {n}" style="--sw: var(--cart-{n})" onclick={() => (colour = n)}><i></i>{n}</button>
          {/each}
        </div>
        <span id="ns-colour-hint" class="hint">The same colour on the cart wall, the week and the cards. Bank {free} is the next one free.</span>
      </div>
      <div class="field">
        <span class="lab2" id="ns-step">Step</span>
        <StepList bind:value={step} name="ns-step" describedby="ns-step ns-step-hint" />
        <span id="ns-step-hint" class="hint">How much of the studio this show uses. Start at Solo; move up a step whenever you like, at the top of the studio.</span>
      </div>
      {#if canHost}
        <div class="field" data-testid="ns-comes">
          <span class="lab2" id="ns-comes">Comes from</span>
          <span class="seg" role="group" aria-labelledby="ns-comes" aria-describedby="ns-comes-hint">
            <button type="button" aria-pressed={comes === 'room'} onclick={() => (comes = 'room')}>The station’s room</button>
            <button type="button" aria-pressed={comes === 'push'} onclick={() => (comes = 'push')}>The host’s own studio</button>
          </span>
          <span id="ns-comes-hint" class="hint">
            {#if comes === 'push'}The host sends from their own software with a key that belongs to this show. You make the key when the show is made.{:else}The presenter and guests join the studio, as they do today.{/if}
          </span>
        </div>
      {/if}
      <div class="acts">
        <button type="button" class="btn quiet left" onclick={leave}>Cancel</button>
        <button type="submit" class="btn primary">Next: when it airs</button>
      </div>
    </form>
  {:else if part === 2}
    <form onsubmit={(e) => { e.preventDefault(); void make(true); }} novalidate>
      <RulePart bind:form bind:state={pst} {zone} {today} {zoneSet} {canChooseZone} {onzone} {rules} {colour} idp="ns" {live} seeded={seededNote} onunseed={unseed} />
      <div class="foot2">
        {#if errors.form}<p class="err" role="alert">{errors.form}</p>{/if}
        <div class="acts">
          <button type="button" class="btn quiet left" onclick={back} disabled={saving}>Back</button>
          <button type="button" class="btn quiet" onclick={() => make(false)} disabled={saving}>No dates yet</button>
          <button type="submit" class="btn primary" disabled={saving || !!status.held} title={status.held || undefined} aria-busy={saving || undefined}>{saving ? 'Making the show…' : status.clash ? 'Make the show anyway' : 'Make the show'}</button>
        </div>
      </div>
    </form>
  {:else}
    <div class="done">
      <p class="lead"><svg class="g-good" aria-hidden="true"><use href="#g-good" /></svg><span><strong>{saved?.show.name ?? name}</strong> is a show.</span></p>
      <ul>
        {#if firstEp}
          <li><b class="ink">Episode {firstEp.number} is drafted</b>{firstWhen ? ` for ${firstWhen}` : ''}. Give it a title and guests when you know them.</li>
          {#if madeRule?.repeat.kind === 'once'}
            <li>It airs once, so there is nothing more to draft.</li>
          {:else if draftedAll}
            <li>All {rest} are drafted, and wait in the week for their titles and guests.</li>
          {:else}
            <li>The other dates wait in the week as dotted blocks. Draft each when you are ready{#if rest}, or all {rest} now{/if}.{#if onDates}{' '}They are listed under the show’s <b class="ink">Dates</b>.{/if}</li>
          {/if}
        {:else if hasDates}
          <li>Its dates wait in the week as dotted blocks. Select one to draft the episode.{#if onDates}{' '}They are listed under the show’s <b class="ink">Dates</b>.{/if}</li>
        {:else}
          <li>It has no dates yet, so nothing is drafted. Give it dates from the Schedule tab.</li>
        {/if}
        <li data-step={step}><b class="ink">{starts.lead}</b> {starts.rest}</li>
        {#if canHost && comes === 'push' && saved}
          <li data-testid="ns-host" data-keyed={keyed || undefined}>
            {#if keyed}<b class="ink">Its host has a key.</b> {saved.show.name} comes from their own studio; they arrive standing by, and go on air at their time.
            {:else}<b class="ink">It comes from the host’s own studio once a key is made.</b> Until then it is the station’s room. <button type="button" class="btn sm primary hostbtn" onclick={setUpHost}>Set up a host…</button>{/if}
          </li>
        {/if}
        <li>Hosts, a description, the card and the presenter’s details are in the show editor.</li>
      </ul>
      {#if errors.form}<p class="err" role="alert">{errors.form}</p>{/if}
      <div class="acts">
        <button type="button" class="btn quiet left" onclick={() => dialogEl?.close()}>Close</button>
        {#if onDates && hasDates && saved}<button type="button" class="btn quiet" onclick={seeDates}>See its dates</button>{/if}
        {#if firstEp && rest && !draftedAll}<button type="button" class="btn" onclick={draftAll} disabled={drafting} aria-busy={drafting || undefined}>{drafting ? 'Drafting…' : `Draft all ${rest}`}</button>{/if}
        {#if firstEp}<button type="button" class="btn primary" onclick={openFirst}>Open episode {firstEp.number}</button>{/if}
      </div>
    </div>
  {/if}
</dialog>

{#if discarding}
  <dialog bind:this={confirmEl} class="wz confirm" role="alertdialog" aria-labelledby="ns-discard-h" aria-describedby="ns-discard-p" oncancel={(e) => { e.preventDefault(); keepEditing(); }}>
    <div class="head">
      <h2 id="ns-discard-h">Discard this show?</h2>
      <p id="ns-discard-p" class="small">Nothing has been saved.</p>
    </div>
    <div class="acts">
      <button type="button" class="btn danger-hollow" onclick={() => { discarding = false; dialogEl?.close(); }}>Discard this show</button>
      <!-- svelte-ignore a11y_autofocus -->
      <button type="button" class="btn primary" autofocus onclick={keepEditing}>Keep editing</button>
    </div>
  </dialog>
{/if}

{#if saved && canHost && comes === 'push'}
  <!-- A sibling of the dialog, not inside it: its keys must not reach New show's (rule 5), and
       the secret's dialog has one way out of its own. -->
  <HostKeyFlow bind:this={flow} shows={[{ slug: saved.show.slug, name: saved.show.name, holders: [] }]} onChanged={() => { keyed = true; }} />
{/if}
