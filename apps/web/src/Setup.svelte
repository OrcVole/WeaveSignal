<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The station's setup page (ADR-0237), at /setup/. It asks what a running station can take
  // (the name, the takedown contact, the jurisdiction, the stream's password where the install
  // has one, and the look) and explains what was fixed when the station was installed.
  //
  // The page is a shell: it holds nothing about the station until an admin signs in, with the
  // station key or as a member of staff who is an admin. The bearer is kept in memory only, so
  // a reload asks again. Each part saves on its own.
  import { tick } from 'svelte';
  import { deriveBoth, normaliseHex } from '../../../packages/wizard/accent.ts';
  import { readAnswersFile } from '../../../packages/wizard/generate.ts';
  import Glyphs from '../../studio/src/lib/Glyphs.svelte';
  import Mark from '../../studio/src/lib/Mark.svelte';
  import BrandPreview from './lib/BrandPreview.svelte';
  import * as api from './lib/setup-api.ts';

  let bearer = $state('');
  let how = $state<'key' | 'staff'>('key');
  let key = $state('');
  let username = $state('');
  let password = $state('');
  let signing = $state(false);
  let signProblem = $state<string | null>(null);

  let setup = $state<api.SetupView | null>(null);
  let presets = $state<api.Preset[]>([]);
  let presetNow = $state('');
  let heading = $state<HTMLElement | undefined>();

  // What is typed, apart from what is stored.
  let name = $state('');
  let contact = $state('');
  let preset = $state('');
  let accent = $state('');
  let streamOn = $state(false);
  let streamNew = $state('');
  let filled = $state<string | null>(null);

  type Part = 'identity' | 'jurisdiction' | 'accent' | 'icon' | 'banner' | 'stream' | 'finish';
  let busy = $state<Part | null>(null);
  let said = $state<Partial<Record<Part, string>>>({});
  let wrong = $state<Partial<Record<Part, string>>>({});
  let wrongField = $state<string | null>(null);

  const look = $derived(setup?.branding ?? { accent: null, icon: null, banner: null });
  const accentHex = $derived(normaliseHex(accent));
  const shades = $derived(accentHex ? deriveBoth(accentHex) : null);
  const chosenPreset = $derived(presets.find((p) => p.key === preset));
  const accentChanged = $derived((accentHex ?? '') !== (look.accent ?? ''));
  const identityChanged = $derived(!!setup && (name.trim() !== setup.name.value || contact.trim() !== setup.contact.value));

  /** Take the station's answer. Only the part just saved is read back into its fields, so
   * what is typed in the other parts (from an answers file, say) is not thrown away. */
  function take(view: api.SetupView, part: 'all' | 'identity' | 'stream' | 'none' = 'all') {
    setup = view;
    if (part === 'all' || part === 'identity') {
      name = view.name.value;
      contact = view.contact.value;
    }
    if (part === 'all') accent = view.branding.accent ?? '';
    if (part === 'all' || part === 'stream') streamOn = view.stream_password.asked ?? view.stream_password.enabled ?? false;
  }

  async function open(candidate: string) {
    const view = await api.view(candidate);
    bearer = candidate;
    take(view);
    try {
      const r = await api.retention(candidate);
      presets = r.presets.filter((p) => p.key !== 'custom');
      presetNow = r.setting.preset;
      preset = presetNow;
    } catch {
      presets = [];
    }
    await tick();
    heading?.focus();
  }

  async function signIn(e: SubmitEvent) {
    e.preventDefault();
    if (signing) return;
    signProblem = null;
    if (how === 'key' ? !key.trim() : !username.trim() || !password) {
      signProblem = how === 'key' ? 'Enter the station key.' : 'Enter your username and your password.';
      return;
    }
    signing = true;
    try {
      await open(how === 'key' ? key.trim() : await api.signIn(username.trim(), password));
      key = '';
      password = '';
    } catch (err) {
      // Say which thing was wrong, not only that something was.
      if (err instanceof api.Problem && err.signIn) signProblem = how === 'key' ? 'That is not the station key. Check it and try again.' : 'That username and password were not accepted. Check them and try again.';
      else signProblem = err instanceof api.Problem ? err.message : 'That did not work. Try again.';
    } finally {
      signing = false;
    }
  }

  function lock() {
    if (bearer.startsWith('rss_')) void api.signOut(bearer).catch(() => {});
    bearer = '';
    setup = null;
    said = {};
    wrong = {};
  }

  /** Run one part's save: its own busy state, its own message, and back to sign-in on a 401. */
  async function run(part: Part, work: () => Promise<string>) {
    if (busy) return;
    busy = part;
    wrong = { ...wrong, [part]: undefined };
    said = { ...said, [part]: undefined };
    wrongField = null;
    try {
      said = { ...said, [part]: await work() };
    } catch (err) {
      if (err instanceof api.Problem && err.signIn) {
        lock();
        signProblem = 'Your sign-in has ended. Sign in again.';
        return;
      }
      wrong = { ...wrong, [part]: err instanceof api.Problem ? err.message : 'That did not work. Try again.' };
      wrongField = err instanceof api.Problem ? (err.field ?? null) : null;
    } finally {
      busy = null;
    }
  }

  const saveIdentity = (e: SubmitEvent) => {
    e.preventDefault();
    void run('identity', async () => {
      take(await api.save(bearer, name.trim(), contact.trim()), 'identity');
      return 'Saved. Listeners and callers see it now.';
    });
  };
  const saveJurisdiction = (e: SubmitEvent) => {
    e.preventDefault();
    void run('jurisdiction', async () => {
      const r = await api.choosePreset(bearer, preset);
      presetNow = r.setting.preset;
      return 'Saved.';
    });
  };
  const saveAccent = (value: string) =>
    run('accent', async () => {
      const b = await api.setAccent(bearer, value);
      if (setup) setup = { ...setup, branding: b };
      accent = b.accent ?? '';
      return value ? 'Saved. The public pages use it now.' : 'Back to WeaveSignal’s own colour.';
    });
  function pick(kind: 'icon' | 'banner', e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    void run(kind, async () => {
      const problem = api.pictureProblem(kind, file);
      if (problem) throw new api.Problem(422, problem);
      const b = await api.setPicture(bearer, kind, file);
      if (setup) setup = { ...setup, branding: b };
      return kind === 'icon' ? 'Saved. It is the station’s icon now.' : 'Saved. It is on the player now.';
    });
  }
  const remove = (kind: 'icon' | 'banner') =>
    run(kind, async () => {
      const b = await api.removePicture(bearer, kind);
      if (setup) setup = { ...setup, branding: b };
      return 'Removed.';
    });
  const saveStream = (e: SubmitEvent) => {
    e.preventDefault();
    void run('stream', async () => {
      take(await api.streamPassword(bearer, streamOn, streamNew), 'stream');
      streamNew = '';
      return 'Saved. It applies when the app next restarts.';
    });
  };
  const finish = () =>
    run('finish', async () => {
      take(await api.finish(bearer), 'none');
      return 'Setup is finished.';
    });

  async function openAnswers(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = '';
    filled = null;
    if (!file) return;
    try {
      if (file.size > 64 * 1024) throw new Error('That file is not an answers file from the install wizard.');
      const a = readAnswersFile(await file.text());
      if (a.name) name = a.name;
      if (a.contact) contact = a.contact;
      if (a.jurisdiction && presets.some((p) => p.key === a.jurisdiction)) preset = a.jurisdiction;
      if (a.accent) accent = a.accent;
      filled = 'Filled in from the file. Nothing is saved yet: check each part and save it.';
    } catch (err) {
      filled = err instanceof Error ? err.message : 'That file could not be read.';
    }
  }

  const when = (ms: number) => new Date(ms).toLocaleDateString(undefined, { day: 'numeric', month: 'long', year: 'numeric' });
  const mailWords = { command: 'Sent by email', outbox: 'Written to files, not sent', off: 'Off: staff sign in with a password, and the chat is read-only' } as const;
