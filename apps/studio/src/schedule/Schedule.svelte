<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The Schedule tab (calendar.md; ADR-0021, ADR-0045, ADR-0055): the toolbar, the week grid
  // drawn through the adapter (Event Calendar for now), and the editor column, which shows the
  // selected episode (with its series and show a level away), the generated hour of an
  // automation block with its rule checks, or the card editor. Panel names `calendar.toolbar`,
  // `calendar.week`, `calendar.episode-editor`, `calendar.series-editor`,
  // `calendar.aired-episode`, `calendar.log-editor`, `calendar.card-editor` (ADR-0070); the
  // design's `calendar.frame` is the app's own `studio.frame`, so it is not drawn twice.
  //
  // New show opens the guided dialog (`calendar.new-show`, ADR-0278). The week's dotted blocks
  // are the dates each show's rule gives, sent by the control plane with the episodes: the
  // studio expands no rule. A dotted block offers Draft this episode and Skip this date; with
  // nothing selected the right panel lists the shows with nothing coming up
  // (`calendar.unscheduled`).
  import { onDestroy, onMount, untrack } from 'svelte';
  import EventCalendarGrid from './calendar/EventCalendarGrid.svelte';
  import EpisodeEditor from './EpisodeEditor.svelte';
  import LogEditor from './LogEditor.svelte';
  import ShowEditor from './ShowEditor.svelte';
  import CardEditor from './CardEditor.svelte';
  import * as api from '../lib/schedule.ts';
  import { loadWeek, moveWeek, sched, select, today, weekBlocks } from '../lib/schedule.svelte.ts';
  import { say, stationNow, studio } from '../lib/studio.svelte.ts';
  import { load as loadRetention } from '../lib/retention.ts';
  import { dayLength, hm, rangeWords, whenWords, zonedString } from './model.ts';
  import type { Block } from './model.ts';
  import * as rulesApi from '../lib/api.ts';
  import type { IsoDate, Rule, ShowSaved } from '../lib/api.ts';
  import type { Show } from '../lib/schedule.ts';
  import NewShow from './rule/NewShow.svelte';
  import RuleDialog from './rule/RuleDialog.svelte';
  import Unscheduled from './rule/Unscheduled.svelte';
  import { closeRuleDialog, openRuleDialog, ruleDialog } from './rule/dialog.svelte.ts';
  import { dateWords, ruleOfSlot } from './rule/form.ts';
  import { dateIn } from './rule/view.ts';
  import { deletedWords } from './showDelete.ts';
  import { Heard, takeOwn } from '../lib/ruleNotice.ts';

  import { account } from '../lib/account.svelte.ts';
  import { isAdmin } from '../lib/account.ts';

  let { live = true, dialog: firstDialog = null }: { live?: boolean; /** The dialog open (a test's). */ dialog?: 'new-show' | null } = $props();

  /** Only an admin (or the station key) can choose the zone; for anyone else the button would lead nowhere. */
  const canChooseZone = $derived(!account.loaded || account.me === null || isAdmin(account.me));
  // The week to the minute, not the studio clock's tenth: the clock ticks every 100 ms, and the
  // week's blocks rebuilt from it redrew the whole calendar ten times a second, which made typing in
  // the editors lag and lost clicks on blocks (the operator's 1.0.2 run). A derived number that does
  // not change stops here, so the grid and the editors update once a minute.
  const now = $derived(Math.floor(stationNow() / 60_000) * 60_000);
  const week = $derived(weekBlocks(now));
  const dst = $derived.by(() => {
    const i = week.days.findIndex((_, k) => k < 7 && Math.round((week.days[k + 1] - week.days[k]) / 3_600_000) !== 24);
    return i < 0 ? '' : `${['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'][i]}: ${dayLength(week.days, i)}`;
  });
  const selectedBlock = $derived(week.blocks.find((b) => b.id === sched.selected) ?? null);
  const selectedEpisode = $derived(
    selectedBlock?.kind === 'episode' ? sched.episodes.find((e) => `ep:${e.episode.id}` === selectedBlock.id) ?? null : null,
  );
  const rulesOn = $derived(sched.rules === true);
  const todayIso = $derived(dateIn(now, sched.zone));
  const editShow = $derived(sched.shows.find((s) => s.slug === sched.editShow) ?? null);
  const ruleShow = $derived(selectedBlock?.kind === 'rule' ? sched.shows.find((s) => s.slug === selectedBlock.slug) ?? null : null);

  onMount(() => {
    if (!live) return;
    void refresh();
    void loadRetention()
      .then((v) => (sched.us = v.setting.us_statutory_licence))
      .catch(() => {});
  });

  // The week is fetched again when the anchor moves or something was saved.
  let seen = $state(-1);
  $effect(() => {
    const v = sched.version;
    if (!live || v === seen) return;
    seen = v;
    void refresh();
  });

  async function refresh() {
    await loadWeek();
  }

  async function changed() {
    sched.version++;
    await loadWeek();
  }

  /** A block dragged or resized: the episode's new time, or the reason the control plane refused. */
  async function onMove(id: string, start: number, end: number): Promise<string | null> {
    const w = sched.episodes.find((e) => `ep:${e.episode.id}` === id);
    if (!w) return 'not an episode';
    const minutes = Math.max(1, Math.round((end - start) / 60_000));
    try {
      await api.patchEpisode(w.show.slug, w.series.number, w.episode.number, {
        air_at: zonedString(start, sched.zone),
        minutes,
      });
      await changed();
      return null;
    } catch (e) {
      const why = e instanceof Error ? e.message : String(e);
      tell('Not moved', why);
      return why;
    }
  }

  // ---- the guided dialogs ----
  // svelte-ignore state_referenced_locally
  let newShow = $state<{ at: { date: IsoDate; from: string } | null } | null>(firstDialog === 'new-show' ? { at: null } : null);
  /** Give it dates…: the rule dialog for a show with nothing coming up. */
  function giveDates(s: Show) {
    openRuleDialog({
      slug: s.slug,
      name: s.name,
      rule: rulesOn ? (s.rule ?? null) : null,
      follow: !rulesOn && s.slot ? ruleOfSlot(s.slot) : null,
      nextOn: s.next_on?.on ?? null,
      colour: s.colour_slot,
      onSaved: async () => {
        await changed();
        tell('Saved.', `${s.name} has its dates; they are in the week as dotted blocks.`);
      },
    });
  }
  /** The tab's own line: something done here, with the way back when there is one. */
  let toast = $state<{ title: string; message: string; undo: (() => Promise<void>) | null } | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | null = null;
  function tell(title: string, message: string, undo: (() => Promise<void>) | null = null) {
    if (toastTimer) clearTimeout(toastTimer);
    toast = { title, message, undo };
    toastTimer = setTimeout(() => (toast = null), undo ? 12_000 : 6000);
  }

  // ---- another window changed when a show airs ----
  // The station tells every window (`rule.changed`, new-show.md section 3). The tab says it in
  // its own line and reads the week and the shows again; the change this window made itself it
  // has already said and drawn, so that notice is passed over. The line is its own, beside the
  // toast and not in its place: a Skipped with its Undo is not lost to somebody else's change.
  let heard = $state<string | null>(null);
  let heardTimer: ReturnType<typeof setTimeout> | null = null;
  const notices = new Heard();
  $effect(() => {
    const v = studio.view;
    if (!live || !v) return;
    const news = notices.fresh(v.notices, v.now_ms);
    if (!news.length) return;
    untrack(() => {
      const others = news.filter((n) => !takeOwn(n.title, sched.shows));
      const last = others.at(-1);
      if (!last) return;
      if (heardTimer) clearTimeout(heardTimer);
      heard = last.title;
      heardTimer = setTimeout(() => (heard = null), 10_000);
      void reread();
    });
  });
  onDestroy(() => {
    if (heardTimer) clearTimeout(heardTimer);
    if (toastTimer) clearTimeout(toastTimer);
  });

  /** Read the week and the shows again after a change made elsewhere. A read already on its way
   *  may have left before the change, so it is let finish and the week is read once more. */
  async function reread() {
    if (sched.loading) await loadWeek();
    await changed();
  }

  /** A show was deleted from its editor (ADR-0279): the editor closes, whatever was selected of it
   *  goes with it, the week and the shows without dates are read again, and the tab says so. It
   *  cannot be put back, so there is no Undo. */
  async function showDeleted(name: string) {
    sched.pane = 'episode';
    sched.editShow = null;
    select(null);
    await changed();
    tell(deletedWords(name), '');
  }

  /** New show; with a block selected in the week, its day and hour start First on and from. */
  function openNewShow() {
    const b = selectedBlock;
    newShow = { at: b ? { date: dateIn(b.start, sched.zone), from: hm(b.start, sched.zone) } : null };
  }

  /** Open episode 1: the week moves to the episode and selects it. */
  function openEpisode(saved: ShowSaved) {
    const ep = saved.drafted?.episode;
    if (!ep) return;
    if (ep.air_at !== null) {
      sched.anchor = ep.air_at * 1000;
      sched.version++;
    }
    sched.pane = 'episode';
    select(`ep:${ep.id}`);
  }

  /** Skip this date: the rule is read first, then saved with the date added to its skipped
   *  dates. Nothing moves; automation fills the hour; Undo takes the date out again. */
  async function skipDate(b: Block) {
    const s = sched.shows.find((x) => x.slug === b.slug);
    if (!s || !b.on) return;
    const on = b.on;
    /** The rule the date comes from: the series' own when it has one, else the show's. */
    const read = async (): Promise<{ rule: Rule; series: number | null } | null> => {
      const view = await api.show(s.slug);
      const own = b.series !== null ? (view.series.find((x) => x.number === b.series)?.rule ?? null) : null;
      const rule = own ?? view.show.rule ?? null;
      return rule ? { rule, series: own ? b.series : null } : null;
    };
    const put = (rule: Rule, series: number | null) => (series !== null ? rulesApi.putSeriesRule(s.slug, series, rule, 'leave') : rulesApi.putShowRule(s.slug, rule, 'leave'));
    try {
      const got = await read();
      if (!got) return;
      await put({ ...got.rule, skip: [...got.rule.skip.filter((d) => d !== on), on].sort(), moves: got.rule.moves.filter((m) => m.from !== on) }, got.series);
      select(null);
      await changed();
      tell('Skipped.', `${s.name} will not air on ${dateWords(on, { weekday: 'long' })}.`, async () => {
        const again = await read();
        if (again) await put({ ...again.rule, skip: again.rule.skip.filter((d) => d !== on) }, again.series);
        await changed();
      });
    } catch (e) {
      tell('Not skipped', e instanceof Error ? e.message : String(e));
    }
  }

  /** An older station sends no dates for the week, so a show's next episode is drafted from
   *  the list of shows instead: the station takes the next slot after the series' last. */
  async function draftNext(s: Show) {
    try {
      const view = await api.show(s.slug);
      const n = view.series.at(-1)?.number ?? 1;
      if (!view.series.length) await api.putSeries(s.slug, 1, {});
      const made = await api.createEpisodes(s.slug, n, { count: 1 });
      const ep = made[0]?.episode;
      if (ep?.air_at) sched.anchor = ep.air_at * 1000;
      await changed();
      if (ep) select(`ep:${ep.id}`);
    } catch (e) {
      tell('Not drafted', e instanceof Error ? e.message : String(e));
    }
  }

  async function draftFromRule(b: Block) {
    const s = sched.shows.find((x) => x.slug === b.slug);
    if (!s) return;
    try {
      // Drafting stays in date order: the station drafts the series' next date that no episode
      // fills, never into the past, and says which rule date the episode fills.
      let n = b.series;
      if (n === null) {
        const view = await api.show(s.slug);
        n = view.series.at(-1)?.number ?? 1;
        if (!view.series.length) await api.putSeries(s.slug, 1, {});
      }
      const made = await api.createEpisodes(s.slug, n, { count: 1 });
      await changed();
      const ep = made[0]?.episode;
      if (ep) select(`ep:${ep.id}`);
    } catch (e) {
      tell('Not drafted', e instanceof Error ? e.message : String(e));
    }
  }
