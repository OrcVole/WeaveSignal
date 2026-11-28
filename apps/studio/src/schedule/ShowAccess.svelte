<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Who can listen to a show (keyed shows, 1.3.2), in the show editor: everyone, or people with
  // a key; for a keyed show, when it opens to everyone by itself (early access), and what the
  // public hears meanwhile. It is read and saved through the show's access rule, apart from
  // Save show: changing it changes who may hear the show at once for new listeners, so it has
  // its own button and says what it did. A producer's and an admin's to change; anyone else
  // reads it. Nothing is drawn at a station older than 1.3.2. Panel name `calendar.show-access`.
  //
  // It is step 1 of the three that put a key in a listener's hands (the operator's click-through,
  // 2026-11-25): the steps stand at its top for whoever may change it, and step 2 goes to Make a
  // key once the show is keyed. Other screens lead here (`toWhoCanListen`: the show's pane, an
  // episode, the Keys tab, New show's Done): the fieldset is then brought into view with the
  // focus in it and an outline for a moment, and, when asked, with People with a key already
  // chosen and waiting for Save who can listen.
  import { untrack } from 'svelte';
  import { account } from '../lib/account.svelte.ts';
  import { FieldError, sandboxed } from '../lib/api.ts';
  import { SLATE_LINE, mayManageKeys, timeWords } from '../lib/keys.ts';
  import { setShowAccess, showAccess, type AccessRule, type ShowAccess, type Slate } from '../lib/keysApi.ts';
  import { accessUi, keyed, loadKeyed, reloadKeyed } from '../lib/keys.svelte.ts';
  import { toKeys as goToKeys } from '../lib/keyedNav.ts';
  import { FILES, holdsFiles } from '../lib/keyed.ts';
  import { APP_VERSION } from '../appVersion.ts';
  import KeySteps from '../keys/KeySteps.svelte';
  import { studio } from '../lib/studio.svelte.ts';
  import { fromLocalInput, localInputValue } from './model.ts';
  import Field from './Field.svelte';

  let { slug, name, initial = null }: { slug: string; name: string; initial?: ShowAccess | null } = $props();

  const may = $derived(mayManageKeys(account.me));
  const zone = $derived(studio.zone || Intl.DateTimeFormat().resolvedOptions().timeZone);
  const now = $derived(studio.now || Date.now());
  const seed = untrack(() => initial);

  /** `absent`: the station is older than 1.3.2, so nothing is drawn. `closed`: the account may
   *  not read the rule (a presenter, a screener): the public list says whether it is keyed. */
  let phase = $state<'loading' | 'ok' | 'absent' | 'closed' | 'not-set-up' | 'problem'>(seed ? 'ok' : 'loading');
  let stored = $state<ShowAccess | null>(seed);
  let said = $state('');
  let rule = $state<AccessRule>(seed?.rule ?? 'public');
  let slate = $state<Slate>(seed?.slate ?? 'announcement');
  let slateNote = $state(seed?.slate_note ?? '');
  let opens = $state(seed?.opens_at_ms != null ? localInputValue(seed.opens_at_ms, untrack(() => zone)) : '');
  let saving = $state(false);
  let problem = $state('');
  let note = $state('');

  function take(a: ShowAccess) {
    stored = a;
    rule = a.rule;
    slate = a.slate;
    slateNote = a.slate_note;
    opens = a.opens_at_ms != null ? localInputValue(a.opens_at_ms, zone) : '';
  }

  $effect(() => {
    if (seed) return;
    // The website's sandbox has no station to keep a rule: nothing is drawn there.
    if (sandboxed()) return void (phase = 'absent');
    untrack(() => {
      void loadKeyed();
      void showAccess(slug)
        .then((a) => {
          take(a);
          phase = 'ok';
        })
        .catch((e) => {
          const status = e instanceof FieldError ? e.status : 0;
          said = e instanceof FieldError && !/^\d{3}\b/.test(e.message) ? e.message : '';
          phase = status === 404 || status === 405 ? 'absent' : status === 403 ? 'closed' : status === 503 ? 'not-set-up' : 'problem';
        });
    });
  });

  const opensMs = $derived(opens ? fromLocalInput(opens, zone) : null);
  const dirty = $derived(!!stored && (rule !== stored.rule || (rule === 'key' && (slate !== stored.slate || slateNote.trim() !== stored.slate_note || (opensMs ?? null) !== (stored.opens_at_ms ?? null)))));
  const opened = $derived(stored?.rule === 'key' && stored.opens_at_ms != null && stored.opens_at_ms <= now);
  /** The station holds a keyed show's podcast feed and episode files back (1.3.3). */
  const held = $derived(holdsFiles(studio.stationVersion, APP_VERSION));
  /** For someone who may not read the rule: what the public list says of this show. */
  const listed = $derived(keyed.list?.shows.find((s) => s.slug === slug) ?? null);

  // The rule was changed elsewhere while this is open (the tick at the top of the pane, in the
  // list of shows, on the Keys tab; another window): the station's list no longer agrees with
  // what was read here, so it is read again, unless something typed here waits to be saved.
  // Without this a save here would send the old rule back over the new one. The time it opens
  // to everyone counts too (1.3.3): a show that had opened by itself and is ticked again keeps
  // its rule and loses that time, and a form that still held the past time would say the show
  // was open, and send the past time back with the next save.
  // Once for each reading of the list: a list and a rule that still disagree are not asked about in a loop.
  let rereadFor: unknown = null;
  $effect(() => {
    const list = keyed.list;
    if (phase !== 'ok' || !stored || keyed.state !== 'ok' || !list) return;
    const row = list.shows.find((s) => s.slug === slug) ?? null;
    const now = row ? 'key' : 'public';
    const same = now === stored.rule && (now === 'public' || (row?.opens_at_ms ?? null) === (stored.opens_at_ms ?? null));
    if (same || dirty || saving || list === rereadFor) return;
    rereadFor = list;
    untrack(() => {
      void showAccess(slug)
        .then((a) => {
          if (!dirty && !saving) take(a);
        })
        .catch(() => {});
    });
  });

  async function save() {
    if (!stored || saving) return;
    problem = '';
    note = '';
    if (opens && opensMs === null) return void (problem = 'Enter a date and a time for when it opens to everyone, or leave it empty.');
    if ([...slateNote].length > 500) return void (problem = 'The announcement’s line is up to 500 characters.');
    saving = true;
    try {
      const was = stored.rule;
      take(await setShowAccess(slug, rule === 'key' ? { rule, slate, slate_note: slateNote.trim(), opens_at_ms: opensMs } : { rule }));
      void reloadKeyed();
      note =
        rule === 'key'
          ? was === 'key'
            ? `${name} is still for people with a key; what was changed is saved.`
            : `${name} is now for people with a key. Make keys for its listeners on the Keys tab.`
          : `${name} is open to everyone again. Its keys are kept, and open nothing until it is keyed again.`;
    } catch (e) {
      // The station refuses a key it could not keep from the public: playout with keyed shows off
      // (409), or not answering (503).
      if (e instanceof FieldError && e.status === 409 && rule === 'key')
        problem = 'Playout has keyed shows off, so nothing would keep this show from the public. Turn them on (KEYED_SHOWS in the Compose bundle’s settings), restart playout, then save again.';
      else if (e instanceof FieldError && e.status === 503 && rule === 'key')
        problem = 'Playout is not answering, so the station cannot make sure it keeps this show from the public. Save again once playout is back.';
      else problem = e instanceof Error && e.message ? e.message : 'The station did not answer. Try again.';
    } finally {
      saving = false;
    }
  }

  function toKeys() {
    goToKeys(null, stored?.show_id ?? '');
  }
  /** Step 2: Make a key, on the Keys tab. */
  function makeKey() {
    goToKeys('make');
  }

  // Another screen led here (`toWhoCanListen`): into view, the focus in it, an outline for a moment.
  let box = $state<HTMLFieldSetElement | null>(null);
  let jumped = $state(false);
  $effect(() => {
    const j = accessUi.jump;
    if (!j || j.slug !== slug || phase === 'loading') return;
    // A station without keyed shows draws nothing here: there is nowhere to arrive.
    if (phase === 'absent') return void untrack(() => (accessUi.jump = null));
    const el = box;
    if (!el) return;
    untrack(() => {
      accessUi.jump = null;
      if (j.pick && may && phase === 'ok' && stored?.rule === 'public') rule = 'key';
      jumped = true;
      setTimeout(() => (jumped = false), 2600);
      queueMicrotask(() => {
        el.scrollIntoView({ block: 'start' });
        (el.querySelector<HTMLElement>('input[type="radio"]:checked') ?? el).focus({ preventScroll: true });
      });
    });
  });
