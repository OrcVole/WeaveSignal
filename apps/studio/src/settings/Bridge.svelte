<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Settings › Bridge (`settings.bridge`; Phase 17g): the host's own software sends to this
  // computer, and the app carries it to the station over one connection on the web's usual
  // port, so the station needs no source port and the host's network needs nothing but HTTPS.
  // The packaged app only: a browser build has no bridge, and draws nothing.
  //
  // Three parts, in the order a host meets them: where it stands now, with Start and Stop; the
  // station and the show key (the key is written once and kept in this computer's keychain);
  // and what to type into their own software, each line with Copy.
  import { onDestroy, onMount, untrack } from 'svelte';
  import StateChip from '../../../../packages/ui/StateChip.svelte';
  import { packaged } from '../lib/api.ts';
  import {
    bridgeChip,
    bridgeForget,
    bridgeNewPassword,
    bridgeSave,
    bridgeStart,
    bridgeStop,
    bridgeView,
    droppedKb,
    keyProblem,
    softwareFields,
    uplinkWords,
    type BridgeView,
  } from '../lib/bridge.ts';

  /** `initial` is a test's view: the panel is then drawn in a browser build too, and asks nothing. */
  let { initial = null }: { initial?: BridgeView | null } = $props();

  const seed = untrack(() => initial);
  const here = packaged() || !!seed;
  let view = $state<BridgeView | null>(seed);
  let url = $state(seed?.saved.url ?? '');
  let password = $state('');
  let icecastPort = $state(seed?.saved.icecast_port ?? 8100);
  let srtPort = $state(seed?.saved.srt_port ?? 9100);
  let error = $state('');
  let note = $state('');
  let busy = $state(false);
  let copied = $state<string | null>(null);
  let timer: ReturnType<typeof setInterval> | undefined;

  const words = (e: unknown) => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));
  const problem = $derived(keyProblem(password));
  const chip = $derived(view ? bridgeChip(view) : null);
  const fields = $derived(view?.local_password ? softwareFields(view) : null);
  const dirty = $derived(!!view && (url.trim() !== view.saved.url || !!password.trim() || icecastPort !== view.saved.icecast_port || srtPort !== view.saved.srt_port));
  const portsProblem = $derived(
    !Number.isInteger(icecastPort) || !Number.isInteger(srtPort) || icecastPort < 1024 || srtPort < 1024 || icecastPort > 65535 || srtPort > 65535 ? 'A port here is a whole number from 1024 to 65535.' : icecastPort === srtPort ? 'Give the two ports different numbers.' : null,
  );
  const canStart = $derived(!!view?.has_key && !!view?.saved.url);

  async function act(f: () => Promise<BridgeView>, done = '') {
    if (busy) return;
    busy = true;
    error = '';
    note = '';
    try {
      view = await f();
      note = done;
    } catch (e) {
      error = words(e);
    } finally {
      busy = false;
    }
  }

  onMount(async () => {
    if (!here || seed) return;
    try {
      const v = await bridgeView();
      view = v;
      url = v.saved.url;
      icecastPort = v.saved.icecast_port;
      srtPort = v.saved.srt_port;
    } catch (e) {
      error = words(e);
    }
    timer = setInterval(async () => {
      if (view?.running) view = await bridgeView().catch(() => view);
    }, 1000);
  });
  onDestroy(() => clearInterval(timer));

  function onSave(e: SubmitEvent) {
    e.preventDefault();
    if (problem) return document.getElementById('bridge-key')?.focus();
    if (portsProblem) return document.getElementById('bridge-ice')?.focus();
    void act(async () => {
      const v = await bridgeSave(url.trim(), password.trim() || null, icecastPort, srtPort);
      password = '';
      return v;
    }, 'Saved on this computer.');
  }

  async function copy(id: string, text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = id;
      note = 'Copied.';
    } catch {
      note = 'This window could not reach the clipboard. Select the text and copy it by hand.';
    }
  }
