<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The DJ's show setup (ADR-0218, ADR-0219, ADR-0278): four parts in a dialog over the surface.
  // About you (the on-air name and the takedown contact), Your show (its name, the links to
  // your channels, whether it plays commercial music, and how much of the studio to start
  // with: the show's step, Solo picked, ADR-0271), When it is on (`calendar.rule`: the same
  // questions and preview as New show, opened on the list), Done (what the answers now do, and
  // where the studio starts). They are "parts" so that "step" means one thing.
  // The answers become the show's own fields in the station, tied to this account, so the
  // consent words name the DJ while the show is on air (ADR-0214). Save my show saves the show
  // and drafts its first episode in one transaction. A first date that is another show's is
  // refused, with the other show named (a presenter may not draft over another show); a later
  // date that clashes is only a warning until the DJ comes to draft it. Run again from
  // Settings, the third part shows the show's rule and behaves as a change: the drafted
  // episodes are listed before anything is saved. It opens by itself for a newly signed-in
  // presenter with no show of their own. No email is sent or needed. Panel name
  // `studio.show-setup` (ADR-0070). A dialog owns the keyboard (keymap rule 5): only the ⌃
  // chords reach the keymap while it is open.
  import { untrack } from 'svelte';
  import * as rulesApi from '../lib/api.ts';
  import type { DraftedChoice, IsoDate, Me, RulePreview, ShowSaved } from '../lib/api.ts';
  import { isAdmin, placeError } from '../lib/account.ts';
  import * as api from '../lib/schedule.ts';
  import type { Show } from '../lib/schedule.ts';
  import { sched } from '../lib/schedule.svelte.ts';
  import { stationNow, studio } from '../lib/studio.svelte.ts';
  import {
    CONTACT_MAX,
    MUSIC_REMINDER,
    PRESENTER_MAX,
    STEP_HINT,
    STEP_QUESTION,
    startsAt,
    SHOW_NAME_MAX,
    aboutProblems,
    freeSlug,
    fromShow,
    nextColour,
    showProblems,
    splitLinks,
    toShowInput,
    type Answers,
  } from '../lib/wizard.ts';
  import StepList from '../lib/StepList.svelte';
  import RuleChange from '../schedule/rule/RuleChange.svelte';
  import RulePart from '../schedule/rule/RulePart.svelte';
  import { fromRule, newForm, ruleOfSlot, whenFields, type RuleForm } from '../schedule/rule/form.ts';
  import { dialogKeys } from '../schedule/rule/keys.ts';
  import { dateIn, firstState, firstWords, hasEpisodes, movable, ruleStatus, saveWords } from '../schedule/rule/view.ts';

  let {
    me,
    shows,
    initial = null,
    part: firstPart = 1,
    zone = sched.zone,
    today = dateIn(stationNow(), sched.zone),
    rules = sched.rules === true,
    form: firstForm = null,
    preview = null,
    saved: firstSaved = null,
    changing = false,
    live = true,
    onDone,
    onSkip,
    onClose,
  }: {
    me: Me;
    /** Every show, for the path check and the colour; the one tied to `me` is the one changed. */
    shows: readonly Show[];
    /** The show this account presents, when the wizard is run again. */
    initial?: Show | null;
    /** The part to open on (a test's). */
    part?: 1 | 2 | 3 | 4;
    /** The station's zone, its date today, and whether it stores rules (1.0.4 or later). */
    zone?: string;
    today?: IsoDate;
    rules?: boolean;
    /** The third part's answers, its preview, the saved show and Before you save (a test's). */
    form?: RuleForm | null;
    preview?: RulePreview | null;
    saved?: ShowSaved | null;
    changing?: boolean;
    live?: boolean;
    /** The show was saved. */
    onDone: (saved: ShowSaved) => void;
    /** Skipped for now: not asked again on this machine. */
    onSkip: () => void;
    onClose: () => void;
  } = $props();

  const seed = untrack(() => ({ initial, part: firstPart, firstForm, firstSaved, today, rules, changing, preview }));
  const PARTS = ['About you', 'Your show', 'When it is on', 'Done'] as const;

  /** When the show is on, as the third part first shows it: the show's rule when it has one
   *  (from its next date, as a change does), else today, once only. Always on the list where
   *  the list can say it. */
  function firstAnswers(): RuleForm {
    if (seed.firstForm) return seed.firstForm;
    const s = seed.initial;
    const stored = s ? (seed.rules ? (s.rule ?? null) : s.slot ? ruleOfSlot(s.slot) : null) : null;
    if (!stored) return newForm(seed.today);
    const start = stored.starts_on && stored.starts_on >= seed.today ? stored.starts_on : (s?.next_on?.on ?? seed.today);
    return fromRule({ ...stored, starts_on: start }, start);
  }

  let part = $state<1 | 2 | 3 | 4>(seed.part);
  let a = $state<Answers>(untrack(() => fromShow(seed.initial, me)));
  let form = $state<RuleForm>(firstAnswers());
  let pst = $state(firstState(seed.preview));
  const status = $derived(ruleStatus(form, pst, rules));
  let errors = $state<{ presenter?: string; contact?: string; show?: string; links?: string; form?: string }>({});
  let saving = $state(false);
  let saved = $state<ShowSaved | null>(seed.firstSaved);
  /** Before you save: the drafted episodes a changed rule would move (run again only). */
  let confirming = $state(seed.changing);
  let choice = $state<DraftedChoice>('move');
  let said = $state(seed.preview?.sentence ?? '');
  let dialogEl = $state<HTMLDialogElement | null>(null);

  const again = $derived(seed.initial !== null);
  const hadRule = seed.initial ? (seed.rules ? !!seed.initial.rule : !!seed.initial.slot) : false;
  const initialForm = JSON.stringify(untrack(() => form));
  const ruleChanged = $derived(JSON.stringify(form) !== initialForm);
  const starts = $derived(startsAt(a.step));
  const musicNote = $derived(a.commercial_music ? MUSIC_REMINDER : '');
  const linkCount = $derived(splitLinks(a.links).length);
  const canChooseZone = $derived(isAdmin(me));
  let lastPlan = $state(seed.preview?.change ?? null);
  $effect(() => {
    if (status.answer) {
      lastPlan = status.answer.change;
      said = status.answer.sentence;
    }
  });
  const plan = $derived(lastPlan);
  const firstEp = $derived(saved?.drafted?.episode ?? null);
  const savedRule = $derived(saved?.show.rule ?? null);
  const firstWhen = $derived(firstEp?.air_at ? firstWords({ start: firstEp.air_at, minutes: firstEp.minutes ?? savedRule?.minutes ?? 60 }, zone, today) : '');
  /** The sentence inside a sentence: `It is on every week on Friday, 23:00 to 01:00 the next day.` */
  const isOn = $derived.by(() => {
    const s = saved?.show.rule_words?.sentence ?? (saved && (savedRule || saved.show.slot) ? said : '');
    return s ? `It is on ${s.charAt(0).toLowerCase()}${s.slice(1)}` : '';
  });

  const focusFirst = (id: string) => queueMicrotask(() => document.getElementById(id)?.focus());
  const firstField = () => (part === 1 ? 'sw-presenter' : part === 2 ? 'sw-show' : part === 3 ? 'sw-first' : 'sw-done');

  $effect(() => {
    const d = dialogEl;
    if (d && !d.open) {
      d.showModal();
      focusFirst(firstField());
    }
  });

  function next() {
    if (part === 1) {
      const p = aboutProblems(a);
      errors = p;
      if (p.presenter || p.contact) return focusFirst(p.presenter ? 'sw-presenter' : 'sw-contact');
      part = 2;
    } else if (part === 2) {
      const p = showProblems(a);
      errors = p;
      if (p.show || p.links) return focusFirst(p.show ? 'sw-show' : 'sw-links');
      part = 3;
    }
    focusFirst(firstField());
  }

  function back() {
    errors = {};
    if (confirming) confirming = false;
    else part = part === 3 ? 2 : 1;
    focusFirst(firstField());
  }

  /** Save my show. `withDates` false is No dates yet: the show is saved with none. */
  async function save(withDates: boolean) {
    if (saving || (withDates && (status.held || !status.rule))) return;
    // Run again with a changed rule: the drafted episodes are listed before anything is saved.
    if (again && rules && withDates && hadRule && ruleChanged && !confirming && hasEpisodes(plan) && movable(plan) > 0) {
      confirming = true;
      return focusFirst('sw-h');
    }
    saving = true;
    errors = {};
    try {
      // The show this account presents is changed in place; a new one takes a free path.
      const existing = seed.initial ? await api.show(seed.initial.slug) : null;
      const slug = existing ? existing.show.slug : freeSlug(a.show, shows.map((s) => s.slug));
      const rule = withDates ? status.rule : null;
      // A show with a rule already has it changed on the rule's own route, after the show is
      // saved with the rule as stored; a show without one takes the rule with the save, and
      // its first episode is drafted in the same transaction.
      const viaRuleRoute = !!existing && rules && hadRule;
      const when = viaRuleRoute
        ? { ...whenFields(existing.show.rule ?? null, { rules: true }), slot: existing.show.slot }
        : whenFields(rule, { rules, draftFirst: !existing || existing.series.length === 0 });
      let v = await rulesApi.saveShow(slug, toShowInput(a, me, existing, nextColour(shows.length), when));
      if (viaRuleRoute && withDates && ruleChanged) {
        await rulesApi.putShowRule(slug, rule, choice);
        v = { ...(await api.show(slug)), drafted: null };
      }
      saved = { ...v, show: api.asShow(v.show) };
      confirming = false;
      part = 4;
      focusFirst('sw-done');
    } catch (err) {
      const placed = placeError(err, ['name', 'presenter', 'contact', 'links'] as const);
      confirming = false;
      if (placed.field === 'presenter' || placed.field === 'contact') {
        part = 1;
        errors = { [placed.field]: placed.message };
        focusFirst(`sw-${placed.field}`);
      } else if (placed.field === 'name' || placed.field === 'links') {
        part = 2;
        errors = { [placed.field === 'name' ? 'show' : 'links']: placed.message };
        focusFirst(placed.field === 'name' ? 'sw-show' : 'sw-links');
      } else {
        // A clash's refusal names the other show, in the station's words.
        errors = { form: placed.message };
      }
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

  /** Esc: leaving without saving is not a skip, so the wizard asks again next time. */
  function leave() {
    if (confirming) return back();
    if (saved) onDone(saved);
    dialogEl?.close();
  }

  function keys(e: KeyboardEvent) {
    if (!dialogEl) return;
    dialogKeys(e, dialogEl, leave, () => {
      if (part < 3) next();
      else if (part === 3) void save(true);
      else finish();
    });
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<dialog bind:this={dialogEl} class="wz" class:wide={part === 3 && rules && !confirming} class:mid={part === 3 && (!rules || confirming)} data-panel="studio.show-setup" aria-labelledby="sw-h" tabindex="-1" onkeydown={keys} oncancel={(e) => { e.preventDefault(); leave(); }} onclose={onClose}>
  <div class="head">
    <p class="crumbs" aria-hidden="true">
      {#each PARTS as s, i (s)}
        <span class:now={part === i + 1} class:done={part > i + 1}>{i + 1} {s}</span>
      {/each}
    </p>
    <h2 id="sw-h" data-panel-title tabindex="-1">{confirming ? 'Before you save' : again ? 'Your show' : 'Set up your show'}</h2>
    <p class="small"><span class="sr-only">Part {part} of 4: {PARTS[part - 1]}. </span>
      {#if part === 1}
        What callers hear about you. Two questions, then your show.
      {:else if part === 2}
        Your show in the station: its name, where people find you, and how much of the studio it starts with.
      {:else if part === 3 && confirming}
        <b class="ink">{a.show.trim()}</b>{said ? `: ${said}` : '.'}
      {:else if part === 3}
        When your show is on. Give its first date, then say whether it comes back.
      {:else}
        {again ? 'Your show is up to date.' : 'That is everything the station needs.'}
      {/if}
    </p>
  </div>

  {#if part === 1}
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
  {:else if part === 2}
    <form onsubmit={(e) => { e.preventDefault(); next(); }} novalidate>
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
      <div class="check">
        <input id="sw-music" type="checkbox" bind:checked={a.commercial_music} aria-describedby="sw-music-hint" />
        <label for="sw-music">The show plays commercial music</label>
        <p id="sw-music-hint" class="hint" role={a.commercial_music ? 'note' : undefined}>{musicNote || 'Released records, rather than your own or licence-free music.'}</p>
      </div>
      <div class="field">
        <span class="q" id="sw-step">{STEP_QUESTION}</span>
        <StepList bind:value={a.step} name="sw-step" describedby="sw-step sw-step-hint" />
        <span id="sw-step-hint" class="hint">{STEP_HINT}</span>
      </div>
      <div class="acts">
        <button type="button" class="btn quiet" onclick={back}>Back</button>
        <button type="submit" class="btn primary">Next: when it is on</button>
      </div>
    </form>
  {:else if part === 3}
    <form onsubmit={(e) => { e.preventDefault(); void save(true); }} novalidate>
      <!-- The questions stay mounted behind Before you save, so going back finds them as they were. -->
      <div class="keep" hidden={confirming}>
        <RulePart
          bind:form
          bind:state={pst}
          {zone}
          {today}
          zoneSet={sched.zoneSet !== false}
          {canChooseZone}
          onzone={() => (studio.settings = true)}
          {rules}
          show={initial?.slug ?? null}
          colour={initial?.colour_slot ?? nextColour(shows.length)}
          label={hadRule && (plan?.aired ?? 0) > 0 ? 'Changes from' : 'First on'}
          idp="sw"
          {live}
        />
      </div>
      {#if confirming && plan}<RuleChange {plan} {zone} bind:choice name="sw" />{/if}
      <div class="foot2">
        {#if errors.form}<p class="err" role="alert">{errors.form}</p>{/if}
        <div class="acts">
          <button type="button" class="btn quiet left" onclick={back} disabled={saving}>Back</button>
          {#if !hadRule && !confirming}<button type="button" class="btn quiet" onclick={() => save(false)} disabled={saving}>No dates yet</button>{/if}
          <button type="submit" class="btn primary" disabled={saving || !!status.held} title={status.held || undefined} aria-busy={saving || undefined}>{saving ? 'Saving…' : confirming ? saveWords(plan, choice) : again ? 'Save changes' : 'Save my show'}</button>
        </div>
      </div>
    </form>
  {:else}
    <div class="done" id="sw-done" tabindex="-1">
      <p class="lead"><svg class="g-good" aria-hidden="true"><use href="#g-good" /></svg><span><strong>{saved?.show.name ?? a.show}</strong> is set up as your show.</span></p>
      <ul>
        <li>While it is on air, callers agree to be recorded under your name{a.contact.trim() ? ', with your takedown contact first and the station’s as the host' : ', with the station’s takedown contact'}.</li>
        {#if firstEp}<li><b class="ink">Episode {firstEp.number} is drafted</b>{firstWhen ? ` for ${firstWhen}` : ''}. Give it a title and guests when you know them.</li>{/if}
        {#if isOn}
          <li>{isOn}</li>
          {#if savedRule?.repeat.kind === 'once'}
            <li>It airs once, so there is nothing more to draft.</li>
          {:else}
            <li>The other dates wait in the week as dotted blocks. Draft each when you are ready.</li>
          {/if}
        {:else}
          <li>It has no dates yet, so nothing is drafted. Give it dates from the Schedule tab when you know them.</li>
        {/if}
        {#if a.commercial_music}<li>{MUSIC_REMINDER}</li>{/if}
        <li data-step={a.step}><b class="ink">{starts.lead}</b> {starts.rest}</li>
        <li>Change any of this from Settings, under <b>Your show</b>, or on the Schedule tab.</li>
      </ul>
      <div class="acts">
        <button type="button" class="btn primary" onclick={finish}>Done</button>
      </div>
    </div>
  {/if}
</dialog>