</script>

{#if phase !== 'absent' && phase !== 'loading'}
  <fieldset bind:this={box} class="group access" class:jumped tabindex="-1" data-panel="calendar.show-access" data-rule={stored?.rule ?? listed?.now ?? 'public'}>
    <legend data-panel-title><svg class="kg" aria-hidden="true"><use href="#g-key" /></svg>Who can listen</legend>
    {#if phase === 'ok' && stored}
      {#if may}
        <KeySteps compact current={1} go={{ 2: stored.rule === 'key' ? makeKey : undefined }} waits={{ 2: 'Choose People with a key and save, and this step leads to Make a key.', 3: 'Send opens by itself the moment a key is made.' }} />
        <!-- A show that opened to everyone by itself still has People with a key chosen below: said
             first, above the choice, so the form is not read as a show still kept (1.3.3). -->
        {#if opened && !dirty}<p class="src-note" data-testid="access-opened">This show opened to everyone on {timeWords(stored.opens_at_ms!, zone)}, so no key is needed now. To keep it for people with a key again, clear <b>Opens to everyone</b> or give it a later time, and save.</p>{/if}
        <div class="choice" role="radiogroup" aria-label="Who can listen to {name}">
          <label class="opt" class:on={rule === 'public'}><input type="radio" name="access-{slug}" value="public" bind:group={rule} /><span><b>Everyone</b><span class="hint">The show is on the station’s streams as ever.</span></span></label>
          <label class="opt" class:on={rule === 'key'}><input type="radio" name="access-{slug}" value="key" bind:group={rule} /><span><b>People with a key</b><span class="hint">Advertised to everyone, heard and watched only with a key you give out.</span></span></label>
        </div>
        {#if rule === 'key'}
          <Field label="The public hears" marker="own" plain hint="{SLATE_LINE[slate]} Kept for a later release: in 1.3.2 the station plays its own slate whichever you choose (its slate file, else its fallback music).">
            <select aria-label="What the public hears while the show is on" bind:value={slate}>
              <option value="announcement">An announcement</option>
              <option value="fallback">The fallback music</option>
              <option value="teaser">A teaser</option>
            </select>
          </Field>
          {#if slate === 'announcement'}
            <Field label="The announcement’s line" marker={slateNote ? 'set' : 'own'} plain hint="Where to get a key, or when the public programme returns. Empty: the station’s own words.">
              <input type="text" bind:value={slateNote} maxlength="500" aria-label="A line for the announcement" placeholder="Supporters hear this show. Join at…" />
            </Field>
          {/if}
          <Field label="Opens to everyone" marker={opens ? 'set' : 'own'} plain hint="For early access: from this moment the show is public by itself, with nobody pressing anything. Empty: never. Station time, {zone.replaceAll('_', ' ')}.">
            <input type="datetime-local" bind:value={opens} aria-label="When the show opens to everyone" />
          </Field>
        {/if}
        {#if dirty && rule !== stored.rule}
          <p class="src-note" role="status" data-testid="access-will">{rule === 'key' ? `From the moment you save, new listeners need a key to hear this show. ${held ? `${FILES.tick} ${FILES.gone}` : FILES.older}` : `From the moment you save, anyone can listen to this show again.${held ? ` ${FILES.back}` : ''}`}</p>
        {/if}
        {#if problem}<p class="err" role="alert">{problem}</p>{/if}
        <div class="acts">
          <button type="button" class="btn sm primary" disabled={!dirty || saving} aria-busy={saving || undefined} onclick={save}>{saving ? 'Saving…' : 'Save who can listen'}</button>
          {#if stored.rule === 'key'}<button type="button" class="btn sm" onclick={toKeys}>Its keys…</button>{/if}
          {#if stored.updated_ms > 0}<span class="small">Set {timeWords(stored.updated_ms, zone)} by {stored.updated_by}</span>{/if}
        </div>
        {#if note}<p class="small" role="status" data-testid="access-note">{note}</p>{/if}
      {:else}
        <p class="src-note plain" data-testid="access-read">{stored.rule === 'key' ? (opened ? 'Kept for people with a key until it opened to everyone.' : 'People with a key. A producer or an admin sets this.') : 'Everyone. A producer or an admin can keep a show for people with a key.'}</p>
      {/if}
    {:else if phase === 'closed'}
      <p class="src-note plain" data-testid="access-read">{listed?.now === 'key' ? 'People with a key. A producer or an admin sets this, and makes the keys.' : listed ? 'Kept for people with a key until it opened to everyone.' : 'Everyone. A producer or an admin can keep a show for people with a key.'}</p>
    {:else if phase === 'not-set-up'}
      <p class="src-note plain" data-testid="access-unset">Keys are not set up at this station, so every show is open to everyone.{#if said} <span class="mono">{said}</span>{/if}</p>
    {:else}
      <p class="err" role="alert">Who can listen could not be read.{#if said} {said}{/if}</p>
    {/if}
  </fieldset>
{/if}

<style>
  .access legend {
    display: flex;
    align-items: center;
    gap: var(--s-1);
  }
  .kg {
    width: 12px;
    height: 12px;
  }
  .choice {
    display: grid;
    gap: var(--s-2);
  }
  .opt {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--s-2);
    align-items: start;
    border: 1px solid var(--hairline);
    border-radius: var(--r-2);
    padding: var(--s-2);
    cursor: pointer;
    line-height: var(--lh-label);
  }
  .opt.on {
    border-color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  .opt input {
    width: auto;
    margin-top: 0.2rem;
    accent-color: var(--accent);
  }
  .opt b {
    display: block;
    font-weight: 600;
  }
  .hint {
    font-size: var(--fs-13);
    color: var(--ink-2);
  }
  .acts {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    flex-wrap: wrap;
  }
  .src-note {
    margin: 0;
    font-size: var(--fs-13);
    line-height: var(--lh-label);
    color: var(--ink);
    background: var(--surface-2);
    border-left: 3px solid var(--accent);
    border-radius: var(--r-1);
    padding: var(--s-2);
  }
  .src-note.plain {
    background: transparent;
    border-left: 0;
    padding: 0;
    color: var(--ink-2);
  }
  .err {
    border-left: 3px solid var(--status-critical);
    padding-left: var(--s-2);
    font-size: var(--fs-13);
    line-height: var(--lh-body);
  }
</style>
