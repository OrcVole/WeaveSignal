<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Send a key (keyed shows, 1.3.2): the one thing to send, four ways. The link that opens the
  // player with the key entered, the key itself to read out or print, a QR code of the link, and
  // a message ready to send with all of it, which staff may change before it goes. The station
  // sends nothing for you from here: the message is copied into your own mail or messenger.
  //
  // A key's text is shown here and kept nowhere in the app. Asking for it again is logged on the
  // key by the station (who saw it, when); a key just made or replaced brings its text with it,
  // and that is not asked for a second time. At its top, the three steps: this is the third;
  // the second makes another key, the first keeps a show for people with a key.
  // Panel name `keys.send` (ADR-0070).
  import { untrack } from 'svelte';
  import { qr, qrPath, qrSvg } from '../../../../packages/ui/qr.ts';
  import { saveLabel, saveText, savedWords } from '../lib/saveFile.ts';
  import { actionProblem, fileStem, messageFor, opensWords, stationAddress } from '../lib/keys.ts';
  import { keyText, type KeyText, type ListenerKey } from '../lib/keysApi.ts';
  import { keyed } from '../lib/keys.svelte.ts';
  import { studio } from '../lib/studio.svelte.ts';
  import KeySteps from './KeySteps.svelte';

  let {
    key,
    given = null,
    why = 'send',
    onclose,
    onreplace,
    onkeep,
    onmake,
  }: {
    /** Step 1: Make a show keyed, over this dialog. */
    onkeep?: () => void;
    /** Step 2: make another key (this dialog closes). */
    onmake?: () => void;
    key: ListenerKey;
    /** The text and link the station just answered with (a key made or replaced a moment ago). */
    given?: KeyText | null;
    /** `made`: the key was made just now; `replaced`: it is the new key of a replacement. */
    why?: 'send' | 'made' | 'replaced';
    onclose: () => void;
    /** Replace the key instead, when its text can no longer be read. */
    onreplace?: () => void;
  } = $props();

  let el = $state<HTMLDialogElement | null>(null);
  const first = untrack(() => given);
  let got = $state<KeyText | null>(first);
  let problem = $state('');
  let unreadable = $state(false);
  let personal = $state(true);
  let subject = $state('');
  let body = $state('');
  let copied = $state('');

  const station = $derived(studio.view?.station || 'the station');
  const opens = $derived(opensWords(key, keyed.list?.shows ?? []));
  const code = $derived(got ? qr(got.link) : null);
  const path = $derived(code ? qrPath(code) : null);

  function write() {
    if (!got) return;
    const m = messageFor({ station, label: key.label, kind: key.kind, link: got.link, text: got.text, opens, valid_from_ms: key.valid_from_ms, valid_until_ms: key.valid_until_ms, max_devices: key.max_devices, personal, contact: '' }, studio.now || Date.now(), studio.zone);
    subject = m.subject;
    body = m.body;
  }

  $effect(() => {
    el?.showModal();
    untrack(() => {
      if (got) return write();
      void keyText(key.id)
        .then((t) => {
          got = t;
          write();
        })
        .catch((e) => {
          problem = actionProblem(e);
          unreadable = /can no longer be read/.test(problem);
        });
    });
  });

  async function copy(what: string, text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = what;
    } catch {
      copied = '';
      problem = 'This window could not copy. Select the text and copy it by hand.';
    }
  }

  /** Save the QR code as a picture: where the person says in the packaged app, a download in a
   *  browser. What happened is said beside the button. */
  let qrSaved = $state('');
  let qrSaving = $state(false);
  async function saveQr() {
    if (!code || qrSaving) return;
    qrSaving = true;
    try {
      qrSaved = savedWords('The QR code', await saveText(`key-${fileStem(key.label, 'qr')}.svg`, qrSvg(code)));
    } catch (e) {
      qrSaved = '';
      problem = e instanceof Error ? e.message : 'The file was not saved.';
    } finally {
      qrSaving = false;
    }
  }

  function close() {
    el?.close();
    onclose();
  }
</script>

