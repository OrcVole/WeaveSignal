<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Writing to the room: a chat line, a request (lands in the studio's queue as well), or a
  // private message to the studio (ADR-0040). It counts characters against the room's limit,
  // says each answer from the server in place (held, refused with a countdown), and carries
  // the sender's playhead (see model.ts `playheadMs`). A trusted chatter may attach a picture or a
  // document where the station allows it (ADR-0262); it waits for the station before anyone else
  // sees it.
  import { sizeWords, waitWords, type ChatInfo, type Msg } from '../../../../../packages/ui/chat.ts';
  import type { ChatClient, PostKind } from './client.svelte.ts';
  import { attachTooLarge, canAttach, charCount, hhmm, playheadMs, postingBlocked, rulesLine, studioLaneClosed } from './model.ts';

  let {
    client,
    info,
    playing,
    reply = $bindable(null),
  }: { client: ChatClient; info: ChatInfo | null; playing: boolean; reply: Msg | null } = $props();

  let kind = $state<PostKind>('chat');
  let body = $state('');
  let sending = $state(false);
  let refusal = $state<{ words: string; until: number | null; code: string | null } | null>(null);
  let sent = $state<string | null>(null);
  let now = $state(Date.now());
  let box = $state<HTMLTextAreaElement | undefined>();
  let picker = $state<HTMLInputElement | undefined>();
  /** The file chosen to go with the message, and a preview of a picture. */
  let file = $state<File | null>(null);
  let filePreview = $state<string | null>(null);

  const rules = $derived(client.rules ?? info?.rules ?? null);
  const max = $derived(rules?.max_chars ?? 500);
  const n = $derived(charCount(body));
  const over = $derived(n > max);
  const line = $derived(rules ? rulesLine(rules) : '');
  const commentsOnly = $derived(rules?.comments_only ?? false);
  const blocked = $derived(postingBlocked(client.me, now + client.skewMs));
  const guestMe = $derived(client.me?.kind === 'guest');
  // Between shows a moderator may post in the room, but the lane to the studio stays the staff's.
  const noStudio = $derived(studioLaneClosed(rules?.hours, client.me));
  $effect(() => {
    if (noStudio && kind === 'studio') kind = 'chat';
  });
  const waitMs = $derived(refusal?.until ? Math.max(0, refusal.until - now) : 0);
  // Slow mode paces the room, not the private lane to the studio (crates/chat/src/rules.rs).
  const waiting = $derived(waitMs > 0 && !(kind === 'studio' && refusal?.code === 'slow'));
  const offline = $derived(client.link !== 'open');
  // Pictures and documents (ADR-0262): the station's switch, a trusted chatter, an ordinary message.
  const attachOk = $derived(canAttach(rules, client.me) && kind === 'chat');
  $effect(() => {
    if (!attachOk && file) clearFile();
  });
  const canSend = $derived(!sending && !waiting && !blocked && !over && (n > 0 || !!file) && !offline);
  const replying = $derived(kind === 'chat' && !commentsOnly ? reply : null);

  // A clock only while something counts down.
  $effect(() => {
    if (!refusal?.until && client.me?.timed_out_until_ms == null) return;
    const t = setInterval(() => (now = Date.now()), 500);
    return () => clearInterval(t);
  });
  $effect(() => {
    if (refusal?.until && waitMs === 0 && refusal.code !== null) refusal = null;
  });
  // A reply chosen in the log moves focus here.
  $effect(() => {
    if (reply) {
      kind = 'chat';
      box?.focus();
    }
  });

  const label = $derived(kind === 'request' ? 'Your request' : kind === 'studio' ? 'Message the studio' : 'Message');
  const hint = $derived(
    kind === 'request'
      ? "A song or a dedication. Everyone here sees it, and it goes on the studio's request list."
      : kind === 'studio'
        ? 'Private: only the producer and the presenter see it.'
        : commentsOnly
          ? 'Comments on the show only: replies are off.'
          : '',
  );

  function clearFile() {
    if (filePreview) URL.revokeObjectURL(filePreview);
    file = null;
    filePreview = null;
    if (picker) picker.value = '';
  }

  function chose(e: Event) {
    const f = (e.currentTarget as HTMLInputElement).files?.[0] ?? null;
    clearFile();
    if (!f) return;
    refusal = null;
    const big = attachTooLarge(f, info?.attachments);
    if (big) {
      refusal = { words: big, until: null, code: null };
      if (picker) picker.value = '';
      return;
    }
    file = f;
    if (f.type.startsWith('image/')) filePreview = URL.createObjectURL(f);
  }

  async function send(e?: SubmitEvent) {
    e?.preventDefault();
    if (!canSend) {
      if (n === 0 && !file && !blocked) refusal = { words: 'Type a message first.', until: null, code: null };
      return;
    }
    if (file) {
      sending = true;
      sent = null;
      refusal = null;
      const to = replying ? { id: replying.id, name: replying.author.name } : null;
      const r = await client.attach(file, body, to, playheadMs(playing, Date.now(), client.skewMs, client.listenerDelayMs));
      sending = false;
      if (r.state === 'refused') {
        refusal = { words: r.words, until: r.retry_ms ? Date.now() + r.retry_ms : null, code: r.code };
        now = Date.now();
        return;
      }
      body = '';
      reply = null;
      clearFile();
      sent = 'Sent. It waits for the station: nobody else sees it until staff or a moderator approve it.';
      box?.focus();
      return;
    }
    sending = true;
    sent = null;
    refusal = null;
    const text = body;
    const sentKind = kind;
    const to = replying ? { id: replying.id, name: replying.author.name } : null;
    body = '';
    const r = await client.post(sentKind, text, to, playheadMs(playing, Date.now(), client.skewMs, client.listenerDelayMs));
    sending = false;
    if (r.state === 'refused') {
      if (!body) body = text;
      refusal = { words: r.words, until: r.retry_ms ? Date.now() + r.retry_ms : null, code: r.code };
      now = Date.now();
      return;
    }
    reply = null;
    client.problem = null;
    sent = sentKind === 'studio' ? 'Sent to the studio.' : sentKind === 'request' ? 'Request sent.' : null;
    box?.focus();
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      void send();
    }
  }

  const describedBy = $derived(
    ['composer-hint', 'composer-chars', refusal || client.problem ? 'composer-refusal' : '', blocked ? 'composer-blocked' : ''].filter(Boolean).join(' '),
  );
