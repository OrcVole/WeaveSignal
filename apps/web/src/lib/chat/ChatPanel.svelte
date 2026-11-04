<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The live chat on the player page (M6; ADR-0033 to ADR-0040). Reading needs no account. The
  // log is a `role="log"` region with a pause (WCAG 2.2.2), no scrolling under a reader who has
  // scrolled up, and a choice of what a screen reader announces: mentions of you (the default),
  // everything, or nothing. Announcements of mentions go through their own hidden live region,
  // so the log itself is silent unless the reader asked for everything. "Sync chat to my
  // stream" holds each message until the reader hears its moment (ADR-0038). The chat never
  // touches the stream or a call.
  import { onDestroy, onMount, tick } from 'svelte';
  import { mentions, type ChatInfo, type Msg, type SignedIn } from '../../../../../packages/ui/chat.ts';
  import * as api from './api.ts';
  import { ChatClient } from './client.svelte.ts';
  import Composer from './Composer.svelte';
  import Message from './Message.svelte';
  import { due, mentionName, newBelow, noticeLocation, parsePrefs, postingBlocked, shownMessages, waitingBehindPause, type Announce, type Prefs } from './model.ts';
  import NoticeForm from './NoticeForm.svelte';
  import ReportDialog from './ReportDialog.svelte';
  import SignIn from './SignIn.svelte';
  import Statements from './Statements.svelte';
  import StudioLane from './StudioLane.svelte';
  import Terms from './Terms.svelte';
  import './chat.css';

  let {
    playing,
    inCall,
    oninvite,
    onfeatured,
  }: { playing: boolean; inCall: boolean; oninvite: () => void; onfeatured?: (m: Msg | null) => void } = $props();

  /** How long a message put on air stays as text beside the stream. */
  const ON_AIR_MS = 2 * 60 * 1000;

  const client = new ChatClient(api.loadSession());
  let info = $state<ChatInfo | null>(null);
  let infoProblem = $state(false);
  let prefs = $state<Prefs>(parsePrefs(api.loadPrefs()));
  let now = $state(Date.now());
  let paused = $state<Set<number> | null>(null);
  let atBottom = $state(true);
  let lastSeenId = $state<number | null>(null);
  let reply = $state<Msg | null>(null);
  let reportTarget = $state<Msg | null>(null);
  let notice = $state<string | null>(null);
  let say = $state('');
  let signedOutNote = $state<string | null>(null);
  let logEl = $state<HTMLElement | undefined>();

  let infoTimer: ReturnType<typeof setTimeout> | undefined;
  async function loadInfo() {
    try {
      info = await api.info();
      infoProblem = false;
    } catch {
      infoProblem = true;
      infoTimer = setTimeout(loadInfo, 30_000);
    }
  }

  const clock = setInterval(() => (now = Date.now()), 1000);
  onMount(() => {
    client.onPrivate = (e) => {
      if (prefs.announce !== 'nothing') announce(`The studio replied: ${e.body}`);
    };
    client.onSignedOut = () => {
      api.saveSession(null);
      signedOutNote = "You've been signed out of the chat. You can still read it.";
    };
    client.start();
    void loadInfo();
  });
  onDestroy(() => {
    client.stop();
    clearInterval(clock);
    clearTimeout(infoTimer);
  });

  $effect(() => api.savePrefs(JSON.stringify(prefs)));

  const me = $derived(client.me);
  const myName = $derived(me?.name ?? '');
  const meId = $derived(me?.id ?? null);
  const rules = $derived(client.rules ?? info?.rules ?? null);
  // In comments-only mode a mention is plain text: not highlighted, not announced (ADR-0147).
  const mentionOf = $derived(mentionName(myName, rules));
  const merged = $derived.by(() => {
    const ids = new Set(client.list.map((m) => m.id));
    const mine = client.held.filter((m) => !ids.has(m.id));
    return mine.length ? [...client.list, ...mine] : client.list;
  });
  const view = $derived({ sync: prefs.sync, nowLocalMs: now, skewMs: client.skewMs, delayMs: client.listenerDelayMs, meId, pausedIds: paused });
  const shown = $derived(shownMessages(merged, view));
  const behind = $derived(waitingBehindPause(merged, view));
  const syncWaiting = $derived(prefs.sync ? merged.filter((m) => !due(m, view)).length : 0);
  const newCount = $derived(atBottom ? 0 : newBelow(shown, lastSeenId));
  const canPost = $derived(!!me && !postingBlocked(me, now + client.skewMs));
  const canReply = $derived(canPost && !(rules?.comments_only ?? false));
  const delayS = $derived(Math.round(client.listenerDelayMs / 1000));
  const pinned = $derived(client.pinned && client.pinned.state !== 'hidden' ? client.pinned : null);

  // The message on air, as text beside the stream, once the listener hears it (D32).
  const onAir = $derived.by(() => {
    const f = client.featured;
    if (!f) return null;
    const from = f.at + (playing ? client.listenerDelayMs : 0);
    return now >= from && now < f.at + client.listenerDelayMs + ON_AIR_MS ? f.m : null;
  });
  $effect(() => onfeatured?.(onAir));

  // Announce mentions of the reader as they enter the log (never while paused: they have not).
  const known = new Set<number>();
  let primed = false;
  $effect(() => {
    const list = shown;
    if (!primed) {
      if (client.link !== 'open') return;
      for (const m of list) known.add(m.id);
      primed = true;
      return;
    }
    for (const m of list) {
      if (known.has(m.id)) continue;
      known.add(m.id);
      if (prefs.announce === 'mentions' && m.author.id !== meId && mentions(m.body, mentionOf)) announce(`${m.author.name} mentioned you: ${m.body}`);
    }
  });

  let sayTimer: ReturnType<typeof setTimeout> | undefined;
  function announce(text: string) {
    say = '';
    clearTimeout(sayTimer);
    sayTimer = setTimeout(() => (say = text), 60);
  }

  // Keep the newest message in view, unless the reader has scrolled up to read back.
  $effect(() => {
    void shown.length;
    if (atBottom && logEl) logEl.scrollTop = logEl.scrollHeight;
  });

  function scrolled() {
    if (!logEl) return;
    const bottom = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 32;
    if (bottom === atBottom) return;
    atBottom = bottom;
    lastSeenId = bottom ? null : (shown.at(-1)?.id ?? null);
  }

  function toBottom() {
    if (!logEl) return;
    const smooth = !globalThis.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    logEl.scrollTo({ top: logEl.scrollHeight, behavior: smooth ? 'smooth' : 'auto' });
    atBottom = true;
    lastSeenId = null;
  }

  function togglePause() {
    if (paused) {
      const n = behind;
      paused = null;
      atBottom = true;
      lastSeenId = null;
      void tick().then(() => logEl && (logEl.scrollTop = logEl.scrollHeight));
      if (n > 0 && prefs.announce !== 'nothing') announce(`${n} new ${n === 1 ? 'message' : 'messages'}.`);
    } else {
      paused = new Set(shown.map((m) => m.id));
    }
  }

  function signed(s: SignedIn) {
    api.saveSession(s.session);
    signedOutNote = null;
    client.setSession(s.session);
    client.me = s.me;
    void tick().then(() => document.getElementById('chat-body')?.focus());
  }

  async function signOut() {
    const s = client.session;
    api.saveSession(null);
    client.setSession(null);
    reply = null;
    signedOutNote = 'Signed out. You can still read the chat.';
    if (s) await api.signout(s);
    await tick();
    document.querySelector<HTMLElement>('[data-chat="signin-open"]')?.focus();
  }

  function report(m: Msg) {
    if (client.session && me) reportTarget = m;
    else notice = noticeLocation(m);
  }

  function reviewed(id: string) {
    client.statements = client.statements.map((s) => (s.id === id ? { ...s, review_requested: true } : s));
  }

  function setAnnounce(a: Announce) {
    prefs = { ...prefs, announce: a };
  }
