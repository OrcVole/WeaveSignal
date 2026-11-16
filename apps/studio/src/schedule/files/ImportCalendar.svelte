<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Import a calendar… (`calendar.import`; ADR-0281): a dialog over the Schedule tab in three
  // parts, as New show is. Choose a file; look at the shows it proposes, each said back in New
  // show's own words (the rule's sentence, its next dates, its clashes), tick which to make and
  // rename any; then Done. Nothing is made until Make the shows, and the station makes them all
  // in one transaction or none.
  //
  // The studio reads no calendar and works out no dates (ADR-0278): the file goes to the
  // station as it is (`POST /api/v1/schedule/import/preview`, which writes nothing), and what
  // comes back is drawn. An admin or a producer is offered this; the station refuses anyone
  // else. The dialog owns the keyboard (keymap rule 5): ⏎ in a name does not make the shows.
  //
  // Three ways to give the file (the 1.0.5 test run: a path could not be pasted into the system's
  // file chooser): Choose a file…; a file dropped anywhere on the dialog; and, in the packaged
  // app, the file's path pasted into a field, which the app's own side reads (`calendar_read`:
  // an `.ics` of at most 1 MB that begins as a calendar, and no other file). A page cannot read
  // a path, so a browser build has no such field. In the packaged app a dropped file comes as a
  // path through Tauri's own event, because the webview takes the drop before the page does.
  import { untrack } from 'svelte';
  import { packaged, type IsoDate } from '../../lib/api.ts';
  import { dialogKeys } from '../rule/keys.ts';
  import { dateWords, hhmm } from '../rule/form.ts';
  import { CLASH_COST, clashNotice, rows as rowsOf } from '../rule/view.ts';
  import { edges } from '../rule/edges.ts';
  import {
    MAX_EVENTS,
    afterWords,
    allChoices,
    allThere,
    alreadyNote,
    alreadyWords,
    chosen,
    clashingTicked,
    decodeCalendar,
    droppedFile,
    eventsWords,
    fileProblem,
    fileWords,
    firstChoices,
    madeWords,
    makeLabel,
    mappingWords,
    nameProblems,
    nothingTicked,
    overlapWords,
    placeRefusal,
    refusalWords,
    sameNames,
    sharedClash,
    warningWords,
    type Choice,
    type ImportItem,
    type ImportMade,
    type ImportPreview,
  } from './import.ts';
  import { importPreview, importShows, onFilesDropped, readCalendarAt } from './station.ts';

  let {
    today,
    onMade,
    onOpen,
    onClose,
    file: firstFile = '',
    preview: firstPreview = null,
    made: firstMade = null,
  }: {
    /** The station's date today. */
    today: IsoDate;
    /** The shows were made: the week and the list of shows are read again. */
    onMade: () => void | Promise<void>;
    /** Open one of the shows made, at its Dates. */
    onOpen: (slug: string) => void;
    onClose: () => void;
    /** The file's name, the preview and the reply to open on (a test's). */
    file?: string;
    preview?: ImportPreview | null;
    made?: ImportMade | null;
  } = $props();

  const PARTS = ['The file', 'The shows', 'Done'] as const;
  const seed = untrack(() => ({ firstFile, firstPreview, firstMade }));

  let fileName = $state(seed.firstFile);
  let preview = $state<ImportPreview | null>(seed.firstPreview);
  let choices = $state<Choice[]>(seed.firstPreview ? firstChoices(seed.firstPreview) : []);
  let made = $state<ImportMade | null>(seed.firstMade);
  let reading = $state(false);
  /** Why the file chosen last was not read. */
  let problem = $state('');
  let making = $state(false);
  /** A refusal of Make the shows: on the form, or under one row's name or dates. */
  let refusal = $state<{ index: number | null; part: 'name' | 'rule' | null; message: string } | null>(null);
  let discarding = $state(false);
  let dialogEl = $state<HTMLDialogElement | null>(null);
  let confirmEl = $state<HTMLDialogElement | null>(null);
  let input = $state<HTMLInputElement | null>(null);
  /** The packaged app: its own side can read a path, and hands over what is dropped. */
  const app = packaged();
  /** The path as pasted, quotes and `~` and all. */
  let pathText = $state('');
  /** A file is being dragged over the window. */
  let over = $state(false);

  const part = $derived<1 | 2 | 3>(made ? 3 : preview ? 2 : 1);
  const picked = $derived(preview ? chosen(preview, choices) : []);
  const names = $derived(preview ? nameProblems(preview, choices) : new Map<number, string>());
  const twins = $derived(preview ? sameNames(preview, choices) : new Map<number, string>());
  const clashing = $derived(preview ? clashingTicked(preview, choices) : 0);
  /** Said once for the file when the station already has some of its shows. */
  const there = $derived(preview ? alreadyNote(preview) : null);
  /** Tick all has nothing to tick: every show that can be made is on the station already. */
  const noneNew = $derived(!!preview && allThere(preview));
  /** Why Make the shows is held, or empty. */
  const held = $derived(!picked.length ? (preview ? nothingTicked(preview) : '') : names.size ? 'Put right the names marked first.' : '');
  const dirty = $derived(!!preview && !made && JSON.stringify(choices) !== JSON.stringify(firstChoices(preview)));

  const focus = (id: string) => queueMicrotask(() => document.getElementById(id)?.focus());

  $effect(() => {
    const d = dialogEl;
    if (d && !d.open) {
      d.showModal();
      focus(part === 1 ? 'ic-choose' : 'ic-h');
    }
  });
  $effect(() => {
    const c = confirmEl;
    if (c && !c.open) c.showModal();
  });

  /** A file was given, by any of the three ways: its text is sent as it is, and the station
   *  says what it holds. A second file that cannot be read leaves the first one's shows where
   *  they are. `named` is the file's name where it is known before it is read; a path's file
   *  is named by the app's own side, whose refusal is a whole sentence. */
  async function take(read: () => Promise<{ name: string; text: string }>, named: string | null) {
    if (reading || making) return;
    problem = '';
    refusal = null;
    reading = true;
    try {
      let got: { name: string; text: string };
      try {
        got = await read();
      } catch (err) {
        problem = named ? `${named} was not read. ${refusalWords(err)}` : refusalWords(err);
        return;
      }
      try {
        const p = await importPreview(got.text);
        preview = p;
        choices = firstChoices(p);
        fileName = got.name;
        pathText = '';
        focus('ic-h');
      } catch (err) {
        problem = `${got.name} was not read. ${refusalWords(err)}`;
      }
    } finally {
      reading = false;
    }
  }

  /** A file from the chooser, or one dropped on a browser's page. */
  function takeFile(f: File) {
    if (reading || making) return;
    problem = '';
    refusal = null;
    const early = fileProblem(f);
    if (early) problem = early;
    else void take(async () => ({ name: f.name, text: decodeCalendar(await f.arrayBuffer()) }), f.name);
  }

  function choose(e: Event) {
    const el = e.currentTarget as HTMLInputElement;
    const f = el.files?.[0];
    el.value = '';
    if (f) takeFile(f);
  }

  /** A path, pasted or dropped (the packaged app): its own side reads the file, or says in a
   *  sentence why not. */
  function takePath(raw: string) {
    if (reading || making) return;
    if (!raw.trim()) {
      problem = 'Paste the path of a calendar file (.ics) first.';
      focus('ic-path');
      return;
    }
    void take(() => readCalendarAt(raw), null);
  }

  /** What was dropped: the first file named as a calendar is read; anything else is said. */
  function dropped<T>(list: readonly T[], nameOf: (f: T) => string, read: (f: T) => void) {
    over = false;
    if (reading || making || made || discarding) return;
    const d = droppedFile(list, nameOf);
    if ('problem' in d) {
      problem = d.problem;
      refusal = null;
    } else read(d.file);
  }

  // A drop anywhere in the window is a drop on the dialog, which is all there is to drop on
  // while it is open. In a browser the page hears it; the default is always stopped, or a file
  // that missed would be opened in place of the studio.
  $effect(() => {
    const hasFiles = (e: DragEvent) => [...(e.dataTransfer?.types ?? [])].includes('Files');
    const onOver = (e: DragEvent) => {
      e.preventDefault();
      if (!app && hasFiles(e) && !made) over = true;
    };
    const onLeave = (e: DragEvent) => {
      if (!e.relatedTarget) over = false;
    };
    const onDrop = (e: DragEvent) => {
      e.preventDefault();
      // The packaged app is told the paths by Tauri (below); the page's own event has no file.
      if (app) return;
      dropped([...(e.dataTransfer?.files ?? [])], (f) => f.name, takeFile);
    };
    window.addEventListener('dragover', onOver);
    window.addEventListener('dragleave', onLeave);
    window.addEventListener('drop', onDrop);
    return () => {
      window.removeEventListener('dragover', onOver);
      window.removeEventListener('dragleave', onLeave);
      window.removeEventListener('drop', onDrop);
    };
  });
  $effect(() => {
    if (!app) return;
    let stop: (() => void) | null = null;
    let gone = false;
    void onFilesDropped((e) => {
      if (e.kind === 'drop') dropped(e.paths, (p) => p, takePath);
      else over = e.kind === 'over' && !made;
    })
      .then((s) => {
        if (gone) s();
        else stop = s;
      })
      .catch(() => {});
    return () => {
      gone = true;
      stop?.();
    };
  });

  function setAll(on: boolean) {
    if (!preview) return;
    choices = allChoices(preview, choices, on);
  }

  async function make() {
    if (!preview || making || held) return;
    const sent = picked;
    making = true;
    refusal = null;
    try {
      made = await importShows(sent);
      focus('ic-h');
      await onMade();
    } catch (err) {
      refusal = placeRefusal(err, sent);
      // The row the station named is brought into view, with its name ready to change.
      if (refusal.index !== null) focus(`ic-name-${refusal.index}`);
    } finally {
      making = false;
    }
  }

  /** Close, Cancel or Esc: ticks and names changed are asked about once. */
  function leave() {
    // While the shows are being made there is nothing to leave: the answer is a moment away.
    if (making) return;
    if (dirty) discarding = true;
    else dialogEl?.close();
  }
  function keepOn() {
    discarding = false;
    focus('ic-h');
  }

  function keys(e: KeyboardEvent) {
    if (!dialogEl || discarding) return;
    dialogKeys(e, dialogEl, leave, () => {
      if (part === 1) input?.click();
      else if (part === 2) void make();
      else dialogEl?.close();
    });
  }

  function open(slug: string) {
    onOpen(slug);
    dialogEl?.close();
  }

  const rows = (i: ImportItem, zone: string) => (i.rule ? rowsOf(i.preview, zone, today, hhmm(i.rule.start)) : []);
  const nextWords = (on: string | null | undefined) => (on ? `next ${dateWords(on, { weekday: 'short', month: 'short' })}` : 'nothing coming up');
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<dialog
  bind:this={dialogEl}
  class="wz files import"
  class:wide={part === 2}
  class:mid={part !== 2}
  class:over
  data-panel="calendar.import"
  data-part={part}
  aria-labelledby="ic-h"
  tabindex="-1"
  onkeydown={keys}
  oncancel={(e) => { e.preventDefault(); leave(); }}
  onclose={onClose}
