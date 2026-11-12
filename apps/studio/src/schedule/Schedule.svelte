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
  import { onMount } from 'svelte';
  import EventCalendarGrid from './calendar/EventCalendarGrid.svelte';
  import EpisodeEditor from './EpisodeEditor.svelte';
  import LogEditor from './LogEditor.svelte';
  import ShowEditor from './ShowEditor.svelte';
  import CardEditor from './CardEditor.svelte';
  import * as api from '../lib/schedule.ts';
  import { loadWeek, moveWeek, sched, select, today, weekBlocks } from '../lib/schedule.svelte.ts';
  import { say, stationNow, studio } from '../lib/studio.svelte.ts';
  import { load as loadRetention } from '../lib/retention.ts';
  import { dayLength, rangeWords, zonedString } from './model.ts';
  import type { Block } from './model.ts';

  import { account } from '../lib/account.svelte.ts';
  import { isAdmin } from '../lib/account.ts';

  let { live = true }: { live?: boolean } = $props();

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
  /** Shows with no weekly rule: nothing of them is drawn in the week, so the pane lists them. */
  const unruled = $derived(sched.shows.filter((s) => !s.slot));
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
      say('warning', 'Not moved', why);
      return why;
    }
  }

  async function draftFromRule(b: Block) {
    const s = sched.shows.find((x) => x.slug === b.slug);
    if (!s) return;
    try {
      const view = await api.show(s.slug);
      const last = view.series.at(-1);
      const n = last?.number ?? 1;
      // The control plane drafts the rule's next slot after the series' last episode, or after
      // its start date, or after now. A block selected in the week is a slot of its own, perhaps
      // one already under way (the operator's 1.0.2 run drafted next week's instead of today's), so
      // the series starts on that block's day unless it starts earlier already.
      const day = new Intl.DateTimeFormat('en-CA', { timeZone: sched.zone, year: 'numeric', month: '2-digit', day: '2-digit' }).format(b.start);
      if (!last) await api.putSeries(s.slug, 1, { starts_on: day });
      else if (!last.starts_on || last.starts_on > day) {
        const { title, description, hosts, slot, planned_episodes, tags } = last;
        await api.putSeries(s.slug, n, { title, description, hosts, slot, planned_episodes, tags, starts_on: day });
      }
      const made = await api.createEpisodes(s.slug, n, { count: 1 });
      await changed();
      const ep = made[0]?.episode;
      if (ep) select(`ep:${ep.id}`);
    } catch (e) {
      say('warning', 'Not drafted', e instanceof Error ? e.message : String(e));
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
    <button type="button" class="btn primary" onclick={() => { sched.pane = 'new-show'; }}>New show</button>
    {#if sched.loading}<span class="load" role="status">Loading…</span>{/if}
    {#if sched.error}<span class="load g-warning" role="alert">{sched.error}</span>{/if}
    <div class="legend" aria-label="Legend">
      <span><svg aria-hidden="true"><use href="#g-mic" /></svg>Live</span>
      <span><svg aria-hidden="true"><use href="#g-auto" /></svg>Automation fills every gap by its clock</span>
      <span><svg aria-hidden="true"><use href="#g-repeat" /></svg>Instance of a weekly rule</span>
      <span><svg aria-hidden="true" style="color:var(--status-warning)"><use href="#g-fault" /></svg>Conflict</span>
      <span><i class="past"></i>Aired, frozen</span>
    </div>
  </div>

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

    {#if sched.pane === 'new-show'}
      <aside class="editor" data-panel="calendar.episode-editor" aria-label="New show">
        <ShowEditor slug={null} show={null} onSaved={async (v) => { await changed(); sched.pane = 'episode'; const s = v.show; sched.selected = null; say('good', 'Show saved', `${s.name} is a show now; draft its first episode from its weekly rule.`); }} onCancel={() => (sched.pane = 'episode')} onCard={() => {}} />
      </aside>
    {:else if sched.pane === 'edit-show' && unruled.find((s) => s.slug === sched.editShow)}
      {@const s = unruled.find((x) => x.slug === sched.editShow)!}
      <aside class="editor" data-panel="calendar.episode-editor" aria-label="Show editor">
        <ShowEditor slug={s.slug} show={s} onSaved={async (v) => { await changed(); sched.pane = 'episode'; sched.editShow = null; say('good', 'Show saved', v.show.slot ? `${v.show.name}'s weekly rule is in the week now: select its dotted block to draft an episode.` : `${v.show.name} still has no weekly rule.`); }} onCancel={() => { sched.pane = 'episode'; sched.editShow = null; }} onCard={() => {}} />
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
        <EpisodeEditor item={selectedEpisode} zone={sched.zone} {now} us={sched.us} onChanged={changed} onCard={() => (sched.pane = 'card')} />
      </aside>
    {:else if selectedBlock?.kind === 'rule' && ruleShow}
      <aside class="editor" data-panel="calendar.episode-editor" aria-label="Weekly rule">
        <div class="head">
          <div class="crumbs"><b>{ruleShow.name}</b> › weekly rule</div>
          <h2 data-panel-title>{ruleShow.name} · not drafted</h2>
        </div>
        <div class="nothing">
          <p>This is where the show's weekly rule puts its next episode. Nothing is drafted yet, so playout fills the hour with automation until someone drafts it.</p>
          <p><b>Draft it</b> and the episode takes its number from the series (the last plus one) and this time from the rule; the title and guests are yours to add.</p>
        </div>
        <div class="foot">
          <div class="r"><button type="button" class="btn primary" onclick={() => draftFromRule(selectedBlock!)}>Draft this episode</button><button type="button" class="btn" onclick={() => (sched.pane = 'card')}>Card…</button></div>
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
              <li><button type="button" class="btn primary" onclick={() => { sched.pane = 'new-show'; }}>New show</button> and give it a name.</li>
              <li>Give it a <b>weekly rule</b>: a day, a start time and a length.</li>
              <li>Its weeks appear here as <b>dotted blocks</b>: select one and draft the episode.</li>
            </ol>
            <p>While an episode's time holds, its show is on air, and its step (a new show starts at Solo) shapes the studio.</p>
          </div>
        {:else}
          <div class="head"><h2 data-panel-title>Nothing selected</h2></div>
          <div class="nothing">
            {#if unruled.length}
              <!-- A show without a weekly rule draws nothing in the week (the operator's 1.0.2 run). -->
              <div class="unruled" data-testid="unruled-shows">
                <p><b>{unruled.length === 1 ? 'A show has' : 'Shows have'} no weekly rule yet</b>, so nothing of {unruled.length === 1 ? 'it' : 'them'} is in the week:</p>
                <ul>
                  {#each unruled as s (s.slug)}
                    <li><span>{s.name}</span> <button type="button" class="btn primary sm" onclick={() => { sched.editShow = s.slug; sched.pane = 'edit-show'; }}>Set its weekly rule…</button></li>
                  {/each}
                </ul>
              </div>
            {/if}
            <p>Select a block in the week: an <b>episode</b> opens its editor, with its series and show a level away; an <b>automation</b> block opens the generated hour with its rule checks; a dotted <b>weekly rule</b> offers to draft the episode.</p>
            <p>Keys on this tab: <kbd>←</kbd> <kbd>→</kbd> move a week, <kbd>T</kbd> is today. The grid's blocks take focus with <kbd>Tab</kbd> and open with <kbd>⏎</kbd>.</p>
          </div>
        {/if}
      </aside>
    {/if}
  </div>
</div>