</script>

<Glyphs />
<div class="page setup">
  <header class="top">
    <Mark />
    <span class="station">Station setup</span>
    <span class="spacer"></span>
    {#if bearer}<button type="button" class="btn quiet" onclick={lock}>Sign out</button>{/if}
  </header>

  {#if !bearer || !setup}
    <main class="card" aria-labelledby="signin-h">
      <h1 id="signin-h">Set up your station</h1>
      <p class="muted">This page is for the station’s admin. Nothing about the station shows until you sign in.</p>
      <form class="stack" onsubmit={signIn} novalidate>
        <fieldset class="choice">
          <legend class="sr-only">How to sign in</legend>
          <label class="seg"><input type="radio" name="how" value="key" bind:group={how} /><span>With the station key</span></label>
          <label class="seg"><input type="radio" name="how" value="staff" bind:group={how} /><span>As a member of staff</span></label>
        </fieldset>
        {#if how === 'key'}
          <div class="field">
            <label for="key">Station key</label>
            <input id="key" type="password" autocomplete="off" spellcheck="false" bind:value={key} aria-describedby="key-hint{signProblem ? ' sign-err' : ''}" aria-invalid={signProblem ? 'true' : undefined} />
            <p class="hint" id="key-hint">The installer made it. It is the line <code>CONTROL_ADMIN_TOKEN</code> in the station’s settings.</p>
          </div>
          <details class="trouble">
            <summary>Where to find the station key</summary>
            <div>
              <p class="small"><b>Compose and Quadlets:</b> in <code>deploy/compose/.env</code>, or the settings file you gave <code>weavesignal.sh</code>.</p>
              <p class="small"><b>Cloudron:</b> in <code>/app/data/secrets.env</code>, with the app’s File Manager.</p>
              <p class="small">Keep it private: it opens everything the station has.</p>
            </div>
          </details>
        {:else}
          <div class="field">
            <label for="user">Username</label>
            <input id="user" type="text" autocomplete="username" autocapitalize="none" spellcheck="false" bind:value={username} aria-invalid={signProblem ? 'true' : undefined} />
          </div>
          <div class="field">
            <label for="pass">Password</label>
            <input id="pass" type="password" autocomplete="current-password" bind:value={password} aria-describedby={signProblem ? 'sign-err' : undefined} aria-invalid={signProblem ? 'true' : undefined} />
            <p class="hint">Only a member of staff whose role is admin can set the station up.</p>
          </div>
        {/if}
        {#if signProblem}<p class="err" id="sign-err" role="alert"><svg aria-hidden="true"><use href="#g-fault" /></svg>{signProblem}</p>{/if}
        <button type="submit" class="btn primary wide" disabled={signing}>{signing ? 'Signing in…' : 'Sign in'}</button>
      </form>
    </main>
  {:else}
    <main class="main" aria-labelledby="setup-h">
      <section class="status" class:pending={setup.status === 'pending'} aria-labelledby="setup-h">
        <h1 id="setup-h" tabindex="-1" bind:this={heading}>
          {setup.status === 'pending' ? 'This station is not set up yet' : `${setup.name.value} is set up`}
        </h1>
        {#if setup.status === 'pending'}
          <ul class="missing">
            {#each setup.missing as m (m)}<li>{m}</li>{/each}
          </ul>
        {:else if setup.completed_at_ms}
          <p class="muted">Setup was finished on {when(setup.completed_at_ms)}. Change anything here whenever you need to.</p>
        {:else}
          <p class="muted">Its settings file gave it a name and a takedown contact. Add the look, check the rest, then finish.</p>
        {/if}
        <div class="answers">
          <label class="btn" for="answers">Open an answers file</label>
          <input id="answers" class="sr-only" type="file" accept="application/json,.json" onchange={openAnswers} />
          <p class="small">If you used the install wizard on the website, open <code>weavesignal-setup.json</code> here to fill this page in.</p>
        </div>
        {#if filled}<p class="note" role="status">{filled}</p>{/if}
      </section>

      <form class="card" aria-labelledby="id-h" onsubmit={saveIdentity} novalidate>
        <h2 id="id-h">Name and takedown contact</h2>
        <div class="field">
          <label for="name">Station name</label>
          <input id="name" type="text" maxlength="200" autocomplete="off" bind:value={name} aria-describedby="name-hint" aria-invalid={wrongField === 'name' ? 'true' : undefined} />
          <p class="hint" id="name-hint">
            Listeners read it on the player, in podcast feeds and on show cards.
            {#if setup.name.source === 'page'}Set here; the settings file says “{setup.name.settings_value}”. Clear the box to go back to it.{:else}This is the settings file’s name.{/if}
          </p>
        </div>
        <div class="field">
          <label for="contact">Takedown contact</label>
          <input id="contact" type="text" maxlength="200" autocomplete="off" autocapitalize="none" spellcheck="false" inputmode="email" placeholder="calls@station.example" bind:value={contact} aria-describedby="contact-hint" aria-invalid={wrongField === 'contact' ? 'true' : undefined} />
          <p class="hint" id="contact-hint">
            An email address or a link where a caller can ask for their call to be taken down. Callers read it before they agree to be recorded. Call-in stays closed until there is one.
            {#if setup.contact.source === 'page' && setup.contact.settings_value}Set here; the settings file says “{setup.contact.settings_value}”.{/if}
          </p>
        </div>
        <p class="small">Changing either changes the words callers agree to, so each caller is asked again. Calls already agreed keep the words they were given.</p>
        <div class="act">
          <button type="submit" class="btn primary" disabled={busy !== null || !identityChanged}>{busy === 'identity' ? 'Saving…' : 'Save name and contact'}</button>
          {#if wrong.identity}<p class="err" role="alert"><svg aria-hidden="true"><use href="#g-fault" /></svg>{wrong.identity}</p>{:else if said.identity}<p class="ok" role="status"><svg aria-hidden="true"><use href="#g-tick" /></svg>{said.identity}</p>{/if}
        </div>
      </form>

      <form class="card" aria-labelledby="jur-h" onsubmit={saveJurisdiction} novalidate>
        <h2 id="jur-h">Jurisdiction</h2>
        <p class="small">It decides how long the station keeps recordings, logs and chat, and so what callers are told. A licensed broadcaster has to choose one.</p>
        {#if presets.length}
          <div class="field">
            <label for="preset">Where the station answers to</label>
            <select id="preset" bind:value={preset} aria-describedby="preset-sum">
              {#if presetNow === 'custom'}<option value="custom" disabled>Custom (set in the studio)</option>{/if}
              {#each presets as p (p.key)}<option value={p.key}>{p.name}</option>{/each}
            </select>
            <p class="hint" id="preset-sum">
              {#if preset === 'custom'}The periods were set one by one in the studio app’s Settings. Choosing a preset here replaces them.{:else}{chosenPreset?.summary ?? ''}{/if}
            </p>
          </div>
          <div class="act">
            <button type="submit" class="btn primary" disabled={busy !== null || preset === presetNow || preset === 'custom'}>{busy === 'jurisdiction' ? 'Saving…' : 'Save jurisdiction'}</button>
            {#if wrong.jurisdiction}<p class="err" role="alert"><svg aria-hidden="true"><use href="#g-fault" /></svg>{wrong.jurisdiction}</p>{:else if said.jurisdiction}<p class="ok" role="status"><svg aria-hidden="true"><use href="#g-tick" /></svg>{said.jurisdiction}</p>{/if}
          </div>
          <p class="small">Each period, holds and Custom are in the studio app, under Settings.</p>
        {:else}
          <p class="problem">The list could not load. Set the jurisdiction in the studio app, under Settings.</p>
        {/if}
      </form>

      <section class="card" aria-labelledby="look-h">
        <h2 id="look-h">The look</h2>
        <p class="small">Your colour, icon and banner go on the player, the call-in and guest pages. The studio app keeps its own colours, so a desk reads the same at every station.</p>

        <BrandPreview station={name.trim() || setup.name.value} derived={shades} icon={look.icon?.url ?? null} banner={look.banner?.url ?? null} />

        <form class="stack" onsubmit={(e) => (e.preventDefault(), void saveAccent(accentHex ?? ''))} novalidate>
          <div class="field">
            <label for="accent">Accent colour</label>
            <div class="colour">
              <input type="color" aria-label="Pick the accent colour" value={accentHex ?? '#7aa2f7'} oninput={(e) => (accent = e.currentTarget.value)} />
              <input id="accent" type="text" maxlength="7" autocomplete="off" autocapitalize="none" spellcheck="false" placeholder="#7aa2f7" bind:value={accent} aria-describedby="accent-hint" aria-invalid={accent.trim() !== '' && !accentHex ? 'true' : undefined} />
            </div>
            <p class="hint" id="accent-hint">
              {#if accent.trim() !== '' && !accentHex}
                A colour is six hex digits, such as #b4532a.
              {:else if shades?.dark && shades.light}
                Used for links, the main button and the focus ring. Each theme shows the nearest shade that stays readable:
                dark {shades.dark.accent}{shades.dark.adjusted ? ' (lightened)' : ''}, light {shades.light.accent}{shades.light.adjusted ? ' (darkened)' : ''}.
              {:else}
                Used for links, the main button and the focus ring. Leave it empty for WeaveSignal’s own blue.
              {/if}
            </p>
          </div>
          <div class="act">
            <button type="submit" class="btn primary" disabled={busy !== null || !accentChanged || (accent.trim() !== '' && !accentHex)}>{busy === 'accent' ? 'Saving…' : 'Save colour'}</button>
            {#if look.accent}<button type="button" class="btn" disabled={busy !== null} onclick={() => saveAccent('')}>Use WeaveSignal’s own</button>{/if}
            {#if wrong.accent}<p class="err" role="alert"><svg aria-hidden="true"><use href="#g-fault" /></svg>{wrong.accent}</p>{:else if said.accent}<p class="ok" role="status"><svg aria-hidden="true"><use href="#g-tick" /></svg>{said.accent}</p>{/if}
          </div>
        </form>

        {#each ['icon', 'banner'] as const as kind (kind)}
          {@const pic = look[kind]}
          <div class="picture">
            <h3>{kind === 'icon' ? 'Brand icon' : 'Banner'}</h3>
            <p class="small" id="{kind}-hint">
              {kind === 'icon' ? 'Shown beside the station’s name, and as the icon of the browser tab and the phone’s home screen.' : 'Shown across the top of the player.'}
              {api.PICTURE_RULES[kind].hint}
            </p>
            <div class="act">
              {#if pic}<img class="thumb {kind}" src={pic.url} width={pic.width} height={pic.height} alt="The {kind} as the station stored it" />{/if}
              <label class="btn" class:busy={busy === kind} for="{kind}-file">{busy === kind ? 'Sending…' : pic ? 'Replace' : `Choose ${kind === 'icon' ? 'an icon' : 'a banner'}`}</label>
              <input id="{kind}-file" class="sr-only" type="file" accept="image/png,image/jpeg" disabled={busy !== null} aria-describedby="{kind}-hint" onchange={(e) => pick(kind, e)} />
              {#if pic}<button type="button" class="btn quiet" disabled={busy !== null} onclick={() => remove(kind)}>Remove</button>{/if}
            </div>
            {#if wrong[kind]}<p class="err" role="alert"><svg aria-hidden="true"><use href="#g-fault" /></svg>{wrong[kind]}</p>{:else if said[kind]}<p class="ok" role="status"><svg aria-hidden="true"><use href="#g-tick" /></svg>{said[kind]}</p>{/if}
          </div>
        {/each}
      </section>

      <form class="card" aria-labelledby="stream-h" onsubmit={saveStream} novalidate>
        <h2 id="stream-h">Stream password</h2>
        {#if setup.stream_password.available}
          <p class="small">Ask listeners for a password while the station is not ready to be heard. They use the name <code>listener</code> and the password.</p>
          <label class="check">
            <input type="checkbox" bind:checked={streamOn} />
            <span>Ask listeners for a password<span class="small">The player page cannot play a stream with a password: a browser does not ask for one for sound from another address. Radio apps and players can.</span></span>
          </label>
          <div class="field">
            <label for="stream-pass">New password</label>
            <input id="stream-pass" type="text" autocomplete="off" autocapitalize="none" spellcheck="false" maxlength="64" bind:value={streamNew} aria-describedby="stream-hint" aria-invalid={wrongField === 'password' ? 'true' : undefined} />
            <p class="hint" id="stream-hint">8 to 64 letters, digits, dots, underscores, tildes or hyphens. Leave it empty to keep the one already set.</p>
          </div>
          <div class="act">
            <button type="submit" class="btn primary" disabled={busy !== null}>{busy === 'stream' ? 'Saving…' : 'Save stream password'}</button>
            {#if wrong.stream}<p class="err" role="alert"><svg aria-hidden="true"><use href="#g-fault" /></svg>{wrong.stream}</p>{:else if said.stream}<p class="ok" role="status"><svg aria-hidden="true"><use href="#g-tick" /></svg>{said.stream}</p>{/if}
          </div>
          <p class="small">
            Now: the stream {setup.stream_password.enabled ? 'asks for a password' : 'is open to anyone with its address'}.
            {#if setup.stream_password.restart_needed}<b>Restart the app in Cloudron’s dashboard to apply what you saved.</b>{/if}
          </p>
        {:else}
          <p class="small">This install has no stream password: the stream is open to anyone with its address, and in no directory unless you list it. If it needs a password, put one in the proxy in front of the station.</p>
        {/if}
      </form>

      <section class="card" aria-labelledby="install-h">
        <h2 id="install-h">Set when the station was installed</h2>
        <p class="small">These need a restart or a change of DNS, so this page shows them and does not change them.</p>
        <dl class="facts">
          <div><dt>Public address</dt><dd>{setup.install.public_url || 'None: this machine only'}</dd></div>
          <div><dt>Listener stream</dt><dd>{setup.install.stream_url || 'Not published'}</dd></div>
          <div><dt>Live room</dt><dd>{setup.install.room_url}</dd></div>
          <div><dt>Room media</dt><dd>UDP port {setup.install.media_port}</dd></div>
          <div><dt>TURN over TLS</dt><dd>TCP port {setup.install.turn_tls_port}</dd></div>
          <div><dt>Sign-in codes</dt><dd>{mailWords[setup.install.mail]}</dd></div>
          <div><dt>Search engines</dt><dd>{setup.install.listed ? 'May list the public pages' : 'Asked not to list the station'}</dd></div>
        </dl>
        <details class="trouble">
          <summary>How to change them</summary>
          <div>
            <p class="small"><b>Compose and Quadlets:</b> edit the settings file (<code>deploy/compose/.env</code>) and run <code>weavesignal.sh</code> again; a new name is <code>--public</code> and the name.</p>
            <p class="small"><b>Cloudron:</b> the names and the public ports are in the app’s Location settings; video, mail and listing are in <code>/app/data/weavesignal.env</code>. Restart the app after an edit.</p>
            <p class="small"><b>DNS:</b> the name guests’ browsers reach TURN at needs an A record and no AAAA record, or guests on IPv6 networks that block UDP cannot join. On Compose that is the station’s one name; on Cloudron it is the studio’s name.</p>
            <p class="small">The install guides are at <a href="https://weavesignal.top/docs/" rel="noopener">weavesignal.top/docs</a>.</p>
          </div>
        </details>
      </section>

      {#if !setup.completed_at_ms}
        <section class="card finish" aria-labelledby="finish-h">
          <h2 id="finish-h">Finish</h2>
          <p class="small">
            {#if setup.missing.length}Still missing: {setup.missing.join(' ')} You can finish anyway and come back.{:else}Finishing stops the station offering setup. This page stays here for you.{/if}
          </p>
          <div class="act">
            <button type="button" class="btn primary" disabled={busy !== null} onclick={finish}>{busy === 'finish' ? 'Finishing…' : 'Finish setup'}</button>
            {#if wrong.finish}<p class="err" role="alert"><svg aria-hidden="true"><use href="#g-fault" /></svg>{wrong.finish}</p>{/if}
          </div>
        </section>
      {:else if said.finish}
        <p class="ok" role="status"><svg aria-hidden="true"><use href="#g-tick" /></svg>{said.finish}</p>
      {/if}
    </main>
  {/if}
</div>

<style>
  .setup { max-width: 44rem; }
  .main { display: grid; gap: var(--s-4); }
  h1 { font-size: var(--fs-24); }
  h1:focus-visible { outline-offset: 4px; }
  h3 { font-size: var(--fs-16); }
  code { font-family: var(--font-mono); font-size: 0.9em; background: var(--surface-2); padding: 0.1em 0.3em; border-radius: var(--r-1); overflow-wrap: anywhere; }
  .stack { display: grid; gap: var(--s-3); }
  .status { display: grid; gap: var(--s-3); padding: var(--s-4); border-radius: var(--r-3); border: 1px solid var(--hairline); border-left: 4px solid var(--status-good); background: var(--surface); }
  .status.pending { border-left-color: var(--status-warning); }
  .missing { margin: 0; padding-left: 1.2rem; display: grid; gap: var(--s-1); }
  .answers { display: flex; flex-wrap: wrap; gap: var(--s-3); align-items: center; }
  .answers .small { flex: 1 1 16rem; }
  /* a file input's label is the button; the input keeps the focus, so the ring is drawn on the label */
  :global(input.sr-only:focus-visible) + :global(.btn),
  .btn:has(+ input.sr-only:focus-visible) { outline: var(--focus-ring); outline-offset: var(--focus-offset); }
  .choice { border: 0; padding: 0; margin: 0; display: grid; grid-template-columns: 1fr 1fr; gap: var(--s-2); }
  .seg { position: relative; display: block; cursor: pointer; }
  .seg input { position: absolute; opacity: 0; inset: 0; margin: 0; cursor: pointer; }
  .seg span { display: flex; align-items: center; justify-content: center; text-align: center; min-height: var(--target-touch); padding: 0 var(--s-3); border: 1px solid var(--axis); border-radius: var(--r-2); font-weight: 650; background: var(--surface-2); line-height: var(--lh-label); }
  .seg input:checked + span { border-color: var(--accent); box-shadow: inset 0 0 0 1px var(--accent); background: var(--surface-3); }
  .seg input:focus-visible + span { outline: var(--focus-ring); outline-offset: var(--focus-offset); }
  .field :is(input[type='password'], select) { min-height: var(--target-touch); padding: 0 var(--s-3); border-radius: var(--r-2); border: 1px solid var(--axis); background: var(--plane); color: var(--ink); font: inherit; font-size: var(--fs-16); width: 100%; }
  .act { display: flex; flex-wrap: wrap; gap: var(--s-3); align-items: center; }
  .act .err, .act .ok { flex: 1 1 14rem; }
  .ok { font-size: var(--fs-14); color: var(--ink); line-height: var(--lh-label); display: flex; gap: var(--s-2); align-items: flex-start; }
  .ok svg { width: 16px; height: 16px; flex: none; margin-top: 1px; color: var(--status-good); }

  .colour { display: grid; grid-template-columns: auto 1fr; gap: var(--s-2); align-items: center; }
  .colour input[type='color'] { width: var(--target-touch); height: var(--target-touch); padding: 2px; border: 1px solid var(--axis); border-radius: var(--r-2); background: var(--plane); cursor: pointer; }
  .colour input[type='text'] { font-family: var(--font-mono); }
  .picture { display: grid; gap: var(--s-2); padding-top: var(--s-3); border-top: 1px solid var(--hairline); }
  .thumb { border: 1px solid var(--hairline); border-radius: var(--r-2); background: repeating-conic-gradient(var(--surface-3) 0% 25%, var(--surface-2) 0% 50%) 50% / 12px 12px; object-fit: contain; }
  .thumb.icon { width: 44px; height: 44px; }
  .thumb.banner { width: 132px; height: 44px; object-fit: cover; }
  .btn.busy { opacity: 0.55; }
  .facts { margin: 0; display: grid; gap: 0; }
  .facts div { display: grid; grid-template-columns: 9.5rem 1fr; gap: var(--s-3); padding: var(--s-2) 0; border-top: 1px solid var(--hairline); }
  .facts div:first-child { border-top: 0; }
  .facts dt { color: var(--ink-2); font-size: var(--fs-14); }
  .facts dd { margin: 0; overflow-wrap: anywhere; }
  @media (max-width: 26rem) {
    .facts div { grid-template-columns: 1fr; gap: 0; }
  }
</style>
