<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The studio app's frame: the tabs with their keys (⌃1 to ⌃5), the link to the station and
  // the theme toggle (dark by default, never the system's, ADR-0065), over the surface of the
  // tab. One keyboard handler for every surface, from the keymap (ADR-0063).
  import { onMount } from 'svelte';
  import Glyphs from './lib/Glyphs.svelte';
  import Mark from './lib/Mark.svelte';
  import DeskB from './desk/DeskB.svelte';
  import Screener from './screener/Screener.svelte';
  import CartWall from './carts/CartWall.svelte';
  import Settings from './settings/Settings.svelte';
  import StationSetup from './lib/StationSetup.svelte';
  import Account from './lib/Account.svelte';
  import SessionEnded from './lib/SessionEnded.svelte';
  import Schedule from './schedule/Schedule.svelte';
  import { follow, packaged, savedStation } from './lib/api.ts';
  import { installSessionWatch, loadMe } from './lib/account.svelte.ts';
  import { installUpdateCheck, updateNag } from './lib/update.svelte.ts';
  import { run } from './lib/actions.ts';
  import { studio, takeView } from './lib/studio.svelte.ts';
  import { chat } from './lib/chat.svelte.ts';
  import { theme, setTheme } from './lib/theme.svelte.ts';
  import { zoom, zoomIn, zoomOut, zoomReset, installZoom } from './lib/zoom.svelte.ts';
  import { installPanelNames } from './lib/panelNames.ts';
  import { PANELS } from '../../../packages/ui/panels.ts';
  import { ARM_MS, KeyEngine, type Tab } from '../../../packages/ui/keymap.ts';

  const tabs: { tab: Tab; label: string; built: boolean }[] = [
    { tab: 'desk', label: 'Desk', built: true },
    { tab: 'screener', label: 'Screener', built: true },
    { tab: 'carts', label: 'Carts', built: true },
    { tab: 'chat', label: 'Chat', built: false },
    { tab: 'schedule', label: 'Schedule', built: true },
  ];

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

  function onKeydown(e: KeyboardEvent) {
    // Settings is a form: only the global chords (⌃) reach the keymap, and as from a text field,
    // so no bare letter, Space or F-key acts on the desk, and Esc stays the dialog's and the
    // drop-down's own.
    if (studio.settings && !(e.ctrlKey || e.metaKey)) return;
    // The account and session dialogs are forms too: their keys are their own.
    if ((e.target as Element | null)?.closest?.('[data-panel="studio.account"], [data-panel="studio.session-ended"]') && !(e.ctrlKey || e.metaKey)) return;
    const ctx = {
      tab: studio.tab,
      inTextField: studio.settings || inTextField(document.activeElement),
      dialog: studio.tab === 'screener' && chat.sor ? ('statement' as const) : null,
    };
    const out = keys.down(input(e), ctx, Date.now());
    if (!out.handled) return;
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

  /** A held key whose key-up went to another window would stay open: let everything go. */
  function letGo() {
    for (const a of keys.releaseAll()) void run(a);
    studio.armed = null;
  }

  onMount(() => {
    document.documentElement.dataset.surface = studio.tab;
    // Who this window runs as, and the 401 watch, before the first request goes out.
    installSessionWatch();
    void loadMe();
    const stop = follow(takeView, (s, why) => {
      studio.link = s;
      studio.linkWhy = why ?? '';
    });
    // The feature's panels are merged until packages/ui/panels.ts lists them (ADR-0070).
    const names = installPanelNames(document, PANELS);
    const stopZoom = installZoom();
    // One look at the update feed a little after the app opens (packaged only); it installs nothing.
    const stopUpdateCheck = installUpdateCheck();
    const tick = setInterval(() => {
      studio.now = Date.now();
      if (studio.armed && studio.now - studio.armed.at > ARM_MS) studio.armed = null;
      if (studio.note && studio.note.until < studio.now) studio.note = null;
    }, 100);
    const hidden = () => {
      if (document.hidden) letGo();
    };
    document.addEventListener('visibilitychange', hidden);
    return () => {
      stop();
      names.destroy();
      stopZoom();
      stopUpdateCheck();
      clearInterval(tick);
      document.removeEventListener('visibilitychange', hidden);
    };
  });

  const linkWord = $derived(
    studio.link === 'live' ? 'connected' : studio.link === 'connecting' ? 'connecting' : 'not connected',
  );
</script>

<svelte:window onkeydown={onKeydown} onkeyup={onKeyup} onblur={letGo} />

<Glyphs />
{#if packaged() && !savedStation()}
  <StationSetup />
{:else}
<div class="shell">
  <header class="frame" data-panel="studio.frame">
    <span class="brand" data-panel-title><Mark />{studio.view?.station || 'WeaveSignal'}</span>
    <nav class="tabs" aria-label="Studio">
      {#each tabs as t, i (t.tab)}
        <button
          type="button"
          class="tab"
          aria-current={!studio.settings && studio.tab === t.tab ? 'page' : undefined}
          aria-disabled={!t.built}
          onclick={() => {
            if (t.built) studio.settings = false;
            void run({ type: 'tab', tab: t.tab });
          }}
          >{t.label}{#if !t.built}<span class="sr-only"> (not in this build)</span>{/if} <kbd>⌃{i + 1}</kbd></button
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
    {#if updateNag()}
      <!-- Points at Settings; the install itself waits for a yes there, and for a quiet desk. -->
      <button type="button" class="update-nag" onclick={() => (studio.settings = true)}
        >Update {updateNag()} available</button
      >
    {/if}
    <Account />
    <span class="link" role="status" title={studio.linkWhy}>
      <svg class={studio.link === 'live' ? 'g-good' : 'g-warning'} aria-hidden="true"
        ><use href={studio.link === 'live' ? '#g-good' : '#g-degraded'} /></svg
      >Control plane <b>{linkWord}</b>
    </span>
    <div class="seg" role="group" aria-label="Text size">
      <button type="button" onclick={zoomOut} aria-label="Smaller text" title="Smaller text (Ctrl+−)">A−</button>
      <button type="button" onclick={zoomReset} aria-label="Reset text size" title="Reset (Ctrl+0)">{Math.round(zoom.level * 100)}%</button>
      <button type="button" onclick={zoomIn} aria-label="Larger text" title="Larger text (Ctrl++)">A+</button>
    </div>
    <div class="seg" role="group" aria-label="Theme">
      <button type="button" aria-pressed={theme.current === 'dark'} onclick={() => setTheme('dark')}>Dark</button>
      <button type="button" aria-pressed={theme.current === 'light'} onclick={() => setTheme('light')}>Light</button>
    </div>
  </header>

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
</div>
{/if}

<style>
  .shell {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    height: 100vh;
  }
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
    overflow: hidden;
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
    gap: var(--s-1);
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
  .seg {
    display: inline-flex;
    border: 1px solid var(--axis);
    border-radius: var(--r-pill);
    overflow: hidden;
    background: var(--surface);
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
