<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The episode editor (calendar.md §4; ADR-0055): the path, the title, the date and a chip for
  // tonight, the level switch (Episode, Series n, Show), and every field with its marker.
  // Inherited fields follow the level above by reference until Set here; Reset to series puts
  // one back (the control plane's `null`); an aired episode is frozen and read-only. The foot
  // saves, discards, drafts the next episode or the rest of the season from the weekly rule.
  import Field from './Field.svelte';
  import SeriesEditor from './SeriesEditor.svelte';
  import ShowEditor from './ShowEditor.svelte';
  import * as api from '../lib/schedule.ts';
  import type { Correction, CorrectionInput, EpisodeKind, EpisodePatch, WeekEpisode } from '../lib/schedule.ts';
  import {
    changeWords,
    fromLocalInput,
    initials,
    localInputValue,
    marker,
    nextInstance,
    slotWords,
    splitList,
    whenWords,
    dayWords,
    hm,
    zonedString,
  } from './model.ts';

  let {
    item,
    zone,
    now,
    us,
    onChanged,
    onCard,
  }: {
    item: WeekEpisode;
    zone: string;
    now: number;
    us: boolean | null;
    onChanged: () => Promise<void>;
    onCard: () => void;
  } = $props();

  type Level = 'episode' | 'series' | 'show';
  let level = $state<Level>('episode');

  const ep = $derived(item.episode);
  const r = $derived(item.resolved);
  const frozen = $derived(ep.aired_at !== null);
  const start = $derived((ep.air_at ?? 0) * 1000);
  const minutes = $derived(r.episode?.minutes ?? ep.minutes ?? r.slot?.minutes ?? 60);
  const end = $derived(start + minutes * 60_000);
  const tonight = $derived(dayWords(start, zone) === dayWords(now, zone));
  const seriesWord = $derived(`Series ${item.series.number}`);

  // The draft: what differs from the episode as the control plane holds it.
  let draft = $state<EpisodePatch>({});
  let moving = $state(false);
  let moveAt = $state('');
  let moveMinutes = $state(60);
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
  const onRule = $derived(r.slot !== null && !moved());

  function moved(): boolean {
    if (!r.slot) return false;
    const rule = nextInstance(r.slot, start - 7 * 24 * 3_600_000, zone);
    // The rule's instance in the same week as the episode.
    const sameWeek = Math.abs(rule - start) < 7 * 24 * 3_600_000;
    return !(sameWeek && rule === start);
  }

  function setHere(field: 'hosts' | 'tags') {
    draft = { ...draft, [field]: field === 'hosts' ? [...r.hosts] : [...r.tags] };
  }
  function reset(field: 'hosts' | 'tags') {
    draft = { ...draft, [field]: null };
  }

  function beginMove() {
    moveAt = localInputValue(start, zone);
    moveMinutes = minutes;
    moving = true;
  }
  function applyMove() {
    const ms = fromLocalInput(moveAt, zone);
    if (ms === null) {
      problem = 'The date and time are not readable; use the picker.';
      return;
    }
    draft = { ...draft, air_at: zonedString(ms, zone), minutes: moveMinutes };
    moving = false;
  }
  function backToRule() {
    if (!r.slot) return;
    const rule = nextInstance(r.slot, start - 7 * 24 * 3_600_000, zone);
    draft = { ...draft, air_at: zonedString(rule, zone), minutes: null };
    moving = false;
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
    moving = false;
    problem = '';
  }

  const nextAt = $derived(r.slot ? nextInstance(r.slot, Math.max(start, now), zone) : null);
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
  <div class="crumbs"><b>{r.show}</b> › <b>{seriesWord}</b> › Episode {ep.number}</div>
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
    <button type="button" role="tab" aria-selected={level === 'show'} onclick={() => (level = 'show')}>Show</button>
  </div>
</div>

{#if level === 'series'}
  <SeriesEditor slug={item.show.slug} number={item.series.number} {zone} {onChanged} />
{:else if level === 'show'}
  <ShowEditor slug={item.show.slug} show={item.show} onSaved={onChanged} {onCard} embedded />
{:else}
  <div class="fields">
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
      marker={frozen ? 'frozen' : draft.air_at !== undefined || !onRule ? 'set' : 'inherited'}
      from={seriesFrom}
      hint={r.slot ? `${slotWords(r.slot)}, ${zone}` : 'The series has no weekly rule; the time is this episode’s own.'}
    >
      {#snippet actions()}
        {#if !moving}<button type="button" class="act" onclick={beginMove}>Move…</button>{/if}
        {#if r.slot && (!onRule || draft.air_at !== undefined)}<button type="button" class="act" onclick={backToRule}>Back to the rule</button>{/if}
      {/snippet}
      {#if moving}
        <div class="row">
          <input type="datetime-local" bind:value={moveAt} aria-label="New date and time, station time" />
          <input type="number" min="1" max="1440" bind:value={moveMinutes} aria-label="Length in minutes" /> min
          <button type="button" class="btn sm" onclick={applyMove}>Set</button>
          <button type="button" class="btn sm quiet" onclick={() => (moving = false)}>Cancel</button>
        </div>
      {:else}
        <span class="num">{draft.air_at ? `${draft.air_at.slice(0, 16).replace('T', ' ')} · ${draft.minutes ?? minutes} min (unsaved)` : ep.air_at === null ? 'not set' : `${whenWords(start, end, zone)}`}</span>
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
        <button type="button" class="btn" onclick={() => draft1(1)} disabled={!r.slot}>Create next episode {#if nextAt}<small>· Ep {(lastNumber ?? ep.number) + 1}, {dayWords(nextAt, zone)} {hm(nextAt, zone)}</small>{/if}</button>
        <button type="button" class="btn" onclick={() => remaining && draft1(remaining)} disabled={!remaining}>Draft the season {#if remaining}<small>· {remaining} more, to Ep {planned}</small>{:else}<small>· {planned === null ? 'no plan on the series' : 'all drafted'}</small>{/if}</button>
      </div>
      <p class="caption">Inherited until set, frozen when aired (ADR-0055).</p>
    {/if}
  </div>
{/if}
