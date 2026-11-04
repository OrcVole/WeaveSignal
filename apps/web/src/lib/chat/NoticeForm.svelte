<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A notice from someone without an account (DSA Article 16): what is wrong, where it is, why,
  // who is sending it, and their statement that it is accurate and complete. A real <dialog>.
  import { tick } from 'svelte';
  import type { ChatInfo, NoticeRequest, Reason } from '../../../../../packages/ui/chat.ts';
  import * as api from './api.ts';
  import { looksLikeEmail } from './model.ts';

  let { info, location, onclose }: { info: ChatInfo | null; location: string | null; onclose: () => void } = $props();

  let dialog = $state<HTMLDialogElement | undefined>();
  let heading = $state<HTMLElement | undefined>();
  let opener: HTMLElement | null = null;
  let reason = $state<Reason | ''>('');
  let where = $state('');
  let detail = $state('');
  let name = $state('');
  let email = $state('');
  let goodFaith = $state(false);
  let errors = $state<Record<string, string>>({});
  let problem = $state<string | null>(null);
  let busy = $state(false);
  let ref = $state<string | null>(null);

  // Article 16(2)(c): a notice about child sexual abuse need not name its sender.
  const anonymousOk = $derived(reason === 'child_safety');

  $effect(() => {
    if (location === null || !dialog || dialog.open) return;
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    reason = '';
    where = location;
    detail = '';
    goodFaith = false;
    errors = {};
    problem = ref = null;
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
    problem = null;
    const out: Record<string, string> = {};
    if (!reason) out.reason = 'Choose what is wrong.';
    if (!where.trim()) out.location = 'Say where it is: a name, a message, a time.';
    if (!detail.trim()) out.detail = 'Say why it should come down.';
    if (!anonymousOk && !name.trim()) out.name = 'Enter your name.';
    if (!anonymousOk && !looksLikeEmail(email)) out.email = 'Enter your email address, like name@example.org.';
    if (anonymousOk && email.trim() && !looksLikeEmail(email)) out.email = 'Check your email address, or leave it empty.';
    if (!goodFaith) out.good_faith = 'Tick this to confirm the notice.';
    errors = out;
    const first = ['reason', 'location', 'detail', 'name', 'email', 'good_faith'].find((k) => out[k]);
    if (first) {
      (first === 'reason' ? document.querySelector<HTMLInputElement>('#nt-reasons input') : document.getElementById(`nt-${first}`))?.focus();
      return;
    }
    const req: NoticeRequest = { reason: reason as Reason, location: where.trim(), detail: detail.trim(), name: name.trim(), email: email.trim(), good_faith: goodFaith };
    busy = true;
    try {
      ref = (await api.notice(req)).id;
      await tick();
      heading?.focus();
    } catch (err) {
      if (err instanceof api.ApiError && err.field && ['reason', 'location', 'detail', 'name', 'email', 'good_faith'].includes(err.field)) {
        errors = { [err.field]: err.message };
        document.getElementById(`nt-${err.field}`)?.focus();
      } else {
        problem = err instanceof Error ? err.message : String(err);
      }
    } finally {
      busy = false;
    }
  }

  const desc = (id: string, hint?: string) => [hint, errors[id] ? `nt-${id}-err` : ''].filter(Boolean).join(' ') || undefined;
</script>

{#snippet err(id: string)}
  {#if errors[id]}<span class="err" id="nt-{id}-err"><svg aria-hidden="true"><use href="#g-fault" /></svg>{errors[id]}</span>{/if}
{/snippet}

<dialog class="sheet" bind:this={dialog} onclose={closed} aria-labelledby="nt-h">
  {#if location !== null}
    {#if ref}
      <div class="inner">
        <h2 id="nt-h" tabindex="-1" bind:this={heading}>Notice sent</h2>
        <p>{info?.station || 'The station'}'s moderators will look at it.</p>
        <p class="small">Your reference: <b class="num">{ref}</b>. Quote it if you write to the station about this notice.</p>
        <button type="button" class="btn primary wide" onclick={() => dialog?.close()}>Done</button>
      </div>
    {:else}
      <form class="inner" novalidate onsubmit={submit}>
        <h2 id="nt-h" tabindex="-1" bind:this={heading}>Report something</h2>
        <p class="small">No account needed. The station's moderators read every notice.</p>

        <fieldset class="group" id="nt-reasons" data-chat="notice-reason" aria-describedby={errors.reason ? 'nt-reason-err' : undefined}>
          <legend>What is wrong?</legend>
          {#each info?.reasons ?? [] as r (r.reason)}
            <label class="check">
              <input type="radio" name="nt-reason" value={r.reason} bind:group={reason} aria-describedby={r.hint ? `nt-hint-${r.reason}` : undefined} />
              <span>{r.words}{#if r.hint}<span class="small" id="nt-hint-{r.reason}">{r.hint}</span>{/if}</span>
            </label>
          {:else}
            <p class="small">The reasons could not load. Close this and try again in a moment.</p>
          {/each}
          {@render err('reason')}
        </fieldset>

        <div class="field">
          <label for="nt-location">Where is it?</label>
          <span class="hint" id="nt-location-hint">A chatter's name and their message, or a time in a show.</span>
          <textarea id="nt-location" rows="2" maxlength="1000" bind:value={where} aria-invalid={!!errors.location} aria-describedby={desc('location', 'nt-location-hint')}></textarea>
          {@render err('location')}
        </div>

        <div class="field">
          <label for="nt-detail">Why should it come down?</label>
          <span class="hint" id="nt-detail-hint">Why it is unlawful, or which of the chat terms it breaks.</span>
          <textarea id="nt-detail" rows="4" maxlength="4000" bind:value={detail} aria-invalid={!!errors.detail} aria-describedby={desc('detail', 'nt-detail-hint')}></textarea>
          {@render err('detail')}
        </div>

        <div class="field">
          <label for="nt-name">Your name</label>
          {#if anonymousOk}<span class="hint" id="nt-name-hint">Optional for a notice about a child's safety.</span>{/if}
          <input id="nt-name" type="text" autocomplete="name" maxlength="120" bind:value={name} aria-invalid={!!errors.name} aria-describedby={desc('name', anonymousOk ? 'nt-name-hint' : undefined)} />
          {@render err('name')}
        </div>

        <div class="field">
          <label for="nt-email">Your email</label>
          <span class="hint" id="nt-email-hint">{anonymousOk ? 'Optional. ' : ''}For the station's answer. Only its moderators see it.</span>
          <input id="nt-email" type="email" autocomplete="email" inputmode="email" maxlength="254" bind:value={email} aria-invalid={!!errors.email} aria-describedby={desc('email', 'nt-email-hint')} />
          {@render err('email')}
        </div>

        <div class="field">
          <label class="check">
            <input id="nt-good_faith" type="checkbox" bind:checked={goodFaith} aria-invalid={!!errors.good_faith} aria-describedby={desc('good_faith')} />
            <span>I believe this notice is accurate and complete</span>
          </label>
          {@render err('good_faith')}
        </div>

        {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
        <button type="submit" class="btn primary wide" disabled={busy || !info} data-chat="notice-submit">{busy ? 'Sending…' : 'Send notice'}</button>
        <button type="button" class="btn quiet wide" onclick={() => dialog?.close()}>Cancel</button>
      </form>
    {/if}
  {/if}
</dialog>
