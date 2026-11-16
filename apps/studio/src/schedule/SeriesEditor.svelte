<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The series level (calendar.md §4, the sheet's `calendar.series-editor`): when it airs, the
  // hosts, the planned count, the tags, each inherited from the show until set, and Draft the
  // season. Saved with PUT, which the control plane answers by drawing the series' cards. When
  // it airs is the show's rule until the series sets its own (ADR-0278): Set here and Change…
  // open the rule dialog, and Reset to show opens its Before you save; the rule is saved
  // there, on its own route, and every save here sends it back as stored.
  import { onMount } from 'svelte';
  import Field from './Field.svelte';
  import * as api from '../lib/schedule.ts';
  import type { SeriesInput, SeriesView } from '../lib/schedule.ts';
  import { initials, splitList } from './model.ts';
  import { noteUnsaved, sched } from '../lib/schedule.svelte.ts';
  import { openRuleDialog } from './rule/dialog.svelte.ts';
  import { dateWords, ruleOfSlot, shortOfSlot } from './rule/form.ts';

  let { slug, number, zone, onChanged }: { slug: string; number: number; zone: string; onChanged: () => Promise<void> } = $props();

  let view = $state<SeriesView | null>(null);
  let draft = $state<SeriesInput>({});
  let problem = $state('');
  let saving = $state(false);
  const rulesOn = $derived(sched.rules === true);

  async function load() {
    try {
      view = await api.series(slug, number);
      draft = {};
      problem = '';
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    }
  }
  onMount(load);

  const s = $derived(view?.series ?? null);
  const r = $derived(view?.resolved ?? null);
  const dirty = $derived(Object.keys(draft).length > 0);
  // Esc on the tab closes the editor column, but never over a draft (closeOpen).
  const me = {};
  $effect(() => {
    noteUnsaved(me, dirty ? 'The series' : null);
    return () => noteUnsaved(me, null);
  });
  const hosts = $derived(draft.hosts !== undefined ? draft.hosts : (s?.hosts ?? null));
  const tags = $derived(draft.tags !== undefined ? draft.tags : (s?.tags ?? null));
  /** The series' own rule, or null while it follows the show's. */
  const own = $derived(rulesOn ? (s?.rule ?? null) : s?.slot ? ruleOfSlot(s.slot) : null);
  /** The rule in force: the series' own, else the show's, in the station's words. */
  const inForce = $derived(r?.rule ?? (r?.slot ? ruleOfSlot(r.slot) : null));
  const short = $derived(s?.rule_words?.short ?? r?.rule_words?.short ?? (r?.slot ? shortOfSlot(r.slot) : ''));
  const showColour = $derived(r?.colour_slot ?? null);
  const title = $derived(draft.title !== undefined ? draft.title : (s?.title ?? null));
  const description = $derived(draft.description !== undefined ? draft.description : (s?.description ?? null));
  const startsOn = $derived(draft.starts_on !== undefined ? draft.starts_on : (s?.starts_on ?? null));
  const planned = $derived(draft.planned_episodes !== undefined ? draft.planned_episodes : (s?.planned_episodes ?? null));
  const aired = $derived(view?.episodes.filter((e) => e.episode.aired_at !== null).map((e) => e.episode.number) ?? []);
  const drafted = $derived(view?.episodes.filter((e) => e.episode.aired_at === null).map((e) => e.episode.number) ?? []);
  const last = $derived(view?.episodes.reduce((m, e) => Math.max(m, e.episode.number), 0) ?? 0);
  const remaining = $derived(planned !== null ? Math.max(0, planned - last) : null);

  /** The input as the control plane takes it: every field, with the draft over the saved. */
  function input(): SeriesInput {
    return {
      title,
      description,
      hosts,
      // When it airs goes back as stored: it is changed in the rule dialog, not here.
      slot: s?.slot ?? null,
      ...(rulesOn ? { rule: s?.rule ?? null } : {}),
      starts_on: startsOn,
      planned_episodes: planned,
      tags,
    };
  }

  async function save() {
    if (saving) return;
    saving = true;
    problem = '';
    try {
      await api.putSeries(slug, number, input());
      await load();
      await onChanged();
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    } finally {
      saving = false;
    }
  }

  async function draftMore(count: number) {
    problem = '';
    try {
      await api.createEpisodes(slug, number, { count });
      await load();
      await onChanged();
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    }
  }

  /** Set here or Change…, or Reset to show: the rule dialog, which the Schedule tab draws. */
  function changeRule(reset: boolean) {
    openRuleDialog({ slug, name: r?.show ?? slug, series: number, rule: own, follow: inForce, reset, colour: showColour, onSaved: ruleSaved });
  }

  async function ruleSaved() {
    await load();
    await onChanged();
  }
</script>

<div class="fields" data-panel="calendar.series-editor">
  {#if !view}
    <p class="hint" data-panel-title>{problem || 'Loading the series…'}</p>
  {:else}
    <h3 class="sr-only" data-panel-title>Series {number}</h3>
    <Field label="Series title" marker={title ? 'set' : 'own'} plain>
      {#snippet actions()}
        {#if title}<button type="button" class="act" onclick={() => (draft = { ...draft, title: null })}>Clear</button>{/if}
      {/snippet}
      <input type="text" value={title ?? ''} placeholder="autumn 2026" aria-label="Series title" onchange={(e) => (draft = { ...draft, title: (e.currentTarget as HTMLInputElement).value.trim() || null })} />
    </Field>

    <Field label="When it airs" marker={own ? 'set' : 'inherited'} from="from the show" hint="The station works out its dates; a move is per episode. Drafting fills each air time from it, across the clock change.">
      {#snippet actions()}
        <button type="button" class="act" onclick={() => changeRule(false)}>{own ? 'Change…' : 'Set here'}</button>
        {#if own}<button type="button" class="act" onclick={() => changeRule(true)}>Reset to show</button>{/if}
      {/snippet}
      <span class="num">{short || 'No dates yet'}{startsOn ? `, from ${dateWords(startsOn)}` : ''}</span>
      {#if planned}<span class="hint">· {planned} episodes planned</span>{/if}
    </Field>

    <Field label="Starts on" marker={startsOn ? 'set' : 'own'} plain hint="The first date the rule gives on or after this is episode 1.">
      <input type="date" value={startsOn ?? ''} aria-label="Starts on" onchange={(e) => (draft = { ...draft, starts_on: (e.currentTarget as HTMLInputElement).value || null })} />
    </Field>

    <Field label="Planned episodes" marker={planned ? 'set' : 'own'} plain>
      <input type="number" min="1" max="520" value={planned ?? ''} aria-label="Planned episodes" onchange={(e) => { const v = (e.currentTarget as HTMLInputElement).value; draft = { ...draft, planned_episodes: v ? Number(v) : null }; }} />
    </Field>

    <Field label="Hosts" marker={hosts ? 'set' : 'inherited'} from="from the show" multi hint={hosts ? `Episodes that still inherit follow this; aired episodes keep what they had.` : ''}>
      {#snippet actions()}
        {#if hosts}<button type="button" class="act" onclick={() => (draft = { ...draft, hosts: null })}>Reset to show</button>
        {:else}<button type="button" class="act" onclick={() => (draft = { ...draft, hosts: [...(r?.hosts ?? [])] })}>Set here</button>{/if}
      {/snippet}
      {#if hosts}
        <input type="text" value={hosts.join(', ')} aria-label="Hosts, separated by commas" onchange={(e) => (draft = { ...draft, hosts: splitList((e.currentTarget as HTMLInputElement).value) })} />
      {:else}
        {#each r?.hosts ?? [] as h (h)}<span class="chip"><span class="ini">{initials(h)}</span>{h}</span>{:else}<span class="hint">no hosts</span>{/each}
      {/if}
    </Field>

    <Field label="Description" marker={description !== null ? 'set' : 'inherited'} from="from the show" plain={description !== null}>
      {#snippet actions()}
        {#if description !== null}<button type="button" class="act" onclick={() => (draft = { ...draft, description: null })}>Reset to show</button>
        {:else}<button type="button" class="act" onclick={() => (draft = { ...draft, description: r?.description ?? '' })}>Set here</button>{/if}
      {/snippet}
      {#if description !== null}
        <textarea value={description} aria-label="Description" onchange={(e) => (draft = { ...draft, description: (e.currentTarget as HTMLTextAreaElement).value })}></textarea>
      {:else}{r?.description || '—'}{/if}
    </Field>

    <Field label="Tags" marker={tags ? 'set' : 'inherited'} from="from the show" multi>
      {#snippet actions()}
        {#if tags}<button type="button" class="act" onclick={() => (draft = { ...draft, tags: null })}>Reset to show</button>
        {:else}<button type="button" class="act" onclick={() => (draft = { ...draft, tags: [...(r?.tags ?? [])] })}>Set here</button>{/if}
      {/snippet}
      {#if tags}
        <input type="text" value={tags.join(', ')} aria-label="Tags, separated by commas" onchange={(e) => (draft = { ...draft, tags: splitList((e.currentTarget as HTMLInputElement).value) })} />
      {:else}
        {#each r?.tags ?? [] as t (t)}<span class="chip">{t}</span>{:else}<span class="hint">no tags</span>{/each}
      {/if}
    </Field>

    <Field label="Episodes" marker="own" multi>
      {#if aired.length}<span class="chip"><svg width="10" height="10" aria-hidden="true"><use href="#g-lock" /></svg>{aired.length === 1 ? aired[0] : `${aired[0]}–${aired.at(-1)}`} aired</span>{/if}
      {#if drafted.length}<span class="chip">{drafted.join(', ')} drafted</span>{/if}
      {#if remaining}<span class="chip">{Array.from({ length: Math.min(remaining, 3) }, (_, i) => last + 1 + i).join(', ')}{remaining > 3 ? '…' : ''} not yet drafted</span>{/if}
      {#if !view.episodes.length && !remaining}<span class="hint">none yet</span>{/if}
    </Field>

    {#if problem}<div class="problem" role="alert"><svg aria-hidden="true"><use href="#g-fault" /></svg><span>{problem}</span></div>{/if}
  {/if}
</div>
<div class="foot">
  <div class="r">
    <button type="button" class="btn primary" disabled={!dirty || saving} onclick={save}>Save series</button>
    <button type="button" class="btn quiet" disabled={!dirty} onclick={() => (draft = {})}>Discard changes</button>
  </div>
  <div class="r">
    <button type="button" class="btn" disabled={!remaining} onclick={() => remaining && draftMore(remaining)}>Draft the season {#if remaining}<small>· episodes {last + 1}{remaining > 1 ? ` to ${last + remaining}` : ''}</small>{/if}</button>
    <button type="button" class="btn" disabled={!inForce} onclick={() => draftMore(1)}>Create next episode</button>
  </div>
  <p class="caption">Each drafted episode inherits everything above and waits for a title and guests. Drafting never touches an aired episode.</p>
</div>

