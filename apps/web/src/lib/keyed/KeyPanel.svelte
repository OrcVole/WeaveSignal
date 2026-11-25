<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A listener's key on the player (keyed shows, 1.3.2): Enter a key, the welcome with what
  // the key opens and until when, the notice in its last days, the words for every way a key
  // does not work (with the devices that hold it and Sign out, when it is in use on too many),
  // Lost your key?, and Can't hear the show?. The word is "key" everywhere. Nothing here asks
  // for an account or a password.
  //
  // It is drawn only where it has something to say: a station with no keyed shows, and no key
  // kept on this device, shows nothing of it. Except at `/listen/`, the page a key's link leads
  // to (`entry`): opened without its key (the link lost its end, or the key was read out or
  // printed), the box is what the page is for, so it is always there and open, with a line for
  // whoever has the link, and a key accepted in it starts its show (the operator, 2026-11-25).
  import { tick } from 'svelte';
  import type { Keyed } from './keyed.svelte.ts';
  import * as routes from './routes.ts';
  import type { KeyedShow } from './routes.ts';
  import { deviceLine, whenWords, emailProblem, endsNotice, entryWords, helpItems, keptWords, lostWords, opens, opensToAll, problemWords, publicHears, welcome } from './words.ts';

  let {
    k,
    station,
    keyedShow,
    onAir,
    playing,
    streamFailed,
    now = Date.now(),
    zone = undefined,
    entry = false,
    onaccepted = undefined,
  }: {
    k: Keyed;
    station: string;
    /** The keyed show that is on now, if one is. */
    keyedShow: KeyedShow | null;
    onAir: boolean;
    playing: boolean;
    streamFailed: boolean;
    now?: number;
    /** A fixed time zone for the dates, for a test; the listener's own otherwise. */
    zone?: string;
    /** This is the page a key's link leads to (`/listen/`): the box is always offered, open. */
    entry?: boolean;
    /** A key typed or pasted into the box was accepted: the press that sent it was the
     *  listener's own, so the page may start the show it opens. */
    onaccepted?: () => void;
  } = $props();

  const session = $derived(k.session);
  const problem = $derived(k.refusal ? problemWords(k.refusal, { station, show: keyedShow?.name ?? null, now, zone }) : null);
  const opensIt = $derived(!!session && !!keyedShow && opens(session.key, keyedShow));
  const hello = $derived(session ? welcome(session.key, station, now, zone) : null);
  const ends = $derived(session ? endsNotice(session.key, now, zone) : null);
  /** The station offers keyed shows at all, or this device keeps a key. */
  const relevant = $derived(entry || !!k.kept || !!k.session || !!k.refusal || k.busy || !!keyedShow || (k.shows?.shows.length ?? 0) > 0);
  const entrySay = $derived(entryWords(station));
  const getText = $derived(k.shows?.get_a_key_text.trim() ?? '');
  const getUrl = $derived(/^https?:\/\//i.test(k.shows?.get_a_key_url ?? '') ? k.shows!.get_a_key_url : '');

  /** The box is open: always while a keyed show is on and no key is held; else on request. */
  let asked = $state(false);
  // A key that is kept and only needs a device freed, a wait or another try is not asked for
  // again: the box is then one press away, for someone with another key.
  let typed = $state('');
  /** The key this page came with (its link's, or the one kept here) is being asked about, and
   *  nothing was typed: the page says so, and does not ask for a key it already has. */
  const checking = $derived(k.busy && !session && !problem && !typed.trim());
  const boxOpen = $derived(!session && !checking && (asked || (problem ? problem.act === 'retype' || problem.act === 'ask' : !!keyedShow || entry)));
  let typedProblem = $state('');
  let input = $state<HTMLInputElement | undefined>();

  async function openBox() {
    asked = true;
    await tick();
    input?.focus();
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    typedProblem = '';
    if (!typed.trim()) {
      typedProblem = 'Type or paste the key you were sent.';
      input?.focus();
      return;
    }
    // A pasted link works as well as the key itself.
    const text = routes.keyInAddress(typed.includes('#') ? typed.slice(typed.indexOf('#')) : '') ?? typed;
    const ok = await k.enter(text, keyedShow?.id ?? null);
    if (ok) {
      typed = '';
      lostOpen = false;
      asked = false;
      onaccepted?.();
    } else {
      // A key the device keeps is not left in the box: only one typed wrong stays, to be put right.
      if (k.refusal?.code !== 'not_recognised') {
        typed = '';
        asked = false;
      }
      await tick();
      if (k.refusal?.code === 'not_recognised') input?.select();
    }
  }

  // ---- devices ----
  let devicesOpen = $state(false);
  let working = $state<string | null>(null);
  async function toggleDevices() {
    devicesOpen = !devicesOpen;
    if (devicesOpen) await k.loadDevices();
  }
  async function signOut(id: string) {
    working = id;
    await k.signOut(id);
    working = null;
  }
  const showDevices = $derived(devicesOpen || k.refusal?.code === 'too_many_devices');
  const deviceRows = $derived((k.devices ?? []).filter((d) => d.ended_ms == null).map((d) => ({ d, line: deviceLine(d, k.kept?.device ?? null, now, k.holdS) })));

  // ---- lost your key? ----
  let lostOpen = $state(false);
  let email = $state('');
  let emailErr = $state('');
  let lostState = $state<'idle' | 'sending' | 'sent' | 'slow' | 'failed'>('idle');
  /** Whether the station sends email: its answer to the request once there is one, else what
   *  the list of keyed shows says; unknown reads as it sends. The words follow it. */
  let lostMail = $state<boolean | null>(null);
  const lostSay = $derived(lostWords(station, lostMail ?? k.shows?.mail ?? null));
  let emailEl = $state<HTMLInputElement | undefined>();
  async function openLost() {
    lostOpen = !lostOpen;
    lostState = 'idle';
    emailErr = '';
    if (lostOpen) {
      await tick();
      emailEl?.focus();
    }
  }
  async function askLost(e: SubmitEvent) {
    e.preventDefault();
    emailErr = emailProblem(email) ?? '';
    if (emailErr) return emailEl?.focus();
    lostState = 'sending';
    try {
      lostMail = (await routes.lost(email.trim())).mail;
      lostState = 'sent';
    } catch (err) {
      lostState = err instanceof routes.Refusal && err.code === 'slow_down' ? 'slow' : 'failed';
    }
  }

  // ---- can't hear the show? ----
  let helpOpen = $state(false);
  const help = $derived(helpItems({ keyed: !!session, problem, onAir, keyedShow: keyedShow?.name ?? null, opensIt, playing, streamFailed, station }));
</script>

{#if relevant}
  <section class="card keyed" data-panel="player.key" aria-labelledby="key-h" data-keyed={session ? 'held' : k.refusal ? k.refusal.code : 'none'}>
    {#if session && hello}
      <h2 id="key-h"><svg class="kg" aria-hidden="true"><use href="#g-key" /></svg>Your key</h2>
      <!-- The welcome is said once when the key is new here; after that it is the key's own line. -->
      <p class="welcome" role={k.fresh ? 'status' : undefined} data-testid="key-welcome"><b>{hello.lead}</b> {hello.rest}</p>
      {#if ends}<p class="note ends" data-testid="key-ends"><svg aria-hidden="true"><use href="#g-hold" /></svg><span><b>{ends}</b> Ask {station || 'the station'} if you need longer.</span></p>{/if}
      {#if keyedShow && !opensIt}
        <p class="problem" role="status" data-testid="key-other-show"><b>This key does not open {keyedShow.name}.</b> It is for other shows. Ask {station || 'the station'} if it should open this one too.</p>
      {:else if !keyedShow}
        <p class="small" data-testid="key-next">Nothing on at the moment needs a key.{#if session.starts_ms > now} {session.show.name} starts {whenWords(session.starts_ms, now, zone)}, and your key is kept on this device for it.{:else} Yours is kept on this device, ready for its show.{/if}</p>
      {/if}
    {:else if problem}
      <h2 id="key-h"><svg class="kg" aria-hidden="true"><use href="#g-key" /></svg>{problem.act === 'retype' ? 'Enter a key' : 'Your key'}</h2>
      <div class="problem" role="alert" data-testid="key-problem" data-code={k.refusal?.code}>
        <p><b>{problem.title}</b></p>
        <p>{problem.body}</p>
      </div>
      {#if problem.act === 'again' && k.kept}
        <button type="button" class="btn primary wide" disabled={k.busy} onclick={() => k.resume(keyedShow?.id ?? null)}>{k.refusal?.code === 'device_signed_out' ? 'Use the key here again' : 'Try again'}</button>
      {/if}
      <!-- A key this device keeps may work again without being sent again (resumed, extended, its
           shows changed, a device freed): one press asks the station. -->
      {#if problem.act !== 'again' && problem.keep && k.kept && k.refusal?.code !== 'slow_down'}
        <button type="button" class="btn wide" disabled={k.busy} aria-busy={k.busy} onclick={() => k.resume(keyedShow?.id ?? null)}>Check again</button>
      {/if}
    {:else if checking}
      <h2 id="key-h"><svg class="kg" aria-hidden="true"><use href="#g-key" /></svg>Your key</h2>
      <p class="small" role="status" data-testid="key-checking">Checking your key…</p>
    {:else}
      <h2 id="key-h"><svg class="kg" aria-hidden="true"><use href="#g-key" /></svg>{keyedShow ? `${keyedShow.name} needs a key` : entry ? entrySay.title : 'Have a key?'}</h2>
      {#if keyedShow}
        <p class="small">{publicHears(station)} {opensToAll(keyedShow, now, zone) ?? ''}</p>
        {#if entry}<p class="small" data-testid="key-entry">{entrySay.link} {entrySay.box}</p>{/if}
      {:else if entry}
        <!-- The page a key's link leads to, opened without its key: the line for whoever has the
             link, then the box for whoever has the key itself. -->
        <p class="small" data-testid="key-entry">{entrySay.link} {entrySay.box}</p>
      {:else}
        <p class="small">Some of {station || 'the station'}’s shows are for people with a key. If you were sent one, enter it here and it is ready when its show starts.</p>
      {/if}
    {/if}

    {#if showDevices && k.kept}
      <div class="devices" data-testid="key-devices">
        <span class="caps">Devices using this key</span>
        {#if k.devices === null && !k.devicesProblem}
          <p class="small" role="status">Reading the devices…</p>
        {:else if deviceRows.length === 0 && !k.devicesProblem}
          <p class="small">No device is using this key at the moment.</p>
        {:else}
          <ul>
            {#each deviceRows as { d, line } (d.id)}
              <li>
                <span class="dn"><b>{line.name}</b>{#if line.here}<span class="here"> · this device</span>{/if}<span class="small dw">{line.when}</span></span>
                <button type="button" class="btn sm" disabled={working !== null} aria-busy={working === d.id || undefined} onclick={() => signOut(d.id)}>{line.here ? 'Sign out here' : 'Sign out'}<span class="sr-only"> {line.name}</span></button>
              </li>
            {/each}
          </ul>
        {/if}
        {#if k.devicesProblem}<p class="problem" role="alert">The devices could not be read, or the sign-out did not go through. Try again in a moment.</p>{/if}
      </div>
    {/if}

    {#if boxOpen}
      <form class="enter" novalidate onsubmit={submit}>
        <div class="field">
          <label for="f-key">{problem && problem.act !== 'retype' ? 'Enter another key' : 'Your key'}</label>
          <span class="hint" id="key-hint">It looks like KETM-7QMP-2D4X. Capitals, spaces and dashes do not matter.</span>
          <input
            id="f-key"
            type="text"
            class="code"
            bind:this={input}
            bind:value={typed}
            autocomplete="off"
            autocapitalize="characters"
            autocorrect="off"
            spellcheck="false"
            maxlength="200"
            aria-invalid={!!typedProblem || k.refusal?.code === 'not_recognised'}
            aria-describedby="key-hint{typedProblem ? ' key-err' : ''}"
          />
          {#if typedProblem}<span class="err" id="key-err"><svg aria-hidden="true"><use href="#g-fault" /></svg>{typedProblem}</span>{/if}
        </div>
        <button type="submit" class="btn primary wide" disabled={k.busy} aria-busy={k.busy}>{k.busy ? 'Checking…' : 'Use this key'}</button>
      </form>
      {#if getText || getUrl}
        <div class="get" data-testid="key-get">
          <span class="caps">How do I get a key?</span>
          {#if getText}<p>{getText}</p>{/if}
          {#if getUrl}<p><a href={getUrl} rel="noopener noreferrer" target="_blank">{getUrl.replace(/^https?:\/\//i, '').replace(/\/$/, '')}</a></p>{/if}
        </div>
      {/if}
    {:else if !session && !checking}
      <button type="button" class="btn wide" onclick={openBox}>{problem ? 'Enter another key' : 'Enter a key'}</button>
    {/if}

    <div class="more">
      {#if session}
        <button type="button" class="link" aria-expanded={devicesOpen} onclick={toggleDevices}>Devices</button>
      {/if}
      {#if !session}
        <button type="button" class="link" aria-expanded={lostOpen} onclick={openLost}>Lost your key?</button>
      {/if}
      <button type="button" class="link" aria-expanded={helpOpen} onclick={() => (helpOpen = !helpOpen)}>Can’t hear the show?</button>
      {#if k.kept}
        <button type="button" class="link" onclick={() => k.forget()}>Remove the key from this device</button>
      {/if}
    </div>

    {#if lostOpen && !session}
      <form class="lost" novalidate onsubmit={askLost} data-testid="key-lost">
        {#if lostState === 'sent'}
          <p class="problem good" role="status" data-mail={lostMail ?? k.shows?.mail ?? undefined}>{lostSay.answer}</p>
          <p class="small">{lostSay.after}</p>
        {:else}
          <div class="field">
            <label for="f-lost">Your email address</label>
            <span class="hint" id="lost-hint">{lostSay.hint}</span>
            <input id="f-lost" type="text" inputmode="email" autocomplete="email" bind:this={emailEl} bind:value={email} maxlength="254" aria-invalid={!!emailErr} aria-describedby="lost-hint{emailErr ? ' lost-err' : ''}" />
            {#if emailErr}<span class="err" id="lost-err"><svg aria-hidden="true"><use href="#g-fault" /></svg>{emailErr}</span>{/if}
          </div>
          {#if lostState === 'slow'}<p class="problem" role="alert">Too many requests from this connection. Try again in an hour.</p>{/if}
          {#if lostState === 'failed'}<p class="problem" role="alert">The station did not answer. Check your connection and try again.</p>{/if}
          <button type="submit" class="btn wide" disabled={lostState === 'sending'} aria-busy={lostState === 'sending'}>{lostState === 'sending' ? lostSay.busy : lostSay.button}</button>
        {/if}
      </form>
    {/if}

    {#if helpOpen}
      <div class="help" data-testid="key-help">
        {#each help as h (h.lead)}
          <p><b>{h.lead}</b> {h.body}</p>
        {/each}
        {#if k.kept}<p class="small">{keptWords(station)}</p>{/if}
      </div>
    {/if}
  </section>
{/if}

<style>
  .keyed h2 {
    display: flex;
    align-items: center;
    gap: var(--s-2);
  }
  .kg {
    width: 18px;
    height: 18px;
    flex: none;
    color: var(--ink-2);
  }
  .welcome {
    line-height: var(--lh-body);
  }
  .ends b {
    color: var(--ink);
  }
  .problem p + p {
    margin-top: var(--s-1);
  }
  .problem.good {
    border-left-color: var(--status-good);
  }
  .enter,
  .lost {
    display: grid;
    gap: var(--s-3);
  }
  .code {
    font-family: var(--font-mono);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .code::placeholder {
    text-transform: none;
  }
  .get {
    display: grid;
    gap: var(--s-1);
    font-size: var(--fs-14);
    line-height: var(--lh-body);
    color: var(--ink-2);
  }
  .get a {
    color: var(--ink);
    overflow-wrap: anywhere;
  }
  .devices {
    display: grid;
    gap: var(--s-2);
  }
  .devices ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
  }
  .devices li {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    padding: var(--s-2) 0;
    border-top: 1px solid var(--hairline);
  }
  .dn {
    flex: 1;
    min-width: 0;
    line-height: var(--lh-label);
  }
  .dw {
    display: block;
  }
  .here {
    color: var(--ink-2);
  }
  .btn.sm {
    min-height: var(--target-touch);
    font-size: var(--fs-14);
    padding: 0 var(--s-3);
    flex: none;
  }
  .more {
    display: flex;
    flex-wrap: wrap;
    column-gap: var(--s-4);
    row-gap: 0;
    border-top: 1px solid var(--hairline);
    padding-top: var(--s-1);
  }
  .link {
    appearance: none;
    background: none;
    border: 0;
    padding: 0;
    min-height: var(--target-touch);
    font: inherit;
    font-size: var(--fs-14);
    color: var(--ink-2);
    text-decoration: underline;
    text-underline-offset: 3px;
    cursor: pointer;
  }
  .link:hover,
  .link[aria-expanded='true'] {
    color: var(--ink);
  }
  .help {
    display: grid;
    gap: var(--s-2);
    font-size: var(--fs-14);
    line-height: var(--lh-body);
    color: var(--ink-2);
  }
  .help b {
    color: var(--ink);
  }
</style>
