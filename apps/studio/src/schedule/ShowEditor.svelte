<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The show level, and New show: the name (its path follows it), the colour as a bank slot
  // (ADR-0066), the card layout (ADR-0067), the weekly rule, the hosts, the tags, the explicit
  // flag. Saved with PUT; the control plane draws the show's cards first and refuses a name
  // that does not fit them, with the field named (422), which is shown here. An existing
  // show also carries its own slate picture (ShowStill, ADR-0200), set by its own route, and
  // its presenter's fields (ADR-0214, ADR-0219): the on-air name and takedown contact the
  // consent words name while it is on air, the links to the presenter's channels, and whether
  // it plays commercial music. The account the show is tied to (ADR-0218) is the wizard's
  // and is left out here, which keeps it.
  import { untrack } from 'svelte';
  import Field from './Field.svelte';
  import ShowStill from './ShowStill.svelte';
  import * as api from '../lib/schedule.ts';
  import type { Show, ShowInput, ShowView, Slot } from '../lib/schedule.ts';
  import { slotWords, slugOf, splitList } from './model.ts';
  import { MUSIC_REMINDER } from '../lib/wizard.ts';

  let {
    slug,
    show,
    onSaved,
    onCancel,
    onCard,
    embedded = false,
  }: {
    slug: string | null;
    show: Show | null;
    onSaved: (v: ShowView) => Promise<void> | void;
    onCancel?: () => void;
    onCard: () => void;
    embedded?: boolean;
  } = $props();

  // The props seed the draft once; after that the draft is this editor's own.
  const seed = untrack(() => ({ slug, show }));
  let view = $state<ShowView | null>(null);
  let draft = $state<ShowInput>({
    name: seed.show?.name ?? '',
    description: seed.show?.description ?? '',
    colour_slot: seed.show?.colour_slot ?? 1,
    layout: seed.show?.layout ?? 'poster',
    slot: seed.show?.slot ?? null,
    hosts: [],
    tags: seed.show?.tags ?? [],
    explicit: seed.show?.explicit ?? false,
    presenter: seed.show?.presenter ?? '',
    contact: seed.show?.contact ?? '',
    links: seed.show?.links ?? [],
    commercial_music: seed.show?.commercial_music ?? false,
  });
  let newSlug = $state(seed.slug ?? '');
  let slugTouched = $state(false);
  let problem = $state('');
  let saving = $state(false);
  let ruleOpen = $state(false);
  let ruleWeekday = $state(6);
  let ruleStart = $state('20:00');
  let ruleMinutes = $state(180);

  // An existing show: its hosts come by name from the resolved view.
  $effect(() => {
    if (!slug) return;
    void api
      .show(slug)
      .then((v) => {
        view = v;
        draft = {
          name: v.show.name,
          description: v.show.description,
          colour_slot: v.show.colour_slot,
          layout: v.show.layout,
          slot: v.show.slot,
          hosts: v.resolved.hosts,
          tags: v.show.tags,
          explicit: v.show.explicit,
          presenter: v.show.presenter ?? '',
          contact: v.show.contact ?? '',
          links: v.show.links ?? [],
          commercial_music: v.show.commercial_music ?? false,
        };
      })
      .catch((e) => (problem = e instanceof Error ? e.message : String(e)));
  });

  const path = $derived(slug ?? (slugTouched ? newSlug : slugOf(draft.name)));
  const canSave = $derived(draft.name.trim().length > 0 && path.length > 0 && !saving);

  async function save() {
    if (!canSave) return;
    saving = true;
    problem = '';
    try {
      const v = await api.putShow(path, { ...draft, hosts: draft.hosts.map((h) => h.trim()).filter(Boolean) });
      view = v;
      await onSaved(v);
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    } finally {
      saving = false;
    }
  }

  function openRule() {
    const base = draft.slot;
    ruleWeekday = base?.weekday ?? 6;
    ruleStart = base ? `${String(Math.floor(base.start / 60)).padStart(2, '0')}:${String(base.start % 60).padStart(2, '0')}` : '20:00';
    ruleMinutes = base?.minutes ?? 180;
    ruleOpen = true;
  }
  function setRule() {
    const [h, m] = ruleStart.split(':').map(Number);
    const s: Slot = { weekday: ruleWeekday, start: h * 60 + m, minutes: ruleMinutes };
    draft = { ...draft, slot: s };
    ruleOpen = false;
  }
