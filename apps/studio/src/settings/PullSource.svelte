<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A show that comes from a stream the station pulls (1.2, PLAN 17b): a partner station's
  // stream, or a host's own server. Two states, drawn inside the show editor's Comes from
  // (`calendar.show-source`):
  //
  //   no pull yet   the address, a name for the desk, and Pull this stream. The address is
  //                 read as it is typed (web audio, an HLS playlist, SRT) and a slip is said
  //                 under it; the station checks it again and its refusal lands there too.
  //   a pull kept   the address, its kind, and where it stands now (open and standing by, on
  //                 air, nothing arriving yet, or not open until the show's time).
  //
  // The key the station makes for a pull has a monitor link like any key, shown this once: the
  // people at the other end may keep it open to see when they are on air. The key's own secret
  // has no use for a pull and is not shown.
  import { untrack } from 'svelte';
  import StateChip from '../../../../packages/ui/StateChip.svelte';
  import { CHIP } from '../../../../packages/ui/states.ts';
  import { makeShowKey, type MadeKey, type SourceNow } from '../lib/api.ts';
  import { placeError } from '../lib/account.ts';
  import { LABEL_MAX, MONITOR_BY_HAND, labelProblem, monitorLink } from '../lib/hosts.ts';
  import { PULL_KIND_WORD, PULL_URL_MAX, pullKindOf, pullProblem, pullRefusal, pullStatus } from '../lib/pull.ts';
  import { loadPublicAddress, sched } from '../lib/schedule.svelte.ts';

  let {
    slug,
    name,
    url = null,
    now = null,
    blocked = '',
    onMade,
  }: {
    slug: string;
    name: string;
    /** The address kept, once a pull is made. */
    url?: string | null;
    /** What the station lists of the pull's key now. */
    now?: SourceNow | null;
    /** Why a pull cannot be made yet (the show has a host's keys); empty when it can. */
    blocked?: string;
    onMade: () => void | Promise<void>;
  } = $props();

  let address = $state('');
  let label = $state(untrack(() => name));
  let errors = $state<{ url?: string; label?: string; form?: string }>({});
  let making = $state(false);
  let made = $state<{ link: string | null; secret: string } | null>(null);
  let copied = $state(false);
  let copyNote = $state('');

  const kind = $derived(pullKindOf(address));
  const keptKind = $derived(url ? pullKindOf(url) : null);
  const status = $derived(pullStatus(now, sched.zone));
  const chip = $derived(
    status.state === 'on_air' ? { chip: CHIP.onAir, word: 'On air' } : status.state === 'standby' ? { chip: CHIP.standby, word: 'Standby' } : status.state === 'trying' ? { chip: CHIP.notConnected, word: 'Not connected' } : null,
  );

  async function make(e: SubmitEvent) {
    e.preventDefault();
    if (making) return;
    const errs: typeof errors = {};
    const u = pullProblem(address);
    if (u) errs.url = u;
    const l = labelProblem(label);
    if (l) errs.label = l;
    errors = errs;
    if (errs.url) return document.getElementById('ps-url')?.focus();
    if (errs.label) return document.getElementById('ps-label')?.focus();
    making = true;
    try {
      const m: MadeKey = await makeShowKey(slug, { label: label.trim(), hint: 'pull', url: address.trim() });
      const publicUrl = await loadPublicAddress();
      made = { link: monitorLink(publicUrl, m.monitor_secret), secret: m.monitor_secret };
      address = '';
      await onMade();
    } catch (err) {
      const placed = placeError(err, ['url', 'label'] as const);
      if (placed.field === 'url') errors = { url: pullRefusal(placed.message) };
      else if (placed.field === 'label') errors = { label: placed.message };
      else errors = { form: placed.message };
      document.getElementById(placed.field === 'label' ? 'ps-label' : 'ps-url')?.focus();
    } finally {
      making = false;
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
      copyNote = 'Copied.';
    } catch {
      copyNote = 'This window could not reach the clipboard. Select the link and copy it by hand.';
    }
  }
</script>

