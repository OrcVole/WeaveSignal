<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The sandbox's guide (ADR-0212, ADR-0222): a bar over the studio app's frame that walks a
  // visitor through one scenario, one step at a time, read from the simulated station; the
  // stand-in for the browser tab a DJ really speaks through (the studio app does not join the
  // room yet, ADR-0013); the switch between what the studio hears and what listeners hear; a
  // caption for every line said; and the cards around a scenario: the chooser, the start card
  // (what you will learn, whose press starts the sound), and the end card (what you did).
  //
  // The scenarios follow the four steps a show can run at (ADR-0271): Learn, a ladder of the
  // four at the start of the bar (`sandbox.steps`), opens the chooser at a step, and the chooser
  // is four rungs, Solo at the top, each with its scenarios.
  import { begin, choose, hear, opened, pull, sandbox, scenario, setMic, station } from './sandbox.svelte.ts';
  import { RUNG_HEAD, SCENARIOS, rungs, type GuideFacts, type Step } from './scenarios/index.ts';
  import type { ShowStep } from '../lib/api.ts';
  import { STEPS, STEP_WORD } from '../lib/steps.ts';
  import Pips from '../lib/Pips.svelte';
  import { firstName } from '../lib/format.ts';
  import { say } from '../lib/studio.svelte.ts';

  // A laptop's F1 to F6 are often mute and volume keys without Fn: the guide's "press F2"
  // turned the visitor's volume down instead of firing a bed (2026-11-10). When a volume key
  // reaches the page, say so once a minute at most.
  let volumeHintAt = 0;
  function volumeKey(e: KeyboardEvent) {
    if (!/^AudioVolume(Down|Up|Mute)$/.test(e.key) || !sandbox.started) return;
    if (Date.now() - volumeHintAt < 60_000) return;
    volumeHintAt = Date.now();
    say('warning', 'That was your volume key', 'On a laptop, hold Fn with F1 to F6 to fire a cart, or click the cart on screen.', 9000);
  }

  let dumpedHeard = $state(false);
  let listened = $state(false);
  /** The end card was closed, or the scenario was left running after it. */
  let endClosed = $state(false);
  /** The chooser's list, or the chosen scenario's start card. */
  let picked: string | null = $state(opened ? scenario.id : null);
  let dialog: HTMLElement | null = $state(null);

  const facts = $derived<GuideFacts>({ hear: sandbox.hear, listened, heardListenersAfterDump: dumpedHeard });

  const steps = $derived.by((): Step[] => {
    void sandbox.version;
    return scenario.steps(station, facts);
  });

  $effect(() => {
    if (sandbox.hear === 'listeners') listened = true;
    if (sandbox.hear === 'listeners' && station.delay.dumps > 0) dumpedHeard = true;
  });

  const at = $derived(steps.findIndex((s) => !s.done));
  const step = $derived(at < 0 ? null : steps[at]);
  const finished = $derived(sandbox.started && at < 0);
  const did = $derived.by(() => {
    void sandbox.version;
    return finished ? scenario.did(station, facts) : [];
  });

  // What the stand-in tab shows: whose tab, and what it is doing.
  const you = $derived.by(() => {
    void sandbox.version;
    return station.person(scenario.you.identity) ?? null;
  });
  const talkingTo = $derived.by(() => {
    void sandbox.version;
    const to = you?.talk_to;
    const p = to ? station.person(to) : null;
    return p ? firstName(p.name, p.identity) : null;
  });
  const tabSays = $derived.by(() => {
    void sandbox.version;
    if (!station.mic) return 'Mic off';
    if (talkingTo) return `Talking to ${talkingTo}`;
    if (you?.on_air && !you.mute) return station.listenersHearDesk() ? 'Live' : 'Open to the room';
    if (you?.cue) return 'In your own cue';
    return 'Mic on';
  });

  // The caption: who is speaking, where, and what.
  const caption = $derived.by(() => {
    void sandbox.version;
    const sp = station.speaking;
    if (!sp || !sandbox.started) return null;
    const p = station.person(sp.line.who);
    const who = p ? firstName(p.name, p.identity) : sp.line.who;
    return { who, where: sp.line.heard === 'booth' ? 'in the booth' : '', text: sp.line.text, recorded: sp.line.clip !== false };
  });

  const showCards = $derived(!sandbox.started || sandbox.chooser);
  const showEnd = $derived(finished && !endClosed && !sandbox.chooser);
  const pickedScenario = $derived(SCENARIOS.find((s) => s.id === picked) ?? null);
  const ladder = rungs();
  /** The step lit on the ladder: the running scenario's. */
  const lit = $derived(sandbox.started ? scenario.step : null);

  /** Learn: a step on the ladder opens the chooser at that rung. */
  function openRung(step: ShowStep) {
    sandbox.rung = step;
    if (sandbox.started) sandbox.chooser = true;
    else picked = null;
  }

  // A card takes the keyboard when it opens or changes: its first button (the first scenario
  // of the rung asked for, on the chooser), so keys do not reach the desk behind it.
  $effect(() => {
    void picked;
    void sandbox.chooser;
    const rung = sandbox.rung;
    if (!dialog) return;
    const b = (rung ? dialog.querySelector<HTMLElement>(`[data-rung="${rung}"] .pick`) : null) ?? dialog.querySelector<HTMLElement>('button, a');
    b?.focus();
    if (rung) b?.scrollIntoView({ block: 'nearest' });
  });

  // A card owns the keyboard while it is up (keymap rule 5): no key reaches the desk behind
  // it, so pressing 1 on the chooser picks nothing on the stage. Tab, Enter and Space keep
  // their defaults, so focus moves and buttons press.
  function onDialogKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      if (sandbox.chooser) sandbox.chooser = false;
      else if (showEnd) endClosed = true;
      else if (picked && !opened) picked = null;
    }
    e.stopPropagation();
  }