</script>

<div class="schedule" data-surface="schedule">
  <div class="toolbar" data-panel="calendar.toolbar">
    <button type="button" class="btn quiet" aria-label="Previous week" title="Previous week (←)" onclick={() => moveWeek(-1)}>‹</button>
    <button type="button" class="btn quiet" aria-label="Next week" title="Next week (→)" onclick={() => moveWeek(1)}>›</button>
    <button type="button" class="btn" onclick={() => today(now)}>Today <kbd>T</kbd></button>
    <span class="range num" data-panel-title>{rangeWords(week.days, sched.zone)}</span>
    <span class="seg" role="group" aria-label="View">
      <button type="button" aria-pressed={sched.view === 'week'} onclick={() => (sched.view = 'week')}>Week</button>
      <button type="button" aria-pressed={sched.view === 'studios'} aria-disabled="true" title="Studios are not in the model yet" onclick={() => say('good', 'Not in this build', 'Studios as columns wait for a studio on each show (a choice for the operator).')}>Studios</button>
    </span>
    <span class="tz">{sched.zone}{dst ? ` · ${dst}` : ''}</span>
    {#if sched.zoneSet === false && canChooseZone}
      <button type="button" class="btn" title="Nobody has chosen the station's time zone yet" onclick={() => (studio.settings = true)}>Choose the time zone…</button>
    {/if}
    <button type="button" class="btn primary" onclick={openNewShow}>New show</button>
    {#if sched.loading}<span class="load" role="status">Loading…</span>{/if}
    {#if sched.error}<span class="load g-warning" role="alert">{sched.error}</span>{/if}
    <div class="legend" aria-label="Legend">
      <span><svg aria-hidden="true"><use href="#g-mic" /></svg>Live</span>
      <span><svg aria-hidden="true"><use href="#g-auto" /></svg>Automation fills every gap by its clock</span>
      <span><svg aria-hidden="true"><use href="#g-repeat" /></svg>A date from the show’s rule</span>
      <span><svg aria-hidden="true" style="color:var(--status-warning)"><use href="#g-fault" /></svg>Conflict</span>
      <span><i class="past"></i>Aired, frozen</span>
    </div>
  </div>

  {#if toast || heard}
    <div class="toast2" role="status" data-testid="schedule-toast">
      {#if toast}<span><b>{toast.title}</b> {toast.message}</span>{/if}
      {#if heard}<span class="heard" data-testid="schedule-heard"><svg aria-hidden="true"><use href="#g-repeat" /></svg><span><b>{heard}</b> The week is up to date.</span></span>{/if}
      {#if toast?.undo}<button type="button" class="btn sm" onclick={async () => { const undo = toast?.undo; toast = null; try { await undo?.(); } catch (e) { tell('Not put back', e instanceof Error ? e.message : String(e)); } }}>Undo</button>{/if}
    </div>
  {/if}
  <div class="body">
    <section class="cal" data-panel="calendar.week" aria-label="Week of {rangeWords(week.days, sched.zone)}">
      <div class="corner" data-panel-title><span class="sr-only">Week grid</span></div>
      <div class="grid-host">
        {#if live}
          <EventCalendarGrid
            blocks={week.blocks}
            days={week.days}
            zone={sched.zone}
            {now}
            selected={sched.selected}
            onSelect={(id) => select(id)}
            {onMove}
          />
        {:else}
          <div class="empty">The grid is drawn by the calendar library in a window; the tests render the editors.</div>
        {/if}
      </div>
    </section>

    {#if sched.pane === 'edit-show' && editShow}
      <aside class="editor" data-panel="calendar.episode-editor" aria-label="Show editor">
        {#key editShow.slug}
          <ShowEditor slug={editShow.slug} show={editShow} onSaved={async (v) => { await changed(); sched.pane = 'episode'; sched.editShow = null; tell('Show saved.', `${v.show.name} is up to date.`); }} onRule={changed} onDeleted={showDeleted} onCancel={() => { sched.pane = 'episode'; sched.editShow = null; }} onCard={() => {}} />
        {/key}
      </aside>
    {:else if sched.pane === 'card' && (selectedEpisode || ruleShow)}
      <aside class="editor" data-panel="calendar.card-editor" aria-label="Card editor">
        <CardEditor
          slug={selectedEpisode?.show.slug ?? ruleShow?.slug ?? ''}
          level={selectedEpisode ? 'episode' : 'show'}
          item={selectedEpisode}
          zone={sched.zone}
          onBack={() => (sched.pane = 'episode')}
          onChanged={changed}
        />
      </aside>
    {:else if selectedBlock?.kind === 'automation'}
      <aside class="editor" data-panel="calendar.log-editor" aria-label="Log editor">
        <LogEditor block={selectedBlock} zone={sched.zone} {now} />
      </aside>
    {:else if selectedEpisode}
      <aside class="editor" data-panel={selectedEpisode.episode.aired_at !== null ? 'calendar.aired-episode' : 'calendar.episode-editor'} aria-label="Episode editor">
        <EpisodeEditor item={selectedEpisode} zone={sched.zone} {now} us={sched.us} onChanged={changed} onDeleted={showDeleted} onCard={() => (sched.pane = 'card')} />
      </aside>
    {:else if selectedBlock?.kind === 'rule' && ruleShow}
      <aside class="editor" data-panel="calendar.episode-editor" aria-label="A date from the show's rule">
        <div class="head">
          <div class="crumbs"><b>{ruleShow.name}</b> › a date from its rule</div>
          <h2 data-panel-title>{ruleShow.name} · not drafted</h2>
          <div class="when"><span class="num">{whenWords(selectedBlock.start, selectedBlock.end, sched.zone)}</span></div>
        </div>
        <div class="nothing">
          <p>This is where the show’s rule puts {selectedBlock.number !== null ? `episode ${selectedBlock.number}` : 'its next episode'}{#if ruleShow.rule_words?.sentence}: <b>{ruleShow.rule_words.sentence}</b>{:else}.{/if} Nothing is drafted yet, so playout fills the hour with automation until someone drafts it.</p>
          <p><b>Draft it</b> and the episode takes its number and this time from the rule; the title and guests are yours to add. <b>Skip it</b> and the show does not air that day; the rule carries on{#if ruleShow.rule?.end.kind === 'count'}, and since a skipped date does not count, its {ruleShow.rule.end.count} episodes end one date later{/if}.</p>
        </div>
        <div class="foot">
          <div class="r">
            <button type="button" class="btn primary" onclick={() => draftFromRule(selectedBlock!)}>Draft this episode</button>
            {#if selectedBlock.on && rulesOn}<button type="button" class="btn" onclick={() => skipDate(selectedBlock!)}>Skip this date</button>{/if}
            <button type="button" class="btn" onclick={() => (sched.pane = 'card')}>Card…</button>
          </div>
          <p class="caption">A skipped date is put back from <b>When it airs</b> in the show editor, where it stays listed.</p>
        </div>
      </aside>
    {:else}
      <aside class="editor" data-panel="calendar.episode-editor" aria-label="Episode editor">
        {#if !sched.shows.length && !sched.loading && !sched.error}
          <!-- An empty station says how to start first (the operator's 1.0.2 run: "we can't see any blocks"). -->
          <div class="head"><h2 data-panel-title>No shows yet</h2></div>
          <div class="nothing">
            <p>The week fills from shows. Make the first one:</p>
            <ol>
              <li><button type="button" class="btn primary" onclick={openNewShow}>New show</button> and give it a name.</li>
              <li>Say <b>when it airs</b>: one date and time, and whether it comes back.</li>
              <li>Its first episode is drafted for you; the other dates wait here as <b>dotted blocks</b>.</li>
            </ol>
            <p>While an episode's time holds, its show is on air, and its step (a new show starts at Solo) shapes the studio.</p>
          </div>
        {:else}
          <div class="head"><h2 data-panel-title>Nothing selected</h2></div>
          <div class="nothing">
            <!-- A show with nothing coming up draws nothing in the week (the operator's 1.0.2 run). -->
            <Unscheduled shows={sched.shows} rules={rulesOn} ondates={giveDates} onedit={(s) => { sched.editShow = s.slug; sched.pane = 'edit-show'; }} ondraft={draftNext} />
            <p>Select a block in the week: an <b>episode</b> opens its editor, with its series and show a level away; an <b>automation</b> block opens the generated hour with its rule checks; a dotted <b>date from a show’s rule</b> offers to draft the episode or skip the date.</p>
            <p>Keys on this tab: <kbd>←</kbd> <kbd>→</kbd> move a week, <kbd>T</kbd> is today. The grid's blocks take focus with <kbd>Tab</kbd> and open with <kbd>⏎</kbd>.</p>
          </div>
        {/if}
      </aside>
    {/if}
  </div>
</div>

{#if newShow}
  <NewShow
    shows={sched.shows}
    zone={sched.zone}
    today={todayIso}
    zoneSet={sched.zoneSet !== false}
    {canChooseZone}
    onzone={() => (studio.settings = true)}
    rules={rulesOn}
    at={newShow.at}
    {live}
    onMade={changed}
    onOpen={openEpisode}
    onClose={() => (newShow = null)}
  />
{/if}
{#if ruleDialog.open}
  {#key ruleDialog.open}
    <RuleDialog
      {...ruleDialog.open}
      zone={sched.zone}
      today={todayIso}
      zoneSet={sched.zoneSet !== false}
      {canChooseZone}
      onzone={() => (studio.settings = true)}
      rules={rulesOn}
      onClose={closeRuleDialog}
    />
  {/key}
{/if}
