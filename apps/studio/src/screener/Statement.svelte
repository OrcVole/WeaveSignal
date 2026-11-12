<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The statement of reasons (screener.statement, D31, ADR-0032): over the Needs you column, it
  // owns the keyboard while open (keymap rule 5): `1` `2` `3` the action, `A` `S` `T` `I` `M` `E`
  // the reason, `⏎` hide and send, `Esc` keep the message (from the note field, the first `Esc`
  // leaves the field). The chat service writes the statement itself from the same facts; the
  // preview here is built from them, in the same few words.
  import { run } from '../lib/actions.ts';
  import { actorName, chat, chatNow } from '../lib/chat.svelte.ts';
  import { SOR_THENS, sorReasonsFor, statementPreview } from '../lib/chatModel.ts';
  import { hm, hms } from '../lib/format.ts';
  import ChatAvatar from './ChatAvatar.svelte';
  import ChatAttachment from './ChatAttachment.svelte';

  const s = $derived(chat.sor);
  /** A chatter's picture, refused or removed (ADR-0235), instead of a message; or a picture or
   *  document posted in the chat (ADR-0262). */
  const pic = $derived(s?.picture ?? null);
  const posted = $derived(pic?.kind === 'picture' || pic?.kind === 'document' ? pic.kind : null);
  const thing = $derived(posted === 'document' ? 'document' : 'picture');
  const refusing = $derived(s?.subject === 'avatar-refuse');
  let box: HTMLElement | undefined = $state();

  // Focus moves into the dialog when it opens (and back to the item when it closes: actions.ts).
  $effect(() => {
    box?.focus();
  });

  const preview = $derived(
    s
      ? statementPreview({
          subject: s.subject,
          posted,
          station: chat.station,
          show: chat.show,
          at: s.target ? hm(s.target.at_ms) : null,
          reason: s.reason ?? 'other',
          then: s.then,
          automated: s.disclose,
          info: chat.info,
        })
      : '',
  );
  const sendWord = $derived(
    !s ? '' : pic ? (refusing ? 'Refuse and send' : 'Remove and send') : s.bulk !== null ? `Hide all ${s.bulk} and send` : s.then === 'ban' ? 'Hide, ban and send' : s.then === 'timeout' ? 'Hide, time out and send' : 'Hide and send',
  );

  /** ⏎ in the note field sends, as it does anywhere else in the dialog. */
  function noteKeys(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      void run({ type: 'sor-send' });
    }
  }
</script>