</script>

<form class="composer" onsubmit={send} aria-label="Write to the chat">
  {#if line}<p class="rules small" data-chat="rules"><svg aria-hidden="true"><use href="#g-lock" /></svg>{line}</p>{/if}

  <fieldset class="kinds">
    <legend class="sr-only">Send as</legend>
    <label class="kind"><input type="radio" name="chat-kind" value="chat" bind:group={kind} data-chat="kind-chat" /><span>Chat</span></label>
    <label class="kind"><input type="radio" name="chat-kind" value="request" bind:group={kind} data-chat="kind-request" /><span>Request</span></label>
    {#if !noStudio}<label class="kind"><input type="radio" name="chat-kind" value="studio" bind:group={kind} data-chat="kind-studio" /><span>To the studio</span></label>{/if}
  </fieldset>

  {#if replying}
    <p class="replying small">
      <span>Replying to <b>{replying.author.name}</b></span>
      <button type="button" class="btn quiet" onclick={() => (reply = null)}>Cancel reply</button>
    </p>
  {/if}

  <div class="field">
    <label for="chat-body">{label}</label>
    <span class="hint" id="composer-hint">{hint}{guestMe ? `${hint ? ' ' : ''}As a guest, your messages wait a few seconds for a moderator.` : ''}</span>
    <textarea
      id="chat-body"
      rows="2"
      bind:this={box}
      bind:value={body}
      onkeydown={keydown}
      oninput={() => (sent = null)}
      disabled={!!blocked}
      aria-invalid={over}
      aria-describedby={describedBy}
      enterkeyhint="send"
      data-chat="composer"
    ></textarea>
    <span class="count num" class:over id="composer-chars" data-chat="chars">{over ? `${n - max} over the ${max}-character limit` : `${n}/${max}`}</span>
  </div>

  {#if attachOk}
    <div class="attach">
      <input
        bind:this={picker}
        class="sr-only"
        type="file"
        id="chat-file"
        accept={info?.attachments?.accept ?? 'image/png,image/jpeg,image/webp,application/pdf,.odt,.ods,.md,.txt'}
        onchange={chose}
        disabled={!!blocked}
        aria-describedby="chat-file-hint"
        data-chat="attach-input"
        tabindex="-1"
      />
      {#if file}
        <div class="chosen" data-chat="attach-chosen">
          {#if filePreview}<img src={filePreview} alt="" width="56" height="56" onerror={() => (filePreview = null)} />{/if}
          <span class="meta"><b class="name" title={file.name}>{file.name}</b><span class="small">{sizeWords(file.size)}</span></span>
          <button type="button" class="btn quiet" onclick={clearFile} data-chat="attach-remove" aria-label="Remove {file.name}">Remove</button>
        </div>
      {:else}
        <button type="button" class="btn" onclick={() => picker?.click()} disabled={!!blocked} data-chat="attach"
          ><svg aria-hidden="true" viewBox="0 0 20 20"
            ><path
              d="M13.5 6.5 7.6 12.4a1.6 1.6 0 0 0 2.3 2.3l6.3-6.3a3.2 3.2 0 0 0-4.5-4.5L5.3 10.3a4.8 4.8 0 0 0 6.8 6.8l4.4-4.4"
              fill="none"
              stroke="currentColor"
              stroke-width="1.6"
              stroke-linecap="round"
            /></svg
          >Attach a picture or document</button
        >
      {/if}
      <span class="hint" id="chat-file-hint"
        >A picture (PNG, JPEG, WebP) or a document (PDF, OpenDocument, Markdown, text). It waits for the station before anyone else sees it.</span
      >
    </div>
  {/if}

  {#if blocked}
    <p class="problem" id="composer-blocked" role="status">
      {#if blocked === 'banned'}This account is banned from the chat. The notice from the station below says why.{:else}You're timed out until {hhmm(client.me?.timed_out_until_ms ?? 0)}. You can read the chat meanwhile.{/if}
    </p>
  {/if}

  <div class="say" id="composer-refusal" role="status">
    {#if refusal}
      <p class="refusal" data-chat="refusal"><svg aria-hidden="true"><use href="#g-fault" /></svg><span>{refusal.words}{waiting ? ` Try again in ${waitWords(waitMs)}.` : ''}</span></p>
    {:else if client.problem}
      <p class="refusal" data-chat="refusal"><svg aria-hidden="true"><use href="#g-fault" /></svg><span>{client.problem}</span></p>
    {:else if sent}
      <p class="small">{sent}</p>
    {/if}
  </div>

  <button type="submit" class="btn primary wide" disabled={!canSend && (sending || waiting || !!blocked || over || offline)} data-chat="send"
    >{sending ? 'Sending…' : waiting ? `Wait ${waitWords(waitMs)}` : kind === 'studio' ? 'Send to the studio' : kind === 'request' ? 'Send request' : file ? 'Send with the file' : 'Send'}</button
  >
</form>

<style>
  .composer {
    display: grid;
    gap: var(--s-3);
  }
  .rules {
    display: flex;
    gap: var(--s-2);
    align-items: flex-start;
  }
  .rules svg,
  .refusal svg {
    width: 16px;
    height: 16px;
    flex: none;
    margin-top: 2px;
  }
  .kinds {
    border: 0;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-2);
  }
  .kind {
    position: relative;
    display: inline-flex;
  }
  .kind input {
    position: absolute;
    opacity: 0;
    inset: 0;
    margin: 0;
    cursor: pointer;
  }
  .kind span {
    display: inline-flex;
    align-items: center;
    min-height: var(--target-touch);
    padding: 0 var(--s-3);
    border-radius: var(--r-pill);
    border: 1px solid var(--axis);
    background: var(--surface-2);
    font-weight: 650;
  }
  .kind input:checked + span {
    background: var(--accent);
    color: var(--accent-ink);
    border-color: transparent;
  }
  .kind input:focus-visible + span {
    outline: var(--focus-ring);
    outline-offset: var(--focus-offset);
  }
  .replying {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-2);
    border-left: 3px solid var(--accent);
    padding-left: var(--s-2);
  }
  textarea {
    min-height: 5.5rem;
    padding: var(--s-2) var(--s-3);
    border-radius: var(--r-2);
    border: 1px solid var(--axis);
    background: var(--plane);
    font-size: var(--fs-16);
    line-height: var(--lh-body);
    width: 100%;
    resize: vertical;
  }
  textarea:focus-visible {
    outline-offset: 0;
  }
  textarea[disabled] {
    opacity: 0.55;
  }
  .count.over {
    color: var(--ink);
    font-weight: 650;
  }
  .say:empty {
    display: none;
  }
  .attach {
    display: grid;
    gap: var(--s-1);
    justify-items: start;
  }
  .attach .btn svg {
    width: 18px;
    height: 18px;
  }
  .chosen {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    width: 100%;
    padding: var(--s-2);
    border: 1px solid var(--axis);
    border-radius: var(--r-2);
    background: var(--surface-2);
  }
  .chosen img {
    width: 56px;
    height: 56px;
    object-fit: cover;
    border-radius: var(--r-1);
    background: #fff;
  }
  .chosen .meta {
    display: grid;
    min-width: 0;
    flex: 1;
  }
  .chosen .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .refusal {
    display: flex;
    gap: var(--s-2);
    font-size: var(--fs-14);
    line-height: var(--lh-label);
    border-left: 3px solid var(--status-warning);
    padding: var(--s-2) var(--s-3);
    background: var(--surface-2);
    border-radius: var(--r-1);
  }
  .refusal svg {
    color: var(--status-warning);
  }
</style>
