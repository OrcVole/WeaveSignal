<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The episode editor (calendar.md §4; ADR-0055): the path, the title, the date and a chip for
  // tonight, the level switch (Episode, Series n, Show), and every field with its marker. Show
  // on the switch leads to the show's own pane, which opens at its Dates (week-clashes.md
  // section 3); the crumbs begin with ‹ Shows, and the show's name in them leads there too.
  // With the episode in a clash, `calendar.clash` is the first thing under the head.
  // Inherited fields follow the level above by reference until Set here; Reset to series puts
  // one back (the control plane's `null`); an aired episode is frozen and read-only. The foot
  // saves, discards, drafts the next episode or the rest of the season from the rule.
  //
  // The date and time are the one field that is not drafted: Move… and Back to the rule are
  // saved as they are pressed, with Undo in the tab's line, as a show's dates are (ADR-0283;
  // `move.ts`). Whatever else is typed here stays in the draft for Save episode.
  //
  // Under the head, who can listen (ListenLine; keyed shows, 1.3.2): the rule is the show's, so
  // the line says which applies and leads to it; an aired episode says a key keeps the whole show.
  import Field from './Field.svelte';
  import SeriesEditor from './SeriesEditor.svelte';
  import ClashBox from './ClashBox.svelte';
  import ListenLine from './ListenLine.svelte';
  import TimeField from './TimeField.svelte';
  import * as api from '../lib/schedule.ts';
  import type { Correction, CorrectionInput, EpisodeKind, EpisodePatch, WeekEpisode } from '../lib/schedule.ts';
  import {
    changeWords,
    initials,
    localInputValue,
    marker,
    instant,
    movedWords,
    splitList,
    whenWords,
    dayWords,
    hm,
  } from './model.ts';
  import { moveAsk, ruleAsk } from './move.ts';
  import { addDays, shortOfSlot } from './rule/form.ts';
  import { tick } from 'svelte';
  import { noteUnsaved, sched } from '../lib/schedule.svelte.ts';
  import type { Block } from './model.ts';

  let {
    item,
    zone,
    now,
    us,
    onChanged,
    onMove,
    onCard,
    block = null,
    blocks = [],
    onShows,
    onShowDates,
    onShowBlock,
    onDay,
  }: {
    item: WeekEpisode;
    zone: string;
    now: number;
    us: boolean | null;
    onChanged: () => Promise<void>;
    /** Move… and Back to the rule: the time, and the length when it changed, saved at once.
     *  The tab says it with Undo and the week follows the episode. It answers with the reason
     *  when the station refuses (a presenter's move onto another show), else null. */
    onMove: (to: EpisodePatch, back: boolean) => Promise<string | null>;
    onCard: () => void;
    /** The episode's block and the week on screen, for what it clashes with. */
    block?: Block | null;
    blocks?: readonly Block[];
    /** ‹ Shows: back to the list of shows. */
    onShows?: () => void;
    /** The show's pane, at its Dates: Show on the level switch, the show's name, All dates. */
    onShowDates?: () => void;
    /** Show it, on a show this one clashes with. */
    onShowBlock?: (id: string) => void;
    /** Open the day. */
    onDay?: () => void;
  } = $props();

  type Level = 'episode' | 'series';
  let level = $state<Level>('episode');

  const ep = $derived(item.episode);
  const r = $derived(item.resolved);
  const frozen = $derived(ep.aired_at !== null);
  const start = $derived((ep.air_at ?? 0) * 1000);
  const minutes = $derived(r.episode?.minutes ?? ep.minutes ?? r.rule?.minutes ?? r.slot?.minutes ?? 60);
  const end = $derived(start + minutes * 60_000);
  const tonight = $derived(dayWords(start, zone) === dayWords(now, zone));
  const seriesWord = $derived(`Series ${item.series.number}`);

  // The draft: what differs from the episode as the control plane holds it.
  let draft = $state<EpisodePatch>({});
  let moving = $state(false);
  let moveDate = $state('');
  let moveTime = $state('');
  let moveMinutes = $state<number | null>(60);
  let moveErr = $state('');
  /** Which of Move…'s fields the reason is about: the date and time, or the length. */
  let moveErrOn = $state<'when' | 'length'>('when');
  /** A move is on its way to the station: a second press waits for the first. */
  let movingNow = $state(false);
  let problem = $state('');
  let saving = $state(false);

  // A new episode selected: the draft starts clean.
  let lastId = $state('');
  $effect(() => {
    if (ep.id !== lastId) {
      lastId = ep.id;
      draft = {};
      moving = false;
      problem = '';
      level = 'episode';
    }
  });

  const dirty = $derived(Object.keys(draft).length > 0);
  // Esc on the tab closes this editor, but never over a draft (lib/schedule.svelte.ts, closeOpen).
  const me = {};
  $effect(() => {
    noteUnsaved(me, dirty ? 'The episode' : null);
    return () => noteUnsaved(me, null);
  });
  const title = $derived(draft.title ?? ep.title);
  const description = $derived(draft.description ?? ep.description);
  const hostsOwn = $derived(draft.hosts !== undefined ? draft.hosts : ep.hosts);
  const hostsShown = $derived(hostsOwn ?? r.hosts);
  const tagsOwn = $derived(draft.tags !== undefined ? draft.tags : ep.tags);
  const tagsShown = $derived(tagsOwn ?? r.tags);
  const guests = $derived(draft.guests ?? r.guests);
  const kind = $derived<EpisodeKind>(draft.kind ?? ep.kind ?? 'live');

  // Live or pre-recorded (ADR-0188): the draft keeps the kind only while it differs.
  function setKind(k: EpisodeKind) {
    const { kind: _gone, ...rest } = draft;
    draft = k === (ep.kind ?? 'live') ? rest : { ...rest, kind: k };
  }
  /** The rule in force for the episode: the series' own, else the show's (ADR-0278). */
  const hasRule = $derived(!!(r.rule ?? r.slot));
  /** On its rule's date and time: the episode fills a rule date and has not been moved off it.
   *  The station says which date it fills (`occurrence`); nothing is worked out here. */
  const onRule = $derived(hasRule && ep.occurrence !== null && movedWords(item, start, zone) === null);
  /** The rule's own time on the date the episode fills, for Back to the rule; null when the
   *  station has not said which date that is (an older station, or an episode made by hand). */
  const ruleTime = $derived.by(() => {
    const o = /^(\d{4})-(\d{2})-(\d{2})$/.exec(ep.occurrence ?? '');
    const usual = r.rule?.start ?? r.slot?.start ?? null;
    if (!o || usual === null) return null;
    return instant({ y: Number(o[1]), m: Number(o[2]), d: Number(o[3]), hh: Math.floor(usual / 60), mm: usual % 60 }, zone);
  });
  // The series carries the words of the rule in force for it (its own, else its show's). The
  // week's episodes carry no words for a rule, so a show's own are taken from the list of
  // shows; without them every show said it had no dates (the 1.0.4 test run).
  const listed = $derived(sched.shows.find((s) => s.slug === item.show.slug));
  const ruleShort = $derived(item.series.rule_words?.short ?? r.rule_words?.short ?? (item.series.rule ? '' : (item.show.rule_words?.short ?? listed?.rule_words?.short)) ?? (r.slot ? shortOfSlot(r.slot) : ''));

  function setHere(field: 'hosts' | 'tags') {
    draft = { ...draft, [field]: field === 'hosts' ? [...r.hosts] : [...r.tags] };
  }
  function reset(field: 'hosts' | 'tags') {
    draft = { ...draft, [field]: null };
  }

  /** The field's own buttons, by their words: the focus goes back to one when its row closes. */
  async function focusAct(field: Element | null, words: string) {
    await tick();
    // Not when the focus has gone elsewhere meanwhile (another field, while the station answered).
    const at = document.activeElement;
    if (at && at !== document.body && !field?.contains(at)) return;
    [...(field?.querySelectorAll<HTMLElement>('.lab button.act') ?? [])].find((b) => b.textContent === words)?.focus();
  }
  async function beginMove(e: MouseEvent) {
    // Two fields, as a show's dates have: one date-and-time box was too narrow for the editor
    // column, and WebKitGTK showed its date and cut its time off (the 1.0.5 click-through).
    const field = (e.currentTarget as HTMLElement).closest('.field');
    const at = localInputValue(ep.air_at === null ? now : start, zone);
    moveDate = at.slice(0, 10);
    moveTime = at.slice(11, 16);
    moveMinutes = minutes;
    moveErr = '';
    moving = true;
    await tick();
    field?.querySelector<HTMLInputElement>('.row.move input.date')?.focus();
  }
  function closeMove(field: Element | null) {
    moving = false;
    moveErr = '';
    void focusAct(field, 'Move…');
  }
  /** ⏎ in a field of Move… moves, and Esc cancels it, as in a show's dates; the focus goes back
   *  to its button. The keys are Move…'s own: Esc does not go on to close the editor with a
   *  date half typed. On a button ⏎ presses that button. */
  function moveKeys(e: KeyboardEvent) {
    const field = (e.currentTarget as HTMLElement).closest('.field');
    if (e.key === 'Enter' && e.target instanceof HTMLInputElement) {
      e.preventDefault();
      e.stopPropagation();
      void doMove(field);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      closeMove(field);
    }
  }
  /** Move: saved at once. The row stays open on a refusal, with what was typed and the reason. */
  async function doMove(field: Element | null) {
    if (movingNow) return;
    const ask = moveAsk({ date: moveDate, time: moveTime, minutes: moveMinutes }, ep.air_at === null ? null : start, minutes, zone);
    if ('problem' in ask) {
      moveErr = ask.problem;
      moveErrOn = ask.about;
      return;
    }
    if ('same' in ask) return closeMove(field);
    movingNow = true;
    moveErr = '';
    try {
      const refused = await onMove(ask.patch, false);
      if (refused === null) closeMove(field);
      else {
        moveErr = refused;
        moveErrOn = 'when';
      }
    } finally {
      movingNow = false;
    }
  }
  /** Back to the rule: the rule's own time and length again, saved at once as Move is. */
  async function backToRule(e: MouseEvent) {
    if (ruleTime === null || movingNow) return;
    const field = (e.currentTarget as HTMLElement).closest('.field');
    movingNow = true;
    try {
      // Its button goes once the episode is on its rule: the focus goes to Move… beside it.
      if ((await onMove(ruleAsk(ruleTime, zone), true)) === null) closeMove(field);
    } finally {
      movingNow = false;
    }
  }

  async function save() {
    if (!dirty || saving) return;
    saving = true;
    problem = '';
    try {
      await api.patchEpisode(item.show.slug, item.series.number, ep.number, draft);
      draft = {};
      await onChanged();
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    } finally {
      saving = false;
    }
  }

  function discard() {
    draft = {};
    problem = '';
  }

  const planned = $derived(item.series.planned_episodes);
  /** How many the season still lacks: the plan less the last number, when a plan exists. */
  let lastNumber = $state<number | null>(null);
  $effect(() => {
    lastNumber = null;
    void api.series(item.show.slug, item.series.number).then((v) => {
      lastNumber = v.episodes.reduce((m, e) => Math.max(m, e.episode.number), 0);
    }).catch(() => {});
  });
  const remaining = $derived(planned !== null && lastNumber !== null ? Math.max(0, planned - lastNumber) : null);

  async function draft1(count: number) {
    problem = '';
    try {
      await api.createEpisodes(item.show.slug, item.series.number, { count });
      await onChanged();
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    }
  }

  async function air() {
    problem = '';
    try {
      await api.airEpisode(item.show.slug, item.series.number, ep.number);
      await onChanged();
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    }
  }

  // A correction to the public text of an aired episode (ADR-0177): the frozen record stays,
  // the cards and the feed change, and the log keeps who and why.
  let correcting = $state(false);
  let fix = $state({ title: '', description: '', hosts: '', guests: '', tags: '', reason: '' });
  let log = $state<Correction[]>([]);
  let note = $state('');

  function beginCorrect() {
    fix = {
      title: r.episode?.title ?? ep.title,
      description: r.episode?.description ?? ep.description,
      hosts: r.hosts.join(', '),
      guests: r.guests.join(', '),
      tags: r.tags.join(', '),
      reason: '',
    };
    problem = '';
    note = '';
    correcting = true;
  }

  /** Only what differs from the aired text goes in the correction. */
  const fixInput = $derived.by(() => {
    const p: Partial<CorrectionInput> = {};
    if (fix.title.trim() !== (r.episode?.title ?? '')) p.title = fix.title.trim();
    if (fix.description !== (r.episode?.description ?? '')) p.description = fix.description;
    if (splitList(fix.hosts).join('\n') !== r.hosts.join('\n')) p.hosts = splitList(fix.hosts);
    if (splitList(fix.guests).join('\n') !== r.guests.join('\n')) p.guests = splitList(fix.guests);
    if (splitList(fix.tags).join('\n') !== r.tags.join('\n')) p.tags = splitList(fix.tags);
    return p;
  });
  const canCorrect = $derived(Object.keys(fixInput).length > 0 && fix.reason.trim().length >= 3 && !saving);

  async function loadLog() {
    try {
      log = await api.corrections(item.show.slug, item.series.number, ep.number);
    } catch {
      log = [];
    }
  }
  $effect(() => {
    if (frozen && ep.id) void loadLog();
    else log = [];
  });

  async function publishCorrection() {
    if (!canCorrect) return;
    saving = true;
    problem = '';
    try {
      const done = await api.correctEpisode(item.show.slug, item.series.number, ep.number, { ...fixInput, reason: fix.reason.trim() });
      correcting = false;
      note = done.files_retagged
        ? 'Corrected. The cards were drawn again, the feed now says the new text, and the published files were tagged again.'
        : done.files_problem
          ? `Corrected, cards and feed. The published files could not be tagged again: ${done.files_problem}`
          : 'Corrected. The cards were drawn again and the feed now says the new text.';
      await loadLog();
      await onChanged();
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    } finally {
      saving = false;
    }
  }

  const mk = (field: string, inheritable = true) => marker(r, field, inheritable);
  const seriesFrom = $derived(`from ${seriesWord}`);