>
  <div class="head">
    <p class="crumbs" aria-hidden="true">
      {#each PARTS as p, i (p)}<span class:now={part === i + 1} class:done={part > i + 1}>{i + 1} {p}</span>{/each}
    </p>
    <h2 id="ic-h" data-panel-title tabindex="-1">Import a calendar{#if part === 2 && fileName}{' '}<span class="of">· {fileName}</span>{/if}</h2>
    <p class="small"><span class="sr-only">Part {part} of 3: {PARTS[part - 1]}. </span>
      {#if part === 1}Read a calendar file (.ics) from another calendar or another station’s system, and choose which of its shows to make here.
      {:else if part === 2 && preview}<span data-testid="ic-file">{fileWords(preview)}.</span> Every time is on the station’s clock, <span class="num">{preview.zone}</span>.
      {:else}Made, and on the schedule.{/if}
    </p>
    {#if part === 2 && preview}
      {#if preview.ready > 0}
        <div class="tools">
          <span class="count num" role="status" data-testid="ic-count">{picked.length} of {preview.ready} ticked</span>
          <button type="button" class="act2" onclick={() => setAll(true)} disabled={making || noneNew} title={noneNew ? 'Every show here is already on the station. Tick one on its own row to make it a second time.' : undefined}>Tick all</button>
          <button type="button" class="act2" onclick={() => setAll(false)} disabled={making}>Untick all</button>
        </div>
      {/if}
    {/if}
  </div>

  {#if part === 1}
    <div class="scroll choose" use:edges>
      <div class="pickfile" class:over>
        <button id="ic-choose" type="button" class="btn primary" disabled={reading} aria-busy={reading || undefined} aria-describedby="ic-limits ic-drop" onclick={() => input?.click()}>{reading ? 'Reading the file…' : 'Choose a file…'}</button>
        <span id="ic-limits" class="hint">Up to 1 MB and {MAX_EVENTS} events.</span>
        <span id="ic-drop" class="hint drop" data-testid="ic-drop">{over ? 'Let go to read the file.' : 'Or drop the file anywhere on this dialog.'}</span>
      </div>
      {#if app}
        <form class="pathrow" data-testid="ic-pathrow" onsubmit={(e) => { e.preventDefault(); takePath(pathText); }}>
          <label for="ic-path">Or paste the file’s path</label>
          <div class="in">
            <input id="ic-path" type="text" bind:value={pathText} placeholder="~/Downloads/schedule.ics" autocomplete="off" autocapitalize="off" spellcheck="false" disabled={reading} aria-describedby="ic-path-hint" />
            <button type="submit" class="btn" disabled={reading || !pathText.trim()}>Read the file</button>
          </div>
          <span id="ic-path-hint" class="hint">As a file manager copies it, or as <b class="ink">Download .ics</b> said it. Quotes around it and <span class="num">~</span> for your home folder are fine. Only a file ending <span class="num">.ics</span> is read.</span>
        </form>
      {/if}
      {#if problem}<p class="err" role="alert" data-testid="ic-problem">{problem}</p>{/if}
      <ul class="carries">
        <li>An event that repeats becomes a show with the same rule.</li>
        <li>Single events that share a name become one show, on chosen dates.</li>
        <li>You see each show’s dates and clashes first, and can rename or leave out any. Nothing is made until you say.</li>
      </ul>
    </div>
    <div class="foot2">
      <div class="acts">
        <button type="button" class="btn quiet" onclick={leave}>Cancel <kbd>Esc</kbd></button>
      </div>
    </div>
  {:else if part === 2 && preview}
    {@const p = preview}
    <!-- The dialog's middle row, and the only part that scrolls: what is said of the file as a
         whole, then the shows. The head and the foot, with Make the shows, stay in the window. -->
    <div class="scroll" use:edges>
    {#each p.notes as n (n)}<p class="note" data-testid="ic-note"><svg aria-hidden="true"><use href="#g-degraded" /></svg><span>{n}</span></p>{/each}
    {#if there}<p class="note there" data-testid="ic-there"><svg aria-hidden="true"><use href="#g-good" /></svg><span>{there}</span></p>{/if}
    {#if clashing > 0}<p class="note cost" data-testid="ic-cost"><svg aria-hidden="true"><use href="#g-fault" /></svg><span><b>{clashing === 1 ? 'One ticked show clashes' : `${clashing} ticked shows clash`} with shows the station has.</b> {CLASH_COST} A clash does not stop the import: skip or move a date afterwards, in the show’s Dates.</span></p>{/if}
    <ol class="items" aria-label="The shows this file proposes">
      {#each p.items as item (item.index)}
        {@const c = choices[item.index]}
        {@const m = mappingWords(item)}
        {@const no = item.mapping === 'refused'}
        {@const rs = rows(item, p.zone)}
        {@const nameErr = c.on ? (refusal?.index === item.index && refusal.part === 'name' ? refusal.message : (names.get(item.index) ?? '')) : ''}
        {@const ruleErr = refusal?.index === item.index && refusal.part === 'rule' ? refusal.message : ''}
        {@const over = c.on ? overlapWords(item, p, choices) : null}
        {@const after = afterWords(item)}
        {@const notice = no ? null : clashNotice(item.preview, p.zone)}
        {@const every = !!notice && sharedClash(item.preview)}
        <li class="item" class:off={!c.on && !no} class:no class:there={!!item.already} data-item={item.index} data-mapping={item.mapping} data-already={item.already?.slug}>
          <div class="pick">
            {#if no}
              <svg class="nope" aria-hidden="true"><use href="#g-muted" /></svg>
            {:else}
              <input id="ic-on-{item.index}" type="checkbox" checked={c.on} disabled={making} aria-label="Make {c.name.trim() || item.name}" onchange={(e) => { choices[item.index] = { ...c, on: e.currentTarget.checked }; refusal = null; }} />
            {/if}
          </div>
          <div class="what">
            <div class="nm">
              {#if no}
                <span class="name fixed">{item.name || 'An event with no name'}</span>
              {:else}
                <input
                  id="ic-name-{item.index}"
                  class="name"
                  type="text"
                  value={c.name}
                  maxlength="160"
                  autocomplete="off"
                  disabled={making}
                  aria-label="Name of show {item.index + 1}"
                  aria-invalid={nameErr ? true : undefined}
                  aria-describedby={nameErr || (c.on && twins.has(item.index)) ? `ic-name-err-${item.index}` : undefined}
                  oninput={(e) => { choices[item.index] = { ...c, name: e.currentTarget.value }; if (refusal?.index === item.index) refusal = null; }}
                />
              {/if}
              <span class="map {item.mapping}" title={m.title}><svg aria-hidden="true"><use href={item.mapping === 'exact' ? '#g-check' : item.mapping === 'dates' ? '#g-pin' : '#g-muted'} /></svg>{m.label}</span>
            </div>
            {#if nameErr}<p id="ic-name-err-{item.index}" class="err" role="alert">{nameErr}</p>
            {:else if c.on && twins.has(item.index)}<p id="ic-name-err-{item.index}" class="hint" data-same-name>{twins.get(item.index)}</p>{/if}
            {#if item.words}
              <div class="summary">
                <p class="say">{item.words.sentence}</p>
                {#if item.words.last && item.rule?.repeat.kind !== 'once'}<p class="last">{item.words.last}</p>{/if}
              </div>
            {/if}
            {#if item.reason}<p class="reason"><span class="sr-only">{m.title} </span>{item.reason}</p>{/if}
            {#if ruleErr}<p class="err" role="alert">{ruleErr}</p>{/if}
            {#if item.already}
              <p class="flagline there" data-there><span class="flag there"><svg aria-hidden="true"><use href="#g-good" /></svg>Already on the station</span><span>{alreadyWords(item, c)}</span></p>
            {/if}
            {#if notice}
              <p class="flagline" data-clash><span class="flag"><svg aria-hidden="true"><use href="#g-fault" /></svg>Conflict</span><span>{notice.title}</span></p>
            {/if}
            {#if over}<p class="flagline" data-overlap><span class="flag"><svg aria-hidden="true"><use href="#g-fault" /></svg>Conflict</span><span>{over}</span></p>{/if}
            {#if item.notes.length || item.zone || item.events > 1}
              <ul class="notes">
                {#if item.events > 1}<li>Made from {eventsWords(item.events)} of the file.</li>{/if}
                {#each item.notes as n (n)}<li>{n}</li>{/each}
                {#if item.zone && !item.notes.some((n) => n.includes(item.zone ?? ''))}<li>Its times were given in <span class="num">{item.zone}</span> and are shown on the station’s clock.</li>{/if}
              </ul>
            {/if}
            {#if item.description && !no}<p class="desc" title="Its description, as the show’s">{item.description}</p>{/if}
          </div>
          <div class="when">
            {#if rs.length}
              <ol class="nd plain" aria-label="Next dates of {c.name.trim() || item.name}">
                {#each rs as r (r.on)}
                  <li class="r" class:skipped={r.state === 'skipped'} class:clash={!!r.clash}>
                    <span class="on">{r.day}</span>
                    <span class="t">{r.times}</span>
                    {#if r.state === 'skipped'}
                      <span class="st"><span class="flag skip"><svg aria-hidden="true"><use href="#g-muted" /></svg>Skipped</span></span>
                    {:else}
                      <!-- A clash every date shares is said once, beside the sentence; the rows keep the mark. -->
                      {#if r.clash}<span class="st"><span class="flag" title={r.clash}><svg aria-hidden="true"><use href="#g-fault" /></svg>Conflict</span></span>{/if}
                      {#if (r.clash && !every) || r.moved || r.clock || r.now}
                        <span class="st below">
                          {#if r.now}<span class="tag">under way now</span>{/if}
                          {#if r.moved}<span class="flag moved"><svg aria-hidden="true"><use href="#g-moved" /></svg>Moved · {r.moved}</span>{/if}
                          {#if r.clash && !every}<span class="why">{r.clash}</span>{/if}
                          {#if r.clock}<span class="tag">{r.clock}</span>{/if}
                        </span>
                      {/if}
                    {/if}
                  </li>
                {/each}
              </ol>
              {#if after}<p class="after">{after}</p>{/if}
            {:else if !no}
              <p class="after">It has no dates still to come.</p>
            {/if}
          </div>
        </li>
      {/each}
    </ol>
    </div>
    <div class="foot2">
      <p class="hint">Each show is made at Solo, with no episode drafted and no host. They are made together, or not at all.</p>
      {#if problem}<p class="err" role="alert" data-testid="ic-problem">{problem}</p>{/if}
      {#if refusal && refusal.index === null}<p class="err" role="alert" data-testid="ic-refusal">{refusal.message}</p>{/if}
      {#if refusal && refusal.index !== null}<p class="err" role="alert" data-testid="ic-refusal">Nothing was made. See <b>{choices[refusal.index]?.name.trim() || `show ${refusal.index + 1}`}</b> above.</p>{/if}
      <div class="acts">
        <button type="button" class="btn quiet left" onclick={leave} disabled={making}>Cancel</button>
        <button type="button" class="btn quiet" onclick={() => input?.click()} disabled={making || reading} aria-busy={reading || undefined}>{reading ? 'Reading the file…' : 'Choose another file…'}</button>
        <button id="ic-make" type="button" class="btn primary" onclick={make} disabled={making || !!held} title={held || undefined} aria-busy={making || undefined}>{making ? 'Making the shows…' : picked.length ? makeLabel(picked.length) : 'Make the shows'}</button>
      </div>
    </div>
  {:else if made}
    <div class="scroll done" use:edges>
      <p class="lead"><svg class="g-good" aria-hidden="true"><use href="#g-good" /></svg><span><strong data-testid="ic-made">{madeWords(made.made.length)}</strong></span></p>
      <ol class="madelist">
        {#each made.made as s (s.slug)}
          <li>
            <i class="sw" style="--sw: var(--cart-{s.colour_slot})" aria-hidden="true"></i>
            <span class="mn"><b class="ink">{s.name}</b> <span class="num path">/{s.slug}</span></span>
            <span class="ms">{s.rule_words?.short ?? 'No dates'} · {nextWords(s.next_on?.on)}</span>
            <button type="button" class="act2" onclick={() => open(s.slug)} aria-label="Open {s.name}">Open</button>
          </li>
        {/each}
      </ol>
      {#if made.warnings.length}
        <div class="warn" role="status" data-testid="ic-warnings">
          <div class="top"><svg aria-hidden="true"><use href="#g-fault" /></svg><span>{made.warnings.length === 1 ? 'One of them overlaps another show.' : `${made.warnings.length} of them overlap other shows.`}</span></div>
          <ul>{#each made.warnings as w (w.show)}<li>{warningWords(w)}</li>{/each}</ul>
          <span>{CLASH_COST} Each show’s <b class="ink">Dates</b> list the clashes, with Skip and Move… on every row.</span>
        </div>
      {/if}
      <ul>
        <li>Their dates wait in the week as dotted blocks. Select one to draft the episode.</li>
        <li><b class="ink">Each starts at Solo.</b> Move a show up a step whenever you like, in its editor or at the top of the studio.</li>
        <li>Hosts, the card and the presenter’s details are in each show’s editor.</li>
      </ul>
    </div>
    <div class="foot2">
      <div class="acts">
        <button type="button" class="btn primary" onclick={() => dialogEl?.close()}>Done <kbd>Esc</kbd></button>
      </div>
    </div>
  {/if}
  <input bind:this={input} type="file" accept=".ics,.ical,text/calendar" aria-label="Choose a calendar file (.ics)" onchange={choose} hidden />
</dialog>

{#if discarding}
  <dialog bind:this={confirmEl} class="wz confirm" role="alertdialog" aria-labelledby="ic-discard-h" aria-describedby="ic-discard-p" oncancel={(e) => { e.preventDefault(); keepOn(); }}>
    <div class="head">
      <h2 id="ic-discard-h">Discard this import?</h2>
      <p id="ic-discard-p" class="small">Nothing has been made. The names and ticks you changed are not kept.</p>
    </div>
    <div class="acts">
      <button type="button" class="btn danger-hollow" onclick={() => { discarding = false; dialogEl?.close(); }}>Discard</button>
      <!-- svelte-ignore a11y_autofocus -->
      <button type="button" class="btn primary" autofocus onclick={keepOn}>Keep going</button>
    </div>
  </dialog>
{/if}
