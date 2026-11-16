<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The Schedule tab (calendar.md; ADR-0021, ADR-0045, ADR-0055): the toolbar, the week grid
  // drawn through the adapter (Event Calendar for now), and the editor column, which shows the
  // selected episode (with its series a level away), the generated hour of an automation block
  // with its rule checks, the card editor, or a show's own pane. Panel names
  // `calendar.toolbar`, `calendar.week`, `calendar.episode-editor`, `calendar.series-editor`,
  // `calendar.aired-episode`, `calendar.log-editor`, `calendar.card-editor` (ADR-0070); the
  // design's `calendar.frame` is the app's own `studio.frame`, so it is not drawn twice.
  //
  // New show opens the guided dialog (`calendar.new-show`, ADR-0278). The week's dotted blocks
  // are the dates each show's rule gives, sent by the control plane with the episodes: the
  // studio expands no rule. A dotted block offers Draft this episode and Skip this date.
  //
  // Week clashes and a show's dates (week-clashes.md, release 1.0.5). Where shows clash the
  // week folds, so that one name is always readable, and a day opens out in the Day view
  // (`calendar.day`, with `calendar.day.strip`), only when asked: Day in the toolbar, a day's
  // header, Open the day in `calendar.clash`, or the count on a fold. With nothing open the
  // editor column lists the shows (`calendar.shows`, with `calendar.unscheduled` at its foot);
  // a row opens the show's pane at its Dates (`calendar.show-dates`), and ‹ Shows in every
  // pane's crumbs leads back to the list.
  //
  // Calendar files (ADR-0281, release 1.0.5). Subscribe… in the toolbar opens the link to the
  // schedule's `.ics` (`calendar.subscribe`), and Import a calendar… reads a file into new
  // shows (`calendar.import`), for an admin or a producer. Both wait for a station that sends
  // its public address with the week: one that does not is older than 1.0.5 and has neither.
  // The toolbar stays on one line at 1440 by 900 (week-clashes.md section 2), and the two
  // buttons with their words do not fit beside the legend there: in a window that has the room
  // they are two buttons, and in a narrower one they are the two entries of one small menu
  // beside New show (calendar-files.md section 1). The menu is a dialog, so it owns the keys.
  import { onDestroy, onMount, untrack } from 'svelte';
  import EventCalendarGrid from './calendar/EventCalendarGrid.svelte';
  import EpisodeEditor from './EpisodeEditor.svelte';
  import LogEditor from './LogEditor.svelte';
  import ShowPane from './ShowPane.svelte';
  import ShowList from './ShowList.svelte';
  import ClashBox from './ClashBox.svelte';
  import CardEditor from './CardEditor.svelte';
  import * as api from '../lib/schedule.ts';
  import { backToShows, closeOpen, keyView, loadWeek, openDay, openShow, sched, select, setView, showInWeek, step, today, weekBlocks } from '../lib/schedule.svelte.ts';
  import { stationNow, studio } from '../lib/studio.svelte.ts';
  import { load as loadRetention } from '../lib/retention.ts';
  import { dayLength, dayRangeWords, hm, longDayWords, rangeWords, whenWords, zonedString } from './model.ts';
  import type { Block } from './model.ts';
  import { clashesOf, dayIndex, shownBlock } from './clash.ts';
  import { namesOf } from './blockHtml.ts';
  import { changeRule, putBackIn, skipIn } from './dates.ts';
  import { stationRules } from './datesStore.ts';
  import type { IsoDate, ShowSaved } from '../lib/api.ts';
  import type { EpisodePatch, EpisodeView, Show, WeekEpisode } from '../lib/schedule.ts';
  import { movedSaid, wayBack } from './move.ts';
  import NewShow from './rule/NewShow.svelte';
  import RuleDialog from './rule/RuleDialog.svelte';
  import { closeRuleDialog, openRuleDialog, ruleDialog } from './rule/dialog.svelte.ts';
  import { dateWords, ruleOfSlot } from './rule/form.ts';
  import { dateIn } from './rule/view.ts';
  import { deletedWords } from './showDelete.ts';
  import { Heard, isAbout, takeOwn } from '../lib/ruleNotice.ts';

  import { account } from '../lib/account.svelte.ts';
  import { isAdmin, mayImportCalendar } from '../lib/account.ts';
  import Subscribe from './files/Subscribe.svelte';
  import ImportCalendar from './files/ImportCalendar.svelte';

  let { live = true, dialog: firstDialog = null }: { live?: boolean; /** The dialog open (a test's). */ dialog?: 'new-show' | 'subscribe' | 'import' | null } = $props();

  /** The station has calendar files (1.0.5): it sends its public address, or null, with the week. */
  const calendarFiles = $derived(sched.publicUrl !== undefined);
  /** Import a calendar… is an admin's or a producer's; the station refuses anyone else. */
  const canImport = $derived(calendarFiles && account.loaded && mayImportCalendar(account.me));

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
  /** The Day view's day, 0 (Monday) to 6: the day `anchor` is in. */
  const dayIdx = $derived(Math.max(0, dayIndex(sched.anchor, week.days)));
  const inDay = $derived(sched.view === 'day');
  // The editor column shows the selected block only while the grid draws it. The lookup used to
  // run over the whole week in the Day view too, so a block pressed on Monday (an automation
  // block, say) still held the column after Wednesday was opened, with nothing selected in the
  // day on the screen (the operator's 1.0.5 run). Now the column rests on the list of shows
  // there, as it does in a week that does not hold the selection; the selection itself is kept.
  const selectedBlock = $derived(shownBlock(week.blocks, week.days, sched.selected, inDay ? dayIdx : null));
  const selectedEpisode = $derived(
    selectedBlock?.kind === 'episode' ? sched.episodes.find((e) => `ep:${e.episode.id}` === selectedBlock.id) ?? null : null,
  );
  const rulesOn = $derived(sched.rules === true);
  const todayIso = $derived(dateIn(now, sched.zone));
  const editShow = $derived(sched.shows.find((s) => s.slug === sched.editShow) ?? null);
  const ruleShow = $derived(selectedBlock?.kind === 'rule' ? sched.shows.find((s) => s.slug === selectedBlock.slug) ?? null : null);
  const NOON = 12 * 3_600_000;

  /** Open a day of the week shown, with whatever is selected still selected. */
  const openDayOf = (i: number) => openDay(week.days[Math.min(6, Math.max(0, i))] + NOON);
  /** Open the day a block is on. */
  const openDayAt = (ms: number) => openDay(ms);

  onMount(() => {
    // The week is the default every time the tab opens; the Day view is not remembered.
    sched.view = 'week';
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
      const was = { air_at: w.episode.air_at === null ? null : zonedString(w.episode.air_at * 1000, sched.zone), minutes: w.episode.minutes };
      await api.patchEpisode(w.show.slug, w.series.number, w.episode.number, {
        air_at: zonedString(start, sched.zone),
        minutes,
      });
      await changed();
      // A drop that makes a clash is allowed (the station refuses a presenter's), and said.
      const now = weekBlocks(stationNow()).blocks;
      const moved = now.find((b) => b.id === id);
      const over = moved ? clashesOf(moved, now).map((o) => o.block) : [];
      if (moved && over.length) {
        tell('Moved.', `It now clashes with ${namesOf(over)} on ${longDayWords(moved.start, sched.zone).split(' ')[0]}.`, async () => {
          if (was.air_at !== null) await api.patchEpisode(w.show.slug, w.series.number, w.episode.number, { air_at: was.air_at, minutes: was.minutes });
          await changed();
        });
      }
      return null;
    } catch (e) {
      const why = e instanceof Error ? e.message : String(e);
      tell('Not moved', why);
      return why;
    }
  }

  /** The week follows an episode whose time was changed from its own editor. The episode as the
   *  station now holds it goes into the week in hand at once, and the week (or the Day view)
   *  moves to its new date while it is still the selection: the editor column shows a block
   *  only while the grid draws it, so a move to another week, or to another day in the Day
   *  view, would otherwise close the editor and lose whatever else was typed in it. */
  function follow(view: EpisodeView) {
    const ep = view.episode;
    const id = `ep:${ep.id}`;
    sched.episodes = sched.episodes.map((x) => (x.episode.id === ep.id ? { ...x, episode: ep, resolved: view.resolved } : x));
    if (sched.selected === id && ep.air_at !== null) showInWeek(id, ep.air_at * 1000);
  }

  /** An episode's Move… and Back to the rule (`move.ts`): the time, and the length when it
   *  changed, saved at once; this line says where the episode is now, and any clash it makes,
   *  with Undo. A refusal (the station's, of a presenter's move onto another show) is the
   *  answer, for Move… to say beside what was typed; Back to the rule has no row, so it is
   *  said here. Which episode, and what it held, are taken now: by the time Undo is pressed
   *  another one may be selected. */
  async function moveEpisode(w: WeekEpisode, to: EpisodePatch, back: boolean): Promise<string | null> {
    const [slug, series, number, id] = [w.show.slug, w.series.number, w.episode.number, `ep:${w.episode.id}`];
    const was = wayBack(w.episode, to, sched.zone);
    try {
      follow(await api.patchEpisode(slug, series, number, to));
      await changed();
    } catch (e) {
      const why = e instanceof Error ? e.message : String(e);
      if (back) tell('Not moved back', why);
      return why;
    }
    const now = weekBlocks(stationNow()).blocks;
    const moved = now.find((b) => b.id === id);
    const over = moved ? clashesOf(moved, now).map((o) => o.block) : [];
    tell('Moved.', moved ? movedSaid(number, moved.start, moved.end, sched.zone, { back, clashes: namesOf(over) }) : '', async () => {
      follow(await api.patchEpisode(slug, series, number, was));
      await changed();
    });
    return null;
  }

  // ---- the guided dialogs ----
  // svelte-ignore state_referenced_locally
  let newShow = $state<{ at: { date: IsoDate; from: string } | null } | null>(firstDialog === 'new-show' ? { at: null } : null);
  // ---- the toolbar's calendar-files menu (a narrower window's form of the two buttons) ----
  let menuEl = $state<HTMLDialogElement | null>(null);
  let menuBtn = $state<HTMLButtonElement | null>(null);
  let menuOpen = $state(false);
  const menuItems = () => [...(menuEl?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [])];
  function openMenu() {
    if (!menuEl || menuEl.open) return;
    // It opens under its button, to the right; near the window's right edge (a wrapped
    // toolbar, large text) it opens to the left instead, so it is always inside the window.
    menuEl.classList.remove('end');
    menuEl.show();
    if (menuEl.getBoundingClientRect().right > document.documentElement.getBoundingClientRect().right - 4) menuEl.classList.add('end');
    menuOpen = true;
    menuItems()[0]?.focus();
  }
  /** Close the menu; the focus goes back to its button, so a dialog opened from an entry
   *  returns the focus there when it closes. */
  function closeMenu(refocus = true) {
    if (!menuEl?.open) return;
    menuEl.close();
    menuOpen = false;
    if (refocus) menuBtn?.focus();
  }
  function menuKeys(e: KeyboardEvent) {
    const items = menuItems();
    const i = items.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === 'Escape') closeMenu();
    else if (e.key === 'ArrowDown') items[(i + 1) % items.length]?.focus();
    else if (e.key === 'ArrowUp') items[(i - 1 + items.length) % items.length]?.focus();
    else if (e.key === 'Home') items[0]?.focus();
    else if (e.key === 'End') items.at(-1)?.focus();
    else if (e.key === 'Tab') return closeMenu(false);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }
  /** A press anywhere else closes it. */
  function pressOutside(e: PointerEvent) {
    if (menuOpen && !(e.target instanceof Node && (menuEl?.contains(e.target) || menuBtn?.contains(e.target)))) closeMenu(false);
  }
  function fromMenu(which: 'import' | 'subscribe') {
    closeMenu();
    if (which === 'import') importing = true;
    else subscribing = true;
  }

  // svelte-ignore state_referenced_locally
  let subscribing = $state(firstDialog === 'subscribe');
  // svelte-ignore state_referenced_locally
  let importing = $state(firstDialog === 'import');
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
        tell('Saved.', `${s.name} has its dates; they are in the week as dotted blocks.`, async () => openShow(s.slug), 'See them');
      },
    });
  }
  /** The tab's own line: something done here, with the way back when there is one. */
  let toast = $state<{ title: string; message: string; undo: (() => Promise<void>) | null; label: string } | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | null = null;
  /** `undo` is the one act the line offers, for 12 seconds: the way back (Undo), or the way on
   *  when `label` says so (Open it, See them). */
  function tell(title: string, message: string, undo: (() => Promise<void>) | null = null, label = 'Undo') {
    if (toastTimer) clearTimeout(toastTimer);
    toast = { title, message, undo, label };
    toastTimer = setTimeout(() => (toast = null), undo ? 12_000 : 6000);
  }

  // ---- Esc, and a press on the grid where no block is ----
  // Both close what the editor column holds and clear the selection, as ‹ Shows does (`closeOpen`;
  // Esc comes through the keymap's `escape`, lib/actions.ts). Neither is obeyed over changes that
  // are not saved, and this line says why.
  $effect(() => {
    const r = sched.notClosed;
    if (!r) return;
    untrack(() => {
      tell('Not closed.', `${r.what} has changes that are not saved. Save or discard them first.`);
      // Said once: a refusal left here would be said again when the tab is next opened.
      sched.notClosed = null;
    });
  });

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
      const others = news.filter((n) => !takeOwn(n, sched.shows));
      const last = others.at(-1);
      if (!last) return;
      // A rule dialog open on that show says so before its save puts this one in its place.
      const open = ruleDialog.open;
      const about = open ? others.filter((n) => isAbout(n, open.slug, open.name)).at(-1) : undefined;
      if (about) ruleDialog.elsewhere = about.title;
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
    backToShows();
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
    try {
      if (!(await changeRule(stationRules, s.slug, b.series, (rule) => skipIn(rule, on)))) return;
      select(null);
      await changed();
      tell('Skipped.', `${s.name} will not air on ${dateWords(on, { weekday: 'long' })}.`, async () => {
        await changeRule(stationRules, s.slug, b.series, (rule) => putBackIn(rule, on));
        await changed();
      });
    } catch (e) {
      tell('Not skipped', e instanceof Error ? e.message : String(e));
    }
  }

  // ---- a show's dates and the week ----
  /** A row of a show's dates was pressed: the week (or the Day view) moves to it and its block
   *  is selected there, open if it is in a clash; the list stays. */
  function showRow(id: string | null, ms: number) {
    showInWeek(id, ms);
  }
  /** Open on a row: the episode's editor, or the dotted date's pane. */
  function openRow(id: string, ms: number) {
    showInWeek(id, ms);
    select(id);
  }
  /** See its dates, from New show's Done. */
  function seeDates(saved: ShowSaved) {
    openShow(saved.show.slug);
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

<svelte:document onpointerdown={pressOutside} />
<div class="schedule" data-surface="schedule">
  <div class="toolbar" data-panel="calendar.toolbar">
    {#if inDay}
      <button type="button" class="btn quiet" aria-label="Previous day" title="Previous day (←)" onclick={() => step(-1)}>‹</button>
      <button type="button" class="btn quiet" aria-label="Next day" title="Next day (→)" onclick={() => step(1)}>›</button>
    {:else}
      <button type="button" class="btn quiet" aria-label="Previous week" title="Previous week (←)" onclick={() => step(-1)}>‹</button>
      <button type="button" class="btn quiet" aria-label="Next week" title="Next week (→)" onclick={() => step(1)}>›</button>
    {/if}
    <button type="button" class="btn" onclick={() => today(now)}>Today <kbd>T</kbd></button>
    <span class="range num" data-panel-title>{inDay ? dayRangeWords(week.days[dayIdx], sched.zone) : rangeWords(week.days, sched.zone)}</span>
    <span class="seg" role="group" aria-label="View">
      <button type="button" aria-pressed={sched.view === 'week'} title="The week (W)" aria-keyshortcuts="W" onclick={() => setView('week')}>Week</button>
      <button type="button" aria-pressed={inDay} title="One day, with a lane for each show (D)" aria-keyshortcuts="D" onclick={() => keyView('day', now)}>Day</button>
      <button type="button" aria-pressed={sched.view === 'studios'} aria-disabled="true" title="Studios are not in the model yet" onclick={() => tell('Not in this build.', 'Studios as columns wait for a studio on each show (a choice for the operator).')}>Studios</button>
    </span>
    <span class="tz">{sched.zone}{dst ? ` · ${dst}` : ''}</span>
    {#if sched.zoneSet === false && canChooseZone}
      <button type="button" class="btn" title="Nobody has chosen the station's time zone yet" onclick={() => (studio.settings = true)}>Choose the time zone…</button>
    {/if}
    <button type="button" class="btn primary" onclick={openNewShow}>New show</button>
    {#if calendarFiles}
      <!-- Calendar files: two buttons where the window has the room, one menu of the same two where it has not. -->
      {#if canImport}<button type="button" class="btn files-wide" aria-haspopup="dialog" title="Read a calendar file (.ics) into new shows" onclick={() => (importing = true)}>Import a calendar…</button>{/if}
      <button type="button" class="btn files-wide" aria-haspopup="dialog" title="The link to the schedule’s calendar file, for listeners’ own calendars" onclick={() => (subscribing = true)}>Subscribe…</button>
      <span class="files-menu">
        <button bind:this={menuBtn} type="button" class="btn more" aria-haspopup="menu" aria-expanded={menuOpen} aria-controls="cal-files-menu" aria-label="Calendar files" title={canImport ? 'Calendar files: Import a calendar…, Subscribe…' : 'Calendar files: Subscribe…'} onclick={() => (menuOpen ? closeMenu() : openMenu())}>
          <svg aria-hidden="true" viewBox="0 0 14 14"><circle cx="7" cy="2.25" r="1.35" /><circle cx="7" cy="7" r="1.35" /><circle cx="7" cy="11.75" r="1.35" /></svg>
        </button>
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <dialog bind:this={menuEl} id="cal-files-menu" class="tmenu" aria-label="Calendar files" onkeydown={menuKeys}>
          <div role="menu" aria-label="Calendar files">
            {#if canImport}<button type="button" role="menuitem" onclick={() => fromMenu('import')}><b>Import a calendar…</b><span>Read an .ics file into new shows</span></button>{/if}
            <button type="button" role="menuitem" onclick={() => fromMenu('subscribe')}><b>Subscribe…</b><span>The schedule’s calendar link, and Download .ics</span></button>
          </div>
        </dialog>
      </span>
    {/if}
    {#if sched.loading}<span class="load" role="status">Loading…</span>{/if}
    {#if sched.error}<span class="load g-warning" role="alert">{sched.error}</span>{/if}
    <div class="legend" aria-label="Legend">
      <span><svg aria-hidden="true"><use href="#g-mic" /></svg>Live</span>
      <span><svg aria-hidden="true"><use href="#g-auto" /></svg>Automation fills the gaps</span>
      <span><svg aria-hidden="true"><use href="#g-repeat" /></svg>A date from the show’s rule</span>
      <span title="Where shows clash, one is open and the others fold beside it"><svg aria-hidden="true" style="color:var(--status-warning)"><use href="#g-fault" /></svg>Conflict</span>
      <span><i class="past"></i>Aired, frozen</span>
    </div>
  </div>

  {#if toast || heard}
    <div class="toast2" role="status" data-testid="schedule-toast">
      {#if toast}<span><b>{toast.title}</b> {toast.message}</span>{/if}
      {#if heard}<span class="heard" data-testid="schedule-heard"><svg aria-hidden="true"><use href="#g-repeat" /></svg><span><b>{heard}</b> The week is up to date.</span></span>{/if}
      {#if toast?.undo}<button type="button" class="btn sm" onclick={async () => { const undo = toast?.undo; const way = toast?.label === 'Undo'; toast = null; try { await undo?.(); } catch (e) { tell(way ? 'Not put back' : 'Not opened', e instanceof Error ? e.message : String(e)); } }}>{toast.label}</button>{/if}
    </div>
  {/if}
  <div class="body">
    <section class="cal" data-panel={inDay ? 'calendar.day' : 'calendar.week'} aria-label={inDay ? `${longDayWords(week.days[dayIdx], sched.zone)}, one day` : `Week of ${rangeWords(week.days, sched.zone)}`}>
      <div class="corner" data-panel-title><span class="sr-only">{inDay ? 'Day grid' : 'Week grid'}</span></div>
      <div class="grid-host">
        {#if live}
          <EventCalendarGrid
            blocks={week.blocks}
            days={week.days}
            zone={sched.zone}
            {now}
            selected={sched.selected}
            onSelect={(id) => select(id)}
            onClear={() => void closeOpen(now)}
            {onMove}
            onOpenDay={openDayOf}
            view={inDay ? 'day' : 'week'}
            day={dayIdx}
            reveal={sched.reveal}
          />
        {:else}
          <div class="empty">The grid is drawn by the calendar library in a window; the tests render the editors.</div>
        {/if}
      </div>
    </section>

    {#if sched.pane === 'show' && editShow}
      <aside class="editor" data-panel={sched.showTab === 'dates' ? 'calendar.show-dates' : 'calendar.episode-editor'} aria-label={sched.showTab === 'dates' ? 'The show’s dates' : 'Show editor'}>
        {#key editShow.slug}
          <ShowPane
            show={editShow}
            tab={sched.showTab}
            zone={sched.zone}
            {now}
            today={todayIso}
            blocks={week.blocks}
            selected={sched.selected}
            ontab={(t) => (sched.showTab = t)}
            onback={backToShows}
            onChanged={changed}
            onTell={tell}
            onShow={showRow}
            onOpen={openRow}
            onGiveDates={() => giveDates(editShow)}
            onSaved={async (v) => { await changed(); tell('Show saved.', `${v.show.name} is up to date.`); }}
            onDeleted={showDeleted}
            onCancel={() => { sched.pane = 'episode'; sched.editShow = null; }}
          />
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
          onShows={backToShows}
          onChanged={changed}
        />
      </aside>
    {:else if selectedBlock?.kind === 'automation'}
      <aside class="editor" data-panel="calendar.log-editor" aria-label="Log editor">
        <LogEditor block={selectedBlock} zone={sched.zone} {now} onShows={backToShows} />
      </aside>
    {:else if selectedEpisode && selectedBlock}
      <aside class="editor" data-panel={selectedEpisode.episode.aired_at !== null ? 'calendar.aired-episode' : 'calendar.episode-editor'} aria-label="Episode editor">
        <EpisodeEditor
          item={selectedEpisode}
          zone={sched.zone}
          {now}
          us={sched.us}
          block={selectedBlock}
          blocks={week.blocks}
          onChanged={changed}
          onMove={(to, back) => moveEpisode(selectedEpisode, to, back)}
          onCard={() => (sched.pane = 'card')}
          onShows={backToShows}
          onShowDates={() => openShow(selectedEpisode.show.slug)}
          onShowBlock={(id) => select(id)}
          onDay={() => openDayAt(selectedBlock.start)}
        />
      </aside>
    {:else if selectedBlock?.kind === 'rule' && ruleShow}
      <aside class="editor" data-panel="calendar.episode-editor" aria-label="A date from the show's rule">
        <div class="head">
          <div class="crumbs"><button type="button" class="back" title="Back to the list of shows (Esc)" aria-keyshortcuts="Escape" onclick={backToShows}>‹ Shows</button> › <button type="button" class="back" title="The show’s dates" onclick={() => openShow(ruleShow.slug)}>{ruleShow.name}</button> › a date from its rule</div>
          <h2 data-panel-title>{ruleShow.name} · not drafted</h2>
          <div class="when"><span class="num">{whenWords(selectedBlock.start, selectedBlock.end, sched.zone)}</span></div>
        </div>
        <div class="fields">
          <ClashBox block={selectedBlock} blocks={week.blocks} zone={sched.zone} onshow={(id) => select(id)} onday={() => openDayAt(selectedBlock.start)} />
          <div class="nothing flat">
            <p>This is where the show’s rule puts {selectedBlock.number !== null ? `episode ${selectedBlock.number}` : 'its next episode'}{#if ruleShow.rule_words?.sentence}: <b>{ruleShow.rule_words.sentence}</b>{:else}.{/if} Nothing is drafted yet, so playout fills the hour with automation until someone drafts it.</p>
            <p><b>Draft it</b> and the episode takes its number and this time from the rule; the title and guests are yours to add. <b>Skip it</b> and the show does not air that day; the rule carries on{#if ruleShow.rule?.end.kind === 'count'}, and since a skipped date does not count, its {ruleShow.rule.end.count} episodes end one date later{/if}.</p>
          </div>
        </div>
        <div class="foot">
          <div class="r">
            <button type="button" class="btn primary" onclick={() => draftFromRule(selectedBlock!)}>Draft this episode</button>
            {#if selectedBlock.on && rulesOn}<button type="button" class="btn" onclick={() => skipDate(selectedBlock!)}>Skip this date</button>{/if}
            <button type="button" class="btn" onclick={() => openShow(ruleShow.slug)}>All its dates</button>
            <button type="button" class="btn" onclick={() => (sched.pane = 'card')}>Card…</button>
          </div>
          <p class="caption">A skipped date is put back from the show’s <b>Dates</b>, where it stays listed.</p>
        </div>
      </aside>
    {:else if !sched.shows.length && !sched.loading && !sched.error}
      <aside class="editor" data-panel="calendar.episode-editor" aria-label="Episode editor">
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
      </aside>
    {:else}
      <!-- Nothing open: every show, by name. A show with nothing coming up draws nothing in the week
           (the operator's 1.0.2 run), and a show was reached only through one of its episodes (the
           1.0.5 finding), so the list is where the column rests. -->
      <aside class="editor" data-panel="calendar.shows" aria-label="Shows">
        <ShowList shows={sched.shows} rules={rulesOn} today={todayIso} onopen={(s) => openShow(s.slug)} ondates={giveDates} ondraft={draftNext} />
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
    onDates={seeDates}
    onClose={() => (newShow = null)}
  />
{/if}
{#if subscribing}
  <Subscribe publicUrl={sched.publicUrl} onClose={() => (subscribing = false)} />
{/if}
{#if importing}
  <ImportCalendar today={todayIso} onMade={changed} onOpen={(slug) => openShow(slug)} onClose={() => (importing = false)} />
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
