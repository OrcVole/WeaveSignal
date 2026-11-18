<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // One source as a tile (desk B, ADR-0061): a fixed head (slot, name, role, one state chip),
  // levels (the R128 meter with its ticks, ported from desk A, and the fader), the four keyed
  // buttons at one offset in every state, then the lines and notes. The keys belong to the
  // slot, not the place on screen (desk.md section 3, rule 4).
  //
  // A host's own studio (role `source`, ADR-0280) is drawn with the same tile; five things
  // differ (hosts-in-their-own-studios.md section 4.1): the role line says Own studio, the
  // head's quiet button is Disconnect, the second readout is the station's leveller, the lines
  // say whether their monitor link is open and how long the link has held, and Talk goes to
  // that monitor link: with it not open, Talk is off and says why.
  //
  // A source whose link is lost keeps its place (design sections 3.6 and 4.1): the tile stays
  // on the stage, dashed in the serious colour, says LINK LOST with how long and until when the
  // place is kept, and leaves when the station lets the place go. The source itself has left
  // the desk then, so the tile is drawn from `kept` and a stand-in with nothing that sounds.
  // Its own On key stays lit, and pressing it is Off air: the place is let go at once.
  // While the station counts a late host on air (section 3.5) the tile says so, with Not yet;
  // Go on air now is the handover's button and key.
  import type { Source } from '../lib/api.ts';
  import { CHIP, isOpen, listenersHear, sourceChip, type Chip, type StationMode } from '../../../../packages/ui/states.ts';
  import StateChip from '../../../../packages/ui/StateChip.svelte';
  import { slotKeys } from '../../../../packages/ui/keymap.ts';
  import { dur, firstName, meterPct, signed } from '../lib/format.ts';
  import { notYet, run } from '../lib/actions.ts';
  import { look, stationNow, studio } from '../lib/studio.svelte.ts';
  import { OFF_AIR_WHY, hurry, keptHeld, keptLeft, keptNote, type Kept } from '../lib/post.ts';
  import Hurry from './Hurry.svelte';
  import { HANDOVER_KEY, type HandoverButton } from '../lib/handover.ts';
  import { STEP_WORD, stationStep } from '../lib/steps.ts';
  import { preparing } from '../lib/prepare.svelte.ts';
  import { OFF_WHY } from '../lib/prepare.ts';
  import { DISCONNECT_WHY, MONITOR_WHY, MONITOR_WORD, levellerFigure, talkOffWhy, tileLink, tileNote, tileRole } from '../lib/hosts.ts';
  import { sched } from '../lib/schedule.svelte.ts';
  import { account } from '../lib/account.svelte.ts';
  import { LEAVE, isMine } from '../lib/invite.ts';
  import { setup } from '../lib/wizard.svelte.ts';

  interface Props {
    /** The source, or null for slot 6, playout. */
    source: Source | null;
    slot: number;
    playoutUp: boolean;
    playoutForMs: number | null;
    /** The station's mode: in auto mode an open fader says OPEN (ADR-0183). */
    mode?: StationMode | null;
    /** The handover button, on the incoming DJ's tile only (ADR-0074): the same action as ⇧⏎.
     *  It sits in the lines row, under the controls, so the four live buttons never move. */
    handover?: HandoverButton | null;
    /** The handover is drawn because everything is shown, and the show's step keeps it off. */
    handoverOff?: boolean;
    /** Its link is lost and its place is kept (ADR-0280); `source` is then the kept stand-in, or
     *  the source itself once its software has reconnected. */
    kept?: Kept | null;
    /** The late count runs for this source: when the station puts it on air, and its show key. */
    count?: { at_ms: number; key: string } | null;
    /** Someone else presents and is on air: a source that reconnects is not put back by itself. */
    othersOnAir?: boolean;
  }
  let { source, slot, playoutUp, playoutForMs, mode = null, handover = null, handoverOff = false, kept = null, count = null, othersOnAir = false }: Props = $props();

  const keys = $derived(slotKeys(slot));
  /** This window is preparing (ADR-0276): the tile shows the live desk, and its controls are off. */
  const prep = $derived(preparing());
  const prepWhy = $derived(prep ? OFF_WHY : undefined);
  const isPlayout = $derived(source === null);
  const ready = $derived(!!source?.caller?.sent && !source.caller.taken);
  /** The place is kept and nothing arrives: the stand-in's controls have nothing to act on. */
  const gone = $derived(!!kept && !kept.back);
  const chip: Chip = $derived(
    kept
      ? CHIP.linkLost
      : source
      ? sourceChip({ on_air: source.on_air, mute: source.mute, cue: source.cue, ready }, playoutUp, mode)
      : playoutUp
        ? CHIP.standby
        : CHIP.playoutDown,
  );
  const open = $derived(!!source && isOpen(source));
  /** Listeners hear the room: playout is up and automation does not hold the air (ADR-0183). */
  const heard = $derived(listenersHear(playoutUp, mode));
  const talking = $derived(!!source && studio.talking.includes(source.identity));
  const ROLE: Record<string, [string, string]> = {
    presenter: ['g-mic', 'Presenter'],
    dj: ['g-disc', 'Remote DJ'],
    guest: ['g-browser', 'Guest'],
    caller: ['g-phone', 'Caller'],
    source: ['g-disc', 'Own studio'],
  };
  /** A host's own studio: what the desk says of how it came in; null for a person in the room. */
  const own = $derived(source?.role === 'source' ? (source.source ?? null) : null);
  const isSource = $derived(source?.role === 'source');
  /** The show's name, where this window has read the shows; else its slug. */
  const ownShow = $derived(own ? ([...sched.shows, ...setup.shows].find((x) => x.slug === own.show)?.name ?? own.show) : '');
  const ownRole = $derived(own ? tileRole(own, ownShow) : null);
  const ownNote = $derived(own && source ? tileNote(own, source.name.trim().split(/\s+/)[0] || 'them', stationStep(studio.view)) : null);
  /** The leveller on the fader's track: nothing at the middle, its whole reach at either end. */
  const levPct = $derived(own ? 50 + (Math.max(-12, Math.min(12, own.leveller_db)) / 12) * 50 : 50);
  const armedDisc = $derived(!!source && studio.armed?.what === 'disconnect' && studio.armed.identity === source.identity);
  /** The show key Disconnect turns away: the source's own, or the kept place's. */
  const discKey = $derived(own?.key ?? kept?.key ?? null);
  const now = $derived(kept || count ? stationNow() : 0);
  const keptWords = $derived(kept ? keptHeld(kept, now, othersOnAir) : '');
  /** What is left of the place in its last 30 s: the figure the mark stands before. */
  const keptLeftFig = $derived(kept ? keptLeft(kept, now) : null);
  /** The late count's and the kept place's last seconds (14a). */
  const countHurry = $derived(count ? hurry(count.at_ms - now) : null);
  const keptHurry = $derived(kept && !kept.back ? hurry(kept.until_ms - now) : null);
  const keptSays = $derived(kept ? keptNote(kept, mode, othersOnAir) : null);
  const GONE_WHY = 'Nothing arrives from their software while the link is lost.';
  const role = $derived(source ? (ROLE[source.role] ?? ['g-standby', source.role]) : ['g-playout', 'Liquidsoap 2.5']);
  // At the Callers step the DJ took the call themselves: no screener sent it. The station's
  // step, so the line holds while everything is shown.
  const roleLine = $derived(
    source?.role === 'caller'
      ? stationStep(studio.view) === 'callers'
        ? 'Caller · you took the call'
        : `Caller · ${source.caller?.sent ? 'sent by the screener' : 'from the screener'}`
      : role[1],
  );
  // At Solo the DJ's tile is whole, so the four buttons keep their row and their keys from the
  // first day; Talk is disabled, as playout's buttons are, because there is nobody to talk to.
  /** A host's monitor link is open: someone listens there, so Talk reaches them. */
  const monitorOpen = $derived(!!own?.monitor);
  const talkOff = $derived((look() === 'solo' && source?.role === 'presenter') || (isSource && !monitorOpen));
  const talkWhy = $derived(isSource ? (gone ? GONE_WHY : talkOffWhy(firstName(source?.name ?? '', 'This host'))) : 'Talk needs someone to talk to. It comes with Guests.');
  const name = $derived(source ? source.caller?.name_on_air || source.name || source.identity : 'Playout');
  const hears = $derived(
    !source
      ? 'Takes the desk into the programme'
      : source.role === 'presenter' || source.role === 'dj'
        ? 'Hears the programme'
        : 'Own mix-minus',
  );
  const lufs = $derived(source?.lufs ?? null);
  const peak = $derived(source?.lufs_peak ?? null);
  const over = $derived(lufs !== null && lufs > -22);
  const away = $derived(source && !source.present ? (source.away_ms ?? 0) : null);
  const degraded = $derived(!!source && !kept && (!source.present || !source.linked));
  /** A person can leave the room and keep their place; a host's software is connected or gone. */
  const awayNote = $derived(!isSource && degraded && away !== null);
  const armedDrop = $derived(studio.armed?.what === 'drop' && slot === 4);
  // End pass (ADR-0119) is for the people who joined with an invite: a presenter, a remote DJ
  // or a guest. A caller has Drop, which ends the pass too; playout has no pass. Two presses,
  // as Drop, and the arm is this tile's own: it carries the identity it was pressed for.
  const invited = $derived(source?.role === 'presenter' || source?.role === 'dj' || source?.role === 'guest');
  const armedEnd = $derived(!!source && studio.armed?.what === 'end-pass' && studio.armed.identity === source.identity);
  /** The presenter's own tile: this window's account in the room by its microphone link. Its
   *  quiet button is Leave, which is End pass for yourself (two presses), and its link line
   *  says so in words. */
  const mine = $derived(!!source && source.role === 'presenter' && isMine(source.identity, account.me));
  const TICKS = [-40, -30, -23, -18, -10];
