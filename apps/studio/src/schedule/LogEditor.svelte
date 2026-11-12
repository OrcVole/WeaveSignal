<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The log editor (ADR-0045): the generated hour an automation block stands for, item by item
  // with the rule each one breaks in the scheduler's words, and the edits the control plane
  // takes: another track for an item (checked against the rules; the station's own separations
  // can be forced, the US statutory licence never), or none. An empty hour offers to generate.
  import * as api from '../lib/schedule.ts';
  import type { LogItem, Track } from '../lib/schedule.ts';
  import { hm, lengthWords, whenWords, type Block } from './model.ts';
  import { dur } from '../lib/format.ts';

  let { block, zone, now }: { block: Block; zone: string; now: number } = $props();

  let items = $state<LogItem[]>([]);
  let tracks = $state<Track[]>([]);
  let loading = $state(false);
  let problem = $state('');
  let editing = $state<number | null>(null);
  let choice = $state('');
  /** The refusal the last edit met, by item, with whether `force` would take it. */
  let refusals = $state<Record<number, { words: string; forceable: boolean }>>({});
  let gaps = $state<string[]>([]);

  async function load() {
    loading = true;
    problem = '';
    try {
      const [i, t] = await Promise.all([api.log(block.start, block.end), tracks.length ? Promise.resolve(tracks) : api.tracks()]);
      items = i;
      tracks = t;
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  let lastId = $state('');
  $effect(() => {
    if (block.id !== lastId) {
      lastId = block.id;
      editing = null;
      refusals = {};
      gaps = [];
      void load();
    }
  });

  const breaking = $derived(items.filter((i) => i.breaches.length).length);
  const legal = $derived(items.some((i) => i.breaches.some((b) => b.endsWith('(US)'))));
  const future = $derived(block.end > now);

  async function replace(item: LogItem, force = false) {
    if (!choice) return;
    try {
      await api.editItem(item.id, choice, force);
      delete refusals[item.id];
      editing = null;
      await load();
    } catch (e) {
      const words = e instanceof Error ? e.message : String(e);
      refusals[item.id] = { words, forceable: words.includes('`force`') };
    }
  }

  async function remove(item: LogItem) {
    try {
      await api.deleteItem(item.id);
      await load();
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    }
  }

  async function generate() {
    problem = '';
    try {
      const out = await api.generate(Math.max(block.start, now), block.end);
      gaps = out.gaps.map((g) => `${hm(g.at_ms, zone)} slot ${g.slot + 1}: ${g.reason}`);
      await load();
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    }
  }

  const byCategory = $derived.by(() => {
    const m = new Map<string, Track[]>();
    for (const t of tracks.filter((t) => t.enabled)) (m.get(t.category) ?? m.set(t.category, []).get(t.category)!).push(t);
    return [...m.entries()].sort(([a], [b]) => a.localeCompare(b));
  });
</script>

<div class="head">
  <div class="crumbs"><b>Automation</b> › {block.clock ?? 'no clock'}</div>
  <h2 data-panel-title>{block.show} · {lengthWords(block.end - block.start)}</h2>
  <div class="when">
    <span class="num">{whenWords(block.start, block.end, zone)}</span>
    {#if breaking}<span class="state hollow hollow-warning sm"><svg aria-hidden="true"><use href="#g-fault" /></svg>{breaking} break{breaking === 1 ? 's' : ''} a rule</span>
    {:else if items.length}<span class="state hollow hollow-good sm"><svg aria-hidden="true"><use href="#g-good" /></svg>Rules kept</span>{/if}
  </div>
</div>
<div class="fields">
  {#if problem}<div class="problem" role="alert"><svg aria-hidden="true"><use href="#g-fault" /></svg><span>{problem}</span></div>{/if}
  {#if loading && !items.length}
    <p class="hint">Reading the log…</p>
  {:else if !items.length}
    <div class="nothing">
      <p><b>Nothing generated for these hours yet.</b> {future ? 'Playout generates 26 hours ahead by itself; generate now to see the hour and check its rules.' : 'These hours have passed; the per-play log says what played (the API\'s /api/v1/reports/plays; the app has no screen for it yet).'}</p>
      {#if !tracks.length}<p>The library has no tracks: import some (Playout › library) before a clock can fill an hour.</p>{/if}
    </div>
  {:else}
    {#if legal}
      <div class="problem serious"><svg aria-hidden="true"><use href="#g-fault" /></svg><span><b>The US statutory licence is broken here</b> (17 U.S.C. 114(j)(13)): in any three hours at most 3 tracks from one album, 2 in a row, and at most 4 by one artist, 3 in a row. The scheduler never relaxes it; change the track.</span></div>
    {/if}
    <div class="log" role="list">
      {#each items as item (item.id)}
        <div class="item {item.kind} {item.status}" role="listitem">
          <span class="at num">{hm(item.planned_at_ms, zone)}</span>
          <span class="what"><b>{item.title || (item.kind === 'cart' ? 'a cart' : 'a track')}</b>{#if item.artist}<span>{item.artist}</span>{/if}</span>
          <span class="meta">{item.category || item.kind} · {dur(item.duration_ms)}{item.status !== 'planned' ? ` · ${item.status}` : ''}</span>
          {#if item.breaches.length || item.note}
            <span class="breaches">
              {#each item.breaches as b (b)}
                <span class="state hollow {b.endsWith('(US)') ? 'hollow-critical' : 'hollow-warning'}"><svg aria-hidden="true"><use href="#g-fault" /></svg>{b}</span>
              {/each}
              {#if item.note}<span class="hint">{item.note}</span>{/if}
            </span>
          {/if}
          {#if item.status === 'planned' && item.kind === 'track'}
            <span class="acts">
              {#if editing === item.id}
                <select bind:value={choice} aria-label="Another track">
                  <option value="">Choose a track…</option>
                  {#each byCategory as [cat, ts] (cat)}
                    <optgroup label={cat}>
                      {#each ts as t (t.id)}<option value={t.id}>{t.title} — {t.artist}</option>{/each}
                    </optgroup>
                  {/each}
                </select>
                <button type="button" class="btn sm" disabled={!choice} onclick={() => replace(item)}>Use it</button>
                {#if refusals[item.id]?.forceable}<button type="button" class="btn sm danger-hollow" onclick={() => replace(item, true)}>Accept the break</button>{/if}
                <button type="button" class="btn sm quiet" onclick={() => { editing = null; delete refusals[item.id]; }}>Cancel</button>
              {:else}
                <button type="button" class="btn sm" onclick={() => { editing = item.id; choice = ''; }}>Another track…</button>
                <button type="button" class="btn sm quiet" onclick={() => remove(item)}>Remove</button>
              {/if}
            </span>
            {#if refusals[item.id]}
              <div class="problem refusal {refusals[item.id].forceable ? '' : 'serious'}" role="alert"><svg aria-hidden="true"><use href="#g-fault" /></svg><span>{refusals[item.id].words}</span></div>
            {/if}
          {/if}
        </div>
      {/each}
    </div>
  {/if}
  {#if gaps.length}
    <div class="problem"><svg aria-hidden="true"><use href="#g-fault" /></svg><span><b>Slots the generator could not fill:</b> {gaps.join('; ')}</span></div>
  {/if}
</div>
<div class="foot">
  <div class="r">
    <button type="button" class="btn" disabled={!future} onclick={generate}>{items.length ? 'Generate again' : 'Generate these hours'}</button>
    <button type="button" class="btn quiet" onclick={load}>Refresh</button>
  </div>
  <p class="caption">Artist, song and album separations are the station's (Playout settings) and can be accepted; the US complement rule is never relaxed.</p>
</div>
