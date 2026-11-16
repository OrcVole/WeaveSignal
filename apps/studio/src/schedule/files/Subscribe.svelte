<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Subscribe… (`calendar.subscribe`; ADR-0281): the link to the schedule's calendar file, or
  // to one show's, to give to listeners and presenters; Copy; and Download .ics. A dialog over
  // the Schedule tab, opened from the toolbar (the schedule) and from the show editor's When it
  // airs (that show).
  //
  // The link is made from the station's public address, sent with the week, and from nothing
  // else: on Cloudron the address this window talks to is behind the login, and a calendar app
  // sent there would be shown a login page. With no public address there is no link, and the
  // dialog says who sets one. The file itself can always be downloaded: it is read through the
  // studio's own sign-in.
  //
  // The dialog owns the keyboard (keymap rule 5). Esc closes it, and the browser puts the focus
  // back on the button that opened it.
  import { edges } from '../rule/edges.ts';
  import { calendarLinks, fileName } from './links.ts';
  import { calendarText, saveCalendar } from './station.ts';

  let {
    show = null,
    publicUrl,
    onClose,
  }: {
    /** One show's calendar; without it, the whole schedule's. */
    show?: { slug: string; name: string; hasDates: boolean } | null;
    /** The station's public address; null when it has none. */
    publicUrl: string | null | undefined;
    onClose: () => void;
  } = $props();

  const links = $derived(calendarLinks(publicUrl, show?.slug));
  const file = $derived(fileName(show?.slug));

  let dialogEl = $state<HTMLDialogElement | null>(null);
  /** What the last press did, announced; `bad` when it failed. */
  let said = $state<{ words: string; bad: boolean } | null>(null);
  let saving = $state(false);

  $effect(() => {
    const d = dialogEl;
    if (d && !d.open) {
      d.showModal();
      queueMicrotask(() => document.getElementById(links ? 'sub-copy' : 'sub-download')?.focus());
    }
  });

  /** Copy a link. A webview may refuse the clipboard to a page: the link is then selected in
   *  its box and copied the old way, and failing that left selected for the person's own ⌃C. */
  async function copy(id: 'sub-webcal' | 'sub-web', what: string) {
    const box = document.getElementById(id) as HTMLInputElement | null;
    const text = box?.value ?? '';
    try {
      await navigator.clipboard.writeText(text);
      said = { words: `Copied the ${what}.`, bad: false };
      return;
    } catch {
      /* the old way next */
    }
    box?.focus();
    box?.select();
    let done = false;
    try {
      done = document.execCommand('copy');
    } catch {
      done = false;
    }
    said = done ? { words: `Copied the ${what}.`, bad: false } : { words: 'It could not be copied from here. The link is selected in its box: copy it from there.', bad: true };
  }

  async function download() {
    if (saving) return;
    saving = true;
    said = null;
    try {
      const where = await saveCalendar(show?.slug, await calendarText(show?.slug));
      said = { words: where ? `Saved as ${where}` : `${file} is with your browser’s downloads.`, bad: false };
    } catch (e) {
      said = { words: e instanceof Error ? e.message : 'The file was not saved. Try again.', bad: true };
    } finally {
      saving = false;
    }
  }
</script>

<dialog bind:this={dialogEl} class="wz mid files subscribe" data-panel="calendar.subscribe" aria-labelledby="sub-h" aria-describedby="sub-lede" onclose={onClose}>
  <div class="head">
    <h2 id="sub-h" data-panel-title>{show ? `Subscribe to ${show.name}` : 'Subscribe to the schedule'}</h2>
    <p id="sub-lede" class="small">
      {#if show}A calendar app that subscribes to this link shows this show’s dates, and follows them when they change.
      {:else}A calendar app that subscribes to this link shows every show’s dates, and follows the schedule when it changes.{/if}
    </p>
  </div>

  <!-- The middle scrolls when the window is short or the text large; the foot, with Download
       .ics and Done, stays in the window. -->
  <div class="scroll" use:edges>
  {#if links}
    <div class="field">
      <label for="sub-webcal">Calendar link</label>
      <div class="link">
        <input id="sub-webcal" type="text" readonly value={links.webcal} spellcheck="false" aria-describedby="sub-webcal-hint" onfocus={(e) => e.currentTarget.select()} />
        <button id="sub-copy" type="button" class="btn primary" onclick={() => copy('sub-webcal', 'calendar link')}>Copy link</button>
      </div>
      <span id="sub-webcal-hint" class="hint">Give this one out. Apple’s Calendar and Outlook open it and offer to subscribe. Where a <span class="mono">webcal</span> link does nothing, as on many Android phones, use the web address.</span>
    </div>
    <div class="field">
      <label for="sub-web">Web address</label>
      <div class="link">
        <input id="sub-web" type="text" readonly value={links.web} spellcheck="false" aria-describedby="sub-web-hint" onfocus={(e) => e.currentTarget.select()} />
        <button type="button" class="btn" onclick={() => copy('sub-web', 'web address')}>Copy</button>
      </div>
      <span id="sub-web-hint" class="hint">The same calendar, to paste where an app asks for a calendar’s address, such as Google Calendar’s <i>From URL</i>.</span>
    </div>
  {:else}
    <div class="warn plain" data-testid="sub-no-address">
      <div class="top"><svg aria-hidden="true"><use href="#g-degraded" /></svg><span>This station has no public address, so there is no link to give out.</span></div>
      <span>A link has to be one that listeners can reach. An admin sets the station’s public address in its settings file (<span class="mono">publishing.public_url</span>) and restarts the station; the link is then shown here.</span>
      <span>The file can still be downloaded.</span>
    </div>
  {/if}

  <div class="carries">
    <p>It carries {show ? 'the show’s' : 'each show’s'} name, description and dates. Nothing drafted is in it: no episode titles and no guests.</p>
    {#if show && !show.hasDates}<p data-testid="sub-empty"><b class="ink">{show.name} has no dates yet</b>, so its calendar is empty until it has.</p>{/if}

    <p id="sub-dl-hint">Or download the file: a copy of {show ? 'its dates' : 'the schedule'} as {show ? 'they are' : 'it is'} now, to open in a calendar once. A copy does not follow changes.</p>
  </div>
  </div>

  <div class="foot2">
    <p class="said" class:bad={said?.bad} role={said?.bad ? 'alert' : 'status'} aria-live={said?.bad ? undefined : 'polite'} data-testid="sub-said">{said?.words ?? ''}</p>
    <div class="acts">
      <button id="sub-download" type="button" class="btn left" disabled={saving} aria-busy={saving || undefined} aria-describedby="sub-dl-hint" onclick={download}>{saving ? 'Fetching the file…' : 'Download .ics'}</button>
      <button type="button" class="btn quiet" onclick={() => dialogEl?.close()}>Done <kbd>Esc</kbd></button>
    </div>
  </div>
</dialog>
