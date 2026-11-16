<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A show's dates (`calendar.show-dates`; week-clashes.md section 3): the Dates tab of the show
  // level. The head says when it airs, in the station's sentence, with Change…; the rows are
  // the dates still to come, drafted, not drafted, skipped, moved or clashing; under them what
  // has aired. The list is the stored schedule, not a draft of a rule: each change is saved as
  // it is made and the tab's line offers Undo, as Skip this date does in the week.
  //
  // Nothing is worked out here. The dates, their states and their clashes are the station's
  // preview of the rule as stored (it is sent back only to be read); the episodes are the
  // series'. A row is two lines, three with a clash. Pressing it shows it in the week and
  // keeps the list; Open is the second, explicit act. The rows are one stop for Tab: ↑ ↓, Home
  // and End move between them, Space shows a row in the week, ⏎ opens it, and Tab goes on to
  // the row's own buttons. No letter acts on a row: a slipped key must not change the schedule.
  import { tick, untrack } from 'svelte';
  import * as api from '../lib/schedule.ts';
  import type { SeriesView, Show, ShowView } from '../lib/schedule.ts';
  import * as rulesApi from '../lib/api.ts';
  import type { IsoDate, Rule, RulePreview } from '../lib/api.ts';
  import { sched } from '../lib/schedule.svelte.ts';
  import { openRuleDialog } from './rule/dialog.svelte.ts';
  import { addDays, dateWords, minutesOf, ruleOfSlot } from './rule/form.ts';
  import { boundsWords, dateIn, moveBounds, unscheduled } from './rule/view.ts';
  import { airedRows, changeRule, countWords, dateRows, draftedToCome, firstRows, hasMore, moreWords, moveIn, putBackIn, rowAria, skipIn, unmoveIn, type AiredRow, type DateRow } from './dates.ts';
  import { stationRules } from './datesStore.ts';
  import { fromLocalInput, hm, zonedString, type Block } from './model.ts';
  import TimeField from './TimeField.svelte';

  let {
    slug,
    show,
    zone,
    now,
    today,
    blocks,
    selected,
    onChanged,
    onTell,
    onShow,
    onOpen,
    onGiveDates,
  }: {
    slug: string;
    /** The show as the list of shows holds it. */
    show: Show;
    zone: string;
    now: number;
    /** The station's date today. */
    today: IsoDate;
    /** The week on screen: a drafted episode's clash is known only while its block is in it. */
    blocks: readonly Block[];
    selected: string | null;
    /** Something was saved: the week is read again. */
    onChanged: () => Promise<void>;
    /** The tab's own line, with one act when there is a way back or on. */
    onTell: (title: string, message: string, act?: (() => Promise<void>) | null, label?: string) => void;
    /** Show a row in the week: the block to select (null when the date draws none) and its moment. */
    onShow: (blockId: string | null, ms: number) => void;
    /** Open a row: its episode's editor, or the dotted date's pane. */
    onOpen: (blockId: string, ms: number) => void;
    /** Give it dates…, for a show with none. */
    onGiveDates: () => void;
  } = $props();

  const rulesOn = $derived(sched.rules === true);

  let view = $state<ShowView | null>(null);
  let series = $state<SeriesView | null>(null);
  let earlier = $state<SeriesView[]>([]);
  let preview = $state<RulePreview | null>(null);
  let shown = $state(10);
  let loaded = $state(false);
  let unreachable = $state(false);
  let busy = $state(false);

  /** The series whose dates these are: the newest (a show's dates are its newest series'). */
  const current = $derived(view?.series.at(-1) ?? null);
  /** The series' own rule, when it has one; else it follows the show's. */
  const own = $derived<Rule | null>(rulesOn ? (current?.rule ?? null) : null);
  const rule = $derived<Rule | null>(rulesOn ? (own ?? view?.show.rule ?? show.rule ?? null) : null);
  const stored = $derived(view?.show ?? show);
  const colour = $derived(stored.colour_slot);

  /** The show asked for, as a value: a prop read in the effect below would follow the list of
   *  shows, which is a new list after every read of the week, and the dates would be read twice. */
  const which = $derived(slug);
  /** The dates are being read: the list says so to whoever waits on it. */
  let reading = $state(false);
  let seq = 0;
  async function load() {
    const mine = ++seq;
    const slug = untrack(() => which);
    reading = true;
    try {
      const v = await api.show(slug);
      const cur = v.series.at(-1) ?? null;
      const [sv, old] = await Promise.all([cur ? api.series(slug, cur.number) : Promise.resolve(null), Promise.all(v.series.slice(0, -1).map((s) => api.series(slug, s.number)))]);
      const r = sched.rules === true ? (cur?.rule ?? v.show.rule ?? null) : null;
      // The stored rule is sent back only to have it read: the station expands it, not the studio.
      // Every date it will send is asked for at once (sixty at most), so that the head can count
      // what is to come; the list shows ten of them, and ten more at a time.
      const pv = r ? await rulesApi.previewRule({ rule: r, show: slug, ...(cur ? { series: cur.number } : {}), limit: 60 }) : null;
      if (mine !== seq) return;
      view = v;
      series = sv;
      earlier = old;
      preview = pv;
      unreachable = false;
    } catch {
      if (mine !== seq) return;
      unreachable = true;
    } finally {
      if (mine === seq) {
        loaded = true;
        reading = false;
      }
    }
  }

  // Read when the pane opens, and again whenever the tab reads the week again: a change made
  // here, in the rule dialog, in the week, or in another window.
  $effect(() => {
    void sched.version;
    void which;
    void untrack(load);
  });

  const every = $derived(
    dateRows(preview, series?.episodes ?? [], { slug, zone, today, now, nominal: rule?.start ?? stored.slot?.start ?? null, blocks }),
  );
  /** The rows in the list: the first ten dates that air, then ten more at a time. */
  const rows = $derived(firstRows(every, shown));
  const aired = $derived<AiredRow[]>([
    ...(series && current ? airedRows(series.episodes, current, zone) : []),
    ...[...earlier].reverse().flatMap((s) => airedRows(s.episodes, s.series, zone)),
  ]);
  const sentence = $derived(preview?.sentence || (own ? (current?.rule_words?.sentence ?? '') : (stored.rule_words?.sentence ?? '')));
  const lastLine = $derived(preview?.last ?? (own ? current?.rule_words?.last : stored.rule_words?.last) ?? null);
  const drafted = $derived(series ? draftedToCome(series.episodes, now) : null);
  const counts = $derived(countWords(every, preview, rule, today, drafted));
  const after = $derived(moreWords(every, shown, preview, rule, lastLine));
  /** A rule that gives nothing more: `its 10 episodes are made`. */
  const finished = $derived(rulesOn && rule && loaded ? (unscheduled([stored], true).finished[0]?.why ?? '') : '');
  const firstOpen = $derived(every.find((r) => r.first) ?? null);
  /** How many dates are still to draft in all, when the station has sent every one the rule
   *  still gives: those not drafted among them. Unknown past sixty, and then not offered. */
  const draftAll = $derived.by(() => {
    if (!rule || rule.end.kind === 'never' || hasMore(preview, rule)) return null;
    const open = every.filter((r) => r.kind === 'open').length;
    return open > 1 ? open : null;
  });

  // ---- the rows: one stop, the arrows inside ----
  let host = $state<HTMLElement | null>(null);
  let rowFocus = $state<string | null>(null);
  /** The row last pressed: marked as the current one until another block is selected. */
  let cur = $state<string | null>(null);
  const rowStop = $derived(rows.some((r) => r.key === rowFocus) ? rowFocus : (rows[0]?.key ?? null));
  const rowEl = (key: string) => host?.querySelector<HTMLElement>(`[data-row="${CSS.escape(key)}"]`) ?? null;
  const isCur = (r: DateRow) => (r.blockId !== null && r.blockId === selected) || (selected === null && cur === r.key);

  function showRow(r: DateRow) {
    cur = r.key;
    onShow(r.blockId, r.start);
  }
  function openRow(r: DateRow) {
    if (r.blockId) onOpen(r.blockId, r.start);
  }
  function rowKeys(e: KeyboardEvent, r: DateRow, i: number) {
    if (e.target !== e.currentTarget || e.ctrlKey || e.metaKey || e.altKey) return;
    const to = e.key === 'ArrowDown' ? i + 1 : e.key === 'ArrowUp' ? i - 1 : e.key === 'Home' ? 0 : e.key === 'End' ? rows.length - 1 : null;
    if (to !== null) {
      if (rows[to]) rowEl(rows[to].key)?.focus();
    } else if (e.key === ' ') showRow(r);
    else if (e.key === 'Enter') openRow(r);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }
  function rowClick(e: MouseEvent, r: DateRow) {
    if ((e.target as HTMLElement).closest('button, input, label')) return;
    showRow(r);
  }

  // ---- Move…: one row open at a time ----
  let moving = $state<string | null>(null);
  let moveDate = $state('');
  let moveTime = $state('');
  let moveErr = $state('');
  const movingRow = $derived(rows.find((r) => r.key === moving) ?? null);
  const bounds = $derived(preview && movingRow?.kind === 'open' && movingRow.on ? moveBounds(preview, movingRow.on, zone, today) : null);
  // Another window took the date out of the rule while Move… was open on it.
  $effect(() => {
    if (moving !== null && loaded && !rows.some((r) => r.key === moving)) {
      moving = null;
      onTell('That date is no longer in the rule.', '');
    }
  });

  async function openMove(r: DateRow) {
    moving = r.key;
    moveDate = r.air;
    moveTime = r.at;
    moveErr = '';
    await tick();
    host?.querySelector<HTMLElement>('li.mv')?.scrollIntoView({ block: 'nearest' });
    host?.querySelector<HTMLInputElement>('#sd-mv-date')?.focus();
  }
  async function closeMove(key: string) {
    moving = null;
    await tick();
    rowEl(key)?.focus();
  }

  // ---- the changes: each saved as it is made, with the way back ----
  const fail = (what: string, e: unknown) => onTell(what, e instanceof Error ? e.message : String(e));
  /** The series the rule of a date is read from: the series' own rule, else the show's. */
  const ruleSeries = $derived(own && current ? current.number : null);
  const edit = (change: (rule: Rule) => Rule) => changeRule(stationRules, slug, ruleSeries, change);

  /** A row's own button was pressed, and it will be replaced by what it did (Skip by Put back):
   *  the focus goes to its row first, which stays. */
  async function act(r: DateRow, what: string, run: () => Promise<void>) {
    if (busy) return;
    rowEl(r.key)?.focus();
    busy = true;
    try {
      await run();
    } catch (e) {
      fail(what, e);
    } finally {
      busy = false;
    }
  }

  const skip = (r: DateRow) =>
    act(r, 'Not skipped', async () => {
      const on = r.on!;
      await edit((x) => skipIn(x, on));
      await onChanged();
      onTell('Skipped.', `${stored.name} will not air on ${r.dayLong}.`, async () => {
        await edit((x) => putBackIn(x, on));
        await onChanged();
      });
    });

  const putBack = (r: DateRow) =>
    act(r, 'Not put back', async () => {
      const on = r.on!;
      await edit((x) => putBackIn(x, on));
      await onChanged();
      onTell('Put back.', `${stored.name} airs on ${dateWords(on, { weekday: 'long' })}.`, async () => {
        await edit((x) => skipIn(x, on));
        await onChanged();
      });
    });

  /** A drafted episode's time, set and said; the way back is the time it had. Only the time is
   *  sent, either way: a length the episode inherits stays inherited. */
  async function patch(r: DateRow, startMs: number, said: string) {
    const n = r.series ?? current?.number ?? 1;
    const was = { air_at: zonedString(r.start, zone) };
    await api.patchEpisode(slug, n, r.number!, { air_at: zonedString(startMs, zone) });
    await onChanged();
    onTell('Moved.', said, async () => {
      await api.patchEpisode(slug, n, r.number!, was);
      await onChanged();
    });
  }

  function doMove(r: DateRow) {
    const at = minutesOf(moveTime);
    if (!moveDate || at === null) {
      moveErr = 'Give a date and a time, such as 20:00.';
      return;
    }
    // The neighbours shown say at once what the station would refuse; it is still the judge.
    if (bounds && (moveDate <= bounds.after || (bounds.before !== null && moveDate >= bounds.before))) {
      moveErr = boundsWords(bounds);
      return;
    }
    const to = moveDate;
    const time = moveTime.slice(0, 5);
    const ms = fromLocalInput(`${to}T${time}`, zone);
    const length = r.end - r.start;
    const said = `${r.number !== null ? `Episode ${r.number}` : stored.name} is on ${dateWords(to, { weekday: 'long' })}, ${time} to ${hm((ms ?? r.start) + length, zone)}.`;
    moving = null;
    void act(r, 'Not moved', async () => {
      if (r.kind === 'open') {
        const on = r.on!;
        await edit((x) => moveIn(x, on, to, time));
        await onChanged();
        onTell('Moved.', said, async () => {
          await edit((x) => unmoveIn(x, on));
          await onChanged();
        });
      } else if (ms !== null) await patch(r, ms, said);
    });
  }

  /** Back to the rule: a moved date airs on the rule's own date and time again. */
  const unmove = (r: DateRow) =>
    act(r, 'Not moved back', async () => {
      const on = r.on!;
      const said = `${r.number !== null ? `Episode ${r.number}` : stored.name} is back on ${dateWords(on, { weekday: 'long' })}, as the rule has it.`;
      if (r.kind === 'open') {
        const had = rule?.moves.find((m) => m.from === on) ?? null;
        await edit((x) => unmoveIn(x, on));
        await onChanged();
        onTell('Moved.', said, had ? async () => {
          await edit((x) => ({ ...unmoveIn(x, on), moves: [...x.moves.filter((m) => m.from !== on), had].sort((a, b) => a.from.localeCompare(b.from)) }));
          await onChanged();
        } : null);
      } else {
        const usual = rule?.start ?? null;
        const ms = usual === null ? null : fromLocalInput(`${on}T${String(Math.floor(usual / 60)).padStart(2, '0')}:${String(usual % 60).padStart(2, '0')}`, zone);
        if (ms !== null) await patch(r, ms, said);
      }
    });

  /** Draft: the station drafts the series' next date that no episode fills, in date order. */
  async function draft(count: number, r: DateRow | null) {
    if (busy) return;
    if (r) rowEl(r.key)?.focus();
    busy = true;
    try {
      let n = current?.number ?? null;
      if (n === null) {
        await api.putSeries(slug, 1, {});
        n = 1;
      }
      const made = await api.createEpisodes(slug, n, { count });
      await onChanged();
      const first = made[0]?.episode;
      const last = made.at(-1)?.episode;
      if (!first) return;
      const day = (s: number | null) => (s === null ? '' : dateWords(dateIn(s * 1000, zone), { weekday: 'long' }));
      const said = made.length === 1 ? `Episode ${first.number}, ${day(first.air_at)}.` : `Episodes ${first.number} to ${last!.number}, the last on ${day(last!.air_at)}.`;
      onTell('Drafted.', said, first.air_at !== null ? async () => onOpen(`ep:${first.id}`, first.air_at! * 1000) : null, made.length === 1 ? 'Open it' : 'Open the first');
    } catch (e) {
      fail('Not drafted', e);
    } finally {
      busy = false;
    }
  }

  /** Change…: the rule dialog, for the series' own rule when it has one, else the show's. */
  function change() {
    const saved = async () => {
      await onChanged();
    };
    if (own && current) openRuleDialog({ slug, name: stored.name, series: current.number, rule: own, follow: stored.rule ?? null, colour, onSaved: saved });
    else openRuleDialog({ slug, name: stored.name, rule: rulesOn ? (stored.rule ?? null) : null, follow: !rulesOn && stored.slot ? ruleOfSlot(stored.slot) : null, nextOn: stored.next_on?.on ?? null, colour, onSaved: saved });
  }
