<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The studio app's frame: the show's step and Show everything (ADR-0271), the tabs with their
  // keys (⌃1 to ⌃5; a tab keeps its number at every step), the link to the station and the
  // theme toggle (Dark, Light or Night; dark by default, never the system's; ADR-0065,
  // ADR-0270), over the surface of the tab. One keyboard handler for every surface, from the
  // keymap (ADR-0063); the key of a part the step hides does nothing, and says so once.
  //
  // A staff window is Live or Preparing (ADR-0276): the control beside who is signed in, and,
  // while preparing, a band under the frame on the tabs that drive the air (Desk, Screener,
  // Carts). A preparing window's keys change nothing on air; the band answers instead.
  import { setupState, stationZone } from './lib/schedule.ts';
  import { dialogLets } from './lib/dialogKeys.ts';
  import { onMount } from 'svelte';
  import Glyphs from './lib/Glyphs.svelte';
  import Mark from './lib/Mark.svelte';
  import DeskB from './desk/DeskB.svelte';
  import Screener from './screener/Screener.svelte';
  import CartWall from './carts/CartWall.svelte';
  import Settings from './settings/Settings.svelte';
  import StationSetup from './lib/StationSetup.svelte';
  import Account from './lib/Account.svelte';
  import StepControl from './lib/StepControl.svelte';
  import PrepareControl from './lib/PrepareControl.svelte';
  import SessionEnded from './lib/SessionEnded.svelte';
  import ShowWizard from './settings/ShowWizard.svelte';
  import Schedule from './schedule/Schedule.svelte';
  import { base, follow, packaged, sandboxed, savedStation } from './lib/api.ts';
  import { isAdmin } from './lib/account.ts';
  import { account, installSessionWatch, loadMe } from './lib/account.svelte.ts';
  import { closeSetup, decideSetup, loadShows, mine, rememberSkip, setup } from './lib/wizard.svelte.ts';
  import { cartShow, nudge, onAir, onBeforePreparing, prepare, preparing, settleStart, switchToLive } from './lib/prepare.svelte.ts';
  import { canPrepare, changesAir, prepareBand } from './lib/prepare.ts';
  import { installUpdateCheck, updateNag } from './lib/update.svelte.ts';
  import { run, tellOff } from './lib/actions.ts';
  import { look, say, setCartShow, setShowAll, studio, takeView } from './lib/studio.svelte.ts';
  import { STEP_WORD, gate, offTag, stationStep, tabShown } from './lib/steps.ts';
  import { chat } from './lib/chat.svelte.ts';
  import { theme, setTheme, THEMES, THEME_WORDS } from './lib/theme.svelte.ts';
  import { zoom, zoomIn, zoomOut, zoomReset, installZoom } from './lib/zoom.svelte.ts';
  import { installPanelNames } from './lib/panelNames.ts';
  import { PANELS } from '../../../packages/ui/panels.ts';
  import { ARM_MS, KeyEngine, type Tab } from '../../../packages/ui/keymap.ts';

  // A tab keeps its number at every step: ⌃1 Desk, ⌃3 Carts, ⌃5 Schedule.
  const TABS: { tab: Tab; n: number; label: string; built: boolean }[] = [
    { tab: 'desk', n: 1, label: 'Desk', built: true },
    { tab: 'screener', n: 2, label: 'Screener', built: true },
    { tab: 'carts', n: 3, label: 'Carts', built: true },
    { tab: 'chat', n: 4, label: 'Chat', built: false },
    { tab: 'schedule', n: 5, label: 'Schedule', built: true },
  ];
  /** The station's step: what is switched on. */
  const step = $derived(stationStep(studio.view));
  /** The tabs this window draws: the step's, or all of them while everything is shown. */
  const tabs = $derived(TABS.filter((t) => tabShown(t.tab, look())));

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
    // A modal dialog owns the keyboard: only ⌃T reaches the desk while one is open (U1).
    if (document.querySelector('dialog[open]') && !dialogLets(input(e))) return;
    // Settings is a form: only the global chords (⌃) reach the keymap, and as from a text field,
    // so no bare letter, Space or F-key acts on the desk, and Esc stays the dialog's and the
    // drop-down's own.
    if (studio.settings && !(e.ctrlKey || e.metaKey)) return;
    // The account, session, show-setup and invite dialogs are forms too: their keys are their own.
    if ((e.target as Element | null)?.closest?.('[data-panel="studio.account"], [data-panel="studio.session-ended"], [data-panel="studio.show-setup"], [data-panel="desk.invite"], [data-panel="desk.end-show"]') && !(e.ctrlKey || e.metaKey)) return;
    const ctx = {
      tab: studio.tab,
      inTextField: studio.settings || inTextField(document.activeElement),
      dialog: studio.tab === 'screener' && chat.sor ? ('statement' as const) : null,
    };
    const out = keys.down(input(e), ctx, Date.now());
    if (!out.handled) return;
    // A part the show's step keeps off: its key does nothing (not even arm), and the first
    // press in a session says so. Still the keymap's key, so the browser does not take it
    // (⌃D would bookmark, ⌃T open a tab). Checked before anything else acts on the key, so
    // ⌃2 at Solo does not close Settings.
    if (out.action) {
      const g = gate(out.action, step, look(), studio.view);
      if (!g.allow) {
        e.preventDefault();
        keys.disarm();
        studio.armed = null;
        tellOff(g.told);
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
    if (out.action) void run(out.action);
  }

  /** A held key whose key-up went to another window would stay open: let everything go. The
   *  promise is for the switch to Preparing, which waits until the releases have landed. */
  function letGo(): Promise<void> {
    const sent = keys.releaseAll().map((a) => run(a));
    studio.armed = null;
    return Promise.all(sent).then(() => {});
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
  const prepBand = $derived(preparing() && !studio.settings && studio.tab !== 'schedule' ? prepareBand(onAir()) : null);
  /** A key or a button met a control the window has switched off: the band answers for a moment. */
  const nudged = $derived(studio.now - prepare.nudgedAt < 1200);
  /** One row for the frame, two for the sandbox's guide, one for each band, then the surface. */
  const rows = $derived(`${'auto '.repeat(1 + (sandboxed() ? 2 : 0) + (studio.showAll ? 1 : 0) + (prepBand ? 1 : 0))}minmax(0, 1fr)`);

  onMount(() => {
    document.documentElement.dataset.surface = studio.tab;
    // Who this window runs as, and the 401 watch, before the first request goes out.
    installSessionWatch();
    // Then, for a presenter signed in as themselves with no show of their own, the setup wizard (ADR-0218).
    void loadMe()
      .then(decideSetup)
      // The station's setup page not yet finished: say so to an admin (ADR-0267).
      .then(() => (isAdmin(account.me) ? setupState() : null))
      .then((s) => (studio.setupPending = s && s.status === 'pending' ? s.missing : null))
      .catch(() => {});
    // The station's time zone, so the rundown's times are the station's (issue 10).
    void stationZone()
      .then((z) => (studio.zone = z.zone))
      .catch(() => {});
    const stop = follow(takeView, (s, why) => {
      studio.link = s;
      studio.linkWhy = why ?? '';
    });
    // The feature's panels are merged until packages/ui/panels.ts lists them (ADR-0070).
    const names = installPanelNames(document, PANELS);
    const stopZoom = installZoom();
    // One look at the update feed a little after the app opens (packaged only); it installs nothing.
    const stopUpdateCheck = installUpdateCheck();
    onBeforePreparing(letGo);
    const tick = setInterval(() => {
      studio.now = Date.now();
      if (studio.armed && studio.now - studio.armed.at > ARM_MS) studio.armed = null;
      if (studio.note && studio.note.until < studio.now) studio.note = null;
      if (studio.offNote && studio.offNote.until < studio.now) studio.offNote = null;
    }, 100);
    const hidden = () => {
      if (document.hidden) void letGo();
    };
    document.addEventListener('visibilitychange', hidden);
    return () => {
      stop();
      names.destroy();
      stopZoom();
      stopUpdateCheck();
      onBeforePreparing(null);
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
<div class="shell" class:with-guide={sandboxed()} class:with-all={studio.showAll} class:with-prepare={!!prepBand} style:grid-template-rows={rows}>
  {#if import.meta.env.MODE === 'sandbox'}{#await import('./sandbox/Guide.svelte') then guide}<guide.default />{/await}{/if}
  <header class="frame" data-panel="studio.frame">
    <span class="brand" data-panel-title><Mark />{studio.view?.station || 'WeaveSignal'}</span>
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
        onclick={() => (studio.settings = true)}>Settings</button
      >
    </nav>
    <span class="spacer"></span>
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
      <button type="button" onclick={zoomReset} aria-label="Reset text size" title="Reset (Ctrl+0)">{Math.round(zoom.level * 100)}%</button>
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

  {#if studio.settings}
    <Settings />
  {:else if studio.tab === 'desk'}
    <DeskB />
  {:else if studio.tab === 'carts'}
    <CartWall />
  {:else if studio.tab === 'schedule'}
    <Schedule />
  {:else}
    <Screener />
  {/if}
  <SessionEnded />
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
  .settings-tab {
    margin-left: var(--s-2);
    border-left: 1px solid var(--hairline);
    border-radius: 0;
  }
  .tab[aria-disabled='true'] {
    color: var(--muted);
    cursor: not-allowed;
  }
  .spacer {
    flex: 1;
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
