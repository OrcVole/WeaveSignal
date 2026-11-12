<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // One message in the room: the sender's avatar (ADR-0235: decoration beside the name, so a
  // screen reader hears the name once), who, when, what it answers, its marks (request, the station, on
  // air), the reader's @mentions highlighted, a picture or document it carries (ADR-0262), and
  // reaction counts. Its actions (react, reply, report) sit behind one button so a long room
  // stays compact on a phone.
  import Avatar from '../../../../../packages/ui/Avatar.svelte';
  import { REACTIONS, type Attachment, type Msg } from '../../../../../packages/ui/chat.ts';
  import AttachmentBox from './AttachmentBox.svelte';
  import { hhmm, markMentions, reportablePicture } from './model.ts';

  let {
    m,
    myName,
    mine,
    canReact,
    canReply,
    onreact,
    onreply,
    onreport,
    onview = () => {},
    preview = null,
  }: {
    m: Msg;
    myName: string;
    mine: boolean;
    canReact: boolean;
    canReply: boolean;
    onreact: (m: Msg, emoji: string) => void;
    onreply: (m: Msg) => void;
    onreport: (m: Msg, what?: 'picture' | 'attachment') => void;
    /** Open a posted picture full size. */
    onview?: (a: Attachment) => void;
    /** A picture this page attached, from the file chosen, while the station decides. */
    preview?: string | null;
  } = $props();

  let open = $state(false);
  const parts = $derived(markMentions(m.body, myName));
  const mentioned = $derived(!mine && parts.some((p) => p.mention));
  const staff = $derived(m.author.standing === 'staff' || m.kind === 'announcement');
  const counts = $derived(m.reactions.filter(([, n]) => n > 0));
  const held = $derived(m.state === 'held');
  // A chatter's own picture, not one of the station's set, may be reported (ADR-0260).
  const picture = $derived(!mine ? reportablePicture(m) : null);
  const actionsId = $derived(`msg-actions-${m.id}`);
</script>