</script>

<section
  class="tile"
  class:is-onair={open && heard}
  class:is-open={open && !heard}
  class:is-source={isSource}
  class:is-kept={!!kept}
  data-panel="desk.tile.{slot}"
  aria-label="Source {slot}, {name}"
>
  <div class="thumb">
    {#if isSource}<b>own studio</b><span>sound only: a host’s picture is not used yet</span>{:else}<b>{source?.role === 'caller' ? 'name card' : 'camera'}</b><span>no picture in the tile yet</span>{/if}
  </div>
  <div class="head">
    <span class="slot num">{slot}</span>
    <div>
      <div class="name" data-panel-title>{name}</div>
      <div class="role" title={ownRole && own ? `${ownRole.long} · ${own.format}` : undefined}>
        <svg aria-hidden="true"><use href="#{role[0]}" /></svg>{roleLine}{#if ownRole}<span class="lg-only">
            · {ownRole.long}</span
          >{:else if source && !isSource}<span class="lg-only">
            · {source.identity}</span
          >{/if}
      </div>
    </div>
    <div class="chips">
      <StateChip {chip} />
      {#if talking}<StateChip chip={CHIP.talk} />{/if}
      {#if source?.role === 'caller'}
        <button
          type="button"
          class="btn quiet"
          class:armed={armedDrop}
          disabled={prep}
          title={prepWhy}
          onclick={() => run(armedDrop ? { type: 'drop' } : { type: 'arm', what: 'drop' })}
          >{armedDrop ? 'Press again' : 'Drop'} <kbd>⌃D</kbd><kbd>⌃D</kbd></button
        >
      {:else if source && isSource && discKey}
        <!-- Disconnect (design section 4.1): two presses, as End pass; a button and no key. On a
             kept place it lets the place go at once, and turns the key away as ever. -->
        <button
          type="button"
          class="btn quiet"
          class:armed={armedDisc}
          disabled={prep}
          title={prepWhy ?? DISCONNECT_WHY}
          aria-label={armedDisc ? `Press again to disconnect ${name}` : `Disconnect ${name}`}
          onclick={() =>
            run(
              armedDisc
                ? { type: 'disconnect', identity: source.identity, key: discKey }
                : { type: 'arm', what: 'disconnect', identity: source.identity },
            )}>{armedDisc ? 'Press again' : 'Disconnect'}</button
        >
      {:else if source && invited}
        <button
          type="button"
          class="btn quiet"
          class:armed={armedEnd}
          disabled={prep}
          data-testid={mine ? 'leave' : undefined}
          title={prepWhy ?? (mine ? LEAVE.why : undefined)}
          aria-label={armedEnd ? (mine ? 'Press again to leave the room' : `Press again to end pass for ${name}`) : mine ? 'Leave the room' : `End pass for ${name}`}
          onclick={() =>
            run(armedEnd ? { type: 'end-pass', identity: source.identity } : { type: 'arm', what: 'end-pass', identity: source.identity })}
          >{armedEnd ? 'Press again' : mine ? LEAVE.button : 'End pass'}</button
        >
      {/if}
    </div>
  </div>

  <div class="levels">
    <div class="hmeter-wrap">
      <div
        class="hmeter"
        role="img"
        aria-label={lufs === null ? 'No level' : `Short-term ${signed(lufs)} LUFS, target −23`}
      >
        <div class="fill" style:width="{meterPct(lufs)}%"></div>
        {#if over && lufs !== null}<div
            class="over"
            style:left="{meterPct(-23)}%"
            style:width="{meterPct(lufs) - meterPct(-23)}%"
          ></div>{/if}
        {#if peak !== null}<div class="peak" style:left="{meterPct(peak)}%"></div>{/if}
        <div class="target" style:left="{meterPct(-23)}%"></div>
      </div>
      <div class="hscale" aria-hidden="true">
        {#each TICKS as t (t)}<span class:t={t === -23} style:left="{meterPct(t)}%">{signed(t).replace('.0', '')}</span>{/each}
      </div>
    </div>
    <span class="rd" class:over><b>{lufs === null ? '—' : signed(lufs)}</b> LUFS</span>
    {#if isSource && !own}
      <!-- The kept place of a source that has left the desk: no level, and the leveller at rest. -->
      <div class="hfader leveller" role="img" aria-label="No leveller reading: nothing arrives from their software">
        <div class="slot"></div>
        <div class="z" style:left="50%"></div>
      </div>
      <span class="rd">leveller <b>—</b> dB</span>
    {:else if own}
      <!-- The station's own hand on this source: a slow gain toward the desk's level, shown on
           the fader's track with nothing at the middle (ADR-0280). Nobody moves it. -->
      <div
        class="hfader leveller"
        role="img"
        aria-label="The station’s leveller is at {levellerFigure(own.leveller_db)} dB, of 12 either way: it brings this source to the desk’s level by itself"
        title="The station brings this source to the desk’s level by itself, slowly, within 12 dB either way."
      >
        <div class="slot"></div>
        <div class="z" style:left="50%"></div>
        <div class="reach" style:left="{Math.min(50, levPct)}%" style:width="{Math.abs(levPct - 50)}%"></div>
        <div class="knob" style:left="{levPct}%"></div>
      </div>
      <span class="rd" class:over={Math.abs(own.leveller_db) >= 11.95}>leveller <b>{levellerFigure(own.leveller_db)}</b> dB</span>
    {:else}
      <div class="hfader" role="img" aria-label="Fader at 0.0 dB; the desk has no fader yet">
        <div class="slot"></div>
        <div class="z" style:left="80%"></div>
        <div class="knob" style:left="80%"></div>
      </div>
      <span class="rd">fader <b>0.0</b> dB</span>
    {/if}
  </div>

  <div class="controls">
    <button
      type="button"
      class="btn"
      class:lit-onair={!!source?.on_air && heard}
      class:lit-open={!!source?.on_air && !heard}
      class:lit-on={gone}
      aria-pressed={!!source?.on_air || gone}
      aria-disabled={isPlayout}
      disabled={prep}
      title={prepWhy ?? (gone ? OFF_AIR_WHY : undefined)}
      onclick={() => run({ type: 'on', slot })}
      ><span><svg aria-hidden="true"><use href={heard ? '#g-onair' : '#g-open'} /></svg> On</span><kbd>{keys.on}</kbd></button
    >
    <button
      type="button"
      class="btn"
      class:lit-cue={!!source?.cue}
      aria-pressed={!!source?.cue}
      aria-disabled={isPlayout || gone}
      disabled={prep}
      title={prepWhy ?? (gone ? GONE_WHY : undefined)}
      onclick={() => run({ type: 'cue', slot })}
      ><span><svg aria-hidden="true"><use href="#g-cue" /></svg> Cue</span><kbd>{keys.cue}</kbd></button
    >
    <button
      type="button"
      class="btn"
      class:lit-talk={talking}
      aria-pressed={talking}
      aria-disabled={isPlayout || talkOff}
      disabled={prep}
      title={prepWhy ?? (talkOff ? talkWhy : undefined)}
      aria-label="Talk to {name}, held"
      onpointerdown={() => run({ type: 'talk', slot, held: true })}
      onpointerup={() => run({ type: 'talk', slot, held: false })}
      onpointerleave={() => talking && run({ type: 'talk', slot, held: false })}
      ><span><svg aria-hidden="true"><use href="#g-talk" /></svg> Talk</span><kbd>{keys.talk}</kbd></button
    >
    <button
      type="button"
      class="btn"
      class:lit-off={!!source?.mute}
      aria-pressed={!!source?.mute}
      aria-disabled={isPlayout || gone}
      disabled={prep}
      title={prepWhy ?? (gone ? GONE_WHY : undefined)}
      onclick={() => run({ type: 'mute', slot })}
      ><span><svg aria-hidden="true"><use href="#g-muted" /></svg> Mute</span><kbd>{keys.mute}</kbd></button
    >
  </div>

  {#snippet monitorLine()}
    {#if own}
      <!-- The line on what they hear (design section 4.1): their monitor link, open or not.
           What their software sends is in the role line's tooltip, and in this one. -->
      <span class="mon" class:open={monitorOpen} data-testid="source-monitor" title="{MONITOR_WHY[monitorOpen ? 'open' : 'closed']}{own.format ? ` Their software sends ${own.format}.` : ''}"
        ><svg aria-hidden="true"><use href={monitorOpen ? '#g-good' : '#g-standby'} /></svg><span>{MONITOR_WORD[monitorOpen ? 'open' : 'closed']}</span></span
      >
    {/if}
  {/snippet}
  <div class="lines" class:has-hand={!!handover}>
    {#if handover}
      <!-- The handover (ADR-0074), the same action as ⇧⏎: in the lines row, under the four live
           buttons, where it fits a full stage too; it takes the place of the line on what they hear. -->
      {@const offWhy = prep ? OFF_WHY : handoverOff ? `The handover is off at ${STEP_WORD[stationStep(studio.view)]}. It comes on at Full.` : ''}
      <button
        type="button"
        class="btn primary hand"
        data-handover={handover.kind}
        title={offWhy || handover.then}
        aria-disabled={handoverOff || undefined}
        disabled={prep}
        aria-describedby="hand-then-{slot}"
        onclick={() => !handoverOff && run({ type: 'handover' })}><span>{handover.label}</span><kbd>{HANDOVER_KEY}</kbd></button
      ><span id="hand-then-{slot}" class="sr-only">{offWhy || handover.then}</span>
    {:else if gone}
      <span class="fmt"><svg aria-hidden="true"><use href="#g-disc" /></svg><span>nothing arriving</span></span>
    {:else if own}
      {@render monitorLine()}
    {:else}
      <span class="mm"><svg aria-hidden="true"><use href="#g-good" /></svg><span>{hears}</span></span>
    {/if}
    {#if isPlayout}
      {#if playoutUp}
        <span><svg class="g-good" aria-hidden="true"><use href="#g-good" /></svg><span
            >SRT connected{playoutForMs !== null ? ` ${dur(playoutForMs)}` : ''}</span
          ></span
        >
      {:else}
        <span
          ><svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg><span
            >{playoutForMs === null ? 'Playout has not connected' : `SRT link lost ${dur(playoutForMs)} ago`} · room alive</span
          ></span
        >
      {/if}
    {:else if kept}
      <!-- The place kept (design section 8): how long the link has been lost and until when the
           place is kept; once its software is back, that it returns by itself. In its last
           seconds it counts what is left, with the mark directly before that figure, as on
           every count on the desk. -->
      <span class="kept" data-testid="source-kept" data-hurry={keptHurry?.level}><svg class="g-serious" aria-hidden="true"><use href="#g-hold" /></svg><span class="num">{keptWords}{#if keptLeftFig}{' · '}<span class="left"><Hurry h={keptHurry} /><b>{keptLeftFig}</b> left</span>{/if}</span>{#if keptHurry}<span class="sr-only"> {keptHurry.word}.</span>{/if}</span>
    {:else if degraded}
      <span class="st"
        ><StateChip chip={CHIP.linkDegraded} /><span class="lg-only"
          >{isSource ? 'nothing arriving from their software' : away !== null ? `left the room ${dur(away)} ago · kept 30 s` : 'no audio reaching the desk'}</span
        ></span
      >
    {:else if own}
      <span data-testid="source-link"><svg class="g-good" aria-hidden="true"><use href="#g-link" /></svg><span>{tileLink(own)}</span></span>
    {:else}
      <span data-testid={mine ? 'in-room' : undefined}><svg class="g-good" aria-hidden="true"><use href="#g-link" /></svg><span>{mine ? `${LEAVE.inRoom} · linked` : 'Linked · in the room'}</span></span>
    {/if}
    {#if handover && own && !kept}
      <!-- With the handover's button in the first place, a source's monitor line keeps a line of
           its own under it: whether the host hears the desk matters most at a handover. -->
      {@render monitorLine()}
    {/if}
  </div>

  <div class="rest">
    {#if source?.caller && (source.caller.line || source.caller.note)}
      <div class="note">{[source.caller.line, source.caller.note].filter(Boolean).join(' · ')}</div>
    {/if}
    {#if count}
      <!-- The late count (design section 3.5): the station puts this source on air by itself
           when it ends. Not yet stops it; a button and no key, as Hold has none. The sentence is
           the next show's card's, mark and all: in its last seconds the mark stands before the figure. -->
      <div class="count" data-testid="late-count" data-hurry={countHurry?.level}>
        <span>On air in <Hurry h={countHurry} /><b class="num">{dur(count.at_ms - now)}</b> <span class="by">· by the schedule</span>{#if countHurry}<span class="sr-only"> {countHurry.word}.</span>{/if}</span>
        <button
          type="button"
          class="btn sm"
          disabled={prep}
          title={prepWhy ?? 'Stops the count. They stay standing by, and go on air by a press.'}
          onclick={() => notYet(count.key)}>Not yet</button
        >
      </div>
    {/if}
    {#if keptSays}
      <div class="extra kept">{keptSays}</div>
    {:else if ownNote}
      <div class="extra">{ownNote}</div>
    {/if}
    {#if awayNote && away !== null}
      <div class="extra">Left the room {dur(away)} ago; their place on the desk is kept for 30 s.</div>
    {/if}
    {#if isPlayout && !playoutUp}
      <div class="extra">Listeners hear nothing of the room until playout is back. Restart it with ⌃R twice.</div>
    {/if}
  </div>
</section>