</script>

<div class="fields dates" bind:this={host} aria-busy={reading || undefined} style:--show-colour="var(--cart-{colour})">
  {#if unreachable && !view}
    <div class="problem" role="alert">
      <svg aria-hidden="true"><use href="#g-degraded" /></svg>
      <span>The station cannot be reached, so the dates cannot be shown.</span>
    </div>
    <div><button type="button" class="btn sm" onclick={load}>Try again</button></div>
  {:else if !loaded}
    <p class="caption" role="status">Reading the dates…</p>
  {:else}
    <div class="sum">
      <div class="lab">
        <span>When it airs</span>
        {#if rule || !rulesOn}<button type="button" class="act" onclick={change}>Change…</button>{/if}
      </div>
      {#if !rulesOn}
        <p class="say">{stored.slot ? (stored.rule_words?.short ?? 'A weekly time') : 'No dates yet'}</p>
        <p class="last">This station keeps a weekly time only, so only drafted episodes are listed. Update it to see every date.</p>
      {:else if !rule}
        <p class="say">No dates yet.</p>
        <p class="last">Give it dates and they are listed here.</p>
        <div><button type="button" class="btn sm primary" onclick={onGiveDates}>Give it dates…</button></div>
      {:else}
        <p class="say">{sentence}</p>
        {#if own && current}<p class="last">Series {current.number} has its own times.</p>{/if}
        {#if finished}
          <p class="last"><b>Finished:</b> {finished}.</p>
          <div><button type="button" class="btn sm" onclick={change}>Give it new dates…</button></div>
        {:else}
          {#if lastLine}<p class="last">{lastLine}</p>{/if}
          {#if rows.length}<p class="count"><b>{counts.lead}</b> {counts.rest}</p>{/if}
        {/if}
      {/if}
      {#if unreachable}<p class="last" role="alert">The station cannot be reached, so these may be out of date. <button type="button" class="act2" onclick={load}>Try again</button></p>{/if}
    </div>

    {#if rows.length}
      <h3 class="lst" id="sd-h">Coming up</h3>
      <ol class="nd dl coming" aria-labelledby="sd-h">
        {#each rows as r, i (r.key)}
          {@const stop = r.key === rowStop ? 0 : -1}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions, a11y_click_events_have_key_events -->
          <li
            class="r"
            class:skipped={r.kind === 'skipped'}
            class:clash={!!r.clash && r.kind !== 'skipped'}
            class:drafted={r.kind === 'drafted' || r.kind === 'hand'}
            class:cur={isCur(r)}
            data-row={r.key}
            data-kind={r.kind}
            tabindex={stop}
            aria-current={isCur(r) ? 'true' : undefined}
            aria-label={rowAria(r)}
            onfocusin={() => (rowFocus = r.key)}
            onkeydown={(e) => rowKeys(e, r, i)}
            onclick={(e) => rowClick(e, r)}
          >
            <span class="n num">{r.ep || '–'}</span>
            <span class="on">{r.day}</span>
            <span class="t">{r.times}</span>
            {#if r.blockId}<button type="button" class="act2 open" tabindex={stop} aria-label="Open {r.number !== null ? `episode ${r.number}` : r.dayLong}" onclick={() => openRow(r)}>Open ›</button>{/if}
            <span class="st below">
              {#if r.kind === 'skipped'}
                <span class="flag skip"><svg aria-hidden="true"><use href="#g-muted" /></svg>Skipped</span>
              {:else}
                <span class="tag">{r.kind === 'open' ? 'not drafted' : 'drafted'}</span>
                <!-- Said only where a rule could have made it: an older station, or a show with no rule, has no other kind. -->
                {#if r.kind === 'hand' && rule}<span class="tag">made by hand</span>{/if}
                {#if r.title}<span class="why">{r.title}</span>{/if}
                {#if r.onAir}<span class="flag onair"><svg aria-hidden="true"><use href="#g-onair" /></svg>On air</span>{/if}
                {#if r.moved}<span class="flag moved"><svg aria-hidden="true"><use href="#g-moved" /></svg>Moved · {r.moved}</span>{/if}
                {#if r.clash}<span class="flag"><svg aria-hidden="true"><use href="#g-fault" /></svg>Conflict</span>{/if}
                {#if r.clock}<span class="tag">{r.clock}</span>{/if}
              {/if}
            </span>
            <span class="do">
              {#if r.kind === 'skipped'}
                <button type="button" class="act2" tabindex={stop} disabled={busy} onclick={() => putBack(r)}>Put back</button>
              {:else}
                {#if r.first}<button type="button" class="act2" tabindex={stop} disabled={busy} onclick={() => draft(1, r)}>Draft</button>{/if}
                {#if r.moved && r.on}
                  <button type="button" class="act2" tabindex={stop} disabled={busy} onclick={() => unmove(r)}>Back to the rule</button>
                {:else}
                  {#if r.kind === 'open'}<button type="button" class="act2" tabindex={stop} disabled={busy} onclick={() => skip(r)}>Skip</button>{/if}
                  <button type="button" class="act2" tabindex={stop} disabled={busy} aria-expanded={moving === r.key} onclick={() => (moving === r.key ? closeMove(r.key) : openMove(r))}>Move…</button>
                {/if}
              {/if}
            </span>
            {#if r.clash && r.kind !== 'skipped'}<span class="why2">{#if r.refused}<b>{r.refused}</b>{' '}{/if}{r.clash}</span>{/if}
          </li>
          {#if moving === r.key}
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <li class="mv" onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); e.stopPropagation(); doMove(r); } else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); void closeMove(r.key); } }}>
              <label for="sd-mv-date">Move this date to</label>
              <input id="sd-mv-date" class="date" type="date" bind:value={moveDate} aria-invalid={moveErr ? true : undefined} aria-describedby="sd-mv-hint" />
              <span class="at"><label for="sd-mv-time">at</label> <TimeField id="sd-mv-time" bind:value={moveTime} oncarry={(d) => (moveDate = addDays(moveDate, d))} /></span>
              <button type="button" class="btn sm primary" onclick={() => doMove(r)}>Move</button>
              <button type="button" class="btn sm quiet" onclick={() => closeMove(r.key)}>Cancel</button>
              {#if moveErr}<p id={bounds && moveErr === boundsWords(bounds) ? 'sd-mv-hint' : undefined} class="err" role="alert">{moveErr}</p>{/if}
              {#if bounds && moveErr !== boundsWords(bounds)}<span id="sd-mv-hint" class="hint">{boundsWords(bounds)}</span>{/if}
            </li>
          {/if}
        {/each}
      </ol>
      {#if after.more || after.words}
        <p class="after">
          {#if after.more}<button type="button" class="btn sm" onclick={() => (shown = Math.min(60, shown + 10))}>Show more dates</button>{/if}
          {#if after.words}<span>{after.words}</span>{/if}
        </p>
      {/if}
    {/if}

    {#if aired.length}
      <details class="aired">
        <summary><b>Aired</b> · {aired.length}</summary>
        <ol class="nd dl">
          {#each aired as a, i (a.id)}
            {#if earlier.length && (i === 0 || aired[i - 1].series !== a.series)}<li class="ser">Series {a.series}</li>{/if}
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_click_events_have_key_events -->
            <li class="r aired" class:cur={selected === `ep:${a.id}`} data-aired={a.id} onclick={() => onShow(`ep:${a.id}`, a.start)}>
              <span class="n num">{a.ep}</span>
              <span class="on">{a.day}</span>
              <span class="t">{a.times}</span>
              <button type="button" class="act2 open" aria-label="Show {a.ep.replace('Ep', 'episode')} in its week" onclick={() => onShow(`ep:${a.id}`, a.start)}>Show ›</button>
              <span class="st below"><span class="tag"><svg aria-hidden="true"><use href="#g-lock" /></svg>aired</span>{#if a.title}<span class="why">{a.title}</span>{/if}</span>
            </li>
          {/each}
        </ol>
      </details>
    {/if}
  {/if}
</div>
<div class="foot">
  {#if firstOpen}
    <div class="r">
      <button type="button" class="btn primary" disabled={busy} onclick={() => draft(1, null)}>Draft the next <small>· {firstOpen.ep}, {firstOpen.day}</small></button>
      {#if draftAll}<button type="button" class="btn" disabled={busy} onclick={() => draft(draftAll, null)}>Draft all {draftAll}</button>{/if}
    </div>
  {/if}
  <p class="caption">Changes here are saved as you make them.</p>
</div>