</script>

<div class="head">
  <div class="crumbs">
    {#if onShows}<button type="button" class="back" title="Back to the list of shows (Esc)" aria-keyshortcuts="Escape" onclick={onShows}>‹ Shows</button> ›{/if}
    {#if onShowDates}<button type="button" class="back" title="The show’s dates" onclick={onShowDates}>{r.show}</button>{:else}<b>{r.show}</b>{/if}
    › <b>{seriesWord}</b> › Episode {ep.number}
  </div>
  <h2 data-panel-title>Episode {ep.number}{title ? ` · ${title}` : ''}</h2>
  <div class="when">
    <span class="num">{ep.air_at === null ? 'no air time yet' : whenWords(start, end, zone)}</span>
    {#if frozen}
      <span class="state hollow hollow-standby sm"><svg aria-hidden="true"><use href="#g-lock" /></svg>Aired</span>
    {:else if start <= now && end > now}
      <span class="state fill-onair sm"><svg aria-hidden="true"><use href="#g-onair" /></svg>On air</span>
    {:else if tonight}
      <span class="state hollow hollow-standby sm"><svg aria-hidden="true"><use href="#g-standby" /></svg>Tonight</span>
    {/if}
  </div>
  <div class="levels" role="tablist" aria-label="Level">
    <button type="button" role="tab" aria-selected={level === 'episode'} onclick={() => (level = 'episode')}>Episode</button>
    <button type="button" role="tab" aria-selected={level === 'series'} onclick={() => (level = 'series')}>{seriesWord}</button>
    <button type="button" role="tab" aria-selected="false" title="The show: its dates and its details" onclick={() => onShowDates?.()}>Show</button>
  </div>
</div>

{#if level === 'series'}
  <SeriesEditor slug={item.show.slug} number={item.series.number} {zone} {onChanged} />
{:else}
  <div class="fields">
    {#if block && onShowBlock && onDay}<ClashBox {block} {blocks} {zone} onshow={onShowBlock} onday={onDay} />{/if}
    <!-- Who can listen is the show's (keyed shows, 1.3.2): said here, with the way to it. -->
    <ListenLine slug={item.show.slug} show={r.show} {zone} aired={frozen} />
    {#if frozen}
      <div class="frozen-note">
        <svg aria-hidden="true"><use href="#g-lock" /></svg>
        <span><b>Aired {ep.aired_at ? dayWords(ep.aired_at * 1000, zone) : ''}, frozen.</b> Its values were written in when it went on air and its cards drawn once more. Changing {seriesWord} does not rewrite it, so the archive, the feeds and the music-usage log stay true to what was broadcast.</span>
      </div>
    {/if}

    <Field label="Title" marker={frozen ? 'frozen' : title ? 'set' : 'own'} plain={!frozen}>
      {#snippet actions()}
        {#if title}<button type="button" class="act" onclick={() => (draft = { ...draft, title: '' })}>Clear</button>{/if}
      {/snippet}
      {#if frozen}{title || '—'}{:else}
        <input type="text" value={title} placeholder="The episode's title" aria-label="Title" oninput={(e) => (draft = { ...draft, title: (e.currentTarget as HTMLInputElement).value })} />
      {/if}
    </Field>

    <Field label="Number" marker={frozen ? 'frozen' : 'inherited'} from="from the series rule">
      {ep.number} <span class="hint">· the last episode plus one</span>
    </Field>

    <Field
      label="Date and time"
      marker={frozen ? 'frozen' : onRule ? 'inherited' : 'set'}
      from={seriesFrom}
      hint={`${ruleShort ? `${ruleShort}, ${zone}.` : 'The show has no dates of its own; the time is this episode’s own.'}${frozen ? '' : ' A move is saved as you make it.'}`}
    >
      {#snippet actions()}
        {#if !moving}<button type="button" class="act" onclick={beginMove}>Move…</button>{/if}
        {#if onShowDates}<button type="button" class="act" onclick={onShowDates}>All dates</button>{/if}
        {#if ruleTime !== null && !onRule}<button type="button" class="act" disabled={movingNow} onclick={backToRule}>Back to the rule</button>{/if}
      {/snippet}
      {#if moving}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="row move" onkeydown={moveKeys}>
          <input class="date" type="date" bind:value={moveDate} aria-label="New date, station time" aria-invalid={moveErr && moveErrOn === 'when' ? true : undefined} aria-describedby={moveErr && moveErrOn === 'when' ? 'ee-mv-err' : undefined} />
          <TimeField bind:value={moveTime} label="New time, station time" invalid={!!moveErr && moveErrOn === 'when'} describedby={moveErr && moveErrOn === 'when' ? 'ee-mv-err' : undefined} oncarry={(d) => (moveDate = addDays(moveDate, d))} />
          <span class="len"><input type="number" min="1" max="1440" bind:value={moveMinutes} aria-label="Length in minutes" aria-invalid={moveErr && moveErrOn === 'length' ? true : undefined} aria-describedby={moveErr && moveErrOn === 'length' ? 'ee-mv-err' : undefined} /> min</span>
          <button type="button" class="btn sm primary" disabled={movingNow} onclick={(e) => doMove((e.currentTarget as HTMLElement).closest('.field'))}>Move</button>
          <button type="button" class="btn sm quiet" onclick={(e) => closeMove((e.currentTarget as HTMLElement).closest('.field'))}>Cancel</button>
          {#if moveErr}<p id="ee-mv-err" class="err" role="alert">{moveErr}</p>{/if}
        </div>
      {:else}
        <span class="num">{ep.air_at === null ? 'not set' : whenWords(start, end, zone)}</span>
      {/if}
    </Field>

    <Field label="Kind" marker={frozen ? 'frozen' : draft.kind !== undefined || kind === 'prerecorded' ? 'set' : 'own'} hint="Live goes out as it happens; pre-recorded plays from a recording. Studios wait for a station that has several (ADR-0188).">
      {#if frozen}
        {kind === 'prerecorded' ? 'Pre-recorded' : 'Live'}
      {:else}
        <span class="seg" role="group" aria-label="Kind">
          <button type="button" aria-pressed={kind === 'live'} onclick={() => setKind('live')}>Live</button>
          <button type="button" aria-pressed={kind === 'prerecorded'} onclick={() => setKind('prerecorded')}>Pre-recorded</button>
        </span>
      {/if}
    </Field>

    <Field label="Hosts" marker={frozen ? 'frozen' : hostsOwn === null ? 'inherited' : 'set'} from={r.inherited.includes('hosts') && item.series.hosts === null ? 'from the show' : seriesFrom} multi hint={hostsOwn === null ? `Change the ${item.series.hosts === null ? "show's" : "series'"} hosts and this episode follows, until someone sets it here.` : ''}>
      {#snippet actions()}
        {#if hostsOwn === null}<button type="button" class="act" onclick={() => setHere('hosts')}>Set here</button>
        {:else}<button type="button" class="act" onclick={() => reset('hosts')}>Reset to series</button>{/if}
      {/snippet}
      {#if hostsOwn !== null && !frozen}
        <input type="text" value={hostsShown.join(', ')} aria-label="Hosts, separated by commas" onchange={(e) => (draft = { ...draft, hosts: splitList((e.currentTarget as HTMLInputElement).value) })} />
      {:else}
        {#each hostsShown as h (h)}<span class="chip"><span class="ini">{initials(h)}</span>{h}</span>{:else}<span class="hint">no hosts</span>{/each}
      {/if}
    </Field>

    <Field label="Guests" marker={frozen ? 'frozen' : 'set'} multi hint="Guests are always per episode.">
      {#snippet actions()}
        <button type="button" class="act" onclick={() => { const v = prompt('Guests, separated by commas', guests.join(', ')); if (v !== null) draft = { ...draft, guests: splitList(v) }; }}>Add</button>
      {/snippet}
      {#each guests as g (g)}<span class="chip"><span class="ini">{initials(g)}</span>{g}</span>{:else}<span class="hint">no guests yet</span>{/each}
    </Field>

    <Field label="Description" marker={frozen ? 'frozen' : description ? 'set' : 'own'} plain={!frozen}>
      {#snippet actions()}
        {#if description}<button type="button" class="act" onclick={() => (draft = { ...draft, description: '' })}>Clear</button>{/if}
      {/snippet}
      {#if frozen}{description || '—'}{:else}
        <textarea value={description} aria-label="Description" oninput={(e) => (draft = { ...draft, description: (e.currentTarget as HTMLTextAreaElement).value })}></textarea>
      {/if}
    </Field>

    <Field label="Tags" marker={frozen ? 'frozen' : tagsOwn === null ? 'inherited' : 'set'} from={item.series.tags === null ? 'from the show' : seriesFrom} multi>
      {#snippet actions()}
        {#if tagsOwn === null}<button type="button" class="act" onclick={() => setHere('tags')}>Set here</button>
        {:else}<button type="button" class="act" onclick={() => reset('tags')}>Reset to series</button>{/if}
      {/snippet}
      {#if tagsOwn !== null && !frozen}
        <input type="text" value={tagsShown.join(', ')} aria-label="Tags, separated by commas" onchange={(e) => (draft = { ...draft, tags: splitList((e.currentTarget as HTMLInputElement).value) })} />
      {:else}
        {#each tagsShown as t (t)}<span class="chip">{t}</span>{:else}<span class="hint">no tags</span>{/each}
      {/if}
    </Field>

    <Field label="Artwork and card layout" marker={frozen ? 'frozen' : 'inherited'} from="from the show">
      {#snippet actions()}
        <button type="button" class="act" onclick={onCard}>Card…</button>
      {/snippet}
      {r.layout === 'slate' ? 'Slate' : 'Poster'} layout · show colour, bank {r.colour_slot}
    </Field>

    <Field label="Up next on the player" marker="setting" hint={us === null ? 'Set by the Jurisdiction (Settings).' : us ? '<b>Off: the station is a US statutory webcaster</b>, where announcing upcoming titles is forbidden (17 U.S.C. 114).' : '<b>Off by default for stations under the US statutory licence</b>; this station is not one.'}>
      <span class="seg"><span aria-current={us === false ? 'true' : undefined}>On</span><span aria-current={us !== false ? 'true' : undefined}>Off</span></span>
    </Field>

    {#if frozen}
      <Field label="Music used" marker="frozen" from="from the per-play log" hint="The per-play log gives the hour's tracks through the API (/api/v1/reports/plays); the app has no screen for it, and the episode's own list is not built yet.">
        <span class="num">{hm(start, zone)}–{hm(end, zone)} on the per-play log</span>
      </Field>
    {/if}

    {#if frozen}
      <Field label="Corrections" marker="own" multi hint="The frozen record stays as broadcast. A correction changes the public text, the cards and the feed (same episode id), and the log below keeps who made it and why; an erasure also takes a person's name out of the log (ADR-0177).">
        {#snippet actions()}
          {#if !correcting}<button type="button" class="act" onclick={beginCorrect}>Correct the public text…</button>{/if}
        {/snippet}
        {#if correcting}
          <div class="correct">
            <label>Title <input type="text" bind:value={fix.title} /></label>
            <label>Description <textarea bind:value={fix.description}></textarea></label>
            <label>Hosts <input type="text" bind:value={fix.hosts} aria-label="Hosts, separated by commas" /></label>
            <label>Guests <input type="text" bind:value={fix.guests} aria-label="Guests, separated by commas" /></label>
            <label>Tags <input type="text" bind:value={fix.tags} aria-label="Tags, separated by commas" /></label>
            <label>Why <input type="text" bind:value={fix.reason} placeholder="Misspelt the guest's surname" aria-label="Reason for the correction" /></label>
            <div class="row">
              <button type="button" class="btn primary sm" disabled={!canCorrect} onclick={publishCorrection}>Publish correction</button>
              <button type="button" class="btn quiet sm" onclick={() => (correcting = false)}>Cancel</button>
            </div>
          </div>
        {:else if note}
          <span class="hint">{note}</span>
        {/if}
        {#each log as c (c.id)}
          <div class="correction" data-correction={c.id}>
            <span class="num">{dayWords(c.made_at * 1000, zone)} {hm(c.made_at * 1000, zone)}</span> · <b>{c.made_by}</b> · {c.reason}
            {#each changeWords(c) as w (w)}<span class="hint">{w}</span>{/each}
            {#if c.redacted}<span class="hint">a name was erased from this line</span>{/if}
          </div>
        {:else}
          {#if !correcting}<span class="hint">no corrections</span>{/if}
        {/each}
      </Field>
    {/if}

    {#if problem}
      <div class="problem" role="alert"><svg aria-hidden="true"><use href="#g-fault" /></svg><span>{problem}</span></div>
    {/if}
  </div>

  <div class="foot">
    {#if frozen}
      <div class="r"><button type="button" class="btn" aria-disabled="true" title="Episode recordings are not built yet">Open the recording</button><button type="button" class="btn quiet" aria-disabled="true">Publish as a podcast episode</button></div>
      <p class="caption">A correction edits the episode page and the feed, not the aired record, and is logged.</p>
    {:else}
      <div class="r">
        <button type="button" class="btn primary" disabled={!dirty || saving} onclick={save}>Save episode</button>
        <button type="button" class="btn quiet" disabled={!dirty} onclick={discard}>Discard changes</button>
        <button type="button" class="btn quiet" onclick={air} title="Freeze the episode as broadcast and draw its cards once more">On air now</button>
      </div>
      <div class="r">
        <button type="button" class="btn" onclick={() => draft1(1)} disabled={!hasRule}>Create next episode {#if hasRule}<small>· Ep {(lastNumber ?? ep.number) + 1}, the rule’s next date</small>{/if}</button>
        <button type="button" class="btn" onclick={() => remaining && draft1(remaining)} disabled={!remaining}>Draft the season {#if remaining}<small>· {remaining} more, to Ep {planned}</small>{:else}<small>· {planned === null ? 'no plan on the series' : 'all drafted'}</small>{/if}</button>
      </div>
      <p class="caption">Inherited until set, frozen when aired (ADR-0055).</p>
    {/if}
  </div>
{/if}
