<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The show level: the name, the colour as a bank slot (ADR-0066), the card layout
  // (ADR-0067), when it airs (`calendar.show-rule`, ADR-0278: the rule in short, the next date
  // and Change…, which opens the rule dialog; the rule is saved there, on its own route, and
  // every save here sends it back as stored so that a change of name cannot lose it), the
  // hosts, the tags, the explicit flag. A new show is made in the New show dialog, not here.
  // Saved with PUT; the control plane draws the show's cards first and refuses a name
  // that does not fit them, with the field named (422), which is shown here. An existing
  // show also carries its own slate picture (ShowStill, ADR-0200), set by its own route, and
  // its presenter's fields (ADR-0214, ADR-0219): the on-air name and takedown contact the
  // consent words name while it is on air, the links to the presenter's channels, and whether
  // it plays commercial music. The account the show is tied to (ADR-0218) is the wizard's
  // and is left out here, which keeps it. Under the name is the show's step (ADR-0271,
  // `calendar.show-step`): the same field the frame's step dialog changes, so there is one
  // truth. A new show starts at Solo; the step goes with the save only when it was changed.
  // Delete show… (`calendar.show-delete`, ADR-0279) is at the foot, left, for an admin or a
  // producer on a station of 1.0.4 or later: a confirmation that says what goes with the show,
  // with its real counts, and shows the station's own sentence when it refuses (409: an episode
  // has aired or is on air now), keeping the show. When it airs also offers Subscribe…
  // (`calendar.subscribe`, ADR-0281): the link to this show's calendar file.
  import { untrack } from 'svelte';
  import Field from './Field.svelte';
  import ShowStill from './ShowStill.svelte';
  import * as api from '../lib/schedule.ts';
  import type { Show, ShowInput, ShowView } from '../lib/schedule.ts';
  import * as rulesApi from '../lib/api.ts';
  import { ApiError } from '../lib/api.ts';
  import { account } from '../lib/account.svelte.ts';
  import { mayDeleteShow } from '../lib/account.ts';
  import { countsOf, goesWith, type GoesWith } from './showDelete.ts';
  import { splitList } from './model.ts';
  import { MUSIC_REMINDER } from '../lib/wizard.ts';
  import StepList from '../lib/StepList.svelte';
  import { STEP_LONG, asStep } from '../lib/steps.ts';
  import { noteUnsaved, sched } from '../lib/schedule.svelte.ts';
  import { openRuleDialog } from './rule/dialog.svelte.ts';
  import { dateWords, ruleOfSlot, shortOfSlot, whenFields } from './rule/form.ts';
  import { unscheduled } from './rule/view.ts';
  import Subscribe from './files/Subscribe.svelte';

  let {
    slug,
    show,
    onSaved,
    onCancel,
    onCard,
    onRule,
    onDeleted,
    embedded = false,
  }: {
    slug: string;
    show: Show;
    onSaved: (v: ShowView) => Promise<void> | void;
    onCancel?: () => void;
    onCard: () => void;
    /** When the show airs was changed in the rule dialog: the week is fetched again. */
    onRule?: () => Promise<void> | void;
    /** The show was deleted: the tab closes the editor, reads the week again and says so. Without
     *  it the editor offers no Delete show…. */
    onDeleted?: (name: string) => Promise<void> | void;
    embedded?: boolean;
  } = $props();

  // The props seed the draft once; after that the draft is this editor's own.
  const seed = untrack(() => ({ slug, show }));
  let view = $state<ShowView | null>(null);
  let draft = $state<ShowInput>({
    name: seed.show.name,
    description: seed.show.description,
    colour_slot: seed.show.colour_slot,
    layout: seed.show.layout,
    hosts: [],
    tags: seed.show.tags,
    explicit: seed.show.explicit,
    presenter: seed.show.presenter ?? '',
    contact: seed.show.contact ?? '',
    links: seed.show.links ?? [],
    commercial_music: seed.show.commercial_music ?? false,
    // The step the show has (Full before steps arrived).
    step: asStep(seed.show.step),
  });
  /** The draft as it was last read from the station or saved to it: Esc on the tab closes the
   *  editor column, but never over a draft that differs (lib/schedule.svelte.ts, closeOpen). */
  let clean = $state(untrack(() => JSON.stringify(draft)));
  const me = {};
  $effect(() => {
    noteUnsaved(me, JSON.stringify(draft) !== clean ? 'The show' : null);
    return () => noteUnsaved(me, null);
  });
  let problem = $state('');
  let saving = $state(false);
  /** The ask before a show's dates are cleared. */
  let clearing = $state(false);

  /** The show as the station last gave it: when it airs is read from here, never from the draft. */
  const stored = $derived(view?.show ?? show);
  const rulesOn = $derived(sched.rules === true);
  const short = $derived(stored.rule_words?.short ?? (stored.slot ? shortOfSlot(stored.slot) : ''));
  const sentence = $derived(stored.rule_words?.sentence ?? '');
  const hasRule = $derived(rulesOn ? !!stored.rule : !!stored.slot);
  /** A rule that gives nothing more: `Finished: its 10 episodes are made`. */
  const finished = $derived(rulesOn && hasRule ? (unscheduled([stored], true).finished[0]?.why ?? '') : '');
  const next = $derived(stored.next_on?.on ? dateWords(stored.next_on.on, { weekday: 'short' }) : '');

  /** Subscribe… (`calendar.subscribe`, ADR-0281): the link to this show's calendar file, on a
   *  station that has calendar files (1.0.5: it sends its public address, or null, with the week). */
  const calendarFiles = $derived(sched.publicUrl !== undefined);
  let subscribing = $state(false);

  /** Delete show… is the station's for an admin or a producer, from 1.0.4 (the same release as
   *  the rule); nobody is offered it before the station has said who this window is. */
  const canDelete = $derived(!!onDeleted && rulesOn && account.loaded && mayDeleteShow(account.me));
  /** The delete confirmation, while it is open. */
  let removing = $state<{ counts: GoesWith; problem: string; refused: boolean; busy: boolean } | null>(null);
  let removeEl = $state<HTMLDialogElement | null>(null);
  $effect(() => {
    const d = removeEl;
    if (d && !d.open) d.showModal();
  });

  /** Delete show…: ask, with what goes with it counted from the series (a failed read leaves the
   *  plain list, which is still true). */
  async function askDelete() {
    removing = { counts: { series: null, episodes: null }, problem: '', refused: false, busy: false };
    try {
      const v = view ?? (await load());
      const counts = countsOf(await Promise.all(v.series.map((x) => api.series(slug, x.number))));
      if (removing) removing = { ...removing, counts };
    } catch {
      // The counts are a courtesy: the station decides.
    }
  }

  async function confirmDelete() {
    if (!removing || removing.busy || removing.refused) return;
    removing = { ...removing, busy: true, problem: '' };
    const name = stored.name;
    try {
      await api.deleteShow(slug);
      removing = null;
      await onDeleted?.(name);
    } catch (e) {
      if (!removing) return;
      removing = { ...removing, busy: false, refused: e instanceof ApiError && e.status === 409, problem: e instanceof Error ? e.message : String(e) };
    }
  }

  async function load() {
    const v = await api.show(slug);
    view = v;
    return v;
  }

  // Its hosts come by name from the resolved view.
  $effect(() => {
    void load()
      .then((v) => {
        draft = {
          name: v.show.name,
          description: v.show.description,
          colour_slot: v.show.colour_slot,
          layout: v.show.layout,
          hosts: v.resolved.hosts,
          tags: v.show.tags,
          explicit: v.show.explicit,
          presenter: v.show.presenter ?? '',
          contact: v.show.contact ?? '',
          links: v.show.links ?? [],
          commercial_music: v.show.commercial_music ?? false,
          step: asStep(v.show.step),
        };
        clean = JSON.stringify(draft);
      })
      .catch((e) => (problem = e instanceof Error ? e.message : String(e)));
  });

  // When it airs was changed elsewhere (another window, or a save the station made): the tab has
  // read the shows again, and the editor reads its own again too, so a save here cannot send the
  // old rule back over the new one. The draft is left as typed.
  const ruleNow = $derived(JSON.stringify([show.rule ?? null, show.slot ?? null]));
  $effect(() => {
    const now = ruleNow;
    const held = untrack(() => (view ? JSON.stringify([view.show.rule ?? null, view.show.slot ?? null]) : null));
    if (held === null || held === now) return;
    void load().catch(() => {});
  });

  const path = $derived(slug);
  const canSave = $derived(draft.name.trim().length > 0 && !saving);

  async function save() {
    if (!canSave) return;
    saving = true;
    problem = '';
    try {
      // When it airs goes back as the station holds it: a station that stores rules clears a
      // weekly rule from a save that names neither `rule` nor `slot` (new-show.md section 9),
      // so a change of name must carry both. An older station is sent its slot alone.
      const when = rulesOn ? { ...whenFields(stored.rule ?? null, { rules: true }), slot: stored.slot } : { slot: stored.slot };
      const body: ShowInput = { ...draft, hosts: draft.hosts.map((h) => h.trim()).filter(Boolean), ...when };
      // An unchanged step is left out, which keeps it: changing it is for an admin, a producer or
      // the show's presenter, and anyone else may still save the rest.
      if (view && body.step === asStep(view.show.step)) delete body.step;
      const v = await api.putShow(path, body);
      view = v;
      clean = JSON.stringify(draft);
      await onSaved(v);
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    } finally {
      saving = false;
    }
  }

  /** Change…, or Give it dates…: the rule dialog, which the Schedule tab draws. */
  function changeRule() {
    openRuleDialog({
      slug,
      name: stored.name,
      rule: rulesOn ? (stored.rule ?? null) : null,
      follow: !rulesOn && stored.slot ? ruleOfSlot(stored.slot) : null,
      nextOn: stored.next_on?.on ?? null,
      colour: stored.colour_slot,
      onSaved: ruleSaved,
    });
  }

  /** The rule dialog saved: the show is read again, and the week with it. */
  async function ruleSaved() {
    try {
      await load();
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    }
    await onRule?.();
  }

  /** Clear: the show keeps its episodes and has no dates to come. */
  async function clearRule() {
    clearing = false;
    problem = '';
    try {
      if (rulesOn) await rulesApi.putShowRule(slug, null, 'leave');
      else if (view) await api.putShow(slug, { ...api.inputOf(view), slot: null });
      await ruleSaved();
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    }
  }
