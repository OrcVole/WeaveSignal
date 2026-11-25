<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The studio app's frame: the show's step and Show everything (ADR-0271), the tabs with their
  // keys (⌃1 to ⌃5; a tab keeps its number at every step), the link to the station and the
  // theme toggle (Dark, Light or Night; dark by default, never the system's; ADR-0065,
  // ADR-0270), over the surface of the tab. One keyboard handler for every surface, from the
  // keymap (ADR-0063); the key of a part the step hides does nothing, and says so once.
  // The Keys tab (⌃6; keyed shows, 1.3.2) is a producer's and an admin's: it is drawn for them
  // alone, at every step, and never in the sandbox, which has no station to keep keys.
  //
  // The frame's spare room, between the tabs and who is signed in, says the studio app's version
  // (the operator, 2026-11-17), and the station's beside it when the two differ (lib/version.ts).
  // Its ink says where the app stands (1.3.2): green with a tick while it is up to date, red with
  // an arrow while it is behind; the word is in the label and the sentence in the tooltip. The
  // glyph stands before the v, not in its place (`✓ v1.3.2`; the operator, 2026-11-25), and the
  // v is never given up for room: a longer version (`1.3.10`) gives up the gap after the glyph.
  // It is words in the frame's own quiet ink, not a control, and it takes only room nobody else
  // wants: the name goes first, then the number, before it would push anything to a second row.
  //
  // A staff window is Live or Preparing (ADR-0276): the control beside who is signed in, and,
  // while preparing, a band under the frame on the tabs that drive the air (Desk, Screener,
  // Carts). A preparing window's keys change nothing on air; the band answers instead.
  import { setupState, stationZone } from './lib/schedule.ts';
  import { dialogLets, escTaken, settingsEscape } from './lib/dialogKeys.ts';
  import { onMount, tick } from 'svelte';
  import Glyphs from './lib/Glyphs.svelte';
  import Mark from './lib/Mark.svelte';
  import DeskB from './desk/DeskB.svelte';
  import RehearsalDesk from './desk/RehearsalDesk.svelte';
  import { startRehearsal } from './lib/rehearsal.svelte.ts';
  import { routeKey } from './lib/rehearsalDesk.ts';
  import { onBeforeStudioSwitch, releaseRehearsalTalk, runRehearsal, settleMoved, tellLiveKey } from './lib/rehearsalDesk.svelte.ts';
  import Screener from './screener/Screener.svelte';
  import CartWall from './carts/CartWall.svelte';
  import Settings from './settings/Settings.svelte';
  import StationSetup from './lib/StationSetup.svelte';
  import Stations from './lib/Stations.svelte';
  import Account from './lib/Account.svelte';
  import StepControl from './lib/StepControl.svelte';
  import PrepareControl from './lib/PrepareControl.svelte';
  import SessionEnded from './lib/SessionEnded.svelte';
  import CloseAsk from './lib/CloseAsk.svelte';
  import ShowWizard from './settings/ShowWizard.svelte';
  import Schedule from './schedule/Schedule.svelte';
  import Keys from './keys/Keys.svelte';
  import { mayManageKeys } from './lib/keys.ts';
  import Tip from '../../../packages/ui/Tip.svelte';
  import { APP_VERSION } from './appVersion.ts';
  import { STANDING_WORD, rereadNow, versionLabel } from './lib/version.ts';
  import { base, follow, noteStation, packaged, sandboxed, savedStation, stationVersion } from './lib/api.ts';
  import { onLeave } from './lib/reset.ts';
  import { hereRow, onBeforeLeave, settleArrival, stations } from './lib/stations.svelte.ts';
  import { isAdmin } from './lib/account.ts';
  import { account, installSessionWatch, loadMe } from './lib/account.svelte.ts';
  import { closeSetup, decideSetup, loadShows, mine, rememberSkip, setup } from './lib/wizard.svelte.ts';
  import { cartShow, nudge, onAir, onBeforePreparing, prepare, preparing, settleStart, switchToLive } from './lib/prepare.svelte.ts';
  import { canPrepare, changesAir, prepareBand } from './lib/prepare.ts';
  import { installUpdateCheck, updateNag } from './lib/update.svelte.ts';
  import { run, tellOff } from './lib/actions.ts';
  import { addInstead, cartEditor } from './lib/cartEdit.svelte.ts';
  import { checkPackNotice, dismissPackNotice, packNotice } from './lib/packNotice.svelte.ts';
  import { PACK_NOTICE } from './lib/packNotice.ts';
  import { look, say, setCartShow, setShowAll, settleBank, studio, takeView } from './lib/studio.svelte.ts';
  import { STEP_WORD, gate, offTag, stationStep, tabShown } from './lib/steps.ts';
  import { chat } from './lib/chat.svelte.ts';
  import { theme, setTheme, THEMES, THEME_WORDS } from './lib/theme.svelte.ts';
  import { zoom, zoomIn, zoomOut, zoomReset, installZoom } from './lib/zoom.svelte.ts';
  import { installPanelNames } from './lib/panelNames.ts';
  import { PANELS } from '../../../packages/ui/panels.ts';
  import { ARM_MS, KeyEngine, type Tab } from '../../../packages/ui/keymap.ts';

  // A tab keeps its number at every step: ⌃1 Desk, ⌃3 Carts, ⌃5 Schedule. ⌃4 has no tab: it
  // opens the Room lane of the Screener tab (a Chat tab may take it back later).
  const TABS: { tab: Tab; n: number; label: string; built: boolean }[] = [
    { tab: 'desk', n: 1, label: 'Desk', built: true },
    { tab: 'screener', n: 2, label: 'Screener', built: true },
    { tab: 'carts', n: 3, label: 'Carts', built: true },
    { tab: 'schedule', n: 5, label: 'Schedule', built: true },
    { tab: 'keys', n: 6, label: 'Keys', built: true },
  ];
  /** Whether this window has the Keys tab: a producer's and an admin's, on a real station. */
  const keysTab = $derived(!sandboxed() && mayManageKeys(account.me));
  /** The station's step: what is switched on. */
  const step = $derived(stationStep(studio.view));
  /** The tabs this window draws: the step's, or all of them while everything is shown. */
  const tabs = $derived(TABS.filter((t) => tabShown(t.tab, look()) && (t.tab !== 'keys' || keysTab)));

  const keys = new KeyEngine();

  function inTextField(el: Element | null): boolean {
    if (!el) return false;
    if (el instanceof HTMLTextAreaElement) return true;
    if (el instanceof HTMLInputElement) {
      return !['button', 'checkbox', 'radio', 'submit', 'reset', 'range'].includes(el.type);
    }
    return (el as HTMLElement).isContentEditable;
  }

  function cartFocused(el: Element | null): boolean {
    return !!el?.closest('[data-cart-slot]') || el === document.body || el === null;
  }

  /** The focus is on a cart's Edit button. */
  function cartEditFocused(el: Element | null): boolean {
    return !!el?.closest('[data-cart-edit]');
  }

  const CONTROL = 'button, a[href], input, select, textarea, summary, [role=option], [role=button]';

  /** ⏎ releases a held message only from nowhere in particular or from Needs you itself; on a
   *  control (Send to desk, a caller in the queue) it stays that control's own ⏎. */
  function releaseFrom(el: Element | null): boolean {
    if (!el || el === document.body) return true;
    return !!el.closest('[data-panel="screener.needs"]') && !el.matches(CONTROL);
  }

  function input(e: KeyboardEvent) {
    return { code: e.code, ctrl: e.ctrlKey, shift: e.shiftKey, alt: e.altKey, meta: e.metaKey, repeat: e.repeat };
  }

  function setupNotice() {
    const where = `${base() || location.origin}/setup/`;
    void navigator.clipboard?.writeText(where).catch(() => {});
    const missing = studio.setupPending?.length ? ` Still to set: ${studio.setupPending.join(', ')}.` : '';
    say('warning', 'The station’s setup is not finished', `Open ${where} in your browser and sign in as an admin (the address is copied).${missing}`, 15000);
  }

  function onKeydown(e: KeyboardEvent) {
    // Add a station lies over the studio (ADR-0285): nothing typed into a sign-in form reaches
    // the desk underneath, not even a chord.
    if (stations.adding) return;
    // An Esc a dialog or a menu has already taken is theirs alone.
    if (escTaken(e)) return;
    // A modal dialog owns the keyboard: only ⌃T reaches the desk while one is open (U1).
    if (document.querySelector('dialog[open]') && !dialogLets(input(e))) return;
    // Esc on Settings leaves it for the tab it came from (a field gives up its focus first,
    // keymap rule 3); the rest of the page's keys are its own, below.
    if (studio.settings) {
      const esc = settingsEscape(e, inTextField(document.activeElement));
      if (esc) {
        e.preventDefault();
        if (esc === 'leave-field') (document.activeElement as HTMLElement | null)?.blur();
        else studio.settings = false;
        return;
      }
    }
    // Settings is a form: only the global chords (⌃) reach the keymap, and as from a text field,
    // so no bare letter, Space or F-key acts on the desk, and Esc stays the dialog's and the
    // drop-down's own.
    if (studio.settings && !(e.ctrlKey || e.metaKey)) return;
    // The account, session, show-setup and invite dialogs are forms too: their keys are their own.
    if ((e.target as Element | null)?.closest?.('[data-panel="studio.account"], [data-panel="studio.session-ended"], [data-panel="studio.show-setup"], [data-panel="desk.invite"], [data-panel="desk.end-show"]') && !(e.ctrlKey || e.metaKey)) return;
    // A cart's Edit button is a control of its own (1.2.1): Space and ⏎ press it, and neither
    // stops the last cart nor fires one. Every other key is the wall's as ever.
    if (cartEditFocused(document.activeElement) && (e.code === 'Space' || e.code === 'Enter' || e.code === 'NumpadEnter') && !(e.ctrlKey || e.metaKey || e.altKey)) return;
    const ctx = {
      tab: studio.tab,
      inTextField: studio.settings || inTextField(document.activeElement),
      dialog: studio.tab === 'screener' && chat.sor ? ('statement' as const) : null,
    };
    const out = keys.down(input(e), ctx, Date.now());
    if (!out.handled) return;
    // The Desk tab shows the rehearsal desk (1.3): its strips' keys and F1 to F6 go to the
    // rehearsal desk by its own route, and every other key that changes the air does nothing,
    // not even arm. Decided before the step's gate and the preparing window's rule, which are
    // about the live desk: a preparing window drives a rehearsal, at the rehearsed show's step.
    if (out.action && !studio.settings) {
      const route = routeKey(out.action, studio.tab, studio.deskStudio);
      if (route !== 'live') {
        e.preventDefault();
        keys.disarm();
        studio.armed = null;
        if (route === 'rehearsal') void runRehearsal(out.action);
        else tellLiveKey();
        return;
      }
    }
    // A part the show's step keeps off: its key does nothing (not even arm), and the first
    // press in a session says so. Still the keymap's key, so the browser does not take it
    // (⌃D would bookmark, ⌃T open a tab). Checked before anything else acts on the key, so
    // ⌃2 at Solo does not close Settings.
    if (out.action) {
      // ⌃6 where the Keys tab is not drawn (a presenter's or a screener's window, the sandbox):
      // the key does nothing, and says why. Still the keymap's key, so the browser does not take it.
      if (out.action.type === 'tab' && out.action.tab === 'keys' && !keysTab) {
        e.preventDefault();
        if (!sandboxed()) say('warning', 'Keys are a producer’s or an admin’s', 'Listener keys hold names and the means to listen, so this account has no Keys tab.', 6000);
        return;
      }
      const g = gate(out.action, step, look(), studio.view);
      if (!g.allow) {
        e.preventDefault();
        keys.disarm();
        studio.armed = null;
        tellOff(g.told);
        return;
      }
      // An empty slot's key adds a cart there (keymap section 4) while nothing is on air, and in
      // a preparing window, whose dialog changes nothing on air; never from a live window on
      // air, where a stray key must not cover the wall. ⏎ only from a cart's own pad.
      if (addInstead(out.action, cartFocused(document.activeElement))) {
        e.preventDefault();
        keys.disarm();
        studio.armed = null;
        return;
      }
      // A preparing window's keys change nothing on air (ADR-0276): the key does nothing, not
      // even arm, and the band under the frame answers. Still the keymap's key, so the browser
      // does not take it.
      if (preparing() && changesAir(out.action)) {
        e.preventDefault();
        keys.disarm();
        studio.armed = null;
        nudge();
        return;
      }
    }
    // A tab's key (⌃1 to ⌃5) leaves Settings for that tab.
    if (out.action?.type === 'tab') studio.settings = false;
    // ⏎ fires the cart the arrows reached only while a cart has focus; on any other control
    // (a bank, Fade all) it is that control's own ⏎, never a cart on air by surprise.
    if (out.action?.type === 'cart-enter' && !cartFocused(document.activeElement)) return;
    // The same for the screener's ⏎: release only away from controls, and in the statement
    // dialog a focused button (Keep the message) takes its own ⏎.
    if (out.action?.type === 'mod-release' && !releaseFrom(document.activeElement)) return;
    if (out.action?.type === 'sor-send' && document.activeElement instanceof HTMLButtonElement) return;
    // The keymap's key, not the browser's: ⌃D would bookmark, ⌃R reload, F1 open help.
    e.preventDefault();
    if (out.action) void run(out.action);
    // Any other key disarms (rule 4), whether the key or its button armed it.
    if (studio.armed && !keys.armedNow(Date.now()) && out.action?.type !== 'arm') studio.armed = null;
  }

  function onKeyup(e: KeyboardEvent) {
    const out = keys.up({ code: e.code });
    if (out.handled) e.preventDefault();
    if (!out.action) return;
    // A held Talk let go: it closes the talk it opened, at whichever desk that was. The
    // rehearsal desk keeps its own list of what is held, so letting go there is always safe,
    // whatever the tab shows by now.
    if (out.action.type === 'talk' && !out.action.held) void runRehearsal(out.action);
    if (routeKey(out.action, studio.tab, studio.deskStudio) === 'live') void run(out.action);
  }

  /** A held key whose key-up went to another window would stay open: let everything go. The
   *  promise is for the switch to Preparing, which waits until the releases have landed. */
  function letGo(): Promise<void> {
    const sent = keys.releaseAll().map((a) => run(a));
    studio.armed = null;
    return Promise.all([...sent, releaseRehearsalTalk()]).then(() => {});
  }

  // The start's default (ADR-0276) is settled once, when the station has said who this is, which
  // shows there are and what is on air: each of the three may be the last to arrive.
  $effect(() => {
    void [account.loaded, account.me, setup.loaded, studio.view?.step];
    settleStart();
  });

  // The show whose own cart banks this window works with follows the air and the window's mode
  // (a cart wall per show, 1.0.2): its catalogue is fetched when it changes.
  const windowCartShow = $derived(cartShow());
  $effect(() => {
    setCartShow(windowCartShow);
  });

  /** The band under the frame: on the tabs that drive the air, while this window is preparing. */
  // Not over the rehearsal desk: the band speaks of the live desk, and a preparing window drives
  // a rehearsal as a live one does.
  const atRehearsal = $derived(studio.tab === 'desk' && studio.deskStudio === 'rehearsal' && !studio.settings);
  // Nor over the Schedule and the Keys tabs, which change nothing on air.
  const prepBand = $derived(preparing() && !studio.settings && studio.tab !== 'schedule' && studio.tab !== 'keys' && !atRehearsal ? prepareBand(onAir()) : null);

  /** New sounds are available (1.3.1): asked once after sign-in, for an admin or a producer, and
   *  drawn on the Carts tab, and on the Desk only while nobody is on air (the band costs the desk
   *  33 px it needs then), where Default pack… is one press away, until dismissed. */
  const packBand = $derived(
    packNotice.shown && !studio.settings && !atRehearsal && (studio.tab === 'carts' || (studio.tab === 'desk' && !studio.view?.sources?.some((s) => s.on_air))),
  );
  $effect(() => {
    if (account.loaded && account.me && studio.view) void checkPackNotice();
  });
  let notNowEl = $state<HTMLButtonElement | null>(null);
  /** Not now: the notice goes, and the focus goes to the selected tab, since the button that held it is gone. */
  async function notNow() {
    dismissPackNotice();
    await tick();
    document.querySelector<HTMLElement>('nav.tabs [aria-current="page"]')?.focus();
  }
  /** Default pack…: the notice stays until the load has finished or Not now is pressed. */
  async function openPack() {
    studio.settings = false;
    await run({ type: 'tab', tab: 'carts' });
    cartEditor.pack = studio.bank;
  }

  /** A key or a button met a control the window has switched off: the band answers for a moment. */
  const nudged = $derived(studio.now - prepare.nudgedAt < 1200);
  /** One row for the frame, two for the sandbox's guide, one for each band, then the surface. */
  const rows = $derived(`${'auto '.repeat(1 + (sandboxed() ? 2 : 0) + (studio.showAll ? 1 : 0) + (prepBand ? 1 : 0) + (packBand ? 1 : 0))}minmax(0, 1fr)`);

  /** The frame's version: the app's own, the station's beside it when they differ, and where the
   *  app stands (up to date, or behind the station or the update feed). */
  const ver = $derived(versionLabel(APP_VERSION, studio.stationVersion, updateNag()));
  /** The glyph that goes with the ink; none while the standing is not known or the app is ahead. */
  const verGlyph = $derived(ver.standing === 'current' ? 'g-check' : ver.standing === 'behind' ? 'g-behind' : null);
  /** How long the app's version is written, 5 (`1.3.2`) to 7 (`1.10.10`) characters: the room the
   *  narrowest form asks for goes by it (the styles below), since a width cannot be asked of the words. */
  const verLen = $derived(Math.min(7, Math.max(5, ver.app.length)));
  let verEl = $state<HTMLElement | null>(null);

  // The station's version is read once at the start (in the packaged app with the arrival below,
  // which needs it too; in a browser when the page opens), and again when the link comes back
  // after a while, which is when a station that was updated says so. An answer that cannot be
  // read leaves what was known.
  let versionReadAt = 0;
  function readVersion(): Promise<string | null> {
    versionReadAt = Date.now();
    return stationVersion().then((v) => {
      if (v !== null) studio.stationVersion = v;
      return v;
    });
  }
  $effect(() => {
    if (studio.link === 'live' && rereadNow(versionReadAt, Date.now())) void readVersion();
  });

  // Once the station has said who the window is (ADR-0285): the list is told what the window
  // sees here, and after a switch the toast slot says where the window is. Once a page.
  let arrived = false;
  $effect(() => {
    if (arrived || !account.loaded || !packaged() || !savedStation()) return;
    arrived = true;
    void readVersion().then(settleArrival);
  });
  // The station's own name, when the view brings one the list does not have yet.
  $effect(() => {
    const [name, row] = [studio.view?.station, hereRow()];
    if (!arrived || !name || !row || row.name === name) return;
    void noteStation(row.id, { name })
      .then((l) => (stations.list = l))
      .catch(() => {});
  });

  onMount(() => {
    document.documentElement.dataset.surface = studio.tab;
    // The cart bank is kept for each station apart, and the station is known by now.
    settleBank();
    // Who this window runs as, and the 401 watch, before the first request goes out.
    installSessionWatch();
    // Then, for a presenter signed in as themselves with no show of their own, the setup wizard (ADR-0218).
    void loadMe()
      .then(decideSetup)
      // The station's setup page not yet finished: say so to an admin (ADR-0267).
      .then(() => (isAdmin(account.me) ? setupState() : null))
      .then((s) => (studio.setupPending = s && s.status === 'pending' ? s.missing : null))
      .catch(() => {});
    // The station's version for the frame; the packaged app reads it with its arrival, and the
    // sandbox has no station to ask.
    if (!packaged() && !sandboxed()) void readVersion();
    // The station's time zone, so the rundown's times are the station's (issue 10).
    void stationZone()
      .then((z) => (studio.zone = z.zone))
      .catch(() => {});
    const stop = follow(takeView, (s, why) => {
      studio.link = s;
      studio.linkWhy = why ?? '';
    });
    // The rehearsal studio (1.3): read once, then followed. A station older than 1.3.0 has
    // none, and the Desk tab then offers no choice of studio. The sandbox has no station.
    const stopRehearsal = sandboxed() ? () => {} : startRehearsal();
    // The feature's panels are merged until packages/ui/panels.ts lists them (ADR-0070).
    const names = installPanelNames(document, PANELS);
    const stopZoom = installZoom();
    // One look at the update feed a little after the app opens (packaged only); it installs nothing.
    const stopUpdateCheck = installUpdateCheck();
    onBeforePreparing(letGo);
    // The Desk tab's studio (1.3): every held key is let go at the desk it was opened on before
    // the tab shows the other.
    onBeforeStudioSwitch(letGo);
    // Leaving the station (ADR-0285): every held key is let go first, then the event stream is
    // closed, before any store is emptied or the Rust side is told.
    onBeforeLeave(letGo);
    const offLeave = onLeave('studio events', stop);
    const tick = setInterval(() => {
      studio.now = Date.now();
      if (studio.armed && studio.now - studio.armed.at > ARM_MS) studio.armed = null;
      if (studio.note && studio.note.until < studio.now) studio.note = null;
      if (studio.offNote && studio.offNote.until < studio.now) studio.offNote = null;
      settleMoved(studio.now);
    }, 100);
    const hidden = () => {
      if (document.hidden) void letGo();
    };
    document.addEventListener('visibilitychange', hidden);
    return () => {
      stop();
      stopRehearsal();
      names.destroy();
      stopZoom();
      stopUpdateCheck();
      onBeforePreparing(null);
      onBeforeStudioSwitch(null);
      onBeforeLeave(null);
      offLeave();
      clearInterval(tick);
      document.removeEventListener('visibilitychange', hidden);
    };
  });

  const linkWord = $derived(studio.link === 'connecting' ? 'connecting' : 'not connected');
