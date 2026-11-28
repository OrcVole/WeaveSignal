<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The microphone check before joining: asks for the microphone with the join pages'
  // constraints, shows the level, and says in words whether it is heard. Once the browser has
  // given a microphone it names them all, so the check also says which one it is and, where
  // there is more than one, lets the person choose (1.3.3). The choice is kept on this device
  // and the call asks for it. The track is let go when the check ends; the call takes its own.
  import { onDestroy } from 'svelte';
  import MicChooser from './MicChooser.svelte';
  import { micProblem } from './mic.ts';
  import { type Mic, captureMic, chosen, micCapture, missingWords, nowUsing, remember, switchProblem, used, wanted } from './mic-devices.ts';
  import { Meter } from './meter.svelte.ts';
  import { MicList } from './mics.svelte.ts';

  let stream = $state<MediaStream | null>(null);
  let meter = $state<Meter | null>(null);
  let problem = $state<string | null>(null);
  /** The microphone the check is on, and a sentence about it. */
  let using = $state<Mic | null>(null);
  let note = $state<string | null>(null);
  let changing = $state(false);
  const devices = new MicList();

  /** Take a capture as the check's own: its level, its name, and the end of it if it is unplugged. */
  async function adopt(s: MediaStream) {
    const track = s.getAudioTracks()[0];
    meter?.stop();
    for (const t of stream?.getTracks() ?? []) t.stop();
    stream = s;
    meter = new Meter(track);
    track.addEventListener('ended', () => {
      if (stream !== s) return;
      const gone = using?.label;
      stop();
      problem = `${gone || 'Your microphone'} stopped. Press Check your microphone to use another.`;
    });
    await devices.refresh();
    using = devices.of(track);
    used(using);
    devices.watch();
  }

  async function check() {
    problem = null;
    note = null;
    try {
      const want = wanted(await devices.refresh());
      await adopt(await captureMic(want));
      note = missingWords(chosen(), using, devices.list);
    } catch (e) {
      problem = micProblem(e);
    }
  }

  /** The person chose another microphone: take it first, then let the old one go, so a refusal
   *  leaves the check as it was. */
  async function choose(id: string) {
    const to = devices.list.find((m) => m.id === id);
    if (!to || changing) return;
    changing = true;
    problem = null;
    note = null;
    try {
      await adopt(await navigator.mediaDevices.getUserMedia({ audio: micCapture(to) }));
      remember(using ?? to);
      note = nowUsing(using ?? to, false);
    } catch (e) {
      problem = switchProblem(e, to, using);
    } finally {
      changing = false;
    }
  }

  function stop() {
    devices.stop();
    meter?.stop();
    meter = null;
    const s = stream;
    stream = null;
    for (const t of s?.getTracks() ?? []) t.stop();
    using = null;
    note = null;
    problem = null;
  }
  onDestroy(stop);
</script>

<div class="mic">
  {#if meter}
    <MicChooser list={devices.list} {using} busy={changing} onchoose={choose} />
    <span class="level" role="img" aria-label={meter.heard ? 'Your microphone is working' : 'Not heard yet'}><i style:width="{meter.pct}%"></i></span>
    <p class="small" role="status">
      {#if meter.heard}<svg class="g-good" aria-hidden="true"><use href="#g-good" /></svg> Working: we can hear you.{:else}Say something: the bar should move.{/if}
    </p>
    <!-- Always in the page, so a screen reader hears what is written into it. -->
    <div class="says" aria-live="polite" aria-atomic="true">{#if note}<p class="small" data-mic-note>{note}</p>{/if}</div>
    <button type="button" class="btn quiet" onclick={stop}>Done</button>
  {:else}
    <button type="button" class="btn wide" onclick={check}><svg aria-hidden="true"><use href="#g-mic" /></svg>Check your microphone</button>
  {/if}
  {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
</div>

<style>
  .mic {
    display: grid;
    gap: var(--s-2);
  }
  /* Empty, it takes no room: not even the gap. */
  .says:empty {
    margin-top: calc(-1 * var(--s-2));
  }
  .small svg {
    width: 14px;
    height: 14px;
    vertical-align: -2px;
  }
</style>
