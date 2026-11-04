<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The card editor (show-cards.md; ADR-0053, ADR-0054, ADR-0067): Poster or Slate, the live
  // preview drawn by the control plane from the same fields a save would use, the alt text in
  // its fixed order, the page the card links to (ADR-0056), and the stress warnings: a name the
  // renderer refuses comes back as the 422 with the field named, and is shown as such. The
  // layout is the show's (ADR-0067): choosing one here saves the show.
  import { onMount, untrack } from 'svelte';
  import * as api from '../lib/schedule.ts';
  import type { CardFields, CardWords, Show, ShowView, WeekEpisode } from '../lib/schedule.ts';
  import { cardFields } from './model.ts';

  let {
    slug,
    level,
    item,
    zone,
    onBack,
    onChanged,
  }: {
    slug: string;
    level: 'show' | 'series' | 'episode';
    item: WeekEpisode | null;
    zone: string;
    onBack: () => void;
    onChanged: () => Promise<void>;
  } = $props();

  let view = $state<ShowView | null>(null);
  let layout = $state<'poster' | 'slate'>(untrack(() => item?.resolved.layout) === 'slate' ? 'slate' : 'poster');
  let output = $state<'link' | 'post' | 'square'>('link');
  let image = $state('');
  let words = $state<CardWords | null>(null);
  let refusal = $state('');
  let drawing = $state(false);
  let saving = $state(false);
  let seq = 0;

  onMount(() => {
    void api
      .show(slug)
      .then((v) => {
        view = v;
        if (!item) layout = v.show.layout === 'slate' ? 'slate' : 'poster';
      })
      .catch((e) => (refusal = e instanceof Error ? e.message : String(e)));
  });

  const fields = $derived.by((): CardFields | null => {
    const r = item?.resolved ?? view?.resolved;
    if (!r) return null;
    return { ...cardFields(r, item ? level : 'show', zone), layout };
  });

  // Draw again whenever the fields change, the last request winning.
  $effect(() => {
    const f = fields;
    const o = output;
    if (!f) return;
    const mine = ++seq;
    drawing = true;
    refusal = '';
    void (async () => {
      try {
        const [w, img] = await Promise.all([api.cardWords(f), api.cardPreview(f, o)]);
        if (mine !== seq) return;
        words = w;
        image = img;
      } catch (e) {
        if (mine !== seq) return;
        refusal = e instanceof Error ? e.message : String(e);
        image = '';
      } finally {
        if (mine === seq) drawing = false;
      }
    })();
  });

  const show = $derived<Show | null>(view?.show ?? item?.show ?? null);
  const layoutSaved = $derived(show?.layout === layout);

  async function keepLayout() {
    const s = view;
    if (!s || saving) return;
    saving = true;
    try {
      await api.putShow(slug, {
        name: s.show.name,
        description: s.show.description,
        colour_slot: s.show.colour_slot,
        layout,
        slot: s.show.slot,
        hosts: s.resolved.hosts,
        tags: s.show.tags,
        explicit: s.show.explicit,
      });
      view = await api.show(slug);
      await onChanged();
    } catch (e) {
      refusal = e instanceof Error ? e.message : String(e);
    } finally {
      saving = false;
    }
  }

  const size = $derived(output === 'link' ? '1,200 × 630' : output === 'post' ? '1,600 × 900' : '3,000 × 3,000');
  /** The field a refusal names, from the control plane's words. */
  const refusedField = $derived.by(() => {
    const m = /the (show name|episode title|series title|host|guest|address|person's name|description|tag)/.exec(refusal);
    return m ? m[1] : null;
  });
</script>

<div class="head">
  <div class="crumbs">
    {#if item}<b>{item.resolved.show}</b> › <b>Series {item.series.number}</b> › Episode {item.episode.number} › card{:else}<b>{show?.name ?? slug}</b> › card{/if}
  </div>
  <h2 data-panel-title>Card · {layout === 'slate' ? 'Slate' : 'Poster'}</h2>
  <div class="when"><span class="num">{size} · {output === 'link' ? 'link preview' : output === 'post' ? 'post image' : 'podcast square'}</span>{#if drawing}<span class="hint">drawing…</span>{/if}</div>
</div>
<div class="fields">
  <div class="field set">
    <div class="lab">Layout <span class="src"><svg aria-hidden="true"><use href="#g-pin" /></svg>{layoutSaved ? 'the show’s' : 'trying'}</span>
      {#if !layoutSaved}<button type="button" class="act" onclick={keepLayout} disabled={saving}>Keep for the show</button>{/if}
    </div>
    <div class="val">
      <span class="seg" role="group" aria-label="Layout">
        <button type="button" aria-pressed={layout === 'poster'} onclick={() => (layout = 'poster')}>Poster</button>
        <button type="button" aria-pressed={layout === 'slate'} onclick={() => (layout = 'slate')}>Slate</button>
      </span>
    </div>
    <div class="hint">Poster: the dark plane, the type carries it. Slate: the light surface, the people show. The square is always Slate stacked.</div>
  </div>

  <div class="field">
    <div class="lab">Output</div>
    <div class="val">
      <span class="seg" role="group" aria-label="Output">
        <button type="button" aria-pressed={output === 'link'} onclick={() => (output = 'link')}>Link</button>
        <button type="button" aria-pressed={output === 'post'} onclick={() => (output = 'post')}>Post</button>
        <button type="button" aria-pressed={output === 'square'} onclick={() => (output = 'square')}>Square</button>
      </span>
    </div>
  </div>

  <div class="card-preview" aria-live="polite">
    {#if refusal}
      <div class="problem serious" role="alert">
        <svg aria-hidden="true"><use href="#g-fault" /></svg>
        <span><b>Refused{refusedField ? ` · ${refusedField}` : ''}.</b> {refusal} The save would be refused the same way; shorten the field named.</span>
      </div>
    {:else if image}
      <img src={image} alt={words?.alt ?? 'The card'} style={output === 'square' ? 'aspect-ratio: 1' : output === 'post' ? 'aspect-ratio: 16 / 9' : ''} />
    {:else}
      <div class="problem"><svg aria-hidden="true"><use href="#g-standby" /></svg><span>Drawing the card from its fields…</span></div>
    {/if}
    {#if words}
      <p class="alt"><b>Alt text</b>{words.alt}</p>
      <p class="path">links to {words.station.address}{words.path}</p>
    {/if}
  </div>

  {#if fields}
    <div class="field inh">
      <div class="lab">Fields on the card <span class="src"><svg aria-hidden="true"><use href="#g-up" /></svg>from the {item ? 'episode and above' : 'show'}</span></div>
      <div class="val multi">
        <span class="chip">bank {fields.colour_slot}</span>
        <span class="chip">{fields.hosts.length} host{fields.hosts.length === 1 ? '' : 's'}</span>
        {#if fields.episode}<span class="chip">{fields.guests.length} guest{fields.guests.length === 1 ? '' : 's'}</span>{/if}
        {#if fields.show.slot}<span class="chip">{fields.show.slot.weekday} {fields.show.slot.start}{fields.show.slot.end ? `–${fields.show.slot.end}` : ''}</span>{/if}
      </div>
      <div class="hint">Fitted, never cropped: a name shrinks to its floor, then wraps, then a list ends “and N more”. Change the words on the episode, series or show; the card follows.</div>
    </div>
  {/if}
</div>
<div class="foot">
  <div class="r"><button type="button" class="btn" onclick={onBack}>Back to the editor</button></div>
  <p class="caption">Every save draws the three images again and names each by its content, so a changed card has a new address.</p>
</div>
