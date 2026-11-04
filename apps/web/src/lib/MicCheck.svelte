<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The microphone check before joining: asks for the microphone with the join pages'
  // constraints, shows the level, and says in words whether it is heard. The track is let go
  // when the check ends; the call takes its own.
  import { onDestroy } from 'svelte';
  import { micProblem } from './mic.ts';
  import { Meter } from './meter.svelte.ts';
  import { voiceCapture } from './rules.ts';

  let stream = $state<MediaStream | null>(null);
  let meter = $state<Meter | null>(null);
  let problem = $state<string | null>(null);

  async function check() {
    problem = null;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: voiceCapture() });
      meter = new Meter(stream.getAudioTracks()[0]);
    } catch (e) {
      problem = micProblem(e);
    }
  }

  function stop() {
    meter?.stop();
    meter = null;
    for (const t of stream?.getTracks() ?? []) t.stop();
    stream = null;
  }
  onDestroy(stop);
</script>

<div class="mic">
  {#if meter}
    <span class="label">Microphone</span>
    <span class="level" role="img" aria-label={meter.heard ? 'Your microphone is working' : 'Not heard yet'}><i style:width="{meter.pct}%"></i></span>
    <p class="small" role="status">
      {#if meter.heard}<svg class="g-good" aria-hidden="true"><use href="#g-good" /></svg> Working: we can hear you.{:else}Say something: the bar should move.{/if}
    </p>
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
  .label {
    font-weight: 650;
  }
  .small svg {
    width: 14px;
    height: 14px;
    vertical-align: -2px;
  }
</style>
