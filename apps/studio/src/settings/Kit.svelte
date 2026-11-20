<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Settings › Presenter kit (`settings.kit`; Phase 17g): the station's half of the kit, an
  // admin's. Two things live here.
  //
  //   One clock    every source with a clock of its own is brought to the desk's 48 kHz once,
  //                at the desk's edge. The badge and the table say how each one is doing: its
  //                drift against the desk, times it was placed afresh, VBAN packets lost.
  //   VBAN         a sound link for one local network: in from a studio PC, bound to a show key
  //                (it stands by like any host's own studio), and out from the desk. It has no
  //                password and no encryption, so it is offered only where the installation
  //                allows it (Compose and the Quadlets), and never on Cloudron: there the
  //                screen says why, and offers none of it.
  //
  // The presets set the routing in one press, toward one address. The rows under them are the
  // same settings, by hand. One Save sends the kit whole; the station's refusal of a field
  // lands under that field.
  import { onDestroy, onMount, untrack } from 'svelte';
  import { placeError } from '../lib/account.ts';
  import { listShowKeys, type KeyList } from '../lib/api.ts';
  import * as schedule from '../lib/schedule.ts';
  import { studio } from '../lib/studio.svelte.ts';
  import {
    OUT_FORMAT_WORD,
    clockWords,
    droppedWords,
    kitClock,
    kitFieldId,
    kitFieldWords,
    kitPreset,
    kitSave,
    kitView,
    outSourceWords,
    ppmWords,
    presetWords,
    vbanOffWords,
    type Kit,
    type KitClock,
    type KitView,
    type VbanIn,
    type VbanOut,
  } from '../lib/bridge.ts';
  import { TILE_WAY } from '../lib/hosts.ts';

  interface KeyChoice {
    key_id: string;
    label: string;
    show: string;
  }
  let { initial = null }: { initial?: { view: KitView; clock: KitClock | null; keys: KeyChoice[] } | null } = $props();

  const seed = untrack(() => initial);
  let view = $state<KitView | null>(seed?.view ?? null);
  let clock = $state<KitClock | null>(seed?.clock ?? null);
  let keys = $state<KeyChoice[]>(seed?.keys ?? []);
  let kit = $state<Kit | null>(seed ? (JSON.parse(JSON.stringify(seed.view.kit)) as Kit) : null);
  let address = $state('');
  let error = $state('');
  let fieldErrors = $state<Record<string, string>>({});
  let note = $state('');
  let busy = $state(false);
  let loadError = $state('');

  const MAX = 4;
  const off = $derived(view && !view.allowed ? vbanOffWords(view.why_not) : null);
  const dirty = $derived(!!view && !!kit && JSON.stringify(kit) !== JSON.stringify(view.kit));
  const keyName = (id: string) => keys.find((k) => k.key_id === id)?.label ?? null;
  const personName = (identity: string) => studio.view?.sources.find((s) => s.identity === identity)?.name ?? null;
  /** The people at the desk a mix-minus can be sent for, and the keys a monitor mix is made for. */
  const outChoices = $derived([
    { value: 'programme', label: outSourceWords('programme') },
    { value: 'programme_live', label: outSourceWords('programme_live') },
    ...keys.map((k) => ({ value: `monitor:${k.key_id}`, label: outSourceWords(`monitor:${k.key_id}`, keyName) })),
    ...(studio.view?.sources ?? []).filter((s) => s.role !== 'source').map((s) => ({ value: `mix_minus:${s.identity}`, label: outSourceWords(`mix_minus:${s.identity}`, keyName, personName) })),
  ]);

  function take(v: KitView) {
    view = v;
    kit = JSON.parse(JSON.stringify(v.kit)) as Kit;
    fieldErrors = {};
  }

  async function readKeys(): Promise<KeyChoice[]> {
    const shows = await schedule.shows().catch(() => []);
    const lists = await Promise.allSettled(shows.map((s) => listShowKeys(s.slug)));
    return lists.flatMap((r, i) => (r.status === 'fulfilled' ? (r.value as KeyList).keys.filter((k) => k.revoked_ms === null && k.hint !== 'pull').map((k) => ({ key_id: k.key_id, label: k.label, show: shows[i].name })) : []));
  }

  async function refresh() {
    loadError = '';
    try {
      take(await kitView());
      clock = await kitClock().catch(() => null);
      if (view?.allowed) keys = await readKeys();
    } catch (e) {
      loadError = placeError(e, []).message;
    }
  }

  let timer: ReturnType<typeof setInterval> | undefined;
  onMount(() => {
    if (seed) return;
    void refresh();
    // The clock's figures move: read them while the panel is open.
    timer = setInterval(async () => {
      if (view) clock = await kitClock().catch(() => clock);
    }, 5000);
  });
  onDestroy(() => clearInterval(timer));

  function fail(e: unknown) {
    const field = (e as { field?: string | null } | null)?.field ?? null;
    const id = kitFieldId(field);
    const message = placeError(e, []).message;
    if (id) {
      fieldErrors = { [id]: kitFieldWords(message) };
      queueMicrotask(() => document.getElementById(id)?.focus());
    } else error = field ? `${field}: ${message}` : message;
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (busy || !kit) return;
    error = '';
    note = '';
    fieldErrors = {};
    busy = true;
    try {
      take(await kitSave({ ...kit, bind: kit.bind?.trim() || null }));
      note = 'Saved. The station takes it within a few seconds.';
    } catch (err) {
      fail(err);
    } finally {
      busy = false;
    }
  }

  async function preset(name: string, title: string) {
    if (busy) return;
    busy = true;
    error = '';
    note = '';
    fieldErrors = {};
    try {
      take(await kitPreset(name, address.trim() || null));
      note = `${title} is set. Its rows are below.`;
    } catch (err) {
      const m = placeError(err, []).message;
      error = /address/i.test(m) ? `${title} needs an address to send to. Type the studio PC’s, transmitter’s or guest machine’s address first. (${m})` : m;
      if (/address/i.test(m)) document.getElementById('kit-addr')?.focus();
    } finally {
      busy = false;
    }
  }

  const addIn = () => kit && kit.vban_in.push({ address: '', name: 'Stream1', key_id: keys[0]?.key_id ?? '', channels: [1, 2] } satisfies VbanIn);
  const addOut = () => kit && kit.vban_out.push({ address: address.trim(), port: 6980, name: 'WeaveSignal', source: 'programme', format: 's16', enabled: true } satisfies VbanOut);
  const outNow = (o: VbanOut) => clock?.vban_out.find((x) => x.name === o.name && x.address === o.address) ?? null;
  const dropped = $derived(clock ? droppedWords(clock.vban_dropped) : []);
