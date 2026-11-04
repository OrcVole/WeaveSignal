<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Needs you (screener.needs): the chat's modes (kept on the control plane, which the chat
  // service follows), then everything that needs a person, oldest first, an intimate image's
  // 48-hour clock first: reports, held messages with their countdown, notices from people
  // without an account (DSA Art. 16), and requests for a review. Each with its keyed actions.
  import type { Action } from '../../../../packages/ui/keymap.ts';
  import { run, liftAndClose } from '../lib/actions.ts';
  import { chat, chatNow, entries } from '../lib/chat.svelte.ts';
  import { TIMEOUT_MIN, ageWords, holdLeft, reasonLabel, type Entry } from '../lib/chatModel.ts';
  import { studio } from '../lib/studio.svelte.ts';
  import { hm, hms } from '../lib/format.ts';
  import LinkLine from './LinkLine.svelte';

  const v = $derived(studio.view);
  const list = $derived(entries());
  const now = $derived(chatNow());
  const selKey = $derived(list.some((e) => e.key === chat.sel.needs) ? chat.sel.needs : (list[0]?.key ?? null));
  const held = $derived(chat.mod.held);
  const influxArmed = $derived(studio.armed?.what === 'influx');
  const banArmed = $derived(studio.armed?.what === 'ban' && chat.banAt === 'needs');
  /** The ring's circumference (r = 8). */
  const RING = 50.3;

  /** A button's press: focus it (so the key's panel rule holds in WebKit too), select its entry,
   *  then run the same action as its key. */
  function press(ev: MouseEvent, e: Entry, a: Action) {
    (ev.currentTarget as HTMLElement).focus();
    chat.sel.needs = e.key;
    void run(a);
  }
  const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
  const heldWhy = (why: string) => why.replace(/^Held:\s*/i, '').replace(/\.$/, '');
  const dueWords = (ms: number) => `${new Date(ms).toLocaleDateString('en-GB', { weekday: 'short' })} ${hm(ms)}`;
</script>

