<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // To the studio (screener.studio, ADR-0040): listeners' private messages, which only the
  // screener and the presenter see. Reply privately `R`, pass to the presenter `⇧M`, invite to
  // call in `I`.
  import type { Action } from '../../../../packages/ui/keymap.ts';
  import type { StudioMsg } from '../../../../packages/ui/chat.ts';
  import { closeReply, run, sendReply } from '../lib/actions.ts';
  import { chat, studioMsg } from '../lib/chat.svelte.ts';
  import { studio } from '../lib/studio.svelte.ts';
  import { firstName, hm, hms } from '../lib/format.ts';

  const v = $derived(studio.view);
  const presenter = $derived(v ? firstName(v.seats.presenter_name, v.seats.presenter ?? 'the presenter') : 'the presenter');
  const list = $derived(chat.mod.studio);
  const selId = $derived(studioMsg()?.m.id ?? null);

  function press(ev: MouseEvent, s: StudioMsg, a: Action) {
    (ev.currentTarget as HTMLElement).focus();
    chat.sel.studio = s.m.id;
    void run(a);
  }

  /** "chatting since 19:40" for an account made today; else the day it was made. */
  function since(ms: number): string {
    const d = new Date(ms);
    const today = new Date(studio.now);
    if (d.toDateString() === today.toDateString()) return `chatting since ${hm(ms)}`;
    return `account since ${d.toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' })}`;
  }

  function replyKeys(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      void sendReply();
    } else if (e.key === 'Escape') {
      // The field's own Esc: close it, back to its Reply button (not the panel title).
      e.preventDefault();
      e.stopPropagation();
      closeReply();
    }
  }
</script>

<div class="panel studio" data-panel="screener.studio" aria-label="Messages to the studio" inert={!!chat.sor}>
  <div class="head">
    <h2><svg class="g" aria-hidden="true"><use href="#g-inbox" /></svg>To the studio <span class="count">{list.length}</span></h2>
    <span class="sub">private: you and {presenter} see these</span>
    <span class="spacer"></span>
  </div>
  <div class="body">
    {#if list.length}
      {#each list as s (s.m.id)}
        {@const reply = s.reply ?? chat.replied[s.m.id] ?? null}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div
          class="pm"
          class:passed={s.passed_ms !== null}
          aria-current={s.m.id === selId ? 'true' : undefined}
          data-chat="studio-msg"
          data-id={s.m.id}
          onclick={() => (chat.sel.studio = s.m.id)}
          onfocusin={() => (chat.sel.studio = s.m.id)}
        >
          <div class="top">
            <span class="n">{s.m.author.name} <small>{since(s.account_since_ms)}</small></span><span class="t num">{hms(s.m.server_ms)}</span>
          </div>
          <p class="x">{s.m.body}</p>
          {#if s.passed_ms !== null}<p class="reply"><b>Passed to {presenter} {hms(s.passed_ms)}</b></p>{/if}
          {#if s.invited_ms !== null}<p class="reply"><b>Invited to call in {hms(s.invited_ms)}</b></p>{/if}
          {#if reply}<p class="reply"><b>You replied:</b> {reply}</p>{/if}
          {#if chat.replyFor === s.m.id}
            <div class="replying">
              <label class="sr-only" for="chat-reply">Reply to {s.m.author.name}, privately</label>
              <input
                id="chat-reply"
                class="in"
                autocomplete="off"
                maxlength={chat.rules?.max_chars}
                placeholder="Reply to {s.m.author.name}, privately"
                bind:value={chat.replyText}
                onkeydown={replyKeys}
              />
              <button type="button" class="btn sm primary" data-chat="studio-reply-send" disabled={!chat.replyText.trim()} onclick={() => sendReply()}
                >Send <kbd>⏎</kbd></button
              >
            </div>
          {/if}
          <div class="acts">
            <button type="button" class="btn" data-chat="studio-reply" onclick={(ev) => press(ev, s, { type: 'reply' })}>Reply <kbd>R</kbd></button>
            {#if s.passed_ms === null}
              <button type="button" class="btn" data-chat="studio-pass" onclick={(ev) => press(ev, s, { type: 'pass' })}
                >Pass to {presenter} <kbd>⇧</kbd><kbd>M</kbd></button
              >
            {/if}
            <button type="button" class="btn quiet" data-chat="studio-invite" onclick={(ev) => press(ev, s, { type: 'invite' })}
              >{s.invited_ms !== null ? 'Invite again' : 'Invite to call in'} <kbd>I</kbd></button
            >
          </div>
        </div>
      {/each}
    {:else if chat.link === 'live'}
      <p class="empty">No private messages yet. When a listener writes to the studio, it lands here; only you and {presenter} see it.</p>
    {/if}
  </div>
</div>

<style>
  :global(.screener .pm[aria-current='true']) {
    border-color: var(--axis);
    box-shadow: inset 4px 0 0 var(--accent);
  }
  :global(.screener .pm .replying) {
    display: flex;
    gap: var(--s-2);
    align-items: center;
  }
  :global(.screener .pm .replying .in) {
    flex: 1;
    min-width: 0;
    min-height: 2rem;
    padding: 0 var(--s-3);
    border: 1px solid var(--axis);
    border-radius: var(--r-2);
    background: var(--surface);
    color: var(--ink);
    font: inherit;
    font-size: var(--fs-14);
  }
</style>