</script>

<section class="panel" data-panel="settings.kit" aria-labelledby="kit-h">
  <div class="ph">
    <h2 id="kit-h" data-panel-title>Presenter kit</h2>
    {#if clock}<span class="badge" role="status" data-testid="kit-clock"><svg aria-hidden="true"><use href="#g-delay" /></svg>{clockWords(clock)}</span>{/if}
  </div>
  <p class="help">How the station keeps every source in step with the desk, and VBAN: a sound link between the station and a studio PC on the same local network.</p>
  {#if loadError}<p class="err" role="alert">The presenter kit could not be read. {loadError} <button type="button" class="btn sm" onclick={refresh}>Try again</button></p>{/if}
  {#if !view && !loadError}
    <p role="status" class="small">Reading the presenter kit…</p>
  {/if}

  {#if clock}
    <section class="part" aria-labelledby="kit-clock-h">
      <h3 id="kit-clock-h">One clock</h3>
      <p class="hint">Every source has a clock of its own, a little fast or slow. The desk follows each one and brings it to its own {clock.rate / 1000} kHz, once, so nothing drifts or jumps.</p>
      {#if clock.sources.length}
        <div class="scroll">
          <table>
            <thead><tr><th scope="col">Source</th><th scope="col">Way in</th><th scope="col">Against the desk</th><th scope="col">Placed afresh</th><th scope="col">Packets lost</th></tr></thead>
            <tbody>
              {#each clock.sources as s (s.identity)}
                <tr>
                  <th scope="row">{s.name}</th>
                  <td>{TILE_WAY[s.way as keyof typeof TILE_WAY] ?? s.way}</td>
                  <td class="num" title={s.ppm === null ? 'The first reading takes about 20 seconds.' : 'Parts per million: how much faster (+) or slower (−) its clock runs than the desk’s.'}>{ppmWords(s.ppm)}</td>
                  <td class="num">{s.replaced}</td>
                  <td class="num">{s.way === 'vban' ? s.lost : '—'}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <p class="none" data-testid="kit-clock-none">No host’s own studio is at the desk now. When one connects, its clock is read here.</p>
      {/if}
      {#if dropped.length}<p class="hint" data-testid="kit-dropped">VBAN packets not taken: {dropped.join('; ')}.</p>{/if}
    </section>
  {/if}

  {#if view && off}
    <!-- VBAN is not offered here: said once, and none of its settings are drawn. -->
    <div class="callout" data-testid="kit-vban-off" data-cloudron={off.cloudron || undefined}>
      <p><b>{off.title}</b> {off.body}</p>
    </div>
  {:else if view && kit}
    {#if view.warning}
      <p class="callout warn" role="alert"><svg class="g-warning" aria-hidden="true"><use href="#g-fault" /></svg><span>{view.warning}</span></p>
    {/if}
    <form onsubmit={save} novalidate aria-labelledby="kit-h">
      <section class="part" aria-labelledby="kit-net-h">
        <h3 id="kit-net-h">The station on the studio network</h3>
        <div class="grid">
          <div class="field">
            <label for="kit-bind">The station’s own address there</label>
            <input id="kit-bind" type="text" value={kit.bind ?? ''} oninput={(e) => (kit!.bind = e.currentTarget.value)} autocomplete="off" spellcheck="false" placeholder="192.168.1.10" aria-invalid={fieldErrors['kit-bind'] ? true : undefined} aria-describedby="kit-bind-hint" />
            <span id="kit-bind-hint" class="hint">VBAN listens on it and sends from it. Needed for any row below.</span>
            {#if fieldErrors['kit-bind']}<p class="err">{fieldErrors['kit-bind']}</p>{/if}
          </div>
          <div class="field">
            <label for="kit-port">VBAN in’s port</label>
            <input id="kit-port" type="number" min="1" max="65535" value={kit.port} oninput={(e) => (kit!.port = Number(e.currentTarget.value))} aria-invalid={fieldErrors['kit-port'] ? true : undefined} aria-describedby="kit-port-hint" />
            <span id="kit-port-hint" class="hint">UDP. 6980 is VBAN’s usual.</span>
            {#if fieldErrors['kit-port']}<p class="err">{fieldErrors['kit-port']}</p>{/if}
          </div>
          <div class="field">
            <label class="check"><input type="checkbox" checked={kit.allow_public} onchange={(e) => (kit!.allow_public = e.currentTarget.checked)} aria-describedby="kit-public-hint" /> Allow addresses outside the local network</label>
            <span id="kit-public-hint" class="hint">Leave this off. VBAN has no password and no encryption: anyone on the way can listen or send. Between two places, carry it through WireGuard instead.</span>
          </div>
        </div>
      </section>

      <section class="part" aria-labelledby="kit-presets-h">
        <h3 id="kit-presets-h">Presets</h3>
        <div class="field addr">
          <label for="kit-addr">Send to</label>
          <input id="kit-addr" type="text" bind:value={address} autocomplete="off" spellcheck="false" placeholder="192.168.1.30" aria-describedby="kit-addr-hint" />
          <span id="kit-addr-hint" class="hint">The studio PC, transmitter or guest machine a preset sends to.</span>
        </div>
        <div class="presets" role="group" aria-label="Presets">
          {#each view.presets as p (p.name)}
            {@const w = presetWords(p)}
            <button type="button" class="preset" aria-pressed={view.kit.preset === p.name} disabled={busy} data-preset={p.name} onclick={() => preset(p.name, w.title)}>
              <b>{w.title}</b>
              <span>{w.does}</span>
            </button>
          {/each}
        </div>
      </section>

      <section class="part" aria-labelledby="kit-in-h">
        <h3 id="kit-in-h">VBAN in <span class="count">{kit.vban_in.length} of {MAX}</span></h3>
        <p class="hint">A stream from a studio PC, bound to a show key. It stands by on the desk like any host’s own studio, and goes on air by a press or at its show’s time.</p>
        {#each kit.vban_in as r, i (i)}
          <fieldset class="row" data-testid="kit-in-row">
            <legend class="sr-only">VBAN in {i + 1}</legend>
            <div class="field">
              <label for="kit-in-{i}-address">Sender’s address</label>
              <input id="kit-in-{i}-address" type="text" bind:value={r.address} autocomplete="off" spellcheck="false" placeholder="192.168.1.20" aria-invalid={fieldErrors[`kit-in-${i}-address`] ? true : undefined} />
              {#if fieldErrors[`kit-in-${i}-address`]}<p class="err">{fieldErrors[`kit-in-${i}-address`]}</p>{/if}
            </div>
            <div class="field">
              <label for="kit-in-{i}-name">Stream name</label>
              <input id="kit-in-{i}-name" type="text" bind:value={r.name} maxlength="16" autocomplete="off" spellcheck="false" aria-invalid={fieldErrors[`kit-in-${i}-name`] ? true : undefined} />
              {#if fieldErrors[`kit-in-${i}-name`]}<p class="err">{fieldErrors[`kit-in-${i}-name`]}</p>{/if}
            </div>
            <div class="field">
              <label for="kit-in-{i}-key">Show key</label>
              <select id="kit-in-{i}-key" bind:value={r.key_id} aria-invalid={fieldErrors[`kit-in-${i}-key`] ? true : undefined}>
                {#if !keys.some((k) => k.key_id === r.key_id)}<option value={r.key_id}>{r.key_id ? `Key ${r.key_id} (ended, or not this window’s to read)` : 'Choose a key'}</option>{/if}
                {#each keys as k (k.key_id)}<option value={k.key_id}>{k.label} · {k.show}</option>{/each}
              </select>
              {#if fieldErrors[`kit-in-${i}-key`]}<p class="err">{fieldErrors[`kit-in-${i}-key`]}</p>{/if}
            </div>
            <div class="field pair">
              <span class="lab" id="kit-in-{i}-ch">Channels, left and right</span>
              <span class="two">
                <select id="kit-in-{i}-channels" aria-label="Left channel" value={r.channels[0]} onchange={(e) => (r.channels = [Number(e.currentTarget.value), r.channels[1]])}>{#each [1, 2, 3, 4, 5, 6, 7, 8] as n (n)}<option value={n}>{n}</option>{/each}</select>
                <select aria-label="Right channel" value={r.channels[1]} onchange={(e) => (r.channels = [r.channels[0], Number(e.currentTarget.value)])}>{#each [1, 2, 3, 4, 5, 6, 7, 8] as n (n)}<option value={n}>{n}</option>{/each}</select>
              </span>
            </div>
            <button type="button" class="btn sm quiet rm" onclick={() => kit!.vban_in.splice(i, 1)}>Remove<span class="sr-only"> VBAN in {i + 1}</span></button>
          </fieldset>
        {:else}
          <p class="none">No VBAN in. A studio PC’s Voicemeeter or VBAN sender is bound here.</p>
        {/each}
        {#if !keys.length && kit.vban_in.length === 0}<p class="hint">A VBAN in belongs to a show key: make one first, under Own studios above.</p>{/if}
        <div><button type="button" class="btn sm" disabled={kit.vban_in.length >= MAX || !keys.length} title={!keys.length ? 'Make a show key first, under Own studios.' : kit.vban_in.length >= MAX ? 'At most four.' : undefined} onclick={addIn}>Add a VBAN in</button></div>
      </section>

      <section class="part" aria-labelledby="kit-out-h">
        <h3 id="kit-out-h">VBAN out <span class="count">{kit.vban_out.length} of {MAX}</span></h3>
        <p class="hint">What the desk sends to one machine: the programme, or the mix one person or one studio hears. A voice never goes back to its own ear, and a cue never to the programme: the desk does not offer either.</p>
        {#each kit.vban_out as o, i (i)}
          {@const now = outNow(o)}
          <fieldset class="row out" data-testid="kit-out-row">
            <legend class="sr-only">VBAN out {i + 1}</legend>
            <div class="field">
              <label for="kit-out-{i}-address">Receiver’s address</label>
              <input id="kit-out-{i}-address" type="text" bind:value={o.address} autocomplete="off" spellcheck="false" placeholder="192.168.1.30" aria-invalid={fieldErrors[`kit-out-${i}-address`] ? true : undefined} />
              {#if fieldErrors[`kit-out-${i}-address`]}<p class="err">{fieldErrors[`kit-out-${i}-address`]}</p>{/if}
            </div>
            <div class="field narrow">
              <label for="kit-out-{i}-port">Port</label>
              <input id="kit-out-{i}-port" type="number" min="1" max="65535" bind:value={o.port} aria-invalid={fieldErrors[`kit-out-${i}-port`] ? true : undefined} />
              {#if fieldErrors[`kit-out-${i}-port`]}<p class="err">{fieldErrors[`kit-out-${i}-port`]}</p>{/if}
            </div>
            <div class="field">
              <label for="kit-out-{i}-name">Stream name</label>
              <input id="kit-out-{i}-name" type="text" bind:value={o.name} maxlength="16" autocomplete="off" spellcheck="false" aria-invalid={fieldErrors[`kit-out-${i}-name`] ? true : undefined} />
              {#if fieldErrors[`kit-out-${i}-name`]}<p class="err">{fieldErrors[`kit-out-${i}-name`]}</p>{/if}
            </div>
            <div class="field wide">
              <label for="kit-out-{i}-source">What it carries</label>
              <select id="kit-out-{i}-source" bind:value={o.source} aria-invalid={fieldErrors[`kit-out-${i}-source`] ? true : undefined}>
                {#if !outChoices.some((c) => c.value === o.source)}<option value={o.source}>{outSourceWords(o.source, keyName, personName)}</option>{/if}
                {#each outChoices as c (c.value)}<option value={c.value}>{c.label}</option>{/each}
              </select>
              {#if fieldErrors[`kit-out-${i}-source`]}<p class="err">{fieldErrors[`kit-out-${i}-source`]}</p>{/if}
            </div>
            <div class="field narrow">
              <label for="kit-out-{i}-format">Format</label>
              <select id="kit-out-{i}-format" bind:value={o.format}>
                {#each ['s16', 's24', 'f32'] as const as f (f)}<option value={f}>{OUT_FORMAT_WORD[f]}</option>{/each}
              </select>
            </div>
            <label class="check on"><input type="checkbox" bind:checked={o.enabled} /> On</label>
            <button type="button" class="btn sm quiet rm" onclick={() => kit!.vban_out.splice(i, 1)}>Remove<span class="sr-only"> VBAN out {i + 1}</span></button>
            {#if now}
              <p class="now" class:bad={!!now.problem} role="status">
                {#if now.problem}<svg class="g-warning" aria-hidden="true"><use href="#g-degraded" /></svg>Not sending: {now.problem}.{:else}Sending: {now.packets.toLocaleString('en-GB')} packets{now.dropped ? `, ${now.dropped.toLocaleString('en-GB')} blocks dropped` : ''}.{/if}
              </p>
            {/if}
          </fieldset>
        {:else}
          <p class="none">No VBAN out. A preset above adds one, or add it by hand.</p>
        {/each}
        <div><button type="button" class="btn sm" disabled={kit.vban_out.length >= MAX} title={kit.vban_out.length >= MAX ? 'At most four.' : undefined} onclick={addOut}>Add a VBAN out</button></div>
      </section>

      {#if error}<p class="err" role="alert">{error}</p>{/if}
      <div class="foot">
        <span class="small" role="status" aria-live="polite">{dirty ? 'Not saved yet.' : note}</span>
        <button type="submit" class="btn primary" disabled={!dirty || busy} aria-busy={busy || undefined}>{busy ? 'Saving…' : 'Save the kit'}</button>
      </div>
    </form>
  {/if}
</section>

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
  .badge {
    display: inline-flex;
    align-items: center;
    gap: var(--s-1);
    border: 1px solid var(--hairline);
    border-radius: 999px;
    padding: 2px var(--s-3);
    font-size: var(--fs-13);
    color: var(--ink-2);
    font-variant-numeric: tabular-nums;
  }
  .badge svg {
    width: 12px;
    height: 12px;
  }
  .help {
    color: var(--ink-2);
    line-height: var(--lh-body);
    max-width: 60rem;
  }
  form {
    display: grid;
    gap: var(--s-3);
  }
  .part {
    display: grid;
    gap: var(--s-2);
    align-content: start;
    border-top: 1px solid var(--hairline);
    padding-top: var(--s-3);
    min-width: 0;
  }
  h3 {
    font-size: var(--fs-16);
    display: flex;
    align-items: baseline;
    gap: var(--s-2);
  }
  .count {
    font-size: var(--fs-12);
    font-weight: 400;
    color: var(--ink-2);
  }
  .hint,
  .none {
    font-size: var(--fs-13);
    color: var(--muted);
    line-height: var(--lh-body);
    max-width: 60rem;
  }
  .none {
    color: var(--ink-2);
  }
  .err {
    font-size: var(--fs-13);
    color: var(--status-serious);
    line-height: var(--lh-label);
  }
  .callout {
    border-left: 3px solid var(--hairline);
    background: var(--surface-2);
    border-radius: var(--r-1);
    padding: var(--s-2) var(--s-3);
    font-size: var(--fs-14);
    line-height: var(--lh-body);
    max-width: 60rem;
  }
  .callout.warn {
    border-left-color: var(--status-warning);
    display: flex;
    gap: var(--s-2);
    align-items: flex-start;
  }
  .callout.warn svg {
    width: 16px;
    height: 16px;
    flex: none;
    margin-top: 2px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(18rem, 100%), 1fr));
    gap: var(--s-3) var(--s-5);
    align-items: start;
  }
  .field {
    display: grid;
    gap: var(--s-1);
    align-content: start;
    min-width: 0;
  }
  .field > label,
  .field > .lab {
    font-size: var(--fs-13);
    font-weight: 600;
  }
  .field > label.check,
  label.check {
    font-weight: 400;
    font-size: var(--fs-14);
    display: flex;
    gap: var(--s-2);
    align-items: center;
  }
  .field.addr {
    max-width: 22rem;
  }
  .presets {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(15rem, 100%), 1fr));
    gap: var(--s-2);
  }
  .preset {
    appearance: none;
    font: inherit;
    text-align: start;
    color: var(--ink);
    background: var(--surface-2);
    border: 1px solid var(--axis);
    border-radius: var(--r-2);
    padding: var(--s-2) var(--s-3);
    display: grid;
    gap: 2px;
    align-content: start;
    cursor: pointer;
    min-height: 4.5rem;
  }
  .preset b {
    font-size: var(--fs-14);
  }
  .preset span {
    font-size: var(--fs-12);
    color: var(--ink-2);
    line-height: var(--lh-label);
  }
  .preset:hover:not(:disabled) {
    background: var(--surface-3);
  }
  .preset[aria-pressed='true'] {
    border-color: var(--accent);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .preset[aria-pressed='true'] b::after {
    content: ' · in use';
    font-weight: 400;
    font-size: var(--fs-12);
    color: var(--ink-2);
  }
  .preset:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .row {
    border: 1px solid var(--hairline);
    border-radius: var(--r-1);
    background: var(--surface-2);
    padding: var(--s-2) var(--s-3);
    margin: 0;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(11rem, 100%), 1fr));
    gap: var(--s-2) var(--s-3);
    align-items: end;
    min-width: 0;
  }
  .row .rm,
  .row .on {
    justify-self: start;
    align-self: end;
    min-height: 2rem;
  }
  .row input,
  .row select {
    width: 100%;
    min-width: 0;
  }
  .pair .two {
    display: flex;
    gap: var(--s-1);
  }
  .row .now {
    grid-column: 1 / -1;
    font-size: var(--fs-12);
    color: var(--ink-2);
    display: flex;
    gap: var(--s-1);
    align-items: center;
  }
  .row .now svg {
    width: 12px;
    height: 12px;
  }
  .scroll {
    overflow-x: auto;
  }
  table {
    border-collapse: collapse;
    font-size: var(--fs-14);
  }
  thead th {
    text-align: start;
    font-size: var(--fs-12);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--ink-2);
    padding: 0 var(--s-4) var(--s-1) 0;
    border-bottom: 1px solid var(--hairline);
    white-space: nowrap;
  }
  tbody th,
  tbody td {
    text-align: start;
    padding: var(--s-1) var(--s-4) var(--s-1) 0;
    border-bottom: 1px solid var(--hairline);
    font-weight: 400;
  }
  tbody th {
    font-weight: 600;
  }
  .num {
    font-variant-numeric: tabular-nums;
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: var(--s-3);
    border-top: 1px solid var(--hairline);
    padding-top: var(--s-3);
  }
  .small {
    font-size: var(--fs-13);
    color: var(--ink-2);
  }
</style>