</script>

{#if here}
  <section class="panel" data-panel="settings.bridge" aria-labelledby="bridge-h">
    <div class="ph">
      <h2 id="bridge-h" data-panel-title>Bridge</h2>
      {#if chip}<StateChip chip={chip.chip} word={chip.word} />{/if}
    </div>
    <p class="help">
      Send from your own software (BUTT, Mixxx, OBS, Voicemeeter) to this computer, and the studio app carries it to the station over the web’s usual port. Nothing has to be opened on your router or at the station. Your software mixes your voice and your music together.
    </p>

    {#if error}<p class="err" role="alert">{error}</p>{/if}

    {#if view}
      <div class="now" data-uplink={view.status.uplink} data-testid="bridge-now">
        <div class="says" role="status" aria-live="polite">
          <b>{uplinkWords(view.status)}</b>
          {#if view.status.station_message}<span>{view.status.station_message}</span>{/if}
          {#if view.status.software}<span class="num">{view.status.software} · {view.status.format ?? 'format not read yet'} · {view.status.kbps} kbit/s</span>{/if}
          {#if view.status.dropped_bytes > 0}<span>{droppedKb(view.status.dropped_bytes)} dropped while the network fell behind.</span>{/if}
        </div>
        <div class="acts">
          {#if view.running}
            <button type="button" class="btn" disabled={busy} onclick={() => act(bridgeStop, 'The bridge is stopped.')}>Stop the bridge</button>
          {:else}
            <button type="button" class="btn primary" disabled={busy} aria-disabled={canStart ? undefined : 'true'} title={canStart ? undefined : 'Save the station’s address and a show key first.'} onclick={() => canStart && act(bridgeStart)}>Start the bridge</button>
          {/if}
        </div>
        {#if view.status.too_fast}
          <p class="alert" role="alert" data-testid="bridge-too-fast"><svg class="g-warning" aria-hidden="true"><use href="#g-fault" /></svg><span><b>Send sound only.</b> Your software sends more than the station takes: it is probably sending a picture too. Turn the picture off in it, or lower its bitrate.</span></p>
        {/if}
      </div>
    {:else if !error}
      <p role="status" class="small">Reading the bridge…</p>
    {/if}

    <div class="parts">
      <form class="part" onsubmit={onSave} novalidate aria-labelledby="bridge-st-h">
        <h3 id="bridge-st-h">The station and your key</h3>
        <div class="field">
          <label for="bridge-url">The station’s public address</label>
          <input id="bridge-url" type="url" bind:value={url} autocomplete="off" autocapitalize="none" spellcheck="false" placeholder="https://radio.example.org" aria-describedby="bridge-url-hint" disabled={view?.running} />
          <span id="bridge-url-hint" class="hint">Where listeners go. On Cloudron that is the public pages’ address, not the studio’s: the studio’s sits behind Cloudron’s sign-in, which the bridge cannot pass.</span>
        </div>
        <div class="field">
          <label for="bridge-key">Your show key</label>
          <input
            id="bridge-key"
            type="password"
            bind:value={password}
            autocomplete="off"
            spellcheck="false"
            placeholder={view?.has_key ? `Saved: ${view.saved.key_id}-…` : 'abcd2345-…'}
            aria-invalid={problem ? true : undefined}
            aria-describedby={problem ? 'bridge-key-err bridge-key-hint' : 'bridge-key-hint'}
            disabled={view?.running}
          />
          <span id="bridge-key-hint" class="hint">The Icecast password from your details: the key’s 8 characters, a hyphen, then its secret. Kept in this computer’s keychain and never shown again.</span>
          {#if problem}<p id="bridge-key-err" class="err">{problem}</p>{/if}
        </div>
        <div class="two">
          <div class="field">
            <label for="bridge-ice">Icecast port here</label>
            <input id="bridge-ice" type="number" min="1024" max="65535" bind:value={icecastPort} aria-invalid={portsProblem ? true : undefined} disabled={view?.running} />
          </div>
          <div class="field">
            <label for="bridge-srt">SRT port here</label>
            <input id="bridge-srt" type="number" min="1024" max="65535" bind:value={srtPort} aria-invalid={portsProblem ? true : undefined} disabled={view?.running} />
          </div>
        </div>
        {#if portsProblem}<p class="err">{portsProblem}</p>{:else}<span class="hint">Ports on this computer only: nothing outside it can reach them.</span>{/if}
        <div class="acts">
          <button type="submit" class="btn" disabled={busy || view?.running || !dirty} title={view?.running ? 'Stop the bridge to change these.' : undefined}>Save</button>
          {#if view?.has_key}<button type="button" class="btn quiet" disabled={busy || view.running} title={view.running ? 'Stop the bridge first.' : 'Takes the key out of this computer’s keychain.'} onclick={() => act(bridgeForget, 'The key is forgotten on this computer.')}>Forget the key</button>{/if}
          <span class="hint note" role="status" aria-live="polite">{view?.running ? 'Stop the bridge to change these.' : dirty ? 'Not saved yet.' : note}</span>
        </div>
      </form>

      <section class="part" aria-labelledby="bridge-sw-h">
        <h3 id="bridge-sw-h">In your software</h3>
        {#if view && fields}
          <p class="hint">Point your software at this computer, not at the station. Use one of the two.</p>
          <div class="way">
            <h4>Icecast <span class="tag">most radio software</span></h4>
            {#each fields.icecast as f (f.label)}
              <div class="f">
                <span class="fl" id="bf-{f.id}-l">{f.label}</span>
                <input id="bf-{f.id}" type="text" readonly value={f.value} aria-labelledby="bf-{f.id}-l" spellcheck="false" onfocus={(e) => e.currentTarget.select()} />
                <button type="button" class="btn sm" aria-label="Copy the Icecast {f.label.toLowerCase()}" onclick={() => copy(f.id, f.value)}>{copied === f.id ? 'Copied' : 'Copy'}</button>
              </div>
            {/each}
            <p class="hint">Any mount will do.</p>
          </div>
          <div class="way">
            <h4>SRT <span class="tag">OBS and others</span></h4>
            <div class="f">
              <span class="fl" id="bf-srt-l">Address</span>
              <input id="bf-srt" type="text" readonly value={fields.srt} aria-labelledby="bf-srt-l" spellcheck="false" onfocus={(e) => e.currentTarget.select()} />
              <button type="button" class="btn sm" aria-label="Copy the SRT address" onclick={() => copy('srt', fields.srt)}>{copied === 'srt' ? 'Copied' : 'Copy'}</button>
            </div>
            <p class="hint">MPEG-TS, sound only.</p>
          </div>
          <div class="acts">
            <button type="button" class="btn sm" disabled={busy} aria-disabled={view.running ? 'true' : undefined} title={view.running ? 'Stop the bridge first: your software is using this one.' : 'Your software will need the new one.'} onclick={() => !view!.running && act(bridgeNewPassword, 'A new local password is made. Give it to your software.')}>New local password</button>
            <span class="hint">The local password only keeps other programs on this computer out. It is not your show key.</span>
          </div>
        {:else}
          <p class="none">Save the station’s address and your show key. The details for your software appear here.</p>
        {/if}
      </section>
    </div>
  </section>
{/if}

<style>
  /* this panel's own inputs: the styles are scoped, so each settings panel carries them */
  .panel :global(input:not([type='checkbox'], [type='radio'], [type='file'])),
  .panel :global(select) {
    font: inherit;
    color: var(--ink);
    background: var(--plane);
    border: 1px solid var(--muted);
    border-radius: var(--r-2);
    min-height: 2.25rem;
    padding: 0 var(--s-2);
    color-scheme: dark;
    min-width: 0;
    max-width: 100%;
  }
  :global(:root[data-theme='light']) .panel :global(select),
  :global(:root[data-theme='light']) .panel :global(input) {
    color-scheme: light;
  }
  .panel :global(input[type='checkbox']),
  .panel :global(input[type='radio']) {
    accent-color: var(--accent);
  }
  .panel :global([aria-invalid='true']) {
    border-color: var(--status-critical);
    border-width: 2px;
  }
  .panel :global(input:disabled),
  .panel :global(select:disabled) {
    opacity: 0.6;
  }
  .panel {
    background: var(--surface);
    border: 1px solid var(--hairline);
    border-radius: var(--r-3);
    box-shadow: var(--e-1);
    padding: var(--s-4);
    display: grid;
    gap: var(--s-3);
    min-width: 0;
  }
  .ph {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    flex-wrap: wrap;
  }
  .ph h2 {
    font-size: var(--fs-20);
  }
  .help {
    color: var(--ink-2);
    line-height: var(--lh-body);
    max-width: 60rem;
  }
  .now {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--s-2) var(--s-4);
    align-items: center;
    border: 1px solid var(--hairline);
    border-left: 3px solid var(--hairline);
    border-radius: var(--r-1);
    background: var(--surface-2);
    padding: var(--s-2) var(--s-3);
  }
  .now[data-uplink='up'] {
    border-left-color: var(--status-good);
  }
  .now[data-uplink='retrying'] {
    border-left-color: var(--status-warning);
  }
  .says {
    display: grid;
    gap: 2px;
    font-size: var(--fs-14);
    line-height: var(--lh-body);
    min-width: 0;
  }
  .says span {
    color: var(--ink-2);
    font-size: var(--fs-13);
  }
  .alert {
    grid-column: 1 / -1;
    display: flex;
    gap: var(--s-2);
    align-items: flex-start;
    font-size: var(--fs-14);
    line-height: var(--lh-body);
    border-top: 1px solid var(--hairline);
    padding-top: var(--s-2);
  }
  .alert svg {
    width: 16px;
    height: 16px;
    flex: none;
    margin-top: 3px;
  }
  .parts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(24rem, 100%), 1fr));
    gap: var(--s-4) var(--s-5);
    align-items: start;
  }
  .part {
    display: grid;
    gap: var(--s-2);
    align-content: start;
    min-width: 0;
    border-top: 1px solid var(--hairline);
    padding-top: var(--s-3);
  }
  h3 {
    font-size: var(--fs-16);
  }
  h4 {
    font-size: var(--fs-14);
    font-weight: 600;
    display: flex;
    gap: var(--s-2);
    align-items: baseline;
  }
  .tag {
    font-size: var(--fs-12);
    font-weight: 400;
    color: var(--ink-2);
  }
  .field {
    display: grid;
    gap: var(--s-1);
    align-content: start;
    min-width: 0;
  }
  .field > label {
    font-size: var(--fs-13);
    font-weight: 600;
  }
  .field > input {
    width: 100%;
  }
  .two {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--s-3);
  }
  .hint,
  .none {
    font-size: var(--fs-13);
    color: var(--muted);
    line-height: var(--lh-body);
  }
  .none {
    color: var(--ink-2);
  }
  .hint.note {
    color: var(--ink-2);
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
  .way {
    display: grid;
    gap: var(--s-1);
    border: 1px solid var(--hairline);
    border-radius: var(--r-1);
    background: var(--surface-2);
    padding: var(--s-2) var(--s-3);
  }
  .f {
    display: grid;
    grid-template-columns: 5.5rem minmax(0, 1fr) auto;
    gap: var(--s-2);
    align-items: center;
  }
  .fl {
    font-size: var(--fs-13);
    color: var(--ink-2);
  }
  .f input {
    font-family: var(--font-mono);
    font-size: var(--fs-13);
    min-width: 0;
    width: 100%;
  }
  .small {
    font-size: var(--fs-13);
    color: var(--ink-2);
  }
  .num {
    font-variant-numeric: tabular-nums;
  }
</style>