</script>

<section class="chat" aria-labelledby="chat-h" data-link={client.link}>
  <div class="chat-top">
    <h2 id="chat-h">Chat</h2>
    {#if client.link === 'open'}<span class="small here num" data-chat="count">{client.count} here</span>{/if}
    <span class="spacer"></span>
    <button type="button" class="btn" onclick={togglePause} data-chat="pause" aria-describedby={paused ? 'chat-paused' : undefined}
      >{#if paused}<svg aria-hidden="true" viewBox="0 0 18 18"><path d="M5 2.5v13l11-6.5z" fill="currentColor" /></svg>Resume{:else}<svg aria-hidden="true" viewBox="0 0 18 18"><path d="M5 3h3v12H5zM10 3h3v12h-3z" fill="currentColor" /></svg>Pause{/if}</button
    >
  </div>

  {#if client.link !== 'open'}
    <p class="problem" role="status" data-chat="link">{client.link === 'connecting' ? 'Connecting to the chat…' : client.everOpen ? 'Chat reconnecting…' : "The chat isn't available just now. Trying again."}</p>
  {/if}
  {#if infoProblem}<p class="small">Some chat details could not load. Trying again.</p>{/if}

  {#if pinned}
    <div class="pinned" data-chat="pinned" data-id={pinned.id}>
      <span class="caps"><svg aria-hidden="true"><use href="#g-pin" /></svg>Pinned</span>
      <p><b>{pinned.author.name}:</b> {pinned.body}</p>
    </div>
  {/if}

  {#if paused}
    <p class="small paused" id="chat-paused" role="status">Paused. {behind === 0 ? 'No new messages yet.' : `${behind} new ${behind === 1 ? 'message' : 'messages'}.`}</p>
  {/if}

  <div class="log-wrap">
    <div class="log" role="log" aria-live={prefs.announce === 'everything' && !paused ? 'polite' : 'off'} aria-relevant="additions" aria-label="Chat messages" data-chat="log" bind:this={logEl} onscroll={scrolled}>
      {#if shown.length}
        <ol>
          {#each shown as m (m.id)}
            <Message
              {m}
              myName={mentionOf}
              mine={meId !== null && m.author.id === meId}
              canReact={canPost}
              {canReply}
              onreact={(msg, e) => client.react(msg.id, e)}
              onreply={(msg) => (reply = msg)}
              onreport={report}
            />
          {/each}
        </ol>
      {:else}
        <p class="small empty">{client.link === 'open' ? (syncWaiting ? 'Messages arrive as you hear their moment.' : 'No messages yet.') : 'Messages show here once the chat connects.'}</p>
      {/if}
    </div>
    {#if newCount > 0 && !paused}
      <button type="button" class="btn primary newer" onclick={toBottom} data-chat="newer">{newCount} new {newCount === 1 ? 'message' : 'messages'} ↓</button>
    {/if}
  </div>
  {#if prefs.sync && syncWaiting > 0}<p class="small">{syncWaiting} {syncWaiting === 1 ? 'message waits' : 'messages wait'} for your stream to catch up.</p>{/if}

  <div class="sr-only" role="status" aria-live="polite" aria-atomic="true" data-chat="announce">{say}</div>

  <details class="trouble disclose settings">
    <summary data-chat="settings">Chat settings</summary>
    <div>
      <div class="field">
        <label class="check">
          <input type="checkbox" role="switch" bind:checked={prefs.sync} aria-describedby="sync-hint" data-chat="sync" />
          <span>Sync chat to my stream</span>
        </label>
        <span class="hint" id="sync-hint">Shows messages when you hear the moment they were written, about {delayS} s behind the studio. Your own messages show at once.</span>
      </div>
      <fieldset class="group">
        <legend>Screen reader announcements</legend>
        <label class="check"><input type="radio" name="chat-announce" checked={prefs.announce === 'mentions'} onchange={() => setAnnounce('mentions')} aria-describedby={me && !rules?.comments_only ? undefined : 'announce-hint'} data-chat="announce-mentions" /><span>Mentions of you</span></label>
        {#if rules?.comments_only}<span class="hint" id="announce-hint">Mentions are off in this chat.</span>{:else if !me}<span class="hint" id="announce-hint">Mentions need you to be signed in.</span>{/if}
        <label class="check"><input type="radio" name="chat-announce" checked={prefs.announce === 'everything'} onchange={() => setAnnounce('everything')} data-chat="announce-everything" /><span>Every message</span></label>
        <label class="check"><input type="radio" name="chat-announce" checked={prefs.announce === 'nothing'} onchange={() => setAnnounce('nothing')} data-chat="announce-nothing" /><span>Nothing</span></label>
      </fieldset>
    </div>
  </details>

  <div class="card write">
    {#if signedOutNote}<p class="small" role="status">{signedOutNote}</p>{/if}
    {#if me}
      <Composer {client} {info} {playing} bind:reply />
      <p class="who small">
        <span>{me.kind === 'guest' ? 'Posting as a guest:' : 'Signed in as'} <b>{me.name}</b></span>
        <button type="button" class="btn quiet" onclick={signOut} data-chat="signout">Sign out</button>
      </p>
    {:else}
      <SignIn {info} onsigned={signed} onnotice={() => (notice = '')} />
    {/if}
  </div>

  {#if me}
    <StudioLane entries={client.lane} {inCall} {oninvite} />
    <Statements statements={client.statements} session={client.session} nowServerMs={now + client.skewMs} onreviewed={reviewed} />
  {/if}

  <details class="trouble disclose rules">
    <summary data-chat="rules-open">Chat rules and contacts</summary>
    <div>
      {#if info?.mode_words}<p class="small">{info.mode_words}</p>{/if}
      <Terms {info} />
      {#if info}
        <dl class="small keep">
          <dt>Messages</dt><dd>{info.retention.messages}</dd>
          <dt>Reports and moderation</dt><dd>{info.retention.moderation}</dd>
          <dt>Addresses</dt><dd>{info.retention.addresses}</dd>
        </dl>
      {/if}
      {#if me}<button type="button" class="btn quiet wide" onclick={() => (notice = '')} data-chat="notice-open">Send a notice about something else</button>{/if}
    </div>
  </details>
</section>

<ReportDialog {info} session={client.session} target={reportTarget} onclose={() => (reportTarget = null)} />
<NoticeForm {info} location={notice} onclose={() => (notice = null)} />

<style>
  .chat {
    display: grid;
    gap: var(--s-3);
  }
  .chat-top {
    display: flex;
    align-items: center;
    gap: var(--s-2);
  }
  .chat-top h2 {
    font-size: var(--fs-20);
  }
  .spacer {
    flex: 1;
  }
  .pinned {
    display: grid;
    gap: var(--s-1);
    padding: var(--s-2) var(--s-3);
    border-radius: var(--r-2);
    background: var(--surface-2);
    border-left: 3px solid var(--accent);
  }
  .pinned .caps {
    display: inline-flex;
    align-items: center;
    gap: var(--s-1);
  }
  .pinned svg {
    width: 12px;
    height: 12px;
  }
  .pinned p {
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
  .log-wrap {
    position: relative;
  }
  .log {
    max-height: min(60dvh, 32rem);
    min-height: 8rem;
    overflow-y: auto;
    overscroll-behavior: contain;
    background: var(--surface);
    border: 1px solid var(--hairline);
    border-radius: var(--r-3);
    padding: var(--s-2) var(--s-2) var(--s-2) 0;
  }
  .log ol {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .empty {
    padding: var(--s-3) var(--s-4);
  }
  .newer {
    position: absolute;
    left: 50%;
    bottom: var(--s-3);
    transform: translateX(-50%);
    box-shadow: var(--e-2);
  }
  .write {
    gap: var(--s-3);
  }
  .who {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-2);
    flex-wrap: wrap;
  }
  .keep {
    display: grid;
    gap: 2px;
    margin: 0;
  }
  .keep dd + dt {
    margin-top: var(--s-1);
  }
  .keep dt {
    font-weight: 650;
    color: var(--ink);
  }
  .keep dd {
    margin: 0;
  }
</style>
