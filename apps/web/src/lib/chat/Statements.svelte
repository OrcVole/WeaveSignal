<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Notices to this chatter from the station: a statement of reasons for each action taken on
  // their messages or account (DSA Article 17), and a way to ask for a review while it is open.
  import type { Statement } from '../../../../../packages/ui/chat.ts';
  import * as api from './api.ts';
  import { dayTime, reviewOpen } from './model.ts';

  let { statements, session, nowServerMs, onreviewed }: { statements: Statement[]; session: string | null; nowServerMs: number; onreviewed: (id: string) => void } = $props();

  let asking = $state<string | null>(null);
  let text = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);
  let problem = $state<string | null>(null);
  let asked = $state<string | null>(null);

  const newestFirst = $derived([...statements].sort((a, b) => b.at_ms - a.at_ms));

  function ask(id: string) {
    asking = id;
    text = '';
    error = problem = null;
    queueMicrotask(() => document.getElementById(`st-text-${id}`)?.focus());
  }

  async function send(e: SubmitEvent, id: string) {
    e.preventDefault();
    if (!session) return;
    error = problem = null;
    if (!text.trim()) {
      error = 'Say what the station should look at again.';
      document.getElementById(`st-text-${id}`)?.focus();
      return;
    }
    busy = true;
    try {
      await api.review(session, id, text.trim());
      asked = id;
      asking = null;
      onreviewed(id);
    } catch (err) {
      problem = err instanceof Error ? err.message : String(err);
    } finally {
      busy = false;
    }
  }
</script>

{#if statements.length}
  <section class="card notices" aria-labelledby="st-h">
    <h3 id="st-h">Notices to you</h3>
    <ol>
      {#each newestFirst as s (s.id)}
        <li data-chat="statement" data-id={s.id}>
          <p class="small"><time datetime={new Date(s.at_ms).toISOString()}>{dayTime(s.at_ms)}</time> · from the station</p>
          <p class="words">{s.text}</p>
          {#if s.automated}<p class="small">An automated filter was involved in this decision.</p>{/if}
          {#if s.review_requested || asked === s.id}
            <p class="small" role="status">You asked for a review. The station's moderators will look again.</p>
          {:else if reviewOpen(s, nowServerMs)}
            {#if asking === s.id}
              <form class="review" novalidate onsubmit={(e) => send(e, s.id)}>
                <div class="field">
                  <label for="st-text-{s.id}">What should the station look at again?</label>
                  <textarea id="st-text-{s.id}" rows="3" maxlength="2000" bind:value={text} aria-invalid={!!error} aria-describedby={error ? `st-err-${s.id}` : undefined}></textarea>
                  {#if error}<span class="err" id="st-err-{s.id}"><svg aria-hidden="true"><use href="#g-fault" /></svg>{error}</span>{/if}
                </div>
                {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
                <button type="submit" class="btn primary wide" disabled={busy} data-chat="review-submit">{busy ? 'Sending…' : 'Ask for a review'}</button>
                <button type="button" class="btn quiet wide" onclick={() => (asking = null)}>Cancel</button>
              </form>
            {:else}
              <button type="button" class="btn" onclick={() => ask(s.id)} data-chat="review">Ask for a review</button>
              <p class="small">You can ask until {dayTime(s.review_by_ms)}.</p>
            {/if}
          {:else}
            <p class="small">The time to ask for a review ended on {dayTime(s.review_by_ms)}.</p>
          {/if}
        </li>
      {/each}
    </ol>
  </section>
{/if}

<style>
  .notices {
    border-left: 3px solid var(--status-warning);
  }
  h3 {
    font-size: var(--fs-16);
  }
  ol {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--s-3);
  }
  li {
    display: grid;
    gap: var(--s-2);
    justify-items: start;
  }
  li + li {
    border-top: 1px solid var(--hairline);
    padding-top: var(--s-3);
  }
  .words {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .review {
    display: grid;
    gap: var(--s-2);
    width: 100%;
  }
</style>
