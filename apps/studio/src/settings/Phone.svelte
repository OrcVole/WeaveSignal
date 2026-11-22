<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Settings › Phone line (`settings.phone`; 1.2, PLAN 17f): an admin's. A telephone line into
  // the screener's queue. Five parts, in the order a station meets them:
  //
  //   the line        on, off or closed, and what puts it right. It is off until a trunk is set
  //                   (the phone provider's connection), which is an install setting.
  //   the words       what a caller hears before the queue, and whether they are played: a
  //                   switch (`consent_words`, `PUT /api/v1/phone/consent`), with a tooltip on
  //                   whether a station needs the words at all. What is changed here is kept
  //                   by the station and wins over the settings file (`PHONE_CONSENT_WORDS`).
  //   the recording   which recording a caller hears (the operator, 2026-11-20): Alba, the
  //                   default, or Edinburgh, the two voices that come with WeaveSignal, each
  //                   with Play (`GET /api/v1/phone/consent/default/{voice}.opus`); or the
  //                   station's own reading, which plays in place of either once it is made:
  //                   upload a sound file, or record one here where the window can reach a
  //                   microphone. The voice is `default_voice` (`PHONE_DEFAULT_VOICE`); choosing
  //                   one while the station has its own recording chooses what plays if that
  //                   recording is removed.
  //   the contact     the station's own takedown contact, which the phone words name (a show's
  //                   contact is for its browser callers).
  //   the numbers     the bans, by the last four digits; Ban a number, and Find a number's
  //                   calls for a takedown. A typed number is used for that request only.
  //
  // A station older than 1.2 has none of these routes: nothing is drawn.
  import { onDestroy, onMount, untrack } from 'svelte';
  import StateChip from '../../../../packages/ui/StateChip.svelte';
  import Tip from '../../../../packages/ui/Tip.svelte';
  import { CHIP } from '../../../../packages/ui/states.ts';
  import { base, headers, netFetch } from '../lib/api.ts';
  import { placeError } from '../lib/account.ts';
  import { account } from '../lib/account.svelte.ts';
  import { routeAbsent } from '../lib/hosts.ts';
  import {
    BAN_LENGTHS,
    CONTACT_MAX,
    DEFAULT_VOICES,
    SET_AT_INSTALL,
    WORDS_TIP,
    banNumber,
    banProblem,
    banUntil,
    canSetConsent,
    consentLock,
    contactProblem,
    findCalls,
    lengthWords,
    liftBan,
    lineWords,
    phoneBans,
    phoneStatus,
    previewVoice,
    putWords,
    removeWords,
    saveStationContact,
    setConsent,
    stationContact,
    voiceInForce,
    wordsLine,
    type ConsentVoice,
    type ContactView,
    type PhoneBan,
    type PhoneStatus,
  } from '../lib/phone.ts';
  import { canRecord, record, type Recorder } from '../lib/wav.ts';
  import { whenWords } from '../lib/hosts.ts';
  import { sched } from '../lib/schedule.svelte.ts';
  import { studio } from '../lib/studio.svelte.ts';

  let { initial = null }: { initial?: { status: PhoneStatus; bans: PhoneBan[]; contact: ContactView | null } | null } = $props();

  const seed = untrack(() => initial);
  let status = $state<PhoneStatus | null>(seed?.status ?? null);
  let bans = $state<PhoneBan[]>(seed?.bans ?? []);
  let contactView = $state<ContactView | null>(seed?.contact ?? null);
  let absent = $state(false);
  let loadError = $state('');

  const zone = $derived(sched.zone || studio.zone);
  const words = $derived(status ? lineWords(status) : null);
  const chip = $derived(
    words?.state === 'open' ? { chip: CHIP.allGood, word: 'Taking calls' } : words?.state === 'closed' ? { chip: CHIP.linkDegraded, word: 'Closed' } : { chip: CHIP.standby, word: 'Off' },
  );
  const lineOn = $derived(!!status && status.closed !== 'off');

  async function load() {
    loadError = '';
    try {
      const [s, b] = await Promise.all([phoneStatus(), phoneBans().catch(() => [] as PhoneBan[])]);
      status = s;
      bans = b;
    } catch (e) {
      if (routeAbsent(e)) absent = true;
      else loadError = placeError(e, []).message;
    }
    try {
      contactView = await stationContact();
      contact = contactView.contact.value;
    } catch {
      // The contact is the setup page's; where it cannot be read the field is left out.
      contactView = null;
    }
  }
  let timer: ReturnType<typeof setInterval> | undefined;
  onMount(() => {
    if (seed) return void (contact = seed.contact?.contact.value ?? '');
    void load();
    // The line's own figures (who is hearing the words) move: read them while the panel is open.
    timer = setInterval(async () => {
      if (absent || !status || busy) return;
      status = await phoneStatus().catch(() => status);
    }, 15_000);
  });
  onDestroy(() => {
    clearInterval(timer);
    clearInterval(clock);
    recorder?.cancel();
    stopPlaying();
  });

  // ---- the words: the switch, and which recording plays ----
  // A station that says what is in force (`consent_words`, `default_voice`) takes the change
  // from here while its line is on; with the line off the switch and the choice are shown as
  // they stand, and say why.
  let tipEl = $state<HTMLButtonElement | null>(null);
  let switchNote = $state('');
  const wordsOn = $derived(status?.words ? status.words.kind !== 'off' : null);
  const settable = $derived(canSetConsent(status));
  const lock = $derived(consentLock(status));
  const voice = $derived(status ? voiceInForce(status) : null);
  const NOT_HERE = $derived(status?.closed === 'off' ? 'The phone line is off: once it is on, this is changed here.' : `It is not changed here on this station’s version. ${SET_AT_INSTALL}`);
  let setting = $state(false);
  async function change(c: { consent_words?: boolean; default_voice?: ConsentVoice }, done: string) {
    if (setting) return;
    switchNote = '';
    const say = (m: string) => (c.consent_words !== undefined ? (switchNote = m) : (playNote = m));
    playNote = '';
    if (!settable) return void say(c.consent_words !== undefined ? `This switch shows the setting. ${NOT_HERE} Its name is PHONE_CONSENT_WORDS.` : `This shows which recording plays. ${NOT_HERE}`);
    setting = true;
    try {
      status = await setConsent(c);
      say(done);
    } catch (e) {
      say(routeAbsent(e) ? `${NOT_HERE}` : placeError(e, []).message);
    } finally {
      setting = false;
    }
  }
  // A default voice, played here: fetched with the window's sign-in, then played from memory.
  let playing = $state<string | null>(null);
  let playNote = $state('');
  let player: HTMLAudioElement | null = null;
  function stopPlaying() {
    player?.pause();
    if (player?.src.startsWith('blob:')) URL.revokeObjectURL(player.src);
    player = null;
    playing = null;
  }
  async function play(v: 'alba' | 'edinburgh', name: string) {
    if (playing === v) return stopPlaying();
    stopPlaying();
    playNote = '';
    try {
      const sound = await previewVoice(v);
      player = new Audio(URL.createObjectURL(sound));
      player.onended = stopPlaying;
      playing = v;
      await player.play();
    } catch (e) {
      stopPlaying();
      playNote = routeAbsent(e) ? `${name} cannot be played here: this station has no copy of it.` : `${name} could not be played. ${placeError(e, []).message}`;
    }
  }

  // ---- the station's own recording ----
  let busy = $state(false);
  let recNote = $state('');
  let recError = $state('');
  let fileEl = $state<HTMLInputElement | null>(null);
  const mic = canRecord();
  const RECORD_MAX_MS = 75_000;
  let recorder = $state<Recorder | null>(null);
  let recMs = $state(0);
  let clock: ReturnType<typeof setInterval> | undefined;
  let copied = $state(false);

  async function keep(file: Blob, how: string) {
    busy = true;
    recError = '';
    recNote = '';
    try {
      status = await putWords(file);
      recNote = `${how} The line plays it from the next call.`;
    } catch (e) {
      recError = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
  async function choose(e: Event) {
    const file = (e.currentTarget as HTMLInputElement).files?.[0];
    if (file) await keep(file, 'Recording kept.');
    if (fileEl) fileEl.value = '';
  }
  async function startRecording() {
    recError = '';
    recNote = '';
    try {
      recorder = await record();
      recMs = 0;
      const from = Date.now();
      // A recording made here is kept whole (a WAV), so it stops itself well inside what the
      // station takes: the words are a few sentences. A longer reading is uploaded as a file.
      clock = setInterval(() => {
        recMs = Date.now() - from;
        if (recMs >= RECORD_MAX_MS) void stopRecording();
      }, 200);
    } catch {
      recError = 'This window was not let near a microphone. Allow it, or record the words with any recorder and upload the file.';
    }
  }
  async function stopRecording() {
    if (!recorder) return;
    clearInterval(clock);
    const r = recorder;
    recorder = null;
    const { file, ms } = await r.stop();
    if (ms < 1500) return void (recError = 'That was under a second and a half. Read both sentences, then press Stop.');
    await keep(file, 'Recorded.');
  }
  function cancelRecording() {
    clearInterval(clock);
    recorder?.cancel();
    recorder = null;
  }
  async function remove() {
    busy = true;
    recError = '';
    try {
      status = await removeWords();
      recNote = 'The station’s recording is removed.';
    } catch (e) {
      recError = placeError(e, []).message;
    } finally {
      busy = false;
    }
  }
  async function copyWords() {
    try {
      await navigator.clipboard.writeText(status?.words_to_record ?? '');
      copied = true;
    } catch {
      recNote = 'This window could not reach the clipboard. Select the words and copy them by hand.';
    }
  }

  // ---- the station's takedown contact ----
  let contact = $state('');
  let contactError = $state('');
  let contactNote = $state('');
  let savingContact = $state(false);
  const contactDirty = $derived(!!contactView && contact.trim() !== contactView.contact.value);
  async function saveContact(e: SubmitEvent) {
    e.preventDefault();
    if (!contactView || savingContact) return;
    contactError = contactProblem(contact) ?? '';
    contactNote = '';
    if (contactError) return document.getElementById('ph-contact')?.focus();
    savingContact = true;
    try {
      contactView = await saveStationContact(contactView, contact);
      contact = contactView.contact.value;
      contactNote = contact ? 'Saved. The phone words and the call-in page name it from now.' : 'Cleared. The settings file’s contact is back in force.';
      if (!seed) status = await phoneStatus().catch(() => status);
    } catch (err) {
      contactError = placeError(err, ['contact'] as const).message;
    } finally {
      savingContact = false;
    }
  }

  // ---- the numbers ----
  let number = $state('');
  let reason = $state('');
  let days = $state<number | null>(30);
  let banErrors = $state<{ number?: string; reason?: string; form?: string }>({});
  let banNote = $state('');
  let banning = $state(false);
  async function ban(e: SubmitEvent) {
    e.preventDefault();
    if (banning) return;
    const errs: typeof banErrors = {};
    if (!number.trim()) errs.number = 'Type the number as the caller or the provider gave it.';
    const r = banProblem(reason);
    if (r) errs.reason = r;
    banErrors = errs;
    banNote = '';
    if (errs.number) return document.getElementById('ph-number')?.focus();
    if (errs.reason) return document.getElementById('ph-reason')?.focus();
    banning = true;
    try {
      const made = await banNumber(number.trim(), reason.trim(), days);
      bans = await phoneBans().catch(() => [...made, ...bans]);
      banNote = `Banned. The station kept its last ${made[0]?.tail.length || 4} digits and a key made from it, not the number.`;
      number = '';
      reason = '';
    } catch (err) {
      banErrors = { form: placeError(err, []).message };
    } finally {
      banning = false;
    }
  }
  async function lift(b: PhoneBan) {
    banNote = '';
    try {
      await liftBan(b.ban_key);
      bans = bans.filter((x) => x.ban_key !== b.ban_key);
      banNote = `The ban on ···${b.tail} is lifted.`;
    } catch (err) {
      banNote = placeError(err, []).message;
    }
  }

  let findNumber = $state('');
  let found = $state<number[] | null>(null);
  let findError = $state('');
  let finding = $state(false);
  let confirmErase = $state(false);
  let erased = $state('');
  async function find(e: SubmitEvent) {
    e.preventDefault();
    if (finding) return;
    findError = '';
    erased = '';
    found = null;
    confirmErase = false;
    if (!findNumber.trim()) return void ((findError = 'Type the number the caller gives you.'), document.getElementById('ph-find')?.focus());
    finding = true;
    try {
      found = (await findCalls(findNumber.trim())).consents;
      findNumber = '';
    } catch (err) {
      findError = placeError(err, []).message;
    } finally {
      finding = false;
    }
  }
  async function eraseFound() {
    const ids = found;
    if (!ids?.length) return;
    const by = account.me?.account?.username ?? 'station key';
    finding = true;
    let done = 0;
    try {
      for (const id of ids) {
        const res = await netFetch(`${base()}/api/v1/retention/consents/${id}/erase`, { method: 'POST', headers: headers(true), body: JSON.stringify({ by }) });
        if (!res.ok) throw new Error(((await res.json().catch(() => ({}))) as { error?: string }).error ?? `${res.status} ${res.statusText}`);
        done++;
      }
      erased = `${done} ${done === 1 ? 'call' : 'calls'} erased: the name and what was said about it are gone. The date and the words agreed stay as the station’s record.`;
      found = null;
    } catch (err) {
      findError = `${done} of ${ids.length} erased, then: ${err instanceof Error ? err.message : String(err)}`;
    } finally {
      finding = false;
      confirmErase = false;
    }
  }
</script>

{#if !absent && (status || loadError)}
  <section class="panel" data-panel="settings.phone" aria-labelledby="ph-h">
    <div class="ph">
      <h2 id="ph-h" data-panel-title>Phone line</h2>
      {#if status}<StateChip chip={chip.chip} word={chip.word} />{/if}
    </div>
    {#if loadError}
      <p class="err" role="alert">The phone line’s state could not be read. {loadError} <button type="button" class="btn sm" onclick={load}>Try again</button></p>
    {/if}
    {#if status && words}
      <p class="help" data-testid="phone-line">A telephone number that rings into the screener’s queue, beside callers from the player page. {words.line}</p>
      {#if words.fix}<p class="callout" class:warn={words.state === 'closed'} data-testid="phone-fix">{words.fix}</p>{/if}

      <div class="parts">
        <section class="part" aria-labelledby="ph-words-h">
          <h3 id="ph-words-h">Before a caller joins the queue</h3>
          <div class="switch-row">
            <button
              type="button"
              role="switch"
              class="switch"
              aria-checked={wordsOn ?? false}
              aria-disabled={settable ? undefined : 'true'}
              class:fixed={!settable}
              aria-describedby="ph-switch-why"
              data-testid="phone-words-switch"
              disabled={setting}
              onclick={() => change({ consent_words: !wordsOn }, wordsOn ? 'The words are off. Callers go straight into the queue.' : 'The words are on.')}
              ><span class="knob" aria-hidden="true"></span><span class="sw-label">Play the consent words</span></button
            >
            <span class="sw-state">{wordsOn === null ? 'Off with the line' : wordsOn ? 'On' : 'Off'}</span>
            <button type="button" class="info" bind:this={tipEl} aria-label="Does my station need the consent words?" aria-describedby="ph-words-tip">?</button>
            <Tip id="ph-words-tip" anchor={tipEl} delay={0}>{WORDS_TIP}</Tip>
            <!-- Why the switch cannot be pressed, in words beside it (1.2.1: the operator read a
                 fixed switch with only a hint underneath as broken). -->
            {#if lock}
              <span class="sw-lock" data-testid="phone-words-locked" data-why={lock.why}
                ><svg class="g" aria-hidden="true"><use href="#g-lock" /></svg>{lock.word}</span
              >
            {/if}
          </div>
          <p id="ph-switch-why" class="hint" class:why={!!lock}>{lock?.why === 'line_off' ? lock.how : `${wordsLine(status)}${lock ? ` ${lock.how}` : ''}`}</p>
          {#if switchNote}<p class="hint note" role="status">{switchNote}</p>{/if}
          {#if status.words && status.words.kind !== 'off'}
            <figure class="said">
              <figcaption>A caller hears</figcaption>
              <blockquote>{status.words.text}</blockquote>
            </figure>
          {/if}
          <details class="more">
            <summary>When a station needs the words</summary>
            <p>{WORDS_TIP}</p>
            <p>{status.consent_words_help}</p>
            <p>This is not legal advice.</p>
          </details>
        </section>

        <section class="part" aria-labelledby="ph-rec-h">
          <h3 id="ph-rec-h">The recording a caller hears</h3>
          {#if !lineOn}
            <div class="slot empty" data-testid="phone-recording"><b>Nothing to choose yet.</b> Once the line is on, choose one of the two voices that come with WeaveSignal, or record the station’s own.</div>
          {:else}
            <!-- Three choices (the operator, 2026-11-20): Alba, the default; Edinburgh; or the
                 station's own recording, which is played in place of either. -->
            <div class="voices" role="radiogroup" aria-labelledby="ph-rec-h" data-testid="phone-voices">
              {#each DEFAULT_VOICES as d (d.voice)}
                <div class="voice" class:on={voice === d.voice} data-voice={d.voice}>
                  <label class="pick">
                    <input type="radio" name="ph-voice" value={d.voice} checked={voice === d.voice} aria-disabled={settable ? undefined : 'true'} disabled={setting} onclick={(e) => { if (!settable || status?.recording) e.preventDefault(); void change({ default_voice: d.voice }, status?.recording ? `${d.name} is chosen. The station’s own recording still plays in place of it; ${d.name} plays if that recording is removed.` : `${d.name} plays from the next call.`); }} />
                    <span><b>{d.name}</b> <span class="who">{d.who}{d.voice === 'alba' ? ' · the default' : ''}</span></span>
                  </label>
                  <button type="button" class="btn sm quiet" aria-pressed={playing === d.voice} aria-label="{playing === d.voice ? 'Stop' : 'Play'} {d.name}" onclick={() => play(d.voice, d.name)}><svg aria-hidden="true" viewBox="0 0 12 12">{#if playing === d.voice}<rect x="2" y="2" width="8" height="8" />{:else}<path d="M3 1.5v9l7.5-4.5z" />{/if}</svg>{playing === d.voice ? 'Stop' : 'Play'}</button>
                </div>
              {/each}
              <div class="voice own" class:on={voice === 'own'} data-voice="own">
                <label class="pick">
                  <input
                    type="radio"
                    name="ph-voice"
                    value="own"
                    checked={voice === 'own'}
                    aria-disabled={settable && status.recording ? undefined : 'true'}
                    disabled={setting}
                    onclick={(e) => {
                      if (!settable) e.preventDefault();
                      if (settable && !status!.recording) return void (e.preventDefault(), (playNote = 'Upload or record the station’s own words first.'));
                      void change({ default_voice: 'own' }, 'The station’s own recording plays, and neither voice if it is removed.');
                    }}
                  />
                  <span><b>The station’s own</b> <span class="who">played in place of either voice</span></span>
                </label>
                <div class="own-body">
                  {#if status.recording}
                    <div class="slot" class:stale={status.recording.out_of_date} data-testid="phone-recording">
                      <b>{lengthWords(status.recording.duration_ms)}</b>
                      <span>recorded by {status.recording.recorded_by}, {whenWords(status.recording.recorded_at_ms, Date.now(), zone)}</span>
                      {#if status.recording.out_of_date}<span class="warn-line"><svg class="g-warning" aria-hidden="true"><use href="#g-degraded" /></svg>It says older words than today’s. Record it again.</span>{/if}
                    </div>
                  {:else}
                    <div class="slot empty" data-testid="phone-recording"><b>None yet.</b> Upload a recording, or record one here.</div>
                  {/if}
                  {#if status.words_to_record}
                    <figure class="said">
                      <figcaption>Read this aloud <button type="button" class="btn sm quiet" onclick={copyWords}>{copied ? 'Copied' : 'Copy'}</button></figcaption>
                      <blockquote data-testid="phone-to-record">{status.words_to_record}</blockquote>
                    </figure>
                    <p class="hint">It names the station’s own takedown contact, not a show’s, so one recording serves every show. When these words change, the line closes until they are recorded again.</p>
                  {/if}
                  <div class="acts">
                    {#if recorder}
                      <span class="recording" role="status"><span class="dot" aria-hidden="true"></span>Recording · <span class="num">{lengthWords(recMs)}</span></span>
                      <button type="button" class="btn primary" onclick={stopRecording}>Stop and keep</button>
                      <button type="button" class="btn quiet" onclick={cancelRecording}>Cancel</button>
                    {:else}
                      <label class="btn file" class:disabled={busy}
                        >Upload a recording…<input bind:this={fileEl} type="file" class="sr-only" accept="audio/*,.wav,.flac,.mp3,.ogg,.opus,.m4a" onchange={choose} disabled={busy} /></label
                      >
                      {#if mic}<button type="button" class="btn" disabled={busy} onclick={startRecording}>Record here</button>{/if}
                      {#if status.recording}<button type="button" class="btn quiet" disabled={busy} onclick={remove}>Remove</button>{/if}
                    {/if}
                  </div>
                  <p class="hint">A sound file (WAV, FLAC, MP3, Ogg or M4A), from a second and a half to two minutes.{mic ? ' Record here stops by itself at 75 seconds.' : ' This window cannot reach a microphone: record the words with any recorder, then upload the file.'}</p>
                </div>
              </div>
            </div>
            {#if status.recording && status.default_voice}<p class="hint" data-testid="phone-fallback">{status.default_voice === 'own' ? 'If the station’s recording is removed, neither voice plays, and the line takes no calls while the words are on.' : `If the station’s recording is removed, ${DEFAULT_VOICES.find((d) => d.voice === status!.default_voice)?.name ?? status.default_voice} plays.`}</p>{/if}
            {#if voice === null}<p class="hint" data-testid="phone-voice-unknown">This station’s version does not say which of the two voices it plays. Choosing one here comes with its next version.</p>{/if}
            {#if typeof status.closed === 'object' && status.closed?.words === 'default_missing'}<p class="hint">The two voices are missing from this installation, so the line stays closed until the station records its own.</p>{/if}
            {#if playNote}<p class="hint note" role="status">{playNote}</p>{/if}
            {#if recError}<p class="err" role="alert">{recError}</p>{/if}
            <p class="hint note" role="status" aria-live="polite">{busy ? 'The station is reading the recording…' : recNote}</p>
          {/if}
        </section>

        {#if contactView}
          <section class="part" aria-labelledby="ph-contact-h">
            <h3 id="ph-contact-h">Takedown contact</h3>
            <form onsubmit={saveContact} novalidate>
              <div class="field">
                <label for="ph-contact">The station’s own contact</label>
                <input id="ph-contact" type="text" bind:value={contact} maxlength={CONTACT_MAX} autocomplete="off" autocapitalize="none" spellcheck="false" placeholder="calls@station.example" aria-invalid={contactError ? true : undefined} aria-describedby={contactError ? 'ph-contact-err ph-contact-hint' : 'ph-contact-hint'} />
                <span id="ph-contact-hint" class="hint"
                  >An email address or a link where a caller can ask for their call to be taken down. The phone words name this one; a show’s own contact is for its callers from the player page.
                  {contactView.contact.source === 'settings' ? 'It comes from the station’s settings file now.' : contactView.contact.settings_value ? `Clear it, and the settings file’s (${contactView.contact.settings_value}) is back in force.` : ''}</span
                >
                {#if contactError}<p id="ph-contact-err" class="err">{contactError}</p>{/if}
              </div>
              <div class="acts">
                <button type="submit" class="btn primary" disabled={!contactDirty || savingContact} aria-busy={savingContact || undefined}>{savingContact ? 'Saving…' : 'Save the contact'}</button>
                <span class="hint note" role="status" aria-live="polite">{contactDirty ? 'Not saved yet.' : contactNote}</span>
              </div>
            </form>
          </section>
        {/if}

        <section class="part wide" aria-labelledby="ph-bans-h">
          <h3 id="ph-bans-h">Banned numbers</h3>
          <p class="hint">A ban is kept as a number’s last four digits and a key made from the number, never the number. Caller ID can be faked, so a ban is a help, not a lock. A caller who withholds their number cannot be banned by number.</p>
          {#if bans.length}
            <ul class="bans" data-testid="phone-bans">
              {#each bans as b (b.ban_key)}
                <li>
                  <span class="num tail">···{b.tail}</span>
                  <span class="why">{b.reason}</span>
                  <span class="when">banned {b.date} · {banUntil(b)}</span>
                  <button type="button" class="btn sm quiet" onclick={() => lift(b)}>Lift<span class="sr-only"> the ban on ···{b.tail}</span></button>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="none" data-testid="phone-bans-none">No number is banned. A screener bans a caller from their card; an admin can ban a number here.</p>
          {/if}
          <p class="hint note" role="status" aria-live="polite">{banNote}</p>
          {#if lineOn}
            <div class="two">
              <form onsubmit={ban} novalidate aria-labelledby="ph-ban-h">
                <h4 id="ph-ban-h">Ban a number</h4>
                <div class="field">
                  <label for="ph-number">The number</label>
                  <input id="ph-number" type="tel" bind:value={number} autocomplete="off" placeholder="0131 496 0123" aria-invalid={banErrors.number ? true : undefined} aria-describedby="ph-number-hint" />
                  <span id="ph-number-hint" class="hint">As the caller or the provider gave it. It is used to make the ban and then forgotten.</span>
                  {#if banErrors.number}<p class="err">{banErrors.number}</p>{/if}
                </div>
                <div class="field">
                  <label for="ph-reason">Why</label>
                  <input id="ph-reason" type="text" bind:value={reason} maxlength="200" autocomplete="off" aria-invalid={banErrors.reason ? true : undefined} />
                  {#if banErrors.reason}<p class="err">{banErrors.reason}</p>{/if}
                </div>
                <div class="field">
                  <label for="ph-days">For how long</label>
                  <select id="ph-days" value={days ?? 0} onchange={(e) => (days = Number(e.currentTarget.value) || null)}>
                    {#each BAN_LENGTHS as l (l.label)}<option value={l.days ?? 0}>{l.label}</option>{/each}
                  </select>
                </div>
                {#if banErrors.form}<p class="err" role="alert">{banErrors.form}</p>{/if}
                <div class="acts"><button type="submit" class="btn" disabled={banning} aria-busy={banning || undefined}>{banning ? 'Banning…' : 'Ban this number'}</button></div>
              </form>
              <form onsubmit={find} novalidate aria-labelledby="ph-find-h">
                <h4 id="ph-find-h">Find a number’s calls</h4>
                <div class="field">
                  <label for="ph-find">The number the caller gives you</label>
                  <input id="ph-find" type="tel" bind:value={findNumber} autocomplete="off" placeholder="0131 496 0123" aria-invalid={findError ? true : undefined} aria-describedby="ph-find-hint" />
                  <span id="ph-find-hint" class="hint">For a caller who asks for their call to be taken down. The number is used for this search only. A call from a withheld number cannot be found this way: find it by its date, time and show.</span>
                  {#if findError}<p class="err" role="alert">{findError}</p>{/if}
                </div>
                <div class="acts"><button type="submit" class="btn" disabled={finding}>{finding && !confirmErase ? 'Looking…' : 'Find their calls'}</button></div>
                {#if found}
                  <div class="found" role="status" data-testid="phone-found">
                    {#if found.length === 0}
                      <p><b>No calls found for that number.</b> Check it with the caller, or find the call by its date, time and show.</p>
                    {:else if !confirmErase}
                      <p><b>{found.length} {found.length === 1 ? 'call' : 'calls'} found.</b> Erasing removes the name and what was said about each for good. The date and the words agreed stay as the station’s record.</p>
                      <button type="button" class="btn danger-hollow" onclick={() => (confirmErase = true)}>Erase {found.length === 1 ? 'this call' : `these ${found.length} calls`}…</button>
                    {:else}
                      <p><b>Erase {found.length === 1 ? 'this call' : `these ${found.length} calls`}?</b> This cannot be undone.</p>
                      <div class="acts">
                        <button type="button" class="btn danger-hollow" disabled={finding} onclick={eraseFound}>{finding ? 'Erasing…' : `Erase ${found.length === 1 ? 'the call' : `${found.length} calls`}`}</button>
                        <button type="button" class="btn quiet" disabled={finding} onclick={() => (confirmErase = false)}>Keep {found.length === 1 ? 'it' : 'them'}</button>
                      </div>
                    {/if}
                  </div>
                {/if}
                {#if erased}<p class="hint note" role="status">{erased}</p>{/if}
              </form>
            </div>
          {/if}
        </section>
      </div>
    {/if}
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
  .part.wide {
    grid-column: 1 / -1;
  }
  h3 {
    font-size: var(--fs-16);
  }
  h4 {
    font-size: var(--fs-14);
    font-weight: 600;
  }
  .hint {
    font-size: var(--fs-13);
    color: var(--muted);
    line-height: var(--lh-body);
  }
  .hint.note:empty {
    display: none;
  }
  .hint.note {
    color: var(--ink-2);
  }
  .err {
    font-size: var(--fs-13);
    color: var(--status-serious);
    line-height: var(--lh-label);
  }
  .switch-row {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    flex-wrap: wrap;
  }
  .switch {
    appearance: none;
    font: inherit;
    color: var(--ink);
    background: none;
    border: 0;
    padding: 0;
    display: inline-flex;
    align-items: center;
    gap: var(--s-2);
    cursor: help;
    min-height: 1.75rem;
  }
  .switch .knob {
    position: relative;
    width: 2.25rem;
    height: 1.25rem;
    border-radius: 999px;
    border: 1px dashed var(--muted);
    background: var(--surface-2);
    flex: none;
  }
  .switch .knob::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: calc(1.25rem - 6px);
    height: calc(1.25rem - 6px);
    border-radius: 50%;
    background: var(--muted);
  }
  .switch[aria-checked='true'] .knob::after {
    left: auto;
    right: 2px;
    background: var(--ink);
  }
  .sw-label {
    font-weight: 600;
    font-size: var(--fs-14);
  }
  .sw-state {
    font-size: var(--fs-13);
    color: var(--ink-2);
    border: 1px solid var(--hairline);
    border-radius: var(--r-1);
    padding: 0 var(--s-2);
  }
  /* The reason a fixed switch cannot be pressed: ink, a lock and the warning colour's edge, so
     it reads as a statement about the switch and not as one more quiet hint. It wraps under the
     switch in a narrow column. */
  .sw-lock {
    display: inline-flex;
    align-items: center;
    gap: var(--s-2);
    font-size: var(--fs-13);
    font-weight: 600;
    color: var(--ink);
    line-height: var(--lh-label);
    border: 1px solid var(--axis);
    border-left: 3px solid var(--status-warning);
    border-radius: var(--r-1);
    background: var(--surface-2);
    padding: var(--s-1) var(--s-2);
  }
  .sw-lock .g {
    width: 14px;
    height: 14px;
    flex: none;
    color: var(--ink-2);
  }
  .hint.why {
    color: var(--ink-2);
  }
  .info {
    appearance: none;
    font: inherit;
    font-size: var(--fs-12);
    font-weight: 700;
    width: 1.5rem;
    height: 1.5rem;
    border-radius: 50%;
    border: 1px solid var(--muted);
    background: var(--surface);
    color: var(--ink-2);
    cursor: help;
  }
  .said {
    margin: 0;
    display: grid;
    gap: 2px;
  }
  .said figcaption {
    font-size: var(--fs-12);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--ink-2);
    display: flex;
    align-items: center;
    gap: var(--s-2);
  }
  .said blockquote {
    margin: 0;
    border-left: 3px solid var(--accent);
    background: var(--surface-2);
    border-radius: var(--r-1);
    padding: var(--s-2) var(--s-3);
    font-size: var(--fs-14);
    line-height: var(--lh-body);
    overflow-wrap: anywhere;
  }
  .more summary {
    cursor: pointer;
    font-size: var(--fs-13);
    color: var(--ink-2);
  }
  .more p {
    font-size: var(--fs-13);
    color: var(--ink-2);
    line-height: var(--lh-body);
    margin-top: var(--s-1);
    max-width: 44rem;
  }
  .voices {
    display: grid;
    gap: var(--s-2);
  }
  .voice {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--s-2) var(--s-3);
    align-items: center;
    border: 1px solid var(--hairline);
    border-radius: var(--r-1);
    padding: var(--s-2) var(--s-3);
  }
  .voice.on {
    border-color: var(--accent);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .voice .pick {
    display: flex;
    gap: var(--s-2);
    align-items: center;
    font-size: var(--fs-14);
    min-height: 1.75rem;
  }
  .voice .who {
    color: var(--ink-2);
    font-size: var(--fs-13);
  }
  .voice .btn svg {
    width: 10px;
    height: 10px;
    fill: currentColor;
    margin-right: 4px;
  }
  .voice.own .own-body {
    grid-column: 1 / -1;
    display: grid;
    gap: var(--s-2);
  }
  .switch.fixed {
    cursor: help;
  }
  .switch:not(.fixed) {
    cursor: pointer;
  }
  .switch:not(.fixed) .knob {
    border-style: solid;
  }
  .switch:not(.fixed)[aria-checked='true'] .knob {
    background: var(--accent);
    border-color: var(--accent);
  }
  .switch:not(.fixed)[aria-checked='true'] .knob::after {
    background: var(--surface);
  }
  .slot {
    border: 1px solid var(--hairline);
    border-radius: var(--r-1);
    background: var(--surface-2);
    padding: var(--s-2) var(--s-3);
    font-size: var(--fs-14);
    line-height: var(--lh-body);
    display: grid;
    gap: 2px;
  }
  .slot.empty {
    border-style: dashed;
    color: var(--ink-2);
    display: block;
  }
  .slot.empty b {
    color: var(--ink);
  }
  .slot.stale {
    border-left: 3px solid var(--status-warning);
  }
  .warn-line {
    display: flex;
    gap: var(--s-1);
    align-items: center;
  }
  .warn-line svg {
    width: 14px;
    height: 14px;
    flex: none;
  }
  .acts {
    display: flex;
    gap: var(--s-2);
    align-items: center;
    flex-wrap: wrap;
  }
  .btn.file {
    cursor: pointer;
  }
  .btn.file:focus-within {
    outline: 2px solid var(--focus, var(--accent));
    outline-offset: 2px;
  }
  .btn.file.disabled {
    opacity: 0.6;
    cursor: default;
  }
  .recording {
    display: inline-flex;
    align-items: center;
    gap: var(--s-1);
    font-size: var(--fs-14);
    font-weight: 600;
  }
  .recording .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--status-critical);
  }
  .field {
    display: grid;
    gap: var(--s-1);
    align-content: start;
  }
  .field > input {
    width: 100%;
    max-width: 26rem;
  }
  .field > select {
    justify-self: start;
  }
  .field > label {
    font-size: var(--fs-13);
    font-weight: 600;
  }
  form {
    display: grid;
    gap: var(--s-2);
    align-content: start;
    min-width: 0;
  }
  .two {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(22rem, 100%), 1fr));
    gap: var(--s-4) var(--s-5);
    align-items: start;
    border-top: 1px solid var(--hairline);
    padding-top: var(--s-3);
  }
  .bans {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--s-1);
    max-width: 60rem;
  }
  .bans li {
    display: grid;
    grid-template-columns: 5rem minmax(0, 1fr) auto auto;
    gap: var(--s-3);
    align-items: center;
    border: 1px solid var(--hairline);
    border-radius: var(--r-1);
    background: var(--surface-2);
    padding: var(--s-1) var(--s-2);
    font-size: var(--fs-14);
  }
  .tail {
    font-family: var(--font-mono);
    font-weight: 600;
  }
  .when,
  .none {
    color: var(--ink-2);
    font-size: var(--fs-13);
  }
  .found {
    border: 1px solid var(--hairline);
    border-radius: var(--r-1);
    padding: var(--s-2) var(--s-3);
    display: grid;
    gap: var(--s-2);
    justify-items: start;
    font-size: var(--fs-14);
    line-height: var(--lh-body);
  }
</style>
