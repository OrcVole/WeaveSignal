<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The Carts tab (cart-wall.md; ADR-0020): the bank strip, a six-by-four grid that mirrors the
  // left hand of the keyboard, and the strip of what is sounding. Every key is the keymap's
  // (ADR-0063, section 4); every state carries a word (design system section 3); a bank's
  // colour is its palette slot, a 5 px band and never the face (ADR-0066). Panel names
  // `carts.wall`, `carts.bank.1`–`carts.bank.5`, `carts.sounding` (ADR-0070); the app's own
  // frame is `studio.frame`, so the design's `carts.frame` is not drawn twice.
  //
  // A cart wall per show (1.0.2): the window works with the station's banks and its show's own
  // (`studio.cartShow`). Tab N is the show's bank N where the show has made one, the station's
  // otherwise, and each tab says whose it is: "show" or "station" (packages/ui/carts.ts).
  import { CHIP } from '../../../../packages/ui/states.ts';
  import StateChip from '../../../../packages/ui/StateChip.svelte';
  import { cartKey } from '../../../../packages/ui/keymap.ts';
  import { MODE, bankAt, bankOwner, cartAt, cartNow, cartsAt, stationBankUnder, tabOf, type Bank, type Cart, type CartNow } from '../../../../packages/ui/carts.ts';
  import { studio, stationNow } from '../lib/studio.svelte.ts';
  import { run } from '../lib/actions.ts';
  import { dur, firstName, signed } from '../lib/format.ts';
  import { preparing } from '../lib/prepare.svelte.ts';
  import { setup } from '../lib/wizard.svelte.ts';
  import { OFF_WHY } from '../lib/prepare.ts';

  /** The duck depth the desk's mixer uses (crates/mixer, `DUCK_DB`). */
  const DUCK_DB = 12;
  const SLOTS = Array.from({ length: 24 }, (_, i) => i + 1);
  const RING_R = 11;
  const RING_C = 2 * Math.PI * RING_R;

  const v = $derived(studio.view);
  /** This window is preparing (ADR-0276): the wall shows what is sounding, and fires nothing.
   *  The banks still turn, as they are this window's own. */
  const prep = $derived(preparing());
  const cv = $derived(v?.carts ?? null);
  const cat = $derived(studio.catalogue);
  const now = $derived(stationNow());
  const bank = $derived(bankAt(cat, studio.bank));
  /** The name of the show whose own banks this window works with, for the strip's words. */
  const showName = $derived.by(() => {
    const slug = studio.cartShow;
    if (!slug) return null;
    if (v?.step?.show?.slug === slug) return v.step.show.name;
    return setup.shows.find((s) => s.slug === slug)?.name ?? slug;
  });

  /** Whose a tab's bank is, in words, for its tooltip. */
  function whose(b: Bank | null, n: number): string {
    if (!b) return `Bank ${n} has not been made`;
    if (bankOwner(b) === 'station') return 'The station’s bank, shared by every show';
    const under = stationBankUnder(cat, n);
    const own = `${showName ?? 'The show'}’s own bank`;
    return under ? `${own}: in its windows it takes the place of the station’s bank ${n}, ${under.name}` : own;
  }
  const colour = (n: number | undefined) => (n ? `var(--cart-${n})` : 'var(--deemph)');
  const bankColour = $derived(colour(bank?.colour_slot));

  /** Who has the microphone open, for the ducked line (the desk's sources on air, not muted). */
  const micName = $derived.by(() => {
    const s = v?.sources.find((x) => x.on_air && !x.mute && x.present);
    return s ? firstName(s.name, s.identity) : null;
  });
  const duckWhy = $derived(
    cv?.mic_open ? (micName ? `${micName}’s mic is open` : 'a mic is open') : 'a sweeper is playing',
  );

  const slots = $derived(
    SLOTS.map((slot) => {
      const cart = cartAt(cat, studio.bank, slot);
      return { slot, key: cartKey(slot), cart, now: cartNow(cart, cv, now) };
    }),
  );

  /** Everything sounding, in the order fired, whichever bank it is in. */
  const sounding = $derived(
    (cv?.sounding ?? []).map((s) => {
      const cart = cat?.carts.find((c) => c.id === s.cart) ?? null;
      // A cart this window's banks do not hold (another show's) is named from the view.
      const tab = cart ? tabOf(cat, cart) : null;
      const b = tab === null ? null : bankAt(cat, tab);
      return { s, cart, label: cart?.label ?? s.label ?? 'A cart', tab, colour: colour(b?.colour_slot), now: cartNow(cart, cv, now), here: tab === studio.bank };
    }),
  );
  const soundingWords = $derived(
    sounding.length ? `Sounding: ${sounding.map((x) => x.label).join(', ')}` : 'Nothing sounding',
  );
  const fadeArmed = $derived(studio.armed?.what === 'fade-all');
  const fadeLeft = $derived(fadeArmed && studio.armed ? Math.max(0, Math.ceil((studio.armed.at + 3000 - studio.now) / 1000)) : 0);

  // Durations round up, so a 0.7 s rimshot reads 0:01, never 0:00.
  function left(ms: number | null): string {
    return ms === null ? '' : `−${dur(ms + 999)}`;
  }

  function aria(key: string, cart: Cart | null, n: CartNow): string {
    if (!cart) return `${key}, empty slot`;
    let a = `${key}, ${cart.label}, ${MODE[cart.mode].word}, ${dur(cart.duration_ms + 999)}`;
    if (n.state === 'missing') a += ', file missing, the key does nothing';
    else if (n.state === 'stopping') a += ', fading out';
    else if (n.state === 'cued') a += ', cued, in your headphones';
    else if (n.sounding) {
      a += cart.mode === 'bed' ? ', looping' : `, playing, ${dur((n.leftMs ?? 0) + 999)} left`;
      if (cart.mode === 'bed' && cv?.ducked) a += ', ducked';
    }
    return a;
  }

  function fadeAll() {
    if (fadeArmed) void run({ type: 'fade-all' });
    else void run({ type: 'arm', what: 'fade-all' });
  }
