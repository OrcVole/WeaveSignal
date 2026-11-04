<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The sandbox's guide (ADR-0212): a bar over the studio app's frame that walks a visitor
  // through a short show on Kettle FM, one step at a time, read from the simulated station; the
  // switch between what the studio hears and what listeners hear; and the first screen, whose
  // press starts the sound (a browser plays nothing before one).
  import { slotKeys } from '../../../../packages/ui/keymap.ts';
  import { begin, endShow, hear, pullPlayout, sandbox, station } from './sandbox.svelte.ts';
  import { DELAY_TARGET_MS } from './station.ts';

  type Part = string | { key: string };
  interface Step {
    id: string;
    say: Part[];
    done: boolean;
    action?: { label: string; run: () => void };
  }

  const k = (key: string) => ({ key });
  const slotOf = (id: string) => station.slots.indexOf(id) + 1;

  let dumpedHeard = $state(false);

  const steps = $derived.by((): Step[] => {
    void sandbox.version;
    const s = station;
    const niall = s.callerState('niall');
    const jessie = s.callerState('jessie');
    const nSlot = slotOf('niall') || 4;
    const jSlot = slotOf('jessie') || 3;
    const jessieQueued = s.callers.findIndex((c) => c.identity === 'jessie') + 1 || 2;
    return [
      { id: 'on', say: ['Put Eilidh on air: press ', k('1'), '. You are Eilidh, the presenter, at the desk.'], done: !!s.person('eilidh')?.on_air || s.done('open') },
      { id: 'open', say: ['Press ', k('F1'), ' for the station jingle; Eilidh opens the show when it ends.'], done: s.done('open') || !!s.beat?.beat.id.startsWith('open') },
      { id: 'gregor', say: ['Gregor is waiting on strip 2. Press ', k('2'), ' to bring him on air.'], done: s.done('gregor') || (s.open('gregor') && s.done('open')) },
      {
        id: 'screen',
        say: ['A caller is ringing. ', k('Ctrl+2'), ' opens the Screener; press ', k('1'), ' to pick Niall, then ', k('T'), ' to answer as Shona. ', k('F2'), ' plays a music bed meanwhile.'],
        done: niall !== null && niall !== 'waiting',
      },
      { id: 'send', say: ['Listen to Shona screen him. Write a line for the presenter if you like, then ', k('P'), ' sends Niall to the desk.'], done: niall === 'ready' || !!s.person('niall')?.on_air },
      {
        id: 'air',
        say: [k('Ctrl+1'), ` back to the desk: Niall is on strip ${nSlot}. `, k(slotKeys(nSlot).cue), ' cues him (only you hear), ', k(slotKeys(nSlot).on), ' puts him on air.'],
        done: s.done('niall-air') || !!s.person('niall')?.on_air,
      },
      {
        id: 'jessie',
        say: [`When Niall has finished, ${slotKeys(nSlot).on} takes him off. Then screen Jessie the same way: `, k('Ctrl+2'), ', ', k(String(jessieQueued)), ', ', k('T'), ', ', k('P'), '.'],
        done: jessie === 'ready' || !!s.person('jessie')?.on_air,
      },
      {
        id: 'swear',
        say: [k('Ctrl+1'), `, then `, k(slotKeys(jSlot).on), ` puts Jessie on air. She is about to swear: press `, k('Delete'), ' twice to dump the profanity delay.'],
        done: s.delay.dumps > 0,
      },
      {
        id: 'listen',
        say: [`Switch Hear to Listeners. They never heard her: they were ${DELAY_TARGET_MS / 1000} s behind, and the dump threw those seconds away. The delay builds back on its own.`],
        done: dumpedHeard,
      },
      {
        id: 'down',
        say: ['The safety net: pull playout’s plug for 15 seconds. The lamp says PLAYOUT DOWN, open faders say OPEN, and listeners hear the fallback loop.'],
        done: s.done('down') || !s.playout,
        action: { label: 'Pull the plug', run: pullPlayout },
      },
      { id: 'end', say: ['End the show when you are ready. Eilidh says goodbye.'], done: s.done('end') || s.ended, action: { label: 'End the show', run: endShow } },
    ];
  });

  $effect(() => {
    if (sandbox.hear === 'listeners' && station.delay.dumps > 0) dumpedHeard = true;
  });

  const at = $derived(steps.findIndex((s) => !s.done));
  const step = $derived(at < 0 ? null : steps[at]);
