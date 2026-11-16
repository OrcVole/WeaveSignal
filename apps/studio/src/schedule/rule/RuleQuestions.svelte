<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // When it airs, the questions (new-show.md section 1, the left of `calendar.rule`): First on,
  // from and until with the length beside them; Repeats, as a list of sentences made from the
  // date or, behind More choices…, as the whole rule; and Ends, once the show repeats by a
  // pattern. The form's own slips are said here at once; what only the station can know (a
  // time that has passed, a month without a 31st) comes back with the preview and is placed
  // under the field it names.
  import type { FieldNote } from '../../lib/api.ts';
  import {
    KIND_TABS,
    MONTHS,
    NTHS,
    NTH_WORD,
    WEEKDAYS,
    WEEKDAYS_SHORT,
    choiceKey,
    choices,
    formProblems,
    kindTab,
    lengthWords,
    ordinal,
    pickChoice,
    pickKind,
    repeats,
    setFirst,
    times,
    toList,
    toggleDate,
    toggleWeekday,
    dateWords,
    type KindTab,
    type RuleForm,
  } from './form.ts';
  import { edges } from './edges.ts';
  import TimeField from '../TimeField.svelte';

  let {
    form,
    onchange,
    label = 'First on',
    zone,
    zoneSet = true,
    canChooseZone = false,
    onzone,
    old = false,
    problems = [],
    hints = [],
    countHint = '',
    onmode,
    idp = 'rq',
    seeded = null,
    onunseed,
  }: {
    form: RuleForm;
    /** The answers changed; the host keeps them. */
    onchange: (next: RuleForm) => void;
    /** `First on`, or `Changes from` for a show with a past. */
    label?: string;
    zone: string;
    /** Whether anyone has chosen the station's zone (ADR-0189). */
    zoneSet?: boolean;
    canChooseZone?: boolean;
    onzone?: () => void;
    /** The station is older than 1.0.4: it keeps a weekly time only. */
    old?: boolean;
    /** The station's problems and hints, by field. */
    problems?: FieldNote[];
    hints?: FieldNote[];
    /** `for 10 episodes in all: 3 aired, 3 drafted, 4 to come`, for a show with a past. */
    countHint?: string;
    /** More choices… or Fewer choices was pressed. */
    onmode?: (mode: RuleForm['mode']) => void;
    /** A prefix for the fields' ids: one a page. */
    idp?: string;
    /** The date and time were taken from the block selected in the week (New show): said under
     *  First on, in words, with the way back to today. Null once the person has changed them. */
    seeded?: { words: string; clear: string } | null;
    onunseed?: () => void;
  } = $props();

  const local = $derived(formProblems(form));
  const t = $derived(times(form));
  const rowsList = $derived(choices(form.first));
  const picked = $derived(choiceKey(form));
  const tab = $derived(kindTab(form.kind));

  /** The lines for a field: the form's own first, then the station's. */
  const said = (fields: string[], from: { field: string; message: string }[]) => from.filter((n) => fields.includes(n.field)).map((n) => n.message);
  const firstErrs = $derived([...said(['first'], local), ...said(['starts_on', 'first'], problems)]);
  const firstHints = $derived(said(['starts_on', 'first'], hints));
  const timeErrs = $derived([...said(['from', 'until'], local), ...said(['start', 'minutes'], problems)]);
  const repeatErrs = $derived([...said(['repeat'], local), ...said(['repeat'], problems)]);
  /** What the form already says under Monthly, so the station's same sentence is not said twice. */
  const ownHint = $derived(form.kind === 'monthly_day' && form.day > 28 ? `Months without a ${ordinal(form.day)} are skipped.` : '');
  const repeatHints = $derived(said(['repeat'], hints).filter((m) => m !== ownHint));
  const endErrs = $derived([...said(['end'], local), ...said(['end'], problems)]);
  const overnightErr = $derived(local.find((p) => p.field === 'overnight')?.message ?? '');

  const set = (patch: Partial<RuleForm>) => onchange({ ...form, ...patch });
  const num = (e: Event, fallback: number) => {
    const n = Number((e.currentTarget as HTMLInputElement | HTMLSelectElement).value);
    return Number.isFinite(n) ? Math.trunc(n) : fallback;
  };

  function mode(m: RuleForm['mode']) {
    onchange(m === 'list' ? toList(form) : { ...form, mode: 'full' });
    onmode?.(m);
  }

  /** ← → along a row of buttons that is one stop for Tab (the five kinds, the seven days). */
  function along(e: KeyboardEvent, pick: boolean) {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight' && e.key !== 'Home' && e.key !== 'End') return;
    const all = [...((e.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>('button:not([disabled])'))];
    const i = all.indexOf(e.target as HTMLButtonElement);
    if (i < 0) return;
    e.preventDefault();
    const to = e.key === 'Home' ? 0 : e.key === 'End' ? all.length - 1 : (i + (e.key === 'ArrowRight' ? 1 : -1) + all.length) % all.length;
    all[to].focus();
    if (pick) all[to].click();
  }

  let dayFocus = $state(0);

  /** Remove on a chosen date's chip. The chip goes, so the focus moves first to the chip that
   *  takes its place (the next, else the one before), else to the calendar the dates are picked
   *  in: a button that disappears with the focus on it would drop the focus out of the dialog. */
  function removeChip(e: MouseEvent, d: string) {
    const chip = (e.currentTarget as HTMLElement).closest('.chip');
    const near = (chip?.nextElementSibling ?? chip?.previousElementSibling)?.querySelector<HTMLElement>('button');
    const cal = (e.currentTarget as HTMLElement).closest('.rule')?.querySelector<HTMLElement>('.months [role="gridcell"][tabindex="0"]');
    (near ?? cal)?.focus();
    onchange(toggleDate(form, d));
  }
</script>

<div class="qs" use:edges>
  <div class="field">
    <label for="{idp}-first">{label}</label>
    <div class="rows2">
      <div class="row">
        <input
          id="{idp}-first"
          type="date"
          value={form.first}
          aria-invalid={firstErrs.length ? true : undefined}
          aria-describedby="{seeded ? `${idp}-seeded ` : ''}{idp}-first-notes {idp}-zone"
          onchange={(e) => onchange(setFirst(form, (e.currentTarget as HTMLInputElement).value))}
        />
      </div>
      <div class="row">
        <span class="t"><label for="{idp}-from" aria-hidden="true">from</label><TimeField id="{idp}-from" value={form.from} label="Start, station time" invalid={timeErrs.length > 0} describedby="{idp}-time-notes {idp}-zone" onchange={(v) => set({ from: v, overnightOk: false })} /></span>
        <span class="t"><label for="{idp}-until" aria-hidden="true">until</label><TimeField id="{idp}-until" value={form.until} label="End, station time" invalid={timeErrs.length > 0} describedby="{idp}-time-notes {idp}-zone" onchange={(v) => set({ until: v, overnightOk: false })} /></span>
        {#if t}<span class="len num" data-testid="rule-length">{lengthWords(t.minutes)}</span>{/if}
      </div>
    </div>
    {#if seeded}
      <p id="{idp}-seeded" class="seeded" data-testid="rule-seeded">
        <span>From the block selected in the week: <b>{seeded.words}</b>.</span>
        {#if onunseed}<button type="button" class="act2" onclick={onunseed}>{seeded.clear}</button>{/if}
      </p>
    {/if}
    <span id="{idp}-zone" class="hint">
      {#if zoneSet}Station time, {zone}.{:else}Times are in {zone}, which nobody has confirmed yet.
        {#if canChooseZone && onzone}<button type="button" class="act2" onclick={onzone}>Choose the time zone…</button>{/if}{/if}
    </span>
    <div id="{idp}-first-notes">
      {#each firstErrs as m (m)}<p class="err" role="alert">{m}</p>{/each}
      {#each firstHints as m (m)}<span class="hint">{m}</span>{/each}
    </div>
    <div id="{idp}-time-notes">
      {#each timeErrs as m (m)}<p class="err" role="alert">{m}</p>{/each}
    </div>
    {#if t?.overnight}
      <label class="check-inline"><input id="{idp}-overnight" type="checkbox" checked={form.overnightOk} onchange={(e) => set({ overnightOk: (e.currentTarget as HTMLInputElement).checked })} /> Yes, it runs past midnight</label>
      {#if overnightErr}<p class="err">{overnightErr}</p>{/if}
    {/if}
  </div>

  <fieldset class="field">
    <legend>Repeats</legend>
    {#if old}
      <div class="note"><svg aria-hidden="true"><use href="#g-lock" /></svg><span><b>This station keeps a weekly time only.</b> Update it for other patterns.</span></div>
    {/if}
    {#if form.mode === 'list' || old}
      <div class="choices" role="radiogroup" aria-label="Repeats">
        {#each rowsList as c (c.key)}
          {@const off = old && c.key !== 'weekly'}
          <label class:off>
            <input type="radio" name="{idp}-rep" value={c.key} checked={c.key === picked} disabled={off} onchange={() => onchange(pickChoice(form, c.key))} />
            <span>{c.label}</span>
          </label>
        {/each}
      </div>
      {#if !old}<button type="button" class="btn quiet sm morebtn" onclick={() => mode('full')}>More choices…</button>{/if}
    {:else}
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div class="seg5" role="radiogroup" aria-label="Repeats" onkeydown={(e) => along(e, true)}>
        {#each KIND_TABS as k (k.key)}
          <button type="button" role="radio" aria-checked={k.key === tab} tabindex={k.key === tab ? 0 : -1} onclick={() => onchange(pickKind(form, k.key as KindTab))}>{k.label}</button>
        {/each}
      </div>
      {#if tab === 'weekly'}
        <div class="rulebox">
          <div class="row">Every <input type="number" min="1" max="12" value={form.every} aria-label="Every how many weeks" onchange={(e) => set({ every: num(e, 1) })} /> week{form.every === 1 ? '' : 's'} on</div>
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <div class="wdays" role="group" aria-label="Weekdays" onkeydown={(e) => along(e, false)}>
            {#each WEEKDAYS_SHORT as w, i (w)}
              <button type="button" aria-pressed={form.weekdays.includes(i + 1)} aria-label={WEEKDAYS[i]} tabindex={i === dayFocus ? 0 : -1} onfocus={() => (dayFocus = i)} onclick={() => onchange(toggleWeekday(form, i + 1))}><svg aria-hidden="true"><use href="#g-check" /></svg>{w}</button>
            {/each}
          </div>
        </div>
      {:else if tab === 'monthly'}
        <div class="rulebox">
          <div class="row">Every <input type="number" min="1" max="12" value={form.every} aria-label="Every how many months" onchange={(e) => set({ every: num(e, 1) })} /> month{form.every === 1 ? '' : 's'}</div>
          <div class="monthly">
            <label>
              <input type="radio" name="{idp}-mk" checked={form.kind === 'monthly_nth'} onchange={() => set({ kind: 'monthly_nth' })} /> on the
              <select aria-label="Which one" value={String(form.nth)} onchange={(e) => set({ kind: 'monthly_nth', nth: num(e, 1) })}>
                {#each NTHS as n (n)}<option value={String(n)}>{NTH_WORD[n]}</option>{/each}
              </select>
              <select aria-label="Weekday" value={String(form.weekday)} onchange={(e) => set({ kind: 'monthly_nth', weekday: num(e, 1) })}>
                {#each WEEKDAYS as w, i (w)}<option value={String(i + 1)}>{w}</option>{/each}
              </select>
            </label>
            <label>
              <input type="radio" name="{idp}-mk" checked={form.kind === 'monthly_day'} onchange={() => set({ kind: 'monthly_day' })} /> on day
              <select aria-label="Day of the month" value={String(form.day)} onchange={(e) => set({ kind: 'monthly_day', day: num(e, 1) })}>
                {#each Array.from({ length: 31 }, (_, i) => i + 1) as d (d)}<option value={String(d)}>{d}</option>{/each}
                <option value="-1">the last day</option>
              </select>
            </label>
          </div>
          {#if form.kind === 'monthly_day' && form.day > 28}
            <span class="hint">{ownHint} <button type="button" class="act2" onclick={() => set({ day: -1 })}>Use the last day instead</button></span>
          {/if}
        </div>
      {:else if tab === 'yearly'}
        <div class="rulebox">
          <div class="row">
            Every year on
            <select aria-label="Day" value={String(form.yearDay)} onchange={(e) => set({ yearDay: num(e, 1) })}>
              {#each Array.from({ length: 31 }, (_, i) => i + 1) as d (d)}<option value={String(d)}>{d}</option>{/each}
            </select>
            <select aria-label="Month" value={String(form.yearMonth)} onchange={(e) => set({ yearMonth: num(e, 1) })}>
              {#each MONTHS as m, i (m)}<option value={String(i + 1)}>{m}</option>{/each}
            </select>
          </div>
          {#if form.yearDay === 29 && form.yearMonth === 2 && !repeatHints.length}<span class="hint">Only in leap years.</span>{/if}
        </div>
      {:else if tab === 'dates'}
        <div class="rulebox">
          <span class="hint">Pick the dates in the calendar. Each one airs at the time above.</span>
          <div class="picked">
            {#each form.dates as d (d)}
              <span class="chip"><span>{dateWords(d, { weekday: 'short', month: 'short' })}</span><button type="button" aria-label="Remove {dateWords(d, { weekday: 'long' })}" title="Remove {dateWords(d, { weekday: 'long' })}" onclick={(e) => removeChip(e, d)}><svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 2.5l7 7M9.5 2.5l-7 7" /></svg></button></span>
            {/each}
          </div>
        </div>
      {/if}
      <button type="button" class="btn quiet sm morebtn" onclick={() => mode('list')}>Fewer choices</button>
    {/if}
    {#each repeatErrs as m (m)}<p class="err" role="alert">{m}</p>{/each}
    {#each repeatHints as m (m)}<span class="hint">{m}</span>{/each}
  </fieldset>

  {#if repeats(form) && !old}
    <fieldset class="field">
      <legend>Ends</legend>
      <div class="ends">
        <label><input type="radio" name="{idp}-end" checked={form.end.kind === 'never'} onchange={() => set({ end: { ...form.end, kind: 'never' } })} /> Never</label>
        <label>
          <input type="radio" name="{idp}-end" checked={form.end.kind === 'until'} onchange={() => set({ end: { ...form.end, kind: 'until' } })} /> On
          <input type="date" value={form.end.until} aria-label="The last date" onchange={(e) => set({ end: { ...form.end, kind: 'until', until: (e.currentTarget as HTMLInputElement).value } })} />
        </label>
        <label>
          <input type="radio" name="{idp}-end" checked={form.end.kind === 'count'} onchange={() => set({ end: { ...form.end, kind: 'count' } })} /> After
          <input type="number" min="1" max="520" value={form.end.count} aria-label="How many episodes" onchange={(e) => set({ end: { ...form.end, kind: 'count', count: num(e, 10) } })} /> episodes
        </label>
      </div>
      {#if form.end.kind === 'count' && countHint}
        <span class="hint">{countHint}</span>
      {:else if form.end.kind === 'count' && form.skip.length}
        <span class="hint">Skipped dates do not count: the show runs until it has made {form.end.count} episodes.</span>
      {/if}
      {#each endErrs as m (m)}<p class="err" role="alert">{m}</p>{/each}
    </fieldset>
  {/if}
</div>