{#if s}
  <div
    class="sor"
    data-panel="screener.statement"
    data-chat="sor"
    role="dialog"
    aria-modal="true"
    aria-labelledby="sor-t"
    aria-describedby="sor-d"
    tabindex="-1"
    bind:this={box}
  >
    <div class="head">
      <h3 id="sor-t" data-panel-title>
        {pic
          ? `${refusing ? 'Refuse' : 'Remove'} ${pic.name}'s ${thing} and tell them why`
          : s.bulk !== null
            ? `Hide all ${s.bulk} held messages and tell them why`
            : `Hide ${s.target?.name ?? 'this'}'s message and tell them why`}
      </h3>
      <span class="small muted" id="sor-d"
        >What you choose here is logged as you and sent to {s.bulk !== null ? 'each of them' : 'them'} at once.</span
      >
    </div>
    <div class="body">
      {#if pic && posted}
        <div class="msg pic-msg">
          {#if pic.attachment}<ChatAttachment a={pic.attachment} size={96} />{/if}
          <span class="txt"
            ><b>{pic.name}</b><br />{refusing ? 'Nobody else has seen it; its message is not shown.' : 'It comes down at once, with the message that carried it.'}
            {s.reason === 'suspected_csam'
              ? 'It is kept, with their details, for a year.'
              : s.reason === 'intimate_image'
                ? 'It is kept as evidence under a hold, for staff only, until an admin releases the hold.'
                : 'It is deleted.'}</span
          >
        </div>
      {:else if pic}
        <div class="msg pic-msg">
          <ChatAvatar avatar={null} name={pic.name} held={pic.id} size={64} square label="{pic.name}'s picture" />
          <span class="txt"
            ><b>{pic.name}</b><br />{refusing ? 'Nobody else has seen it.' : 'It comes down from every message at once.'}
            {s.reason === 'suspected_csam'
              ? 'It is kept, with their details, for a year.'
              : s.reason === 'intimate_image'
                ? 'It is kept as evidence under a hold, for staff only, until an admin releases the hold.'
                : 'It is deleted.'}
            Their messages show the avatar they chose from the set.</span
          >
        </div>
      {/if}
      {#if s.target}
        <div class="msg">
          <span class="who">{s.target.name} <small>{hms(s.target.at_ms)}{s.target.count ? ` · ${s.target.count} report${s.target.count === 1 ? '' : 's'}` : ''}</small></span>
          <span class="txt">{s.target.body}</span>
        </div>
      {/if}
      {#if s.bulk === null && !pic}
        <fieldset class="grp">
          <legend>What happens to them</legend>
          <div class="segs">
            {#each SOR_THENS as t (t.then)}
              <label class="btn" class:lit-on={s.then === t.then} data-chat="sor-then-{t.then}"
                ><input class="sr-only" type="radio" name="sor-then" value={t.then} bind:group={s.then} />{t.label} <kbd>{t.key}</kbd></label
              >
            {/each}
          </div>
        </fieldset>
      {/if}
      <fieldset class="grp">
        <legend>Why, in their words and ours</legend>
        <div class="reasons">
          {#each sorReasonsFor(!!pic) as r (r.reason)}
            <label class="radio" class:on={s.reason === r.reason} data-chat="sor-reason-{r.reason}"
              ><input class="sr-only" type="radio" name="sor-reason" value={r.reason} bind:group={s.reason} onchange={() => s && (s.problem = '')} /><i
                class:on={s.reason === r.reason}
              ></i><span>{r.label}</span> <kbd>{r.key}</kbd></label
            >
          {/each}
        </div>
      </fieldset>
      {#if pic && s.reason === 'suspected_csam'}
        <p class="evidence" role="note" data-chat="sor-evidence">
          <b>This is not deleted.</b> The station keeps the {thing} (as uploaded, if it was still waiting) and the uploader's details for at least a year, and
          only the one person named in Settings can see them. Do not forward or copy the picture. Report it to the authorities: the how-to
          “Handle a picture you suspect is child sexual abuse material” says whom and how.
        </p>
      {/if}
      {#if s.bulk === null}
        <div class="field">
          <label for="sor-note">Note for the log only <span class="opt">optional</span></label>
          <input class="in" id="sor-note" autocomplete="off" bind:value={s.note} onkeydown={noteKeys} />
        </div>
      {/if}
      {#if !pic}
        <label class="chk-box disclose"
          ><input type="checkbox" bind:checked={s.disclose} disabled={!!s.automated} />{s.automated
            ? `Say that an automatic filter was involved (it was: ${s.automated})`
            : 'Say that an automatic filter was involved'}</label
        >
      {/if}
      <div class="preview" role="group" aria-labelledby="sor-from">
        <span class="from" id="sor-from">{s.bulk !== null ? 'Each of them receives' : 'They receive'}, from {chat.station || 'the station'}, at once:</span>
        <span>{s.reason ? preview : 'Choose why, and the statement shows here.'}</span>
      </div>
    </div>
    <div class="foot">
      <span class="left">Logged as {actorName()} · screener · {hms(chatNow())}</span>
      {#if s.problem}<span class="problem" role="alert">{s.problem}</span>{/if}
      <button type="button" class="btn" data-chat="sor-cancel" onclick={() => run({ type: 'sor-cancel' })}
        >{pic ? (refusing ? 'Leave it waiting' : 'Keep the picture') : s.bulk !== null ? 'Keep them' : 'Keep the message'} <kbd>Esc</kbd></button
      >
      <button type="button" class="btn primary" data-chat="sor-send" disabled={s.sending} onclick={() => run({ type: 'sor-send' })}
        >{sendWord} <kbd>⏎</kbd></button
      >
    </div>
  </div>
{/if}

<style>
  /* The one reason that keeps a picture instead of deleting it says so before it is sent (ADR-0257). */
  :global(.screener .sor .evidence) {
    margin: 0;
    padding: var(--s-2) var(--s-3);
    border: 1.5px solid var(--status-serious);
    border-left-width: 4px;
    border-radius: var(--r-2);
    background: var(--surface);
    font-size: var(--fs-13);
    line-height: var(--lh-body);
    color: var(--ink);
  }
  /* The dialog's groups are fieldsets (for their names); the design drew them as plain fields. */
  :global(.screener .sor fieldset.grp) {
    border: 0;
    margin: 0;
    padding: 0;
    min-width: 0;
    display: grid;
    gap: 2px;
  }
  /* The groups' names, as the design's field labels. */
  :global(.screener .sor fieldset.grp legend) {
    padding: 0;
    margin-bottom: 2px;
    font-size: var(--fs-12);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
    color: var(--muted);
    font-weight: 600;
  }
  :global(.screener .sor .segs label.btn) {
    cursor: pointer;
  }
  :global(.screener .sor .field .opt) {
    text-transform: none;
    letter-spacing: 0;
    color: var(--muted);
    font-weight: 400;
  }
  :global(.screener .sor .field input.in) {
    font: inherit;
    font-size: var(--fs-14);
    width: 100%;
  }
  /* Native radios, hidden, behind the drawn ones: the drawn control shows their focus. */
  :global(.screener .sor label:has(input:focus-visible)) {
    outline: var(--focus-ring);
    outline-offset: var(--focus-offset);
  }
  :global(.screener .sor .radio) {
    cursor: pointer;
  }
  :global(.screener .sor .disclose input) {
    accent-color: var(--accent);
    width: 1rem;
    height: 1rem;
    margin: 0;
  }
  :global(.screener .sor .disclose) {
    font-size: var(--fs-13);
    color: var(--ink-2);
  }
  :global(.screener .sor .problem) {
    font-size: var(--fs-13);
    color: var(--ink);
    font-weight: 600;
  }
  :global(.screener .sor:focus) {
    outline: none;
  }
  :global(.screener .sor .msg.pic-msg) {
    grid-template-columns: auto minmax(0, 1fr);
    gap: var(--s-3);
    align-items: center;
  }
</style>