<div class="panel needs" data-panel="screener.needs" inert={!!chat.sor}>
  <div class="head">
    <h2>Needs you <span class="count" class:attn={list.length > 0} data-chat="needs-count">{list.length}</span></h2>
    <span class="sub">reports, holds, notices</span>
    <span class="spacer"></span>
  </div>
  <div class="modes" role="group" aria-label="Chat modes">
    <button type="button" class="mode" class:on={v?.chat.slow} aria-pressed={!!v?.chat.slow} onclick={() => run({ type: 'mode', mode: 'slow' })}
      ><span class="dot"></span>Slow 10 s <kbd>S</kbd></button
    >
    <button type="button" class="mode" class:on={v?.chat.hold_new} aria-pressed={!!v?.chat.hold_new} onclick={() => run({ type: 'mode', mode: 'hold_new' })}
      ><span class="dot"></span>Hold new accounts <kbd>K</kbd></button
    >
    <button type="button" class="mode" class:on={v?.chat.links_trusted} aria-pressed={!!v?.chat.links_trusted} onclick={() => run({ type: 'mode', mode: 'links_trusted' })}
      ><span class="dot"></span>Links: trusted only <kbd>L</kbd></button
    >
    <button
      type="button"
      class="mode influx"
      class:on={v?.chat.influx}
      class:armed-mode={influxArmed}
      aria-pressed={!!v?.chat.influx}
      onclick={() => run(influxArmed ? { type: 'influx' } : { type: 'arm', what: 'influx' })}
      ><svg class="g" class:g-warning={v?.chat.influx} aria-hidden="true"><use href="#g-crowd" /></svg>{influxArmed
        ? 'Press G again'
        : v?.chat.influx
          ? `Influx on · ${v.chat.influx_since_ms ? hms(v.chat.influx_since_ms) : ''}`
          : 'Influx'} <kbd>G</kbd><kbd>G</kbd></button
    >
  </div>
  <div class="body" data-chat="needs">
    <LinkLine />
    {#if chat.link === 'live' && !chat.canModerate}
      <p class="empty">This session cannot moderate: the chat sent no moderation list. Sign the studio in again.</p>
    {:else if list.length}
      <ol class="items" aria-label="Moderation queue">
        {#each list as e (e.key)}
          {@const sel = e.key === selKey}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
          <li
            class="item"
            class:held={e.kind === 'held'}
            aria-current={sel ? 'true' : undefined}
            data-chat-key={e.key}
            data-chat={e.kind === 'held' ? 'held' : 'item'}
            data-kind={e.kind === 'held' ? undefined : e.kind}
            data-id={e.kind === 'held' ? e.held.m.id : e.item.id}
            onclick={() => (chat.sel.needs = e.key)}
            onfocusin={() => (chat.sel.needs = e.key)}
          >
            {#if e.kind === 'held'}
              {@const h = e.held}
              {@const left = holdLeft(h, now)}
              <div class="kind">
                <span class="k"><svg class="g-accent" aria-hidden="true"><use href="#g-hold" /></svg>Held</span>
                <span>{heldWhy(h.why)}</span>
                {#if left}
                  <span class="countdown"
                    ><svg class="ring" viewBox="0 0 20 20" aria-hidden="true"
                      ><circle cx="10" cy="10" r="8" stroke="var(--surface-3)" /><circle
                        cx="10"
                        cy="10"
                        r="8"
                        stroke="var(--accent)"
                        stroke-dasharray={RING}
                        stroke-dashoffset={RING * (1 - left.share)}
                        transform="rotate(-90 10 10)"
                      /></svg
                    ><span class="num small">{left.s} s</span></span
                  >
                {/if}
                <span class="when">{hms(h.m.server_ms)}</span>
              </div>
              <div class="msg"><span class="who">{h.m.author.name}</span><span class="txt">{h.m.body}</span></div>
              <div class="why">
                {#if !left}
                  <span>The word filter held it{h.flagged ? ` (${h.flagged})` : ''}. It waits for a moderator; only moderators see it.</span>
                {:else if left.s > 0}
                  <span>Releases to the room by itself in {left.s} s unless you hide it. Only moderators see it until then.</span>
                {:else}
                  <span>Releasing to the room now.</span>
                {/if}
              </div>
              <div class="acts">
                <button type="button" class="btn" data-chat="act-release" onclick={(ev) => press(ev, e, { type: 'mod-release' })}>Release now <kbd>⏎</kbd></button>
                <button type="button" class="btn" data-chat="act-hide" onclick={(ev) => press(ev, e, { type: 'mod-hide' })}>Hide <kbd>H</kbd></button>
              </div>
            {:else if e.kind === 'report'}
              {@const it = e.item}
              <div class="kind">
                <span class="k"><svg class="g-warning" aria-hidden="true"><use href="#g-flag" /></svg>Reported</span>
                <span>{plural(it.count, 'report')} · {reasonLabel(it.reason, chat.info)}</span>
                <span class="when">{hms(it.created_ms)}</span>
              </div>
              <div class="msg">
                <span class="who"
                  >{it.account?.name ?? it.message?.author.name ?? 'Unknown'}
                  {#if it.account}<small
                      >account {ageWords(now - it.account.since_ms)}{it.account.guest ? ' · guest' : ''}{it.account.restricted
                        ? ` · ${it.account.restricted}`
                        : ''}</small
                    >{/if}</span
                >
                <span class="txt">{it.message ? it.message.body : 'The message is no longer in the room.'}</span>
              </div>
              <div class="why">
                {#if it.due_ms !== null}<span><b>Take it down by {dueWords(it.due_ms)}</b>: the 48-hour clock for an intimate image.</span>{/if}
                {#if it.detail}<span>Reporter: “{it.detail}”</span>{/if}
                {#if it.automated}
                  <span class="auto"><svg class="g-accent" aria-hidden="true"><use href="#g-fault" /></svg>An automatic filter ({it.automated}) flagged it.</span>
                {/if}
              </div>
              <div class="acts">
                <button type="button" class="btn primary" data-chat="act-hide" onclick={(ev) => press(ev, e, { type: 'mod-hide' })}
                  >Hide and tell them why <kbd>H</kbd></button
                >
                <button type="button" class="btn" data-chat="act-timeout" onclick={(ev) => press(ev, e, { type: 'mod-timeout' })}
                  >Time out {TIMEOUT_MIN} min <kbd>O</kbd></button
                >
                <button
                  type="button"
                  class="btn danger-hollow"
                  class:armed={banArmed && sel}
                  data-chat="act-ban"
                  onclick={(ev) => press(ev, e, banArmed && sel ? { type: 'ban' } : { type: 'arm', what: 'ban' })}
                  >{banArmed && sel ? 'Press again' : 'Ban'} <kbd>⇧B</kbd><kbd>⇧B</kbd></button
                >
                <span class="spacer"></span>
                <button type="button" class="btn quiet" data-chat="act-leave" onclick={(ev) => press(ev, e, { type: 'mod-leave' })}>Leave it <kbd>D</kbd></button>
              </div>
            {:else if e.kind === 'notice'}
              {@const it = e.item}
              <div class="kind">
                <span class="k"><svg class="g-serious" aria-hidden="true"><use href="#g-inbox" /></svg>Notice</span>
                <span>from someone without an account · DSA Art. 16</span>
                <span class="when">{hms(it.created_ms)}</span>
              </div>
              <div class="msg">
                <span class="who">{it.contact?.email || it.contact?.name || 'No contact given'}</span>
                <span class="txt">“{it.detail}”</span>
              </div>
              <div class="why">
                <span><b>Where:</b> {it.location || 'not given'} · <b>Why:</b> {reasonLabel(it.reason, chat.info)}</span>
                {#if it.contact}<span><b>From:</b> {it.contact.name}, {it.contact.email}</span>{/if}
                {#if it.due_ms !== null}<span><b>Take it down by {dueWords(it.due_ms)}</b>: the 48-hour clock for an intimate image.</span>{/if}
                <span>The clock for a timely answer started at {hms(it.created_ms)}.</span>
              </div>
              <div class="acts">
                <button type="button" class="btn" data-chat="act-escalate" onclick={(ev) => press(ev, e, { type: 'escalate' })}
                  >Preserve evidence and escalate <kbd>⇧</kbd><kbd>E</kbd></button
                >
                <span class="spacer"></span>
                <button type="button" class="btn quiet" data-chat="act-ack" onclick={(ev) => press(ev, e, { type: 'acknowledge' })}>Acknowledge <kbd>A</kbd></button>
              </div>
            {:else}
              {@const it = e.item}
              <div class="kind">
                <span class="k"><svg class="g-accent" aria-hidden="true"><use href="#g-inbox" /></svg>Review asked</span>
                <span>{it.account?.name ?? 'A chatter'} asks for a second look</span>
                <span class="when">{hms(it.created_ms)}</span>
              </div>
              {#if it.location}<div class="why"><span><b>The decision:</b> {it.location}</span></div>{/if}
              <div class="msg">
                <span class="who"
                  >{it.account?.name ?? 'They'}
                  {#if it.account?.restricted}<small>{it.account.restricted}</small>{/if}</span
                >
                <span class="txt">“{it.detail}”</span>
              </div>
              <div class="why"><span>Upholding keeps the decision and closes the review.</span></div>
              <div class="acts">
                <button type="button" class="btn" data-chat="act-leave" onclick={(ev) => press(ev, e, { type: 'mod-leave' })}>Uphold and close <kbd>D</kbd></button>
                {#if it.account?.restricted}
                  <span class="spacer"></span>
                  <button type="button" class="btn quiet" data-chat="act-lift" onclick={() => liftAndClose(it)}>Lift it and close</button>
                {/if}
              </div>
            {/if}
          </li>
        {/each}
      </ol>
    {:else if chat.link === 'live'}
      <p class="empty">Nothing needs you. Reports, held messages and notices arrive here, oldest first.</p>
    {/if}
  </div>
  {#if held.length > 1}
    <div class="bulk">
      <svg class="g g-warning" aria-hidden="true"><use href="#g-crowd" /></svg><b>{held.length} held</b>
      <span>since {hms(Math.min(...held.map((h) => h.m.server_ms)))}</span>
      <span class="grow"></span>
      <button type="button" class="btn sm" data-chat="act-release-all" onclick={() => run({ type: 'release-all' })}
        >Release all {held.length} <kbd>⇧</kbd><kbd>U</kbd></button
      >
      <button type="button" class="btn sm" data-chat="act-hide-all" onclick={() => run({ type: 'hide-all' })}
        >Hide all {held.length} <kbd>⇧</kbd><kbd>X</kbd></button
      >
    </div>
  {/if}
</div>
