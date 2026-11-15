<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The rule dialog (`calendar.rule` alone; ADR-0278, new-show.md section 3): when a show, or
  // one of its series, airs. Opened by Change… on When it airs in the show editor, Set here on
  // the series editor's rule, and Give it dates… beside a show with nothing coming up. For a
  // show with a past, First on becomes Changes from, and before the save the dialog turns to
  // Before you save (`calendar.rule.change`), which lists what moves. Aired episodes are never
  // changed. Saving sends the rule alone, with whether the drafted episodes move or stay.
  import { untrack } from 'svelte';
  import * as api from '../../lib/api.ts';
  import type { DraftedChoice, IsoDate, Rule, RuleChanged, RulePreview } from '../../lib/api.ts';
  import * as sched from '../../lib/schedule.ts';
  import { account } from '../../lib/account.svelte.ts';
  import RuleChange from './RuleChange.svelte';
  import RulePart from './RulePart.svelte';
  import { fromRule, newForm, remembersWhole, slotOf, type RuleForm } from './form.ts';
  import { dialogKeys } from './keys.ts';
  import { airedWords, countWords, firstState, hasEpisodes, ruleStatus, saveWords } from './view.ts';

  let {
    slug,
    name,
    series = null,
    rule = null,
    follow = null,
    reset = false,
    nextOn = null,
    colour = null,
    zone,
    today,
    zoneSet = true,
    canChooseZone = false,
    onzone,
    rules = true,
    preview = null,
    changing = false,
    live = true,
    onSaved,
    onClose,
  }: {
    /** The show's path and name. */
    slug: string;
    name: string;
    /** The series whose own rule this is; null for the show's. */
    series?: number | null;
    /** The rule as stored at this level; null when there is none. */
    rule?: Rule | null;
    /** A series' show's rule: what the questions start from when the series has none of its own. */
    follow?: Rule | null;
    /** Reset to show: the series follows its show again. Only Before you save is shown. */
    reset?: boolean;
    /** The show's next date that has not aired, for Changes from. */
    nextOn?: IsoDate | null;
    colour?: number | null;
    zone: string;
    today: IsoDate;
    zoneSet?: boolean;
    canChooseZone?: boolean;
    onzone?: () => void;
    rules?: boolean;
    /** The first answer and the part shown (a test's). */
    preview?: RulePreview | null;
    changing?: boolean;
    live?: boolean;
    onSaved: (changed: RuleChanged | null) => void | Promise<void>;
    onClose: () => void;
  } = $props();

  const seed = untrack(() => ({ rule, follow, reset, nextOn, today, changing, preview }));
  const from = seed.reset ? seed.follow : (seed.rule ?? seed.follow);

  function firstAnswers(): RuleForm {
    const whole = remembersWhole(account.me?.account?.id);
    if (!from) return { ...newForm(seed.today), mode: whole ? 'full' : 'list' };
    // The new rule starts at the next date that has not aired; what came before is in the
    // episodes, not in the rule.
    const start = from.starts_on && from.starts_on >= seed.today ? from.starts_on : (seed.nextOn ?? seed.today);
    return fromRule({ ...from, starts_on: start }, start, whole);
  }

  let form = $state<RuleForm>(firstAnswers());
  let pst = $state(firstState(seed.preview));
  const status = $derived(ruleStatus(form, pst, rules));
  let step = $state<'questions' | 'change'>(seed.changing || seed.reset ? 'change' : 'questions');
  let choice = $state<DraftedChoice>('move');
  let saving = $state(false);
  let problem = $state('');
  let discarding = $state(false);
  let dialogEl = $state<HTMLDialogElement | null>(null);
  let confirmEl = $state<HTMLDialogElement | null>(null);

  const initialForm = JSON.stringify(untrack(() => form));
  const dirty = $derived(JSON.stringify(form) !== initialForm);
  // The plan last given stays while the rule dialog is on Before you save, where nothing is asked again.
  let lastPlan = $state(seed.preview?.change ?? null);
  let lastSentence = $state(seed.preview?.sentence ?? '');
  $effect(() => {
    if (status.answer) {
      lastPlan = status.answer.change;
      lastSentence = status.answer.sentence;
    }
  });
  const plan = $derived(lastPlan);
  const past = $derived(!!from && (plan?.aired ?? 0) > 0);
  const title = $derived(series ? `When series ${series} of ${name} airs` : `When ${name} airs`);
  const twoSteps = $derived(rules && hasEpisodes(plan));

  const focus = (id: string) => queueMicrotask(() => document.getElementById(id)?.focus());

  $effect(() => {
    const d = dialogEl;
    if (d && !d.open) {
      d.showModal();
      focus(step === 'change' ? 'rd-h' : 'rd-first');
    }
  });
  $effect(() => {
    const c = confirmEl;
    if (c && !c.open) c.showModal();
  });

  /** Save…: with episodes to look at, the dialog turns to Before you save first. */
  function primary() {
    if (saving) return;
    if (step === 'questions') {
      if (status.held || !status.rule) return;
      if (twoSteps) {
        step = 'change';
        return focus('rd-h');
      }
    }
    void save();
  }

  async function save() {
    const next = reset ? null : (status.rule ?? null);
    if (!reset && !next) return;
    saving = true;
    problem = '';
    try {
      let changed: RuleChanged | null = null;
      if (rules) {
        changed = series ? await api.putSeriesRule(slug, series, next, choice) : await api.putShowRule(slug, next, choice);
      } else if (series) {
        // An older station keeps a weekly slot, saved with the rest of the series.
        const v = await sched.series(slug, series);
        const { title: t, description, hosts, starts_on, planned_episodes, tags } = v.series;
        await sched.putSeries(slug, series, { title: t, description, hosts, starts_on, planned_episodes, tags, slot: slotOf(next) });
      } else {
        const v = await sched.show(slug);
        await sched.putShow(slug, { ...sched.inputOf(v), slot: slotOf(next) });
      }
      await onSaved(changed);
      dialogEl?.close();
    } catch (e) {
      // A refusal in the station's words; the questions come back so it can be put right.
      problem = e instanceof Error ? e.message : String(e);
      if (!reset) step = 'questions';
    } finally {
      saving = false;
    }
  }

  function leave() {
    if (step === 'change' && !reset) {
      step = 'questions';
      return focus('rd-first');
    }
    if (dirty) discarding = true;
    else dialogEl?.close();
  }
  function keys(e: KeyboardEvent) {
    if (!dialogEl || discarding) return;
    dialogKeys(e, dialogEl, leave, primary);
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<dialog bind:this={dialogEl} class="wz" data-dialog="rule" class:wide={step === 'questions' && rules} class:mid={step === 'change' || !rules} aria-labelledby="rd-h" tabindex="-1" onkeydown={keys} oncancel={(e) => { e.preventDefault(); leave(); }} onclose={onClose}>
  <div class="head">
    <h2 id="rd-h" tabindex="-1">{step === 'change' ? 'Before you save' : title}</h2>
    <p class="small">
      {#if step === 'change'}
        <b class="ink">{name}</b>{series ? `, series ${series}` : ''}{reset ? ' follows the show again' : ''}{lastSentence ? `: ${lastSentence}` : '.'}
      {:else if past}
        {airedWords(plan)}
      {:else}
        Give one date and time, then say whether it comes back.
      {/if}
    </p>
  </div>

  <form onsubmit={(e) => { e.preventDefault(); primary(); }} novalidate>
    <!-- The questions stay mounted behind Before you save, so going back finds them as they were. -->
    <div class="keep" hidden={step === 'change'}>
      <RulePart
        bind:form
        bind:state={pst}
        {zone}
        {today}
        {zoneSet}
        {canChooseZone}
        {onzone}
        {rules}
        show={slug}
        {series}
        {colour}
        label={past ? 'Changes from' : 'First on'}
        countHint={past && form.end.kind === 'count' ? countWords(plan, form.end.count) : ''}
        idp="rd"
        {live}
      />
    </div>
    {#if step === 'change'}
      {#if plan && hasEpisodes(plan)}
        <RuleChange {plan} {zone} bind:choice name="rd" />
      {:else if !plan}
        <p class="small" role="status">Working out what changes…</p>
      {:else}
        <p class="small">No episode is drafted or aired yet, so nothing moves.</p>
      {/if}
    {/if}
    <div class="foot2">
      {#if problem}<p class="err" role="alert">{problem}</p>{/if}
      <div class="acts">
        <button type="button" class="btn quiet left" onclick={leave} disabled={saving}>{step === 'change' && !reset ? 'Back' : 'Cancel'}</button>
        {#if step === 'change'}
          <button type="submit" class="btn primary" disabled={saving || (reset && !plan && rules)} aria-busy={saving || undefined}>{saving ? 'Saving…' : saveWords(plan, choice)}</button>
        {:else}
          <button type="submit" class="btn primary" disabled={saving || !!status.held} title={status.held || undefined} aria-busy={saving || undefined}>{saving ? 'Saving…' : twoSteps ? 'Save…' : 'Save'}</button>
        {/if}
      </div>
    </div>
  </form>
</dialog>

{#if discarding}
  <dialog bind:this={confirmEl} class="wz confirm" role="alertdialog" aria-labelledby="rd-discard-h" aria-describedby="rd-discard-p" oncancel={(e) => { e.preventDefault(); discarding = false; }}>
    <div class="head">
      <h2 id="rd-discard-h">Discard these changes?</h2>
      <p id="rd-discard-p" class="small">Nothing has been saved.</p>
    </div>
    <div class="acts">
      <button type="button" class="btn danger-hollow" onclick={() => { discarding = false; dialogEl?.close(); }}>Discard the changes</button>
      <!-- svelte-ignore a11y_autofocus -->
      <button type="button" class="btn primary" autofocus onclick={() => (discarding = false)}>Keep editing</button>
    </div>
  </dialog>
{/if}
