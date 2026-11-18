<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // `monitor.listen`: the host's own return, live (the design's section 4.5). A browser plays
  // nothing without a press, so it starts with Start listening; then a level bar of what
  // arrives, a volume control, and what the mix holds. If the browser or the station refuses,
  // the button stays and the reason is under it.
  import type { MonitorView } from './api.ts';
  import type { Listen } from './listen.svelte.ts';
  import { first, listenOff, openStreamWords, standbyStreamWords } from './words.ts';

  let { view, known, listen, onstart, stream = '' }: { view: MonitorView; known: boolean; listen: Listen | null; onstart: () => void; stream?: string } = $props();

  const hearing = $derived(listen?.hearing ?? 'idle');
  const sounding = $derived(listen?.sounding ?? false);
  const cue = $derived(known && view.next_host?.cued ? first(view.next_host.name) : null);
  // The desk makes a monitor mix only for a source it holds: a test never reaches the desk.
  const atDesk = $derived(view.state === 'standby' || view.state === 'on_air' || view.state === 'open');
  const chip = $derived(
    hearing === 'starting'
      ? { cls: 'hollow hollow-standby', glyph: 'g-standby', glyphCls: '', word: 'Connecting' }
      : hearing === 'rejoining'
        ? { cls: 'hollow hollow-standby', glyph: 'g-standby', glyphCls: '', word: 'Reconnecting' }
        : sounding
          ? { cls: 'hollow hollow-good', glyph: 'g-good', glyphCls: 'g-good', word: 'Listening' }
          : { cls: 'hollow hollow-standby', glyph: 'g-standby', glyphCls: '', word: 'Listening' },
  );
  // While the host stands by or is OPEN and nobody on air is in the desk's mix (automation holds
  // the air, and automation is playout's, downstream of the desk), the mix is silent: the host
  // hears the listeners' stream instead, until they are on air. Only while the station is
  // telling the page so; the moment it stops, or the host goes on air, the stream stops.
  const streamWanted = $derived(known && (view.state === 'standby' || view.state === 'open') && !view.monitor_has_programme && stream !== '');
  // While the stream is what plays, the level bar would show the room's mix, which is silent:
  // it is left out, and the sentence under it says what is heard.
  const showLevel = $derived(!(streamWanted && hearing !== 'idle'));
  $effect(() => listen?.setStream(stream));
  $effect(() => listen?.wantStream(streamWanted));
  // Start listening is off while nothing can play: its reason is the sentence beside it, and
  // the card's own line below then does not say it twice.
  const off = $derived(hearing === 'idle' ? listenOff(view, known) : null);
  const holds = $derived(
    streamWanted
      ? view.state === 'open'
        ? openStreamWords(view.listeners_behind_ms, hearing !== 'idle')
        : standbyStreamWords(view.listeners_behind_ms, hearing !== 'idle')
      : !known || atDesk
      ? hearing === 'listening' && !sounding && known
        ? 'Waiting for the desk to send your mix. Everyone but you · cue · talkback.'
        : 'Everyone but you · cue · talkback. Live, not the stream listeners get.'
      : view.state === 'link_lost'
        ? 'Nothing plays here while the station has lost your stream.'
        : 'Nothing plays here until you are standing by. Then: everyone but you · cue · talkback, live.',
  );
  /** A press while it is off does nothing: a disabled look alone would not stop Enter or a tap. */
  function press() {
    if (!off) onstart();
  }
</script>

<section class="card" data-panel="monitor.listen" data-hearing={hearing} data-sounding={sounding} aria-labelledby="listen-h">
  <h2 id="listen-h">Your monitor</h2>
  {#if hearing === 'idle'}
    <button type="button" class="btn primary wide" data-act="listen" aria-disabled={off ? 'true' : undefined} aria-describedby={off ? 'listen-off' : undefined} onclick={press}>Start listening</button>
    {#if off}<p id="listen-off" class="small" data-says="listen-off">{off}</p>{/if}
    {#if listen?.problem}<p class="problem" role="alert" data-says="listen-problem">{listen.problem}</p>{/if}
  {:else}
    <div class="lrow">
      <span class="state {chip.cls}" role="status"><svg class={chip.glyphCls} aria-hidden="true"><use href="#{chip.glyph}" /></svg>{chip.word}</span>
      {#if showLevel}
        <span class="level" role="img" aria-label={sounding ? 'The level of your monitor' : 'No sound is arriving yet'}><i style:width="{listen?.level ?? 0}%"></i></span>
      {/if}
    </div>
    {#if listen?.needsTap || listen?.streamBlocked}
      <button type="button" class="btn primary wide" data-act="tap" onclick={() => listen?.tap()}>Tap to hear your monitor</button>
    {/if}
    <label class="vol">
      <span>Volume</span>
      <input type="range" min="0" max="100" step="1" value={listen?.volume ?? 80} style:--fill="{listen?.volume ?? 80}%" oninput={(e) => listen?.setVolume(Number(e.currentTarget.value))} />
    </label>
    {#if cue}
      <p class="talkline"><span class="state fill-cue"><svg aria-hidden="true"><use href="#g-cue" /></svg>Cue</span><span>{cue} is in your cue</span></p>
    {/if}
    {#if listen?.problem}<p class="problem" role="status" data-says="listen-problem">{listen.problem}</p>{/if}
  {/if}
  {#if !off}<p class="small">{holds}</p>{/if}
  <p class="note"><svg aria-hidden="true"><use href="#g-mic" /></svg><span>Listen on headphones, and do not send this back into your mix.</span></p>
</section>