</script>

{#if !embedded}
  <div class="head">
    <div class="crumbs">Show</div>
    <h2 data-panel-title>{draft.name || stored.name}</h2>
    {#if path}<div class="when"><span class="num">/{path}</span></div>{/if}
  </div>
{/if}
<div class="fields">
  <Field label="Name" marker={draft.name ? 'set' : 'own'} plain hint="Up to three lines on the card at its smallest size; a longer name is refused when saved.">
    <input type="text" value={draft.name} placeholder="The show's name" aria-label="Show name" maxlength="160" oninput={(e) => (draft = { ...draft, name: (e.currentTarget as HTMLInputElement).value })} />
  </Field>
  <Field
    label="Step"
    marker="set"
    plain
    panel="calendar.show-step"
    hint="How much of the studio this show uses. Whatever a step leaves out is switched off while the show is on air, so nothing can wait unseen: under <b>Solo</b> and <b>Guests</b> the call-in page says the lines are closed. Change it whenever you like, here or at the top of the studio; on air, it takes effect at once."
  >
    <StepList value={draft.step ?? 'solo'} name="ed-step" what={STEP_LONG} onpick={(s) => (draft = { ...draft, step: s })} />
  </Field>
  <Field label="Colour" marker="set" hint="The show's bank slot in the twelve-colour palette, kept on every surface (ADR-0066).">
    <div class="swatches" role="group" aria-label="Colour, as a bank slot">
      {#each Array.from({ length: 12 }, (_, i) => i + 1) as n (n)}
        <button type="button" aria-pressed={draft.colour_slot === n} aria-label="Bank {n}" style="--sw: var(--cart-{n})" onclick={() => (draft = { ...draft, colour_slot: n })}><i></i>{n}</button>
      {/each}
    </div>
  </Field>
  <Field label="Card layout" marker="set" hint="Both ship; the station chooses per show (ADR-0067).">
    {#snippet actions()}
      <button type="button" class="act" onclick={onCard}>Card…</button>
    {/snippet}
    <span class="seg" role="group" aria-label="Layout">
      <button type="button" aria-pressed={draft.layout === 'poster'} onclick={() => (draft = { ...draft, layout: 'poster' })}>Poster</button>
      <button type="button" aria-pressed={draft.layout === 'slate'} onclick={() => (draft = { ...draft, layout: 'slate' })}>Slate</button>
    </span>
  </Field>
  <Field
    label="When it airs"
    marker={hasRule ? 'set' : 'own'}
    panel="calendar.show-rule"
    hint="{sentence ? `${sentence} ` : ''}Series and episodes follow it until set on their own level."
  >
    {#snippet actions()}
      <button type="button" class="act" onclick={changeRule}>{hasRule ? 'Change…' : 'Give it dates…'}</button>
      {#if hasRule && !clearing}<button type="button" class="act" onclick={() => (clearing = true)}>Clear</button>{/if}
      {#if calendarFiles}<button type="button" class="act" aria-haspopup="dialog" title="The link to this show’s calendar file, for listeners’ own calendars" onclick={() => (subscribing = true)}>Subscribe…</button>{/if}
    {/snippet}
    {#if clearing}
      <span class="confirm" role="group" aria-label="Clear the show's dates">
        <span>Take its dates away? Its episodes stay where they are.</span>
        <button type="button" class="btn sm danger-hollow" onclick={clearRule}>Clear its dates</button>
        <button type="button" class="btn sm quiet" onclick={() => (clearing = false)}>Keep them</button>
      </span>
    {:else if !hasRule}
      <span class="num">No dates yet</span>
    {:else if finished}
      <span class="num">Finished: {finished}</span>
    {:else}
      <span class="num">{short}</span>{#if next}<span class="next">· next {next}</span>{/if}
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
  {#key slug}<ShowStill {slug} />{/key}
  {#if problem}<div class="problem" role="alert"><svg aria-hidden="true"><use href="#g-fault" /></svg><span>{problem}</span></div>{/if}
</div>
<div class="foot">
  <div class="r">
    {#if canDelete}<button type="button" class="btn danger-hollow del" onclick={askDelete}>Delete show…</button>{/if}
    <button type="button" class="btn primary" disabled={!canSave} onclick={save}>Save show</button>
    {#if onCancel}<button type="button" class="btn quiet" onclick={onCancel}>Cancel</button>{/if}
  </div>
  <p class="caption">Saving draws the show's three cards (link, post, square); a name that does not fit even at its floor is refused with the field named.</p>
</div>
{#if subscribing}
  <Subscribe show={{ slug, name: stored.name, hasDates: hasRule }} publicUrl={sched.publicUrl} onClose={() => (subscribing = false)} />
{/if}
{#if removing}
  <dialog bind:this={removeEl} class="wz confirm" data-panel="calendar.show-delete" role="alertdialog" aria-labelledby="sd-h" aria-describedby="sd-p" oncancel={(e) => { e.preventDefault(); removing = null; }}>
    <div class="head">
      <h2 id="sd-h" data-panel-title>Delete {stored.name}?</h2>
      <p id="sd-p" class="small">{goesWith(removing.counts)} This cannot be undone.</p>
    </div>
    {#if removing.problem}<p class="err" role="alert">{removing.problem}</p>{/if}
    <div class="acts">
      <button type="button" class="btn danger-hollow" disabled={removing.busy || removing.refused} aria-busy={removing.busy || undefined} onclick={confirmDelete}>Delete show</button>
      <!-- svelte-ignore a11y_autofocus -->
      <button type="button" class="btn primary" autofocus onclick={() => (removing = null)}>Keep it</button>
    </div>
  </dialog>
{/if}