</script>

<section class="guide" data-panel="sandbox.guide" aria-label="Try the desk">
  <a class="home" href="https://weavesignal.top/try/" title="Back to weavesignal.top">
    <span class="badge">Sandbox</span>
    <span class="sr-only">Back to the WeaveSignal website</span>
  </a>
  <div class="step" aria-live="polite">
    {#if step}
      <span class="count num">{at + 1}<span class="of">/{steps.length}</span></span>
      <p>
        {#each step.say as part, i (i)}{#if typeof part === 'string'}{part}{:else}<kbd>{part.key}</kbd>{/if}{/each}
      </p>
      {#if step.action}<button type="button" class="btn sm" onclick={step.action.run}>{step.action.label}</button>{/if}
    {:else}
      <span class="count num">✓</span>
      <p>
        That was the show. Everything still answers: carts on <kbd>Ctrl+3</kbd>, mutes on <kbd>Z</kbd>–<kbd>B</kbd>, talkback on
        <kbd>A</kbd>–<kbd>G</kbd> held. Reload to start again, or <a href="https://weavesignal.top/run/">run your own station</a>.
      </p>
    {/if}
  </div>
  <div class="hear" role="group" aria-label="Hear">
    <span class="caps">Hear</span>
    <button type="button" aria-pressed={sandbox.hear === 'studio'} onclick={() => hear('studio')} title="What the presenter hears: everyone open, cue and the caller's booth">Studio</button>
    <button type="button" aria-pressed={sandbox.hear === 'listeners'} onclick={() => hear('listeners')} title="What listeners hear: the programme, behind the delay">Listeners</button>
  </div>
</section>

{#if !sandbox.started}
  <div class="start" role="dialog" aria-modal="true" aria-labelledby="sandbox-title">
    <div class="card">
      <p class="caps">WeaveSignal · Try the desk</p>
      <h1 id="sandbox-title">You are presenting The Morning Kettle on Kettle FM</h1>
      <p class="ink2">
        This is the studio app itself, running in your browser against a station simulated in the page. Gregor, Shona and the callers are
        pre-recorded with designed voices; nothing you press reaches a real station or anyone else.
      </p>
      <p class="ink2">Sound on, headphones if you have them. The bar at the top says what to press next.</p>
      {#if sandbox.error}<p class="error" role="alert">The sound would not start: {sandbox.error}</p>{/if}
      <div class="row">
        <button type="button" class="btn primary big" onclick={() => void begin()} disabled={sandbox.loading}>
          {sandbox.loading ? 'Loading the voices…' : 'Start with sound'}
        </button>
        <a class="btn quiet" href="https://weavesignal.top/try/">Back to the website</a>
      </div>
      <p class="small">Kettle FM and its people are fictional: sample names, designed voices, the repository's licence-clean sounds.</p>
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
  .hear button:last-of-type {
    border-radius: 0 var(--r-pill) var(--r-pill) 0;
    border-left: 0;
  }
  .hear button[aria-pressed='true'] {
    background: var(--accent);
    color: var(--accent-ink);
    border-color: var(--accent);
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
  }
  .card {
    max-width: 34rem;
    display: grid;
    gap: var(--s-3);
    padding: var(--s-5, 1.5rem);
    background: var(--surface);
    border: 1px solid var(--axis);
    border-radius: var(--r-3, 0.75rem);
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.35);
  }
  .card h1 {
    font-size: var(--fs-20, 1.25rem);
  }
  .card p {
    line-height: 1.55;
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
  .error {
    color: var(--status-critical);
  }
</style>