</script>

<svelte:window onkeydown={onKeydown} onkeyup={onKeyup} onblur={() => void letGo()} />

<Glyphs />
{#if packaged() && !savedStation()}
  <StationSetup />
{:else}
{#if stations.adding}
  <!-- Add a station (ADR-0285): the sign-in screen lies over the studio, which stays as it is
       underneath and takes no key or pointer meanwhile. -->
  <div class="setup-over"><StationSetup /></div>
{/if}
<div class="shell" class:with-guide={sandboxed()} class:with-all={studio.showAll} class:with-prepare={!!prepBand} style:grid-template-rows={rows} inert={stations.adding}>
  {#if import.meta.env.MODE === 'sandbox'}{#await import('./sandbox/Guide.svelte') then guide}<guide.default />{/await}{/if}
  <header class="frame" data-panel="studio.frame">
    {#if packaged()}
      <!-- The station's name is the button that lists the stations (ADR-0285). -->
      <Stations />
    {:else}
      <span class="brand" data-panel-title><Mark />{studio.view?.station || 'WeaveSignal'}</span>
    {/if}
    <StepControl />
    <nav class="tabs" aria-label="Studio">
      {#each tabs as t (t.tab)}
        <button
          type="button"
          class="tab"
          aria-current={!studio.settings && studio.tab === t.tab ? 'page' : undefined}
          aria-disabled={!t.built}
          onclick={() => {
            if (t.built) studio.settings = false;
            void run({ type: 'tab', tab: t.tab });
          }}
          >{t.label}{#if !t.built}<span class="sr-only"> (not in this build)</span>{/if} <kbd>⌃{t.n}</kbd></button
        >
      {/each}
      <button
        type="button"
        class="tab settings-tab"
        aria-current={studio.settings ? 'page' : undefined}
        onclick={() => (studio.settings = !studio.settings)}>Settings</button
      >
    </nav>
    <!-- The spare room says the app's version, and gives it up before anything wraps: the
         spacer is a container with no width of its own, and the words are drawn only as far
         as it has room (the styles below). Not a control: a word with its tooltip, as a state
         word has, so it takes a tab stop. -->
    <span class="spacer">
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <span bind:this={verEl} class="ver st-trigger" class:differs={!!ver.station} class:cue={!!verGlyph} data-standing={ver.standing} data-len={verLen} data-testid="app-version" aria-describedby="ver-tip" tabindex="0"
        >{#if verGlyph}<svg class="vg" aria-hidden="true"><use href="#{verGlyph}" /></svg>{/if}<span class="name">WeaveSignal&nbsp;</span><span class="v">v</span>{ver.app}{#if ver.station}<span class="stn">{' '}· station {ver.station}</span><span class="neq">{' '}≠ {ver.stationShort}</span>{/if}{#if STANDING_WORD[ver.standing]}<span class="sr-only">, {STANDING_WORD[ver.standing]}</span>{/if}</span
      >
    </span>
    <Tip id="ver-tip" anchor={verEl} delay={0}>
      {#each ver.tip as line (line)}
        <span>{line}</span>{' '}
      {/each}
    </Tip>
    {#if studio.setupPending}
      <!-- The setup page is the station's web page: the address to open in a browser (ADR-0267). -->
      <button type="button" class="update-nag" onclick={setupNotice} title={`Still to set: ${studio.setupPending.join(', ')}`}
        >Finish the station’s setup</button
      >
    {/if}
    {#if updateNag()}
      <!-- Points at Settings; the install itself waits for a yes there, and for a quiet desk. -->
      <button type="button" class="update-nag" onclick={() => (studio.settings = true)}
        >Update {updateNag()} available</button
      >
    {/if}
    <Account />
    <PrepareControl />
    <!-- While the link is good the frame says one word, which makes room for the step control
         at 1,440 px; the full words return whenever it is not, and stay in the tooltip. Beside
         the Live or Preparing control (ADR-0276) a frame at most 1,600 px wide keeps only the
         tick while the link is good: the word is still there for a screen reader. -->
    <span class="link" class:tight={canPrepare(account.me)} role="status" data-link={studio.link} title={studio.link === 'live' ? 'Control plane connected' : studio.linkWhy}>
      <svg class={studio.link === 'live' ? 'g-good' : 'g-warning'} aria-hidden="true"
        ><use href={studio.link === 'live' ? '#g-good' : '#g-degraded'} /></svg
      >{#if studio.link === 'live'}<b>Connected</b>{:else}Control plane <b>{linkWord}</b>{/if}
    </span>
    <div class="seg" role="group" aria-label="Text size">
      <button type="button" onclick={zoomOut} aria-label="Smaller text" title="Smaller text (Ctrl+−)">A−</button>
      <button type="button" onclick={zoomReset} aria-label="Reset text size" title={zoom.fit > 1 ? `Reset (Ctrl+0). The window adds ×${zoom.fit.toFixed(2)} on top.` : 'Reset (Ctrl+0)'}>{Math.round(zoom.level * 100)}%</button>
      <button type="button" onclick={zoomIn} aria-label="Larger text" title="Larger text (Ctrl++)">A+</button>
    </div>
    <div class="seg" role="group" aria-label="Theme">
      {#each THEMES as t (t)}
        <button type="button" aria-pressed={theme.current === t} onclick={() => setTheme(t)} title={t === 'night' ? 'Night: warm, with no blue' : undefined}
          >{THEME_WORDS[t]}</button
        >
      {/each}
    </div>
  </header>
  {#if studio.showAll}
    <!-- Show everything is a look (ADR-0274): the row says so for as long as it lasts, and holds the way back. -->
    <div class="allbar" data-panel="studio.step.banner">
      <button type="button" class="btn" onclick={() => setShowAll(false)}>Back to {STEP_WORD[step]}</button>
      <span class="say" role="status"
        ><b data-panel-title>Showing everything for now.</b>
        {studio.view?.step.show ? `${studio.view.step.show.name} still runs` : 'The station is still'} at {STEP_WORD[step]}, so the parts marked
        <span class="offtag">{offTag(step)}</span> stay switched off. To switch one on, move the show up a step.</span
      >
    </div>
  {/if}

  {#if prepBand}
    <!-- Preparing (ADR-0276): the band says what this window is and holds the way to Live. -->
    <div class="allbar prepbar" class:nudged data-panel="studio.prepare.banner">
      <button type="button" class="btn" class:primary={prepBand.primary} disabled={prepare.busy} aria-haspopup={prepBand.button.endsWith('…') ? 'dialog' : undefined} onclick={switchToLive}
        >{prepBand.button}</button
      >
      <span class="say" role="status"
        ><b data-panel-title>{prepBand.lead}</b>
        {prepBand.rest}{#if prepare.problem && !prepare.menu}
          <span class="err">{prepare.problem}</span>{/if}</span
      >
    </div>
  {/if}

  {#if packBand}
    <!-- New sounds are available (1.3.1): the station's default pack has sounds this station lacks. -->
    <div class="allbar packbar" data-panel="studio.pack.notice">
      <button type="button" class="btn" aria-haspopup="dialog" onclick={() => void openPack()}>{PACK_NOTICE.action}</button>
      <span class="say" role="status"><b data-panel-title>{PACK_NOTICE.lead(packNotice.count)}</b> {PACK_NOTICE.rest}</span>
      <button type="button" class="btn quiet" bind:this={notNowEl} aria-describedby="pack-dismiss-tip" onclick={notNow}>{PACK_NOTICE.dismiss}</button>
      <Tip id="pack-dismiss-tip" anchor={notNowEl}>{PACK_NOTICE.dismissWhy}</Tip>
    </div>
  {/if}

  {#if studio.settings}
    <Settings />
  {:else if studio.tab === 'desk'}
    {#if studio.deskStudio === 'rehearsal'}<RehearsalDesk />{:else}<DeskB />{/if}
  {:else if studio.tab === 'carts'}
    <CartWall />
  {:else if studio.tab === 'schedule'}
    <Schedule />
  {:else if studio.tab === 'keys'}
    <Keys />
  {:else}
    <Screener />
  {/if}
  <SessionEnded />
  <CloseAsk />
  {#if setup.open && account.me?.kind === 'staff'}
    <ShowWizard
      me={account.me}
      shows={setup.shows}
      initial={mine()}
      onDone={() => void loadShows()}
      onSkip={() => rememberSkip(account.me)}
      onClose={closeSetup}
    />
  {/if}
</div>
{/if}

<style>
  /* Pinned to the window rather than 100vh: under the text size (zoom on <html>) 100vh is the
     window's height times the zoom, and at 120 % the bottom fifth was cut off (the 1.0.1 test run).
     The tabs' narrow layouts are container queries on it, so they follow the text size too. */
  .shell {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    position: fixed;
    inset: 0;
    container-type: inline-size;
    /* The surfaces' wide layouts ask this container, not the window: a media query reads the
       window's own pixels, which the text size and the window factor (zoom on <html>) do not
       change, so a 4K window chose the 2,200 px layout and then drew it twice the size. */
    container-name: studio;
  }
  /* The rows above the surface are counted in the script and set inline: the frame, the
     sandbox's guide bar and its caption row (ADR-0212, ADR-0222), the row that says everything
     is shown (ADR-0274) and the row that says this window is preparing (ADR-0276). */
  /* As tall as the design pages' mock-up bar, so each surface keeps the height it was drawn in. */
  .frame {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    padding: var(--s-1) var(--s-3);
    min-height: 2.5rem;
    background: var(--surface-2);
    border-bottom: 1px solid var(--hairline);
    font-size: var(--fs-13);
    color: var(--ink-2);
    white-space: nowrap;
    /* When the frame is narrower than its controls (a large text size, or a narrow window) it takes
       a second row instead of cutting off the ones at its right end (found at 120-130 %). */
    flex-wrap: wrap;
    row-gap: var(--s-1);
  }
  /* Add a station lies over the whole studio; the dialogs' top layer is empty meanwhile. */
  .setup-over {
    position: fixed;
    inset: 0;
    z-index: 30;
    overflow: auto;
    background: var(--plane);
  }
  .brand {
    display: inline-flex;
    align-items: center;
    gap: var(--s-2);
    font-weight: 700;
    color: var(--ink);
  }
  .tabs {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-1);
    min-width: 0;
  }
  .tab {
    appearance: none;
    background: none;
    border: 0;
    border-bottom: 2px solid transparent;
    border-radius: var(--r-1);
    color: var(--ink-2);
    font-size: var(--fs-13);
    padding: 0 var(--s-3);
    min-height: var(--target);
    display: inline-flex;
    align-items: center;
    gap: var(--s-2);
    cursor: pointer;
  }
  .tab[aria-current='page'] {
    color: var(--ink);
    border-bottom-color: var(--accent);
    font-weight: 600;
  }
  /* A sixth tab (Keys, 1.3.2) took the room the app's version had at 1,440 px: in a frame that
     narrow the tabs stand a little closer, and the version keeps its place. */
  @container (max-width: 1600px) {
    .tab {
      padding: 0 var(--s-2);
    }
  }
  .settings-tab {
    margin-left: var(--s-2);
    border-left: 1px solid var(--hairline);
    border-radius: 0;
  }
  .tab[aria-disabled='true'] {
    color: var(--muted);
    cursor: not-allowed;
  }
  /* The spacer's width is what the rest of the frame leaves, never its own words' (inline-size
     containment), so the version cannot push a control to a second row. */
  .spacer {
    flex: 1;
    min-width: 0;
    container-type: inline-size;
    display: flex;
    justify-content: center;
  }
  /* The app's version: the frame's own quiet ink (muted ink fails on this surface at 13 px, design
     system section 5). Drawn only as far as the spare room goes, and whole or not at all:
       under 3.5 rem   nothing
       from 3.5 rem    v1.1.0
       from 8.25 rem   WeaveSignal 1.1.0
     A station on another version is said beside it, the difference in the ink:
       from 3.5 rem    v1.1.0, in the ink (the tooltip says the rest)
       from 6 rem      1.1.0 ≠ 1.0.5
       from 8.75 rem   v1.1.0 · station 1.0.5
       from 13.25 rem  WeaveSignal 1.1.0 · station 1.0.5
     Up to date or behind (1.3.2), the version carries its glyph before the v (the operator,
     2026-11-25: "a tick then v1.3.1"; the glyph had stood in the v's place to save room), and
     always all three: the tick, the v, the number (the operator, 2026-11-25, of ✓1.3.10: "we
     want to see the tick, the v for version and then the version number"). A version of five
     characters fits the 3.5 rem it was given. A longer one asks for more, by its length
     (`data-len`): until it has the room for the 4 px after the glyph it gives up that gap, to
     1 px, and never its v; in less room than that it is not drawn:
       from 3.5 rem    ✓ v1.3.2            ↑ v1.3.1          (50.6 px)
       from 3.5 rem    ✓v1.3.10                               (55.1 px; ✓ v1.3.10, 58.1 px, from 4 rem)
       from 4 rem      ✓v1.10.10                              (62.5 px; ✓ v1.10.10, 65.5 px, from 4.5 rem)
       from 8.5 rem                         ↑ v1.3.1 ≠ 1.3.2
       from 10.25 rem  ✓ WeaveSignal v1.3.2 ↑ WeaveSignal v1.3.1 (the update feed's offer)
       from 11.5 rem                        ↑ v1.3.1 · station 1.3.2
       from 17.25 rem                       ↑ WeaveSignal v1.3.1 · station 1.3.2
     At 1,440 px an admin's or a producer's frame leaves 65 px (measured in Chromium with the
     Keys tab and a station named Kettle FM; a longer station name leaves less), so they read
     ✓ v1.3.2, ✓ v1.3.10 with its gap, and ✓v1.10.10 without. At 1,280 px their frame is two rows
     and leaves 105 px. The widths are this machine's `system-ui` (Noto Sans): a wider face has
     less to spare in the forms without the gap. The wider forms are measured for a version as
     long as 1.10.10. */
  .ver {
    display: none;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    font-variant-numeric: tabular-nums;
    border-radius: var(--r-1);
  }
  .ver .name,
  .ver .stn,
  .ver .neq {
    display: none;
  }
  .ver .stn,
  .ver .neq,
  .ver.differs {
    color: var(--ink);
    font-weight: 600;
  }
  /* Where the app stands, in the ink (1.3.2). The status colours are borders and glyphs, not
     text (design system section 2: good is 2.94:1 on the light frame, critical 3.31:1 on the
     dark one), so the words take each colour stepped towards the theme's ink, 65 to 35 in
     OKLab: at least 5.0:1 on `--surface-2` in Dark, Light and Night (good 6.26, 5.36, 7.12;
     critical 5.03, 6.86, 5.67). The glyph keeps the same ink, so the pair reads as one word. */
  .ver[data-standing='current'] {
    --ver-ink: color-mix(in oklab, var(--status-good) 65%, var(--ink));
  }
  .ver[data-standing='behind'] {
    --ver-ink: color-mix(in oklab, var(--status-critical) 65%, var(--ink));
  }
  /* The ink is the glyph's and the app's own number's. The name beside it stays the frame's
     quiet ink and the station's version its own (as a difference always was), so a wide bar
     does not carry a whole line of red. */
  .ver.cue {
    color: var(--ver-ink);
    font-weight: 600;
  }
  .ver.cue .name {
    color: var(--ink-2);
    font-weight: 400;
  }
  /* 10 px, and a 4 px gap before the v. A longer version without the room for that gap keeps
     1 px of it (the glyph's own box leaves the tick's tip another half pixel short of the v),
     and never drops its v. */
  .ver .vg {
    width: 10px;
    height: 10px;
    margin-right: var(--s-1);
    vertical-align: -1px;
  }
  @container (min-width: 3.5rem) {
    .ver {
      display: block;
    }
  }
  /* ✓ v1.3.10 is 58 px: its gap from 4 rem. ✓v1.10.10 is 62.5 px: not drawn in less. */
  @container (max-width: 3.99rem) {
    .ver.cue[data-len='6'] .vg {
      margin-right: 1px;
    }
    .ver.cue[data-len='7'] {
      display: none;
    }
  }
  /* ✓ v1.10.10 is 65.5 px: its gap from 4.5 rem. */
  @container (max-width: 4.49rem) {
    .ver.cue[data-len='7'] .vg {
      margin-right: 1px;
    }
  }
  @container (min-width: 6rem) {
    .ver.differs:not(.cue) {
      color: inherit;
      font-weight: inherit;
    }
    .ver.differs:not(.cue) .neq {
      display: inline;
    }
    .ver.differs:not(.cue) .v {
      display: none;
    }
  }
  @container (min-width: 8.25rem) {
    .ver:not(.differs):not(.cue) .name {
      display: inline;
    }
    .ver:not(.differs):not(.cue) .v {
      display: none;
    }
  }
  @container (min-width: 8.75rem) {
    .ver.differs:not(.cue) .stn,
    .ver.differs:not(.cue) .v {
      display: inline;
    }
    .ver.differs:not(.cue) .neq {
      display: none;
    }
  }
  @container (min-width: 8.5rem) {
    .ver.differs.cue .neq {
      display: inline;
    }
  }
  @container (min-width: 10.25rem) {
    .ver.cue:not(.differs) .name {
      display: inline;
    }
  }
  @container (min-width: 11.5rem) {
    .ver.differs.cue .stn {
      display: inline;
    }
    .ver.differs.cue .neq {
      display: none;
    }
  }
  @container (min-width: 13.25rem) {
    .ver.differs:not(.cue) .name {
      display: inline;
    }
    .ver.differs:not(.cue) .v {
      display: none;
    }
  }
  @container (min-width: 17.25rem) {
    .ver.differs.cue .name {
      display: inline;
    }
  }
  .update-nag {
    appearance: none;
    background: none;
    border: 1px solid var(--accent);
    border-radius: var(--r-pill);
    color: var(--ink);
    font-size: var(--fs-13);
    padding: 0 var(--s-3);
    min-height: 1.75rem;
    cursor: pointer;
    font-weight: 600;
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: var(--s-2);
  }
  .link svg {
    width: 12px;
    height: 12px;
  }
  .link b {
    color: var(--ink);
    font-weight: 600;
  }
  /* The frame's room at 1,440 px is spent (the step control took what "Control plane" gave):
     the Live or Preparing control takes the word's place while the link is good. The tick and
     the tooltip stay, and the full words return the moment the link is not good. */
  @container (max-width: 1600px) {
    .link.tight[data-link='live'] b {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }
    .link.tight[data-link='live'] svg {
      width: 14px;
      height: 14px;
    }
  }
  .seg {
    display: inline-flex;
    border: 1px solid var(--axis);
    border-radius: var(--r-pill);
    overflow: hidden;
    background: var(--surface);
    /* Three themes and the text size stay whole in a narrow frame: the name beside them gives way first. */
    flex: none;
  }
  .seg button {
    appearance: none;
    border: 0;
    background: transparent;
    color: var(--ink-2);
    font-size: var(--fs-13);
    padding: 0 var(--s-3);
    min-height: 1.75rem;
    cursor: pointer;
    font-weight: 600;
  }
  .seg button + button {
    border-left: 1px solid var(--hairline);
  }
  .seg button[aria-pressed='true'] {
    background: var(--accent);
    color: var(--accent-ink);
  }
</style>
