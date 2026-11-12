<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The room (screener.room): the chat in real time for the screener (ADR-0038: the studio's
  // view is always real time), with the listeners' lag stated beside it (ADR-0103). It is a log
  // the screener reads, not one a screen reader announces; it never scrolls by itself while the
  // screener has scrolled up ("N new below End"). The selected line carries its actions; the
  // staff composer posts as the station.
  import { tick, untrack, type Snippet } from 'svelte';
  import type { Action } from '../../../../packages/ui/keymap.ts';
  import type { Msg } from '../../../../packages/ui/chat.ts';
  import { run } from '../lib/actions.ts';
  import { chat, mod } from '../lib/chat.svelte.ts';
  import { rateWords } from '../lib/chatModel.ts';
  import { stationNow, studio } from '../lib/studio.svelte.ts';
  import { delayWords, hms, listenerDelayMs } from '../lib/format.ts';
  import ChatAvatar from './ChatAvatar.svelte';
  import ChatAttachment from './ChatAttachment.svelte';
  import LinkLine from './LinkLine.svelte';

  let { children }: { children?: Snippet } = $props();

  const v = $derived(studio.view);
  const behind = $derived(delayWords(v?.listener_delay));
  const lagMs = $derived(listenerDelayMs(v?.listener_delay));
  const now = $derived(stationNow());
  const lines = $derived(chat.room.lines);
  const pinned = $derived(chat.room.pinned);
  const rate = $derived(rateWords(chat.arrivals, studio.now));
  const banArmed = $derived(studio.armed?.what === 'ban' && chat.banAt === 'room');

  let scroller: HTMLElement | undefined = $state();
  let atBottom = true;
  let newBelow = $state(0);
  let lastId = 0;

  function toBottom() {
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
    newBelow = 0;
    atBottom = true;
  }
  function onScroll() {
    if (!scroller) return;
    atBottom = scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 24;
    if (atBottom) newBelow = 0;
  }

  // New lines: follow them while at the bottom; count them while scrolled up.
  $effect(() => {
    const last = lines.at(-1)?.id ?? 0;
    untrack(() => {
      if (last === lastId) return;
      const added = lastId === 0 ? 0 : lines.filter((m) => m.id > lastId).length;
      lastId = last;
      if (atBottom) void tick().then(toBottom);
      else newBelow += added;
    });
  });
  // `End`: the newest messages.
  $effect(() => {
    if (chat.jump) untrack(() => void tick().then(toBottom));
  });

  function press(ev: MouseEvent, m: Msg, a: Action) {
    (ev.currentTarget as HTMLElement).focus();
    chat.sel.room = m.id;
    void run(a);
  }
  function who(m: Msg): string {
    const tags = [m.author.standing === 'staff' ? 'staff' : m.author.guest ? 'guest' : '', m.pinned || pinned?.id === m.id ? 'pinned' : ''];
    return tags.filter(Boolean).join(' · ');
  }
</script>

<section class="panel room" data-panel="screener.room" aria-label="The room" inert={!!chat.sor}>
  <div class="head">
    <h2>Room <span class="count">{chat.count.toLocaleString('en-GB')}</span></h2>
    <span class="sub rate">{rate} · real time{chat.rules?.slow ? ` · slow mode ${chat.rules.slow_s} s` : ''}</span>
    <span class="spacer"></span>
    <span class="lag"><svg class="g-accent" aria-hidden="true"><use href="#g-delay" /></svg>Listeners see <b class="num">{hms(now - lagMs)}</b> · {behind}</span>
  </div>
  <div class="room-top">
    <div class="toasts" aria-live="polite">
      {@render children?.()}
    </div>
    {#if pinned}
      <p class="pinned-bar">
        <svg class="g g-accent" aria-hidden="true"><use href="#g-pin" /></svg><b>Pinned</b>
        <span class="pin-text">{pinned.author.name}: {pinned.body}</span>
        <button type="button" class="btn sm quiet" data-chat="unpin" onclick={() => mod({ op: 'unpin' })}>Unpin</button>
      </p>
    {/if}
  </div>
  <div class="body" bind:this={scroller} onscroll={onScroll}>
    <LinkLine />
    <ol class="feed" role="log" aria-live="off" aria-label="Room messages">
      {#each lines as m (m.id)}
        {@const hid = chat.room.hidden[m.id]}
        {#if hid}
          <li class="line hidden-line" data-chat="room-line" data-id={m.id} data-hidden="true">
            <span class="t">{hms(m.server_ms)}</span><span class="b"
              ><span class="k"
                ><svg aria-hidden="true"><use href="#g-muted" /></svg>Hidden{hid.by ? ` by ${hid.by}` : ''}
                {hms(hid.at_ms)}{hid.why ? ` · ${hid.why}` : ''}</span
              ><span class="n">{m.author.name}</span> <span class="x">hidden from the room</span></span
            >
          </li>
        {:else}
          {@const sel = chat.sel.room === m.id}
          {@const small = who(m)}
          {@const featAt = chat.room.featuredAt[m.id]}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions, a11y_no_noninteractive_tabindex -->
          <li
            class="line"
            class:request={m.kind === 'request'}
            class:feat={m.featured}
            aria-current={sel ? 'true' : undefined}
            tabindex={sel || (chat.sel.room === null && m === lines.at(-1)) ? 0 : -1}
            data-chat="room-line"
            data-id={m.id}
            onclick={() => (chat.sel.room = m.id)}
            onfocus={() => (chat.sel.room = m.id)}
          >
            <span class="t">{hms(m.server_ms)}</span><span class="b"
              >{#if m.reply_to}<span class="re">replying to {m.reply_to.name}</span>{/if}{#if m.kind === 'request'}<span class="k"
                  ><svg class="g-accent" aria-hidden="true"><use href="#g-pin" /></svg>Request</span
                >{/if}{#if m.featured}<span class="k"
                  ><svg class="g-onair" aria-hidden="true"><use href="#g-lower" /></svg>Featured on the player page{featAt ? ` ${hms(featAt)}` : ''}</span
                >{/if}<span class="n"><ChatAvatar avatar={m.author.avatar} name={m.author.name} size={18} />{m.author.name}{#if small}{' '}<small>{small}</small>{/if}</span>
              <span class="x">{m.body}</span>{#if m.attachment}<span class="att"><ChatAttachment a={m.attachment} size={120} /></span>{/if}</span
            >
            {#if sel}
              <span class="acts">
                <button type="button" class="btn primary" data-chat="feature" onclick={(ev) => press(ev, m, { type: 'feature' })}
                  >{m.featured ? 'Feature again' : 'Feature on the player page'} <kbd>F</kbd></button
                >
                <button type="button" class="btn" data-chat="pin" onclick={(ev) => press(ev, m, { type: 'pin' })}
                  ><svg class="g" aria-hidden="true"><use href="#g-pin" /></svg>{pinned?.id === m.id ? 'Unpin' : 'Pin'} <kbd>⇧</kbd><kbd>P</kbd></button
                >
                {#if m.author.standing !== 'staff'}
                  <button type="button" class="btn" data-chat="room-invite" onclick={(ev) => press(ev, m, { type: 'invite' })}>Invite to call in <kbd>I</kbd></button>
                  <span class="sp"></span>
                  <button type="button" class="btn quiet" data-chat="room-hide" onclick={(ev) => press(ev, m, { type: 'mod-hide' })}>Hide <kbd>H</kbd></button>
                  {#if m.attachment}
                    <button
                      type="button"
                      class="btn quiet"
                      data-chat="room-remove-attachment"
                      onclick={(ev) =>
                        press(ev, m, {
                          type: 'attachment-remove',
                          attachment: m.attachment!.id,
                          kind: m.attachment!.kind,
                          account: m.author.id,
                          name: m.author.name,
                        })}>{m.attachment.kind === 'document' ? 'Remove document' : 'Remove picture'}</button
                    >
                  {/if}
                  {#if m.author.avatar?.kind === 'upload'}
                    <button
                      type="button"
                      class="btn quiet"
                      data-chat="room-remove-picture"
                      onclick={(ev) => press(ev, m, { type: 'avatar-remove', account: m.author.id, name: m.author.name, picture: m.author.avatar!.id })}>Remove picture</button
                    >
                  {/if}
                  <button type="button" class="btn quiet" data-chat="room-timeout" onclick={(ev) => press(ev, m, { type: 'mod-timeout' })}>Time out <kbd>O</kbd></button>
                  <button
                    type="button"
                    class="btn quiet"
                    class:armed={banArmed}
                    data-chat="room-ban"
                    onclick={(ev) => press(ev, m, banArmed ? { type: 'ban' } : { type: 'arm', what: 'ban' })}
                    >{banArmed ? 'Press again' : 'Ban'} <kbd>⇧B</kbd><kbd>⇧B</kbd></button
                  >
                {/if}
              </span>
            {/if}
          </li>
        {/if}
      {/each}
    </ol>
    {#if chat.link === 'live' && !lines.length}
      <p class="empty">The room is quiet. Messages show here the moment they are written; listeners see them {behind}.</p>
    {/if}
  </div>
  <div class="compose">
    {#if newBelow > 0}
      <button type="button" class="btn sm" data-chat="newest" onclick={() => run({ type: 'newest' })}>{newBelow} new below <kbd>End</kbd></button>
    {:else}
      <span></span>
    {/if}
    <div class="in">
      <label for="chat-compose"><b>Staff</b></label>
      <input
        id="chat-compose"
        data-chat="compose"
        autocomplete="off"
        maxlength={chat.rules?.max_chars}
        placeholder="Say something to the room"
        bind:value={chat.compose}
      />
    </div>
    <label class="chk-box pin-opt"><input type="checkbox" bind:checked={chat.composePin} />Pin it</label>
    <button type="button" class="btn" data-chat="post" disabled={!chat.compose.trim()} onclick={() => run({ type: 'post' })}>Post <kbd>⌃</kbd><kbd>⏎</kbd></button>
  </div>
</section>

<style>
  :global(.screener .room .compose) {
    grid-template-columns: auto minmax(0, 1fr) auto auto;
  }
  :global(.screener .room .compose input#chat-compose) {
    flex: 1;
    min-width: 0;
    border: 0;
    background: transparent;
    color: var(--ink);
    font: inherit;
    font-size: var(--fs-14);
    min-height: 2rem;
  }
  :global(.screener .room .compose input#chat-compose::placeholder) {
    color: var(--muted);
  }
  :global(.screener .room .compose .pin-opt) {
    font-size: var(--fs-13);
    color: var(--ink-2);
    white-space: nowrap;
  }
  :global(.screener .room .compose .pin-opt input) {
    accent-color: var(--accent);
    margin: 0;
  }
  :global(.screener .room .line) {
    cursor: default;
  }
  /* The sender's avatar sits on the name's line, small enough not to open the rows up. */
  :global(.screener .room .line .n .avatar) {
    margin-right: var(--s-1);
    vertical-align: -0.22em;
  }
  :global(.screener .room .room-top) {
    min-width: 0;
  }
  :global(.screener .room .pinned-bar) {
    overflow: hidden;
    display: flex;
    align-items: center;
    gap: var(--s-2);
    margin: var(--s-2) var(--s-3) 0;
    padding: var(--s-1) var(--s-2);
    border: 1px solid var(--hairline);
    border-radius: var(--r-2);
    background: var(--surface-2);
    font-size: var(--fs-13);
    color: var(--ink-2);
    min-width: 0;
  }
  :global(.screener .room .pinned-bar b) {
    color: var(--ink);
  }
  :global(.screener .room .pinned-bar .pin-text) {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
