<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Report a message, the person who wrote it, the picture of their own beside it, or a picture or
  // document the message carries (ADR-0032, ADR-0260, ADR-0262), with the station's reasons. A real
  // <dialog>: focus moves to its heading on opening and back to the button that opened it.
  import { tick } from 'svelte';
  import type { ChatInfo, Msg, Reason, ReportRequest } from '../../../../../packages/ui/chat.ts';
  import * as api from './api.ts';
  import Avatar from '../../../../../packages/ui/Avatar.svelte';
  import { hhmm, reportablePicture } from './model.ts';

  let {
    info,
    session,
    target,
    opensOn = 'message',
    onclose,
  }: { info: ChatInfo | null; session: string | null; target: Msg | null; opensOn?: 'message' | 'picture' | 'attachment'; onclose: () => void } = $props();

  let dialog = $state<HTMLDialogElement | undefined>();
  let heading = $state<HTMLElement | undefined>();
  let opener: HTMLElement | null = null;
  let what = $state<'message' | 'account' | 'picture' | 'attachment'>('message');
  const picture = $derived(target ? reportablePicture(target) : null);
  let reason = $state<Reason | ''>('');
  let detail = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);
  let problem = $state<string | null>(null);
  let done = $state(false);

  $effect(() => {
    if (!target || !dialog || dialog.open) return;
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    what =
      opensOn === 'picture' && target && reportablePicture(target)
        ? 'picture'
        : opensOn === 'attachment' && target?.attachment
          ? 'attachment'
          : 'message';
    reason = '';
    detail = '';
    error = problem = null;
    done = false;
    dialog.showModal();
    void tick().then(() => heading?.focus());
  });

  function closed() {
    opener?.focus();
    opener = null;
    onclose();
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!target || !session) return;
    error = problem = null;
    if (!reason) {
      error = 'Choose a reason.';
      document.querySelector<HTMLInputElement>('#rp-reasons input')?.focus();
      return;
    }
    const req: ReportRequest =
      what === 'picture' && picture
        ? { account_id: target.author.id, avatar_id: picture, reason }
        : what === 'attachment' && target.attachment
          ? { message_id: target.id, attachment_id: target.attachment.id, reason }
          : what === 'account'
          ? { account_id: target.author.id, reason }
          : { message_id: target.id, reason };
    if (detail.trim()) req.detail = detail.trim();
    busy = true;
    try {
      await api.report(session, req);
      done = true;
      await tick();
      heading?.focus();
    } catch (err) {
      problem = err instanceof Error ? err.message : String(err);
    } finally {
      busy = false;
    }
  }
</script>

<dialog class="sheet" bind:this={dialog} onclose={closed} aria-labelledby="rp-h">
  {#if target}
    {#if done}
      <div class="inner">
        <h2 id="rp-h" tabindex="-1" bind:this={heading}>Report sent</h2>
        <p>A moderator at {info?.station || 'the station'} will look at it. If they act on it, the chatter is told why.</p>
        <button type="button" class="btn primary wide" onclick={() => dialog?.close()}>Done</button>
      </div>
    {:else}
      <form class="inner" novalidate onsubmit={submit}>
        <h2 id="rp-h" tabindex="-1" bind:this={heading}>Report</h2>
        <blockquote class="quote">
          <span class="small"><b>{target.author.name}</b> at {hhmm(target.server_ms)}</span>
          <span class="said">{target.body}</span>
        </blockquote>

        <fieldset class="group">
          <legend>What are you reporting?</legend>
          <label class="check"><input type="radio" name="rp-what" value="message" bind:group={what} /><span>This message</span></label>
          <label class="check"><input type="radio" name="rp-what" value="account" bind:group={what} data-chat="report-user" /><span>{target.author.name}, for what they post</span></label>
          {#if target.attachment}
            <label class="check"
              ><input type="radio" name="rp-what" value="attachment" bind:group={what} data-chat="report-attachment-choice" /><span
                >The {target.attachment.kind === 'document' ? 'document' : 'picture'} in this message, “{target.attachment.name}”</span
              ></label
            >
          {/if}
          {#if picture}
            <label class="check"
              ><input type="radio" name="rp-what" value="picture" bind:group={what} data-chat="report-picture-choice" /><span class="pic"
                ><Avatar avatar={target.author.avatar} name={target.author.name} size={28} />This picture, beside {target.author.name}'s messages</span
              ></label
            >
          {/if}
        </fieldset>

        <fieldset class="group" id="rp-reasons" data-chat="report-reason" aria-describedby={error ? 'rp-err' : undefined}>
          <legend>Why?</legend>
          {#each info?.reasons ?? [] as r (r.reason)}
            <label class="check">
              <input type="radio" name="rp-reason" value={r.reason} bind:group={reason} aria-describedby={r.hint ? `rp-hint-${r.reason}` : undefined} data-reason={r.reason} />
              <span>{r.words}{#if r.hint}<span class="small" id="rp-hint-{r.reason}">{r.hint}</span>{/if}</span>
            </label>
          {:else}
            <p class="small">The reasons could not load. Close this and try again in a moment.</p>
          {/each}
          {#if error}<span class="err" id="rp-err"><svg aria-hidden="true"><use href="#g-fault" /></svg>{error}</span>{/if}
        </fieldset>

        <div class="field">
          <label for="rp-detail">Anything the moderators should know</label>
          <span class="hint" id="rp-detail-hint">Optional.</span>
          <textarea id="rp-detail" rows="3" maxlength="2000" bind:value={detail} aria-describedby="rp-detail-hint"></textarea>
        </div>

        {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
        <button type="submit" class="btn primary wide" disabled={busy || !info} data-chat="report-submit">{busy ? 'Sending…' : 'Send report'}</button>
        <button type="button" class="btn quiet wide" onclick={() => dialog?.close()}>Cancel</button>
      </form>
    {/if}
  {/if}
</dialog>