</script>

{#if !embedded}
  <div class="head">
    <div class="crumbs">{slug ? 'Show' : 'New show'}</div>
    <h2 data-panel-title>{draft.name || 'New show'}</h2>
    {#if path}<div class="when"><span class="num">/{path}</span></div>{/if}
  </div>
{/if}
<div class="fields">
  <Field label="Name" marker={draft.name ? 'set' : 'own'} plain hint="Up to three lines on the card at its smallest size; a longer name is refused when saved.">
    <input type="text" value={draft.name} placeholder="The show's name" aria-label="Show name" maxlength="160" oninput={(e) => (draft = { ...draft, name: (e.currentTarget as HTMLInputElement).value })} />
  </Field>
  {#if !slug}
    <Field label="Path" marker="own" plain hint="Lower-case letters and digits joined by hyphens; it follows the name until you change it.">
      <input type="text" value={path} aria-label="Path" oninput={(e) => { slugTouched = true; newSlug = (e.currentTarget as HTMLInputElement).value; }} />
    </Field>
  {/if}
  <Field label="Colour" marker="set" hint="The show's bank slot in the twelve-colour palette, kept on every surface (ADR-0066).">
    <div class="swatches" role="group" aria-label="Colour, as a bank slot">
      {#each Array.from({ length: 12 }, (_, i) => i + 1) as n (n)}
        <button type="button" aria-pressed={draft.colour_slot === n} aria-label="Bank {n}" style="--sw: var(--cart-{n})" onclick={() => (draft = { ...draft, colour_slot: n })}><i></i>{n}</button>
      {/each}
    </div>
  </Field>
  <Field label="Card layout" marker="set" hint="Both ship; the station chooses per show (ADR-0067).">
    {#snippet actions()}
      {#if slug}<button type="button" class="act" onclick={onCard}>Card…</button>{/if}
    {/snippet}
    <span class="seg" role="group" aria-label="Layout">
      <button type="button" aria-pressed={draft.layout === 'poster'} onclick={() => (draft = { ...draft, layout: 'poster' })}>Poster</button>
      <button type="button" aria-pressed={draft.layout === 'slate'} onclick={() => (draft = { ...draft, layout: 'slate' })}>Slate</button>
    </span>
  </Field>
  <Field label="Weekly rule" marker={draft.slot ? 'set' : 'own'} hint="Series and episodes inherit it until set on their own level.">
    {#snippet actions()}
      <button type="button" class="act" onclick={openRule}>{draft.slot ? 'Change…' : 'Set'}</button>
      {#if draft.slot}<button type="button" class="act" onclick={() => (draft = { ...draft, slot: null })}>Clear</button>{/if}
    {/snippet}
    {#if ruleOpen}
      <div class="row">
        <select bind:value={ruleWeekday} aria-label="Weekday">
          {#each ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'] as d, i (d)}<option value={i + 1}>{d}</option>{/each}
        </select>
        <input type="time" bind:value={ruleStart} aria-label="Start, station time" />
        <input type="number" min="1" max="1440" bind:value={ruleMinutes} aria-label="Length in minutes" /> min
        <button type="button" class="btn sm" onclick={setRule}>Set</button>
        <button type="button" class="btn sm quiet" onclick={() => (ruleOpen = false)}>Cancel</button>
      </div>
    {:else}
      <span class="num">{slotWords(draft.slot)}</span>
    {/if}
  </Field>
  <Field label="Hosts" marker={draft.hosts.length ? 'set' : 'own'} plain>
    <input type="text" value={draft.hosts.join(', ')} placeholder="Names, separated by commas" aria-label="Hosts, separated by commas" onchange={(e) => (draft = { ...draft, hosts: splitList((e.currentTarget as HTMLInputElement).value) })} />
  </Field>
  <fieldset class="group" data-panel="calendar.show-presenter">
    <legend data-panel-title>Presenter</legend>
    <Field label="On-air name" marker={draft.presenter ? 'set' : 'own'} plain hint="The consent words name the presenter of the show on air (ADR-0214).">
      <input type="text" value={draft.presenter} placeholder="The name they go by on air" aria-label="Presenter's on-air name" maxlength="120" oninput={(e) => (draft = { ...draft, presenter: (e.currentTarget as HTMLInputElement).value })} />
    </Field>
    <Field label="Takedown contact" marker={draft.contact ? 'set' : 'own'} plain hint="An email address or a link. While this show is on air the consent words give it first, then the station’s own as the host; empty, the station’s alone (ADR-0215).">
      <input type="text" value={draft.contact} placeholder="the station’s contact alone" aria-label="Takedown contact" maxlength="200" autocapitalize="none" spellcheck="false" oninput={(e) => (draft = { ...draft, contact: (e.currentTarget as HTMLInputElement).value })} />
    </Field>
    <Field label="Channel links" marker={draft.links?.length ? 'set' : 'own'} plain hint="One per line: Rumble, Odysee, AzuraCast…">
      <textarea value={(draft.links ?? []).join('\n')} aria-label="Links to the presenter's channels, one per line" autocapitalize="none" spellcheck="false" onchange={(e) => (draft = { ...draft, links: splitList((e.currentTarget as HTMLTextAreaElement).value) })}></textarea>
    </Field>
    <Field label="Commercial music" marker="own" hint={draft.commercial_music ? MUSIC_REMINDER : 'A reminder for the station (ADR-0219); it blocks nothing.'}>
      <label class="row"><input type="checkbox" checked={draft.commercial_music} onchange={(e) => (draft = { ...draft, commercial_music: (e.currentTarget as HTMLInputElement).checked })} style="width:auto" /> plays commercial music</label>
    </Field>
  </fieldset>
  <Field label="Description" marker={draft.description ? 'set' : 'own'} plain>
    <textarea value={draft.description} aria-label="Description" onchange={(e) => (draft = { ...draft, description: (e.currentTarget as HTMLTextAreaElement).value })}></textarea>
  </Field>
  <Field label="Tags" marker={draft.tags.length ? 'set' : 'own'} plain>
    <input type="text" value={draft.tags.join(', ')} placeholder="live music, Fife, requests" aria-label="Tags, separated by commas" onchange={(e) => (draft = { ...draft, tags: splitList((e.currentTarget as HTMLInputElement).value) })} />
  </Field>
  <Field label="Explicit" marker="own" hint="Marks the podcast feed for Apple and Spotify.">
    <label class="row"><input type="checkbox" checked={draft.explicit} onchange={(e) => (draft = { ...draft, explicit: (e.currentTarget as HTMLInputElement).checked })} style="width:auto" /> explicit content</label>
  </Field>
  {#if view?.show.cards?.alt}
    <Field label="Card" marker="own" hint="Drawn when the show was saved; the alt text travels with it.">
      <span class="hint">{view.show.cards.alt}</span>
    </Field>
  {/if}
  {#if slug}{#key slug}<ShowStill {slug} />{/key}{/if}
  {#if problem}<div class="problem" role="alert"><svg aria-hidden="true"><use href="#g-fault" /></svg><span>{problem}</span></div>{/if}
</div>
<div class="foot">
  <div class="r">
    <button type="button" class="btn primary" disabled={!canSave} onclick={save}>{slug ? 'Save show' : 'Make the show'}</button>
    {#if onCancel}<button type="button" class="btn quiet" onclick={onCancel}>Cancel</button>{/if}
  </div>
  <p class="caption">Saving draws the show's three cards (link, post, square); a name that does not fit even at its floor is refused with the field named.</p>
</div>