{#if url}
  <div class="pull" data-testid="pulled">
    <p class="line"><b>Pulled by the station</b> from <code>{url}</code></p>
    <p class="now" role="status">
      {#if chip}<StateChip chip={chip.chip} word={chip.word} sm focusable={false} hoverable={false} />{/if}
      <span>{keptKind ? `${PULL_KIND_WORD[keptKind]}. ` : ''}{status.words}</span>
    </p>
    {#if made}
      <div class="once" data-testid="pull-monitor">
        <span class="lab" id="ps-mon-l">Monitor link, shown this once</span>
        <div class="row">
          <input id="ps-mon" type="text" readonly value={made.link ?? made.secret} aria-labelledby="ps-mon-l" spellcheck="false" onfocus={(e) => e.currentTarget.select()} />
          <button type="button" class="btn sm" onclick={() => copy(made!.link ?? made!.secret)}>{copied ? 'Copied' : 'Copy'}</button>
          <button type="button" class="btn sm quiet" onclick={() => (made = null)}>Done</button>
        </div>
        <span class="hint">{made.link ? 'A page for the people at the other end: it says when they are on air and plays the station. The station cannot show it again.' : `${MONITOR_BY_HAND} The station cannot show it again.`}</span>
        <span class="hint" role="status" aria-live="polite">{copyNote}</span>
      </div>
    {/if}
    <p class="hint">To change the address, end the key below and pull the new one.</p>
  </div>
{:else if blocked}
  <p class="src-blocked" data-testid="pull-blocked">{blocked}</p>
{:else}
  <form class="pull" onsubmit={make} novalidate data-testid="pull-form">
    <div class="f">
      <label for="ps-url">The stream’s address</label>
      <input
        id="ps-url"
        type="url"
        inputmode="url"
        bind:value={address}
        oninput={() => errors.url && (errors = { ...errors, url: undefined })}
        maxlength={PULL_URL_MAX}
        autocomplete="off"
        autocapitalize="none"
        spellcheck="false"
        placeholder="https://radio.example.org/live.mp3"
        aria-invalid={errors.url ? true : undefined}
        aria-describedby={errors.url ? 'ps-url-err ps-url-hint' : 'ps-url-hint'}
      />
      <span id="ps-url-hint" class="hint" data-testid="pull-kind"
        >{#if kind}<b>{PULL_KIND_WORD[kind]}.</b>
        {:else}An Icecast or other web stream (http:// or https://), an HLS playlist ending .m3u8, or srt://server:port.
        {/if} No user name, password or passphrase in it: the station cannot keep them yet.</span
      >
      {#if errors.url}<p id="ps-url-err" class="err">{errors.url}</p>{/if}
    </div>
    <div class="f">
      <label for="ps-label">Its name on the desk</label>
      <input id="ps-label" type="text" bind:value={label} oninput={() => errors.label && (errors = { ...errors, label: undefined })} maxlength={LABEL_MAX} autocomplete="off" aria-invalid={errors.label ? true : undefined} aria-describedby={errors.label ? 'ps-label-err' : undefined} />
      {#if errors.label}<p id="ps-label-err" class="err">{errors.label}</p>{/if}
    </div>
    {#if errors.form}<p class="err" role="alert">{errors.form}</p>{/if}
    <div class="acts">
      <button type="submit" class="btn sm primary" disabled={making} aria-busy={making || undefined}>{making ? 'Checking the address…' : 'Pull this stream'}</button>
      <span class="hint">The station opens it before {name}’s time. It stands by on the desk, and goes on air at the show’s time.</span>
    </div>
  </form>
{/if}

<style>
  .pull {
    display: grid;
    gap: var(--s-2);
    border: 1px solid var(--hairline);
    border-radius: var(--r-1);
    background: var(--surface-2);
    padding: var(--s-2) var(--s-3);
    min-width: 0;
  }
  .line,
  .now {
    font-size: var(--fs-14);
    line-height: var(--lh-label);
    overflow-wrap: anywhere;
  }
  .now {
    display: flex;
    gap: var(--s-2);
    align-items: center;
    flex-wrap: wrap;
    color: var(--ink-2);
  }
  code {
    font-family: var(--font-mono);
    font-size: var(--fs-13);
  }
  .f {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  label,
  .lab {
    font-size: var(--fs-13);
    font-weight: 600;
  }
  input {
    width: 100%;
    min-width: 0;
  }
  .hint {
    font-size: var(--fs-12);
    color: var(--ink-2);
    line-height: var(--lh-label);
  }
  .hint b {
    color: var(--ink);
  }
  .err {
    font-size: var(--fs-13);
    color: var(--status-serious);
    line-height: var(--lh-label);
  }
  .acts {
    display: flex;
    gap: var(--s-2);
    align-items: center;
    flex-wrap: wrap;
  }
  .acts .hint {
    flex: 1 1 12rem;
  }
  .once {
    display: grid;
    gap: 2px;
    border-top: 1px solid var(--hairline);
    padding-top: var(--s-2);
  }
  .once .row {
    display: flex;
    gap: var(--s-1);
  }
  .once input {
    flex: 1;
    font-family: var(--font-mono);
    font-size: var(--fs-12);
  }
  .src-blocked {
    font-size: var(--fs-13);
    color: var(--ink-2);
    line-height: var(--lh-label);
  }
</style>