</script>

<svelte:window onkeydown={volumeKey} />

<section class="guide" data-panel="sandbox.guide" aria-label="Try the desk">
  <a class="home" href="https://weavesignal.top/try/" title="Back to weavesignal.top">
    <span class="badge">Sandbox</span>
    <span class="sr-only">Back to the WeaveSignal website</span>
  </a>
  <div class="hear steps" data-panel="sandbox.steps" role="group" aria-label="Scenarios by step">
    <span class="caps">Learn</span>
    {#each STEPS as s (s)}
      <button type="button" aria-pressed={s === lit} title="{STEP_WORD[s]}: {RUNG_HEAD[s].toLowerCase()}. Opens its scenarios." onclick={() => openRung(s)}>{STEP_WORD[s]}</button>
    {/each}
  </div>
  <div class="step" aria-live="polite">
    {#if step}
      <span class="count num">{at + 1}<span class="of">/{steps.length}</span></span>
      <p>
        {#each step.say as part, i (i)}{#if typeof part === 'string'}{part}{:else}<kbd>{part.key}</kbd>{/if}{/each}
      </p>
      {#if step.action}<button type="button" class="btn sm" onclick={() => pull(step.action!.run)}>{step.action.label}</button>{/if}
    {:else if finished}
      <span class="count num">✓</span>
      <p>
        {#if scenario.id === 'show'}
          That was the show. Everything still answers: carts on <kbd>Ctrl+3</kbd>, mutes on <kbd>Z</kbd>–<kbd>B</kbd>, talkback on
          <kbd>A</kbd>–<kbd>G</kbd> held. Reload to start again, or <a href="https://weavesignal.top/run/">run your own station</a>.
        {:else}
          Done: {scenario.title}. Everything still answers; keep pressing, or pick another scenario.
        {/if}
      </p>
      {#if endClosed}<button type="button" class="btn sm" onclick={() => (endClosed = false)}>What you did</button>{/if}
    {:else}
      <p class="ink2">{scenario.title}</p>
    {/if}
  </div>
  <div class="tab" data-panel="sandbox.tab" role="group" aria-label="Your browser tab" class:off={!station.mic} class:talk={!!talkingTo}>
    <svg class="win" aria-hidden="true"><use href="#g-browser" /></svg>
    <span class="tab-text">
      <span class="caps">Your browser tab</span>
      <span class="tab-line"><b>{firstName(scenario.you.name, scenario.you.identity)}</b> in the room · {tabSays}</span>
    </span>
    <button type="button" class="btn sm" aria-pressed={!station.mic} onclick={() => setMic(!station.mic)} title={station.mic ? 'Mute the tab’s microphone, as its own button would' : 'Unmute the tab’s microphone'}>
      <svg aria-hidden="true"><use href={station.mic ? '#g-mic' : '#g-muted'} /></svg>{station.mic ? 'Mute' : 'Unmute'}
    </button>
  </div>
  <div class="hear" role="group" aria-label="Hear">
    <span class="caps">Hear</span>
    <button type="button" aria-pressed={sandbox.hear === 'studio'} onclick={() => hear('studio')} title="What the presenter hears: everyone open, cue and the caller's booth">Studio</button>
    <button type="button" aria-pressed={sandbox.hear === 'listeners'} onclick={() => hear('listeners')} title="What listeners hear: the programme, behind the delay">Listeners</button>
  </div>
</section>

<!-- The caption row: its own row under the bar, so it never covers a control and nothing moves
     when someone speaks. -->
<div class="caption" data-panel="sandbox.caption" aria-live="polite" class:quiet={!caption}>
  {#if caption}
    <span class="cap-who">{caption.who}{caption.where ? ` · ${caption.where}` : ''}:</span>
    <span class="cap-text">{caption.text}</span>
    {#if !caption.recorded}<span class="cap-note">· voice not recorded yet</span>{/if}
  {:else if sandbox.started}
    <span class="cap-text muted">Captions: who is speaking, and what they say.</span>
  {/if}
</div>

{#if showCards}
  <div class="start" role="dialog" aria-modal="true" aria-labelledby="sandbox-title" tabindex="-1" bind:this={dialog} onkeydown={onDialogKey}>
    {#if pickedScenario && !sandbox.chooser}
      <!-- The start card: what you will learn, and the press that starts the sound. -->
      <div class="card" data-panel="sandbox.start">
        <p class="caps">WeaveSignal · Try the desk · {pickedScenario.minutes} min</p>
        <h1 id="sandbox-title">{pickedScenario.id === 'show' ? 'You are presenting The Morning Kettle on Kettle FM' : pickedScenario.title}</h1>
        {#if pickedScenario.id === 'show'}
          <p class="ink2">
            This is the studio app itself, running in your browser against a station simulated in the page. Gregor, Shona and the callers are
            pre-recorded with designed voices; nothing you press reaches a real station or anyone else.
          </p>
        {:else}
          <p class="ink2">
            You are {pickedScenario.you.name}, {pickedScenario.you.role === 'screener' ? 'screening the phones' : 'at the desk'} on Kettle FM. The
            studio app itself, in your browser, against a station simulated in the page; nothing you press reaches anyone.
          </p>
        {/if}
        <p class="caps">What you will learn</p>
        <ul class="learn">
          {#each pickedScenario.learn as line, i (i)}<li>{line}</li>{/each}
        </ul>
        <p class="ink2 two">
          <svg aria-hidden="true"><use href="#g-browser" /></svg>
          On a real station you run two windows: the studio app, which drives the desk, and a browser tab you speak through (the app does not
          join the room itself yet). The small panel in the bar stands in for that tab.
        </p>
        <p class="ink2">Sound on, headphones if you have them. The bar at the top says what to press next.</p>
        <p class="ink2">On a laptop, F1 to F6 may be your computer's volume keys: hold <kbd>Fn</kbd> with them, or click the carts on screen.</p>
        {#if sandbox.error}<p class="error" role="alert">The sound would not start: {sandbox.error}</p>{/if}
        <div class="row">
          <button type="button" class="btn primary big" onclick={() => void begin()} disabled={sandbox.loading}>
            {sandbox.loading ? 'Loading the voices…' : 'Start with sound'}
          </button>
          <button type="button" class="btn quiet" onclick={() => (picked = null)}>Other scenarios</button>
          <a class="btn quiet" href="https://weavesignal.top/try/">Back to the website</a>
        </div>
        <p class="small">Kettle FM and its people are fictional: sample names, designed voices, the repository's licence-clean sounds.</p>
      </div>
    {:else}
      <!-- The chooser: four rungs, Solo at the top, each with its scenarios (ADR-0222, ADR-0271). -->
      <div class="card wide ladder" data-panel="sandbox.chooser">
        <p class="caps">WeaveSignal · Try the desk</p>
        <h1 id="sandbox-title">{sandbox.started ? 'Pick another scenario' : 'Pick a scenario'}</h1>
        <p class="ink2">
          The studio app, in your browser, against a station simulated in the page. The scenarios follow the four steps a show can run at: at
          Solo the desk shows only what one DJ needs, and each step adds one kind of person. Start at the top, or go straight to the whole show.
        </p>
        {#each ladder as rung (rung.step)}
          <section class="rung" data-rung={rung.step} aria-labelledby="rung-{rung.step}">
            <div class="runghead">
              <b id="rung-{rung.step}"><Pips step={rung.step} />{STEP_WORD[rung.step]}</b>
              <span>{rung.head}</span>
            </div>
            <ol class="list" aria-label="{STEP_WORD[rung.step]} scenarios">
              {#each rung.scenarios as { n, scenario: sc } (sc.id)}
                <li>
                  <button type="button" class="pick" class:current={sandbox.started && sc.id === scenario.id} onclick={() => (sandbox.started ? choose(sc.id) : (picked = sc.id))}>
                    <span class="n num">{n}</span>
                    <span class="pick-text">
                      <span class="pick-title"
                        >{sc.title}{#if sandbox.started && sc.id === scenario.id}<span class="now">running</span>{:else if sc.id === 'show'}<span class="now">the video’s show</span>{/if}</span
                      >
                      <span class="pick-blurb">{sc.blurb}</span>
                    </span>
                    <span class="min num">{sc.minutes} min</span>
                  </button>
                </li>
              {/each}
            </ol>
          </section>
        {/each}
        <div class="row">
          {#if sandbox.started}
            <button type="button" class="btn" onclick={() => (sandbox.chooser = false)}>Back to {scenario.title}</button>
          {/if}
          <a class="btn quiet" href="https://weavesignal.top/try/">Back to the website</a>
          <span class="small">Picking a scenario starts the station afresh, at that scenario’s step. Kettle FM and its people are fictional.</span>
        </div>
      </div>
    {/if}
  </div>
{/if}

{#if showEnd}
  <div class="start" role="dialog" aria-modal="true" aria-labelledby="sandbox-end-title" tabindex="-1" bind:this={dialog} onkeydown={onDialogKey}>
    <div class="card" data-panel="sandbox.end">
      <p class="caps">WeaveSignal · Try the desk</p>
      <h1 id="sandbox-end-title">{scenario.id === 'show' ? 'That was The Morning Kettle' : `Done: ${scenario.title}`}</h1>
      <p class="caps">What you did</p>
      <ul class="learn">
        {#each did as line, i (i)}<li>{line}</li>{/each}
      </ul>
      <div class="row">
        <button type="button" class="btn primary" onclick={() => (sandbox.chooser = true)}>Other scenarios</button>
        <button type="button" class="btn" onclick={() => choose(scenario.id)}>Run it again</button>
        <button type="button" class="btn quiet" onclick={() => (endClosed = true)}>Keep playing</button>
      </div>
      <p class="small">The real thing runs on a server of your own: <a href="https://weavesignal.top/run/">weavesignal.top/run</a>.</p>
    </div>
  </div>
{/if}

<style>
  .guide {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    min-height: 2.75rem;
    padding: var(--s-1) var(--s-3);
    background: color-mix(in srgb, var(--accent) 14%, var(--surface));
    border-bottom: 1px solid color-mix(in srgb, var(--accent) 45%, var(--axis));
  }
  .home {
    text-decoration: none;
    flex: none;
  }
  .badge {
    display: inline-flex;
    align-items: center;
    height: 1.5rem;
    padding: 0 var(--s-2);
    border-radius: var(--r-pill);
    background: var(--accent);
    color: var(--accent-ink);
    font-size: var(--fs-12);
    font-weight: 700;
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
  }
  .step {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--s-3);
  }
  .step p {
    color: var(--ink);
    font-size: var(--fs-14);
    line-height: 1.5;
  }
  .step a {
    color: var(--ink);
  }
  .count {
    flex: none;
    font-weight: 700;
    color: var(--ink);
    font-size: var(--fs-14);
  }
  .of {
    color: var(--muted);
    font-weight: 600;
  }

  /* The stand-in for the DJ's own browser tab: a small window beside the bar's controls. */
  .tab {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: var(--s-2);
    min-height: 2.1rem;
    padding: 0 var(--s-2) 0 var(--s-2);
    border: 1px solid var(--axis);
    border-radius: var(--r-2);
    background: var(--surface);
    color: var(--ink-2);
  }
  .tab.off {
    border-color: var(--status-warning);
  }
  .tab.talk {
    border-color: var(--live-talk);
  }
  .tab .win {
    width: 16px;
    height: 16px;
    color: var(--muted);
    flex: none;
  }
  .tab-text {
    display: grid;
    line-height: 1.15;
  }
  .tab-text .caps {
    font-size: 10px;
  }
  .tab-line {
    font-size: var(--fs-13);
    color: var(--ink-2);
    white-space: nowrap;
  }
  .tab-line b {
    color: var(--ink);
  }
  .tab.off .tab-line {
    color: var(--status-warning);
  }
  .tab .btn svg {
    width: 14px;
    height: 14px;
  }
  .tab .btn[aria-pressed='true'] {
    background: var(--status-warning);
    color: var(--status-ink-on-warning);
    border-color: transparent;
  }

  .hear {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: var(--s-2);
  }
  .hear button {
    appearance: none;
    border: 1px solid var(--axis);
    background: var(--surface);
    color: var(--ink-2);
    font-size: var(--fs-13);
    font-weight: 600;
    padding: 0 var(--s-3);
    min-height: 1.75rem;
    cursor: pointer;
  }
  .hear button:first-of-type {
    border-radius: var(--r-pill) 0 0 var(--r-pill);
  }
  .hear button + button {
    border-left: 0;
  }
  .hear button:last-of-type {
    border-radius: 0 var(--r-pill) var(--r-pill) 0;
  }
  .hear button[aria-pressed='true'] {
    background: var(--accent);
    color: var(--accent-ink);
    border-color: var(--accent);
  }

  /* The caption row: one line under the bar, kept even when nobody speaks. */
  .caption {
    min-height: 1.6rem;
    display: flex;
    align-items: baseline;
    gap: 0 var(--s-2);
    padding: 0.15rem var(--s-3);
    background: color-mix(in srgb, var(--accent) 6%, var(--surface));
    border-bottom: 1px solid var(--hairline);
    color: var(--ink);
    font-size: var(--fs-13);
    line-height: 1.4;
    white-space: nowrap;
    overflow: hidden;
  }
  .cap-who {
    flex: none;
    font-weight: 700;
    color: var(--accent);
  }
  .cap-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cap-text.muted {
    color: var(--muted);
  }
  .cap-note {
    flex: none;
    font-size: var(--fs-12);
    color: var(--muted);
    font-style: italic;
  }

  .start {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: var(--s-4);
    background: color-mix(in srgb, var(--plane) 78%, transparent);
    backdrop-filter: blur(3px);
    overflow: auto;
  }
  .card {
    max-width: 34rem;
    display: grid;
    gap: var(--s-3);
    padding: var(--s-5, 1.5rem);
    background: var(--surface);
    border: 1px solid var(--axis);
    border-radius: var(--r-3, 0.75rem);
    box-shadow: var(--e-3);
  }
  .card.wide {
    max-width: 46rem;
    width: 100%;
    gap: var(--s-2);
  }
  .card h1 {
    font-size: var(--fs-20, 1.25rem);
  }
  .card p {
    line-height: 1.55;
  }
  .learn {
    margin: 0;
    padding-left: 1.2rem;
    display: grid;
    gap: var(--s-1);
    color: var(--ink);
    line-height: 1.5;
  }
  .two {
    display: flex;
    gap: var(--s-2);
    align-items: flex-start;
    padding: var(--s-2) var(--s-3);
    border: 1px solid var(--axis);
    border-radius: var(--r-2);
    background: var(--surface-2);
    font-size: var(--fs-13);
  }
  .two svg {
    width: 18px;
    height: 18px;
    flex: none;
    margin-top: 2px;
    color: var(--muted);
  }
  .row {
    display: flex;
    gap: var(--s-2);
    align-items: center;
    flex-wrap: wrap;
  }
  .row a {
    text-decoration: none;
  }
  /* The word in ink, the colour on the edge: critical as text is under 4.5:1 on a card. */
  .error {
    color: var(--ink);
    border-left: 3px solid var(--status-critical);
    padding-left: var(--s-2);
  }

  /* The chooser's list: one button a scenario, the whole row pressable. */
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.3rem;
  }
  .pick {
    appearance: none;
    width: 100%;
    display: grid;
    grid-template-columns: 2rem minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--s-3);
    padding: 0.45rem var(--s-3);
    border: 1px solid var(--axis);
    border-radius: var(--r-2);
    background: var(--surface-2);
    color: var(--ink);
    text-align: left;
    cursor: pointer;
    font: inherit;
  }
  .pick:hover {
    background: var(--surface-3);
  }
  .pick.current {
    border-color: var(--accent);
  }
  .pick .n {
    font-weight: 700;
    color: var(--muted);
  }
  .pick-text {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .pick-title {
    font-weight: 700;
    display: flex;
    align-items: center;
    gap: var(--s-2);
  }
  .now {
    font-size: var(--fs-12);
    font-weight: 600;
    color: var(--accent);
    text-transform: uppercase;
    letter-spacing: var(--tracking-caps);
  }
  .pick-blurb {
    color: var(--ink-2);
    font-size: var(--fs-13);
    line-height: 1.4;
  }
  .pick .min {
    color: var(--muted);
    font-size: var(--fs-13);
    white-space: nowrap;
  }

  /* The chooser as four rungs (ADR-0271): the step's pips and word at the left, its scenarios beside it. */
  .card.wide.ladder {
    max-width: 76rem;
    gap: var(--s-3);
  }
  .ladder .rung {
    display: grid;
    grid-template-columns: 9.5rem minmax(0, 1fr);
    gap: var(--s-3);
    padding-top: var(--s-2);
    border-top: 1px solid var(--hairline);
    align-items: start;
  }
  .ladder .runghead {
    display: grid;
    gap: 2px;
    padding-top: 0.45rem;
  }
  .ladder .runghead b {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    font-size: var(--fs-16);
  }
  .ladder .runghead span {
    color: var(--ink-2);
    font-size: var(--fs-13);
    line-height: 1.4;
  }
  .ladder .rung .list {
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--s-2);
  }
  .ladder .pick {
    align-items: start;
    height: 100%;
    grid-template-columns: 1.25rem minmax(0, 1fr) auto;
    gap: var(--s-2);
  }
  .ladder .pick-title {
    flex-wrap: wrap;
    gap: 0 var(--s-2);
  }
  .ladder .row .small {
    flex: 1 1 16rem;
  }
  /* A narrow window (a phone held upright): the rungs stack, one scenario a row. */
  @media (max-width: 60rem) {
    .ladder .rung {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--s-2);
    }
    .ladder .runghead {
      padding-top: 0;
    }
    .ladder .rung .list {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