</script>

<div class="cartwall">
  <h1 class="sr-only">{v?.station || 'WeaveSignal'} carts</h1>
  <section class="wallpanel" data-panel="carts.wall" aria-label="Cart wall">
    <div class="banks">
      <span class="caps" data-panel-title>Banks</span>
      <div class="banklist" role="group" aria-label={showName ? `Cart banks: the station’s, and ${showName}’s own` : 'Cart banks: the station’s'}>
        {#each [1, 2, 3, 4, 5] as n (n)}
          {@const b = bankAt(cat, n)}
          {@const count = cartsAt(cat, n).length}
          <button
            type="button"
            class="bank"
            class:none={!b}
            data-panel="carts.bank.{n}"
            data-owner={b ? bankOwner(b) : undefined}
            aria-pressed={studio.bank === n}
            title={whose(b, n)}
            style="--bank-colour:{colour(b?.colour_slot)}"
            onclick={() => run({ type: 'bank', n })}
            ><span class="band"></span><kbd>⌃F{n}</kbd><b data-panel-title>{b?.name ?? `Bank ${n}`}</b>{#if b}<span class="owner">{bankOwner(b)}</span>{/if}<span
              class="count num">{b ? `${count}/24` : 'not made'}</span
            ></button
          >
        {/each}
      </div>
      <div class="right">
        {#if cv && !cv.desk}
          <StateChip chip={CHIP.fault} title={cv.desk_error ?? ''} />
          <span class="small">Carts cannot sound until the desk answers.</span>
        {:else if cv?.importing}
          <span class="state hollow hollow-standby"
            ><svg aria-hidden="true"><use href="#g-standby" /></svg>Loading carts</span
          >
          <span class="small num">{cv.importing[0]} of {cv.importing[1]}, through the media worker</span>
        {:else if cv}
          <span class="small">Every cart plays at <b class="num">{cat ? `${signed(cat.loudness_lufs)} LUFS` : '…'}</b> · mixed in the desk, heard in every return</span>
        {/if}
      </div>
    </div>

    <div class="wall" role="group" aria-label="{bank?.name ?? `Bank ${studio.bank}`}, 24 carts" style="--cart-colour:{bankColour}">
      {#each slots as { slot, key, cart, now: n } (slot)}
        <button
          type="button"
          class="pad {n.state}"
          class:bed={cart?.mode === 'bed'}
          data-cart-slot={slot}
          aria-label={aria(key, cart, n)}
          aria-disabled={n.state === 'missing' || undefined}
          disabled={prep}
          title={prep ? OFF_WHY : undefined}
          onfocus={() => (studio.cartFocus = slot)}
          onclick={() => run({ type: 'cart-key', slot })}
        >
          {#if !cart}
            <span class="top"><kbd>{key}</kbd><span class="mode">empty</span></span>
            <span class="label">No cart here yet. Upload one to fill this slot.</span>
            <span class="foot"></span>
          {:else}
            <span class="top"
              ><kbd>{key}</kbd>
              {#if n.state === 'missing'}
                <span class="state {CHIP.fault.cls}"
                  ><svg class="g-critical" aria-hidden="true"><use href="#g-fault" /></svg>File missing</span
                >
              {:else if n.state === 'cued'}
                <StateChip chip={CHIP.cue} focusable={false} hoverable={false} />
              {:else}
                <span class="mode"><svg aria-hidden="true"><use href="#{MODE[cart.mode].glyph}" /></svg>{MODE[cart.mode].word}</span>
              {/if}
            </span>
            <span class="label">{cart.label}{#if cart.detail}<small>{cart.detail}</small>{/if}</span>
            <span class="foot">
              <span class="left">
                {#if n.state === 'missing'}
                  <span class="fix">Re-link it in the library. The key does nothing.</span>
                {:else if n.state === 'cued'}
                  <b class="num">{cart.mode === 'bed' ? dur(now - (n.sounding?.started_ms ?? now)) : left(n.leftMs)}</b>
                  <span class="word">in your headphones</span>
                {:else if n.sounding}
                  {#if cart.mode === 'bed'}
                    <b class="num">{dur(now - n.sounding.started_ms)}</b>
                    <span class="word">{n.state === 'stopping' ? 'fading' : 'loop · playing'}</span>
                    {#if cv?.ducked && n.state !== 'stopping'}
                      <span class="duck"><svg aria-hidden="true"><use href="#g-mic" /></svg>ducked −{DUCK_DB} dB · {duckWhy}</span>
                    {/if}
                  {:else}
                    <b class="num">{left(n.leftMs)}</b>
                    <span class="word">{n.state === 'stopping' ? 'fading' : n.state === 'ending' ? 'ending' : 'playing'}</span>
                  {/if}
                {:else}
                  <span class="num">{dur(cart.duration_ms + 999)}</span>
                  {#if cart.ramp_ms}<span class="ramp">ramp {Math.round(cart.ramp_ms / 1000)} s to the vocal</span>{/if}
                {/if}
              </span>
              {#if n.sounding}
                <svg class="ring" viewBox="0 0 28 28" aria-hidden="true"
                  ><circle class="bg" cx="14" cy="14" r={RING_R} /><circle
                    class="fg"
                    cx="14"
                    cy="14"
                    r={RING_R}
                    stroke-dasharray={RING_C.toFixed(1)}
                    stroke-dashoffset={(RING_C * (1 - n.progress)).toFixed(1)}
                  />{#if cart.mode === 'bed'}<text x="14" y="14.5">∞</text>{/if}</svg
                >
              {/if}
            </span>
          {/if}
        </button>
      {/each}
    </div>
  </section>

  <footer class="now" data-panel="carts.sounding">
    <span class="caps" data-panel-title>Sounding</span>
    <div class="sounding" role="status" aria-live="polite" aria-atomic="true">
      <span class="sr-only">{soundingWords}</span>
      {#each sounding as x (x.s.player + x.s.pool)}
        <span class="item" class:ending={x.now.state === 'ending'} style="--cart-colour:{x.colour}" aria-hidden="true"
          ><span class="band"></span>{#if x.cart && x.tab !== null}<kbd>{x.here ? cartKey(x.cart.slot) : `⌃F${x.tab} ${cartKey(x.cart.slot)}`}</kbd>{/if}<b
            >{x.label}</b
          ><span class="t num">{x.cart?.mode === 'bed' ? dur(now - x.s.started_ms) : left(x.now.leftMs)}</span><span class="small"
            >{x.now.state === 'stopping'
              ? 'fading'
              : x.now.state === 'cued'
                ? 'cue · headphones'
                : x.cart?.mode === 'bed'
                ? cv?.ducked
                  ? 'loop · ducked'
                  : 'loop'
                : x.now.state === 'ending'
                  ? 'ending'
                  : 'playing'}</span
          ></span
        >
      {:else}
        <span class="quiet" aria-hidden="true">Nothing sounding</span>
      {/each}
    </div>
    <div class="keys">
      <span>Fire <kbd>key</kbd></span>
      <span>Stop <kbd>key</kbd> again</span>
      <span>Cue <kbd>⇧</kbd><kbd>key</kbd> · clear <kbd>Esc</kbd></span>
      <span>Stop last <kbd>Space</kbd></span>
      <button type="button" class="btn sm" class:armed={fadeArmed} disabled={prep} title={prep ? OFF_WHY : undefined} onclick={fadeAll} aria-describedby="fade-hint"
        >{fadeArmed ? `Fade all? ${fadeLeft}` : 'Fade all'} <kbd>⇧Space</kbd><kbd>⇧Space</kbd></button
      >
      <span id="fade-hint" class="sr-only">Press twice within 3 seconds to fade every cart out.</span>
      <span>Bank <kbd>⌃F1</kbd>–<kbd>⌃F5</kbd></span>
      <span class="quiet">Letters fire while Carts has focus · <kbd>F1</kbd>–<kbd>F6</kbd> fire the top row from any tab</span>
    </div>
  </footer>
</div>