<dialog bind:this={el} class="kdialog wide" data-panel="keys.send" aria-labelledby="ks-h" oncancel={(e) => { e.preventDefault(); close(); }}>
  <h2 id="ks-h" data-panel-title>{why === 'made' ? `The key for ${key.label} is made` : why === 'replaced' ? `The new key for ${key.label}` : `Send ${key.label} their key`}</h2>
  <KeySteps current={3} go={{ 1: onkeep, 2: onmake ? () => { close(); onmake(); } : undefined }} />
  {#if why === 'replaced'}<p class="lead">The old key is ended and will not work again. Send this one to the same person; every other key is untouched.</p>{/if}

  {#if problem && !got}
    <div class="callout serious" role="alert">
      <svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg>
      <div>
        <p><strong>The key’s text could not be shown.</strong> {problem}</p>
        {#if unreadable && onreplace}<button type="button" class="btn sm" onclick={() => { close(); onreplace?.(); }}>Replace the key…</button>{/if}
      </div>
    </div>
  {:else if !got}
    <p role="status" class="small">Asking the station for the key…</p>
  {:else}
    <div class="send">
      <div class="ways">
        <div class="field">
          <label for="ks-link">The link</label>
          <div class="row">
            <input id="ks-link" type="text" class="mono" readonly value={got.link} onfocus={(e) => e.currentTarget.select()} />
            <button type="button" class="btn sm" onclick={() => copy('link', got!.link)}>{copied === 'link' ? 'Copied' : 'Copy'}</button>
          </div>
          <span class="hint">One press and the player opens with the key entered. The key is after the <span class="mono">#</span>, which no server sees.</span>
        </div>
        <div class="field">
          <span class="label" id="ks-code-l">The key, to read out or print</span>
          <div class="row">
            <output class="code mono" aria-labelledby="ks-code-l" data-testid="key-text">{got.text}</output>
            <button type="button" class="btn sm" onclick={() => copy('code', got!.text)}>{copied === 'code' ? 'Copied' : 'Copy'}</button>
          </div>
          <span class="hint">Typed into Enter a key at {stationAddress(got.link).replace(/^https?:\/\//, '')}. It has no 0, 1, I, L or O, and capitals and dashes do not matter.</span>
        </div>
        <div class="qr">
          {#if path}
            <svg class="qr-img" viewBox="0 0 {path.box} {path.box}" role="img" aria-label="A QR code of the link" shape-rendering="crispEdges" data-testid="key-qr"><path d={path.d} /></svg>
          {/if}
          <div class="qr-say">
            <span class="label">A QR code of the link</span>
            <span class="hint">For a ticket, a poster or a screen: a phone’s camera opens the player with the key entered.</span>
            <button type="button" class="btn sm" disabled={qrSaving} aria-busy={qrSaving || undefined} onclick={saveQr}>{saveLabel('Save as a picture (SVG)')}</button>
            {#if qrSaved}<span class="hint" role="status" data-testid="key-qr-saved">{qrSaved}</span>{/if}
          </div>
        </div>
        {#if got.email}
          <div class="field">
            <span class="label">Their email address</span>
            <div class="row">
              <output class="mono addr">{got.email}</output>
              <button type="button" class="btn sm" onclick={() => copy('email', got!.email ?? '')}>{copied === 'email' ? 'Copied' : 'Copy'}</button>
            </div>
          </div>
        {/if}
      </div>

      <div class="msg">
        <div class="field">
          <label for="ks-subject">A message, ready to send</label>
          <input id="ks-subject" type="text" bind:value={subject} aria-label="The subject" />
          <textarea id="ks-body" rows="14" bind:value={body} aria-label="The message" spellcheck="true"></textarea>
        </div>
        <label class="opt-row"><input type="checkbox" bind:checked={personal} onchange={write} /><span>Say the key is theirs alone and not to be passed on <span class="hint">(changing this writes the message again)</span></span></label>
        <div class="row end">
          <span class="hint grow">The station sends nothing from here. Copy the message into your own mail or messenger.</span>
          <button type="button" class="btn primary" onclick={() => copy('message', `${subject}\n\n${body}`)}>{copied === 'message' ? 'Copied' : 'Copy the message'}</button>
        </div>
      </div>
    </div>
    {#if problem}<p class="err" role="alert">{problem}</p>{/if}
    <p class="foot small">{why === 'send' ? 'The station has noted on the key that you looked at its text. ' : 'You can have the text again from the key’s page (the station notes who looks). '}A key limits who may listen; it does not stop someone recording what they hear.</p>
  {/if}

  <div class="acts">
    <button type="button" class="btn" onclick={close}>Done</button>
  </div>
</dialog>