<li class="msg" class:mentioned class:announce={m.kind === 'announcement'} data-chat="msg" data-id={m.id} data-kind={m.kind}>
  <Avatar avatar={m.author.avatar} name={m.author.name} size={28} />
  <div class="text">
    <div class="head">
      <span class="who">{mine ? `${m.author.name} (you)` : m.author.name}</span>
      {#if staff}<span class="tag tag-station">{m.kind === 'announcement' ? 'Announcement' : 'Station'}</span>{/if}
      {#if m.author.guest}<span class="tag">Guest</span>{/if}
      {#if m.kind === 'request'}<span class="tag tag-request">Request</span>{/if}
      {#if m.featured}<span class="tag tag-onair"><svg aria-hidden="true"><use href="#g-lower" /></svg>On air</span>{/if}
      <time class="when num" datetime={new Date(m.server_ms).toISOString()}>{hhmm(m.server_ms)}</time>
    </div>
    {#if m.reply_to}<p class="replying">Replying to {m.reply_to.name}</p>{/if}
    {#if m.body}<p class="body">{#each parts as p, i (i)}{#if p.mention}<mark>{p.text}</mark>{:else}{p.text}{/if}{/each}</p>{/if}
    {#if m.attachment}<AttachmentBox a={m.attachment} waiting={held} {preview} {onview} />{/if}
    {#if mentioned}<span class="sr-only">Mentions you.</span>{/if}
    {#if held && m.attachment}<p class="held small" data-chat="held-attachment"
        ><svg aria-hidden="true"><use href="#g-hold" /></svg>Waiting for the station: nobody else sees it until staff or a moderator approve it.</p
      >{:else if held}<p class="held small" data-chat="held"><svg aria-hidden="true"><use href="#g-hold" /></svg>Waiting for a moderator…</p>{/if}
    {#if counts.length}
      <p class="reactions small" aria-label="Reactions: {counts.map(([e, n]) => `${e} ${n}`).join(', ')}">
        {#each counts as [e, n] (e)}<span aria-hidden="true">{e} <span class="num">{n}</span></span>{/each}
      </p>
    {/if}
  </div>
  {#if !held}
    <button
      type="button"
      class="more"
      aria-expanded={open}
      aria-controls={actionsId}
      aria-label="Actions for {m.author.name}'s message"
      data-chat="actions"
      onclick={() => (open = !open)}
      ><svg aria-hidden="true" viewBox="0 0 18 18"><circle cx="4" cy="9" r="1.6" fill="currentColor" /><circle cx="9" cy="9" r="1.6" fill="currentColor" /><circle cx="14" cy="9" r="1.6" fill="currentColor" /></svg></button
    >
  {/if}
  {#if open && !held}
    <div class="actions" id={actionsId}>
      {#if canReact}
        <div class="react" role="group" aria-label="React">
          {#each REACTIONS as e (e)}
            <button type="button" class="btn emoji" aria-label="React with {e}" onclick={() => { onreact(m, e); open = false; }}>{e}</button>
          {/each}
        </div>
      {/if}
      <div class="row">
        {#if canReply && m.kind !== 'announcement'}<button type="button" class="btn" data-chat="reply" onclick={() => { open = false; onreply(m); }}>Reply</button>{/if}
        <!-- The actions stay open behind the dialog, so focus can come back to this button. -->
        {#if !mine}<button type="button" class="btn quiet" data-chat="report" onclick={() => onreport(m)}><svg aria-hidden="true"><use href="#g-flag" /></svg>Report</button>{/if}
        {#if picture}<button type="button" class="btn quiet" data-chat="report-picture" onclick={() => onreport(m, 'picture')}><svg aria-hidden="true"><use href="#g-flag" /></svg>Report this picture</button>{/if}
        {#if !mine && m.attachment}<button type="button" class="btn quiet" data-chat="report-attachment" onclick={() => onreport(m, 'attachment')}
            ><svg aria-hidden="true"><use href="#g-flag" /></svg>{m.attachment.kind === 'document' ? 'Report this document' : 'Report this posted picture'}</button
          >{/if}
      </div>
    </div>
  {/if}
</li>

<style>
  .msg {
    display: grid;
    grid-template-columns: auto 1fr auto;
    column-gap: var(--s-2);
    padding: var(--s-2) 0 var(--s-2) var(--s-3);
    border-left: 3px solid transparent;
  }
  .text {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .msg + :global(.msg) {
    border-top: 1px solid var(--hairline);
  }
  .msg.mentioned {
    border-left-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .msg.announce {
    border-left-color: var(--axis);
    background: var(--surface-2);
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s-1) var(--s-2);
  }
  .who {
    font-weight: 650;
    overflow-wrap: anywhere;
  }
  .when {
    font-size: var(--fs-13);
    color: var(--muted);
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: var(--fs-12);
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    padding: 1px var(--s-1);
    border-radius: var(--r-1);
    border: 1px solid var(--axis);
    color: var(--ink-2);
    line-height: 1.3;
  }
  .tag svg {
    width: 12px;
    height: 12px;
  }
  .tag-station {
    border-color: var(--accent);
    color: var(--ink);
  }
  .tag-request {
    border-color: var(--status-good);
    color: var(--ink);
  }
  .tag-onair {
    border-color: var(--live-onair);
    color: var(--ink);
  }
  .more {
    appearance: none;
    align-self: start;
    margin-top: calc(-1 * var(--s-1));
    margin-left: calc(-1 * var(--s-1));
    width: var(--target-touch);
    height: var(--target-touch);
    display: inline-grid;
    place-items: center;
    background: transparent;
    border: 0;
    border-radius: var(--r-2);
    color: var(--ink-2);
    cursor: pointer;
  }
  .more:hover,
  .more[aria-expanded='true'] {
    background: var(--surface-2);
    color: var(--ink);
  }
  .more svg {
    width: 18px;
    height: 18px;
  }
  .replying {
    font-size: var(--fs-13);
    color: var(--ink-2);
  }
  .body {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  mark {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    color: inherit;
    border-radius: 2px;
    font-weight: 650;
  }
  .held {
    display: flex;
    align-items: center;
    gap: var(--s-1);
  }
  .held svg {
    width: 14px;
    height: 14px;
    color: var(--status-warning);
  }
  .reactions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-2);
  }
  .actions {
    grid-column: 1 / -1;
    display: grid;
    gap: var(--s-2);
    padding: var(--s-2) var(--s-3) var(--s-1) 0;
  }
  .react {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-1);
  }
  .emoji {
    width: var(--target-touch);
    padding: 0;
    font-size: var(--fs-20);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-2);
  }
</style>
