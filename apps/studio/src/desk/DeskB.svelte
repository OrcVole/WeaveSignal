<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Desk B, the focus (ADR-0061), wired to the desk through the control plane: time on the
  // left (clock, rundown, playout), what is on air in the middle (the lamp, the stage, the
  // sources not on air), the station on the right, and the station's health along the foot.
  // Panel names as on the design page (ADR-0070); the toast takes the chat glance's cell.
  import Tile from './Tile.svelte';
  import Lamp from './Lamp.svelte';
  import Cameras from './Cameras.svelte';
  import SlateStill from './SlateStill.svelte';
  import type { Notice } from '../lib/api.ts';
  import { CHIP, isOpen, listenersHear, masterLamp } from '../../../../packages/ui/states.ts';
  import StateChip from '../../../../packages/ui/StateChip.svelte';
  import { slotKeys } from '../../../../packages/ui/keymap.ts';
  import { studio, stationNow } from '../lib/studio.svelte.ts';
  import { run, readyCaller } from '../lib/actions.ts';
  import { atToday, delayWords, dur, firstName, hm, hms, listenerDelayMs } from '../lib/format.ts';
  import { cartAt, cartNow } from '../../../../packages/ui/carts.ts';
  import { cdnWarning } from '../lib/audience.ts';

  const v = $derived(studio.view);
  /** The desk's picture (M5); null with video off. */
  const pic = $derived(v?.desk.picture ?? null);
  /** More than about 500 connected with video on and no CDN (ADR-0017). */
  const cdn = $derived(cdnWarning(v));
  const listeners = $derived(v?.audience?.listeners ?? null);
  const shotName = $derived.by(() => {
    const id = pic?.identity;
    if (!id) return '';
    const s = v?.sources.find((x) => x.identity === id);
    return s ? firstName(s.name, id) : id;
  });
  /** The listener delay: the `studio.listener_delay_ms` setting, labelled until measured (ADR-0103). */
  const LISTENER_DELAY_MS = $derived(listenerDelayMs(v?.listener_delay));
  const behind = $derived(delayWords(v?.listener_delay));
  const now = $derived(stationNow());
  const playout = $derived(v?.desk.playout ?? { connected: false, for_ms: null });
  const up = $derived(!!v?.desk.reachable && playout.connected);
  /** The station's mode (ADR-0183): `auto` means automation holds the air and the desk never airs. */
  const mode = $derived(v?.mode ?? null);
  const auto = $derived(mode === 'auto');
  /** Whether listeners hear the room: playout is connected and automation does not hold the air. */
  const heard = $derived(up && listenersHear(playout.connected, mode));
  const sources = $derived(v?.sources ?? []);
  const onStage = $derived(sources.filter((s) => s.on_air));
  const offStage = $derived(sources.filter((s) => !s.on_air));
  const openCount = $derived(sources.filter(isOpen).length);
  const lamp = $derived(
    masterLamp({ deskReachable: !!v?.desk.reachable, playoutConnected: playout.connected, anyOnAir: openCount > 0, mode }),
  );
  const peopleIn = $derived(sources.filter((s) => s.present).length + (v?.callers.filter((c) => c.present && c.slot === null).length ?? 0));

  // ---- the cart glance: the current bank's top row, F1 to F6 (the wall's 1 to 6) ----
  const glanceBank = $derived(studio.catalogue?.banks.find((b) => b.number === studio.bank) ?? null);
  const glance = $derived(
    [1, 2, 3, 4, 5, 6].map((n) => {
      const cart = cartAt(studio.catalogue, studio.bank, n);
      return { n, cart, now: cartNow(cart, v?.carts ?? null, now) };
    }),
  );
  const RING_C = 2 * Math.PI * 9;

  // ---- the clock ----
  const tenth = $derived(Math.floor((now % 1000) / 100));
  const rundown = $derived(
    (v?.show.rundown ?? []).map((r) => ({ ...r, ms: atToday(r.at, now) })).sort((a, b) => a.ms - b.ms),
  );
  const post = $derived(rundown.find((r) => r.hard_post && r.ms > now) ?? null);
  const postFrom = $derived.by(() => {
    if (!post) return 0;
    const before = rundown.filter((r) => r.ms < post.ms && r.ms <= now).at(-1);
    return before?.ms ?? post.ms - 10 * 60_000;
  });
  const left = $derived(post ? post.ms - now : 0);
  const coming = $derived(rundown.filter((r, i) => r.ms > now || rundown[i + 1]?.ms > now).slice(0, 5));
  const current = $derived(rundown.filter((r) => r.ms <= now).at(-1) ?? null);

  // ---- the toast slot: one toast, most urgent first ----
  type Toast = {
    tone: 'good' | 'warning' | 'serious';
    glyph: string;
    title: string;
    msg: string;
    acts: { label: string; keys: string[]; primary?: boolean; go: () => void }[];
  };
  const caller = $derived(readyCaller());
  const degradedOnAir = $derived(sources.find((s) => s.on_air && (!s.present || !s.linked)) ?? null);
  const lastNotice: Notice | null = $derived(
    v?.notices
      .filter((n) => n.surface === 'desk' && !studio.dismissed.includes(n.id) && now - n.at_ms < 20_000 && n.kind !== 'caller.ready')
      .at(-1) ?? null,
  );
  const toast: Toast | null = $derived.by(() => {
    if (studio.note) {
      return { tone: studio.note.tone, glyph: 'g-good', title: studio.note.title, msg: studio.note.message, acts: [] };
    }
    if (v?.desk.reachable && !playout.connected) {
      return {
        tone: 'serious',
        glyph: 'g-playout',
        title: 'Playout down · room alive',
        msg: `${playout.for_ms === null ? 'Liquidsoap has not connected to the desk.' : `SRT to Liquidsoap dropped ${dur(playout.for_ms)} ago.`} Listeners hear nothing of the room; the room keeps talking and its sources say OPEN.`,
        acts: [{ label: 'Restart playout', keys: ['⌃R'], primary: true, go: () => run({ type: 'restart-playout' }) }],
      };
    }
    if (caller) {
      const n = caller.caller?.name_on_air || firstName(caller.name, caller.identity);
      const k = slotKeys(caller.slot);
      return {
        tone: 'good',
        glyph: 'g-phone',
        title: `${n} is ready for you · strip ${caller.slot}`,
        msg: [caller.caller?.line, caller.caller?.note].filter(Boolean).join(' · ') || 'Sent by the screener.',
        acts: [
          { label: `Cue ${n}`, keys: [k.cue], primary: true, go: () => run({ type: 'cue', slot: caller.slot }) },
          { label: 'Not now', keys: ['Esc'], go: () => run({ type: 'escape' }) },
        ],
      };
    }
    if (degradedOnAir) {
      const n = firstName(degradedOnAir.name, degradedOnAir.identity);
      const k = slotKeys(degradedOnAir.slot);
      return {
        tone: 'warning',
        glyph: 'g-link-degraded',
        title: degradedOnAir.present ? `${n}'s link is degraded: no audio reaching the desk` : `${n} left the room`,
        msg: degradedOnAir.present
          ? 'Mute them and cue them back in when their audio returns.'
          : 'Their place on the desk is kept for 30 s; if they rejoin they are mixed again.',
        acts: [
          { label: `Mute ${n}`, keys: [k.mute], go: () => run({ type: 'mute', slot: degradedOnAir.slot }) },
          { label: 'Keep them on', keys: ['Esc'], go: () => run({ type: 'escape' }) },
        ],
      };
    }
    if (lastNotice) return { tone: lastNotice.tone, glyph: 'g-good', title: lastNotice.title, msg: lastNotice.message, acts: [] };
    return null;
  });

  const sub = $derived.by(() => {
    if (!v) return '';
    if (!v.desk.reachable) return `The desk is not answering: ${v.desk.error ?? 'no reason given'}`;
    return '';
  });
  const dumpArmed = $derived(studio.armed?.what === 'dump');
  const delay = $derived(v?.desk.delay ?? null);
  /** The dump button is live whenever the delay holds anything, even while it rebuilds (ADR-0166). */
  const dumpable = $derived(!!delay && delay.target_ms > 0 && delay.delay_ms > 0);
  const secs = (ms: number) => (ms / 1000).toFixed(1);
  const screenerSeat = $derived(v?.seats.screener ?? null);
  const screenerName = $derived(screenerSeat ? firstName(v?.seats.screener_name ?? '', screenerSeat) : null);
</script>

<div class="desk">
  <div class="cols">
    <!-- left: time -->
    <div class="col left">
      <section class="panel clock" data-panel="desk.clock" aria-label="Studio clock">
        <div class="time num" data-panel-title>{hms(now)}<small>.{tenth}</small></div>
        {#if post}
          <div class="post">
            <span class="name">{post.title} · hard post {post.at.length === 5 ? `${post.at}:00` : post.at}</span><span
              class="left num"
              class:soon={left < 10_000}>−{dur(left)}</span
            >
            <div class="bar"><i style:width="{Math.min(100, ((now - postFrom) / (post.ms - postFrom)) * 100)}%"></i></div>
          </div>
        {:else}
          <div class="post"><span class="name">No hard post ahead</span></div>
        {/if}
        <div class="listen" title="The station's listener-delay setting; this build does not measure it (ADR-0103)">
          <svg aria-hidden="true" style="width:12px;height:12px;vertical-align:-1px"><use href="#g-delay" /></svg>
          Listeners hear <b class="num">{hms(now - LISTENER_DELAY_MS)}</b> · {behind}
        </div>
      </section>

      <section class="panel" data-panel="desk.rundown" aria-label="Rundown">
        <div class="ph">
          <h2>Coming up</h2>
          <span class="spacer"></span><span class="small">{v?.show.title || 'No show set'}</span>
        </div>
        <div class="rundown">
          {#each coming as r (r.at + r.title)}
            <div class="row" class:now={r === current}>
              <span class="at num">{hm(r.ms)}</span>
              <span class="what"><b>{r.title}</b>{#if r.detail}<span>{r.detail}</span>{/if}</span>
              {#if r === current && openCount > 0}
                <StateChip chip={heard ? CHIP.onAir : CHIP.open} />
              {:else if r.key}
                <span class="small"><kbd>{r.key}</kbd></span>
              {:else}
                <span></span>
              {/if}
            </div>
          {:else}
            <div class="row"><span class="at num">—</span><span class="what"><b>Nothing scheduled</b><span>nothing in <code>studio.rundown</code></span></span><span></span></div>
          {/each}
        </div>
        <div class="small"><span class="proposal">setting</span> rows from <code>studio.rundown</code>; the schedule does not fill this yet</div>
      </section>

      <section class="panel playout" data-panel="desk.playout" aria-label="Playout">
        <div class="ph">
          <h2>Playout</h2>
          <span class="small lg-only">Liquidsoap 2.5</span><span class="spacer"></span>
        </div>
        <div class="plamp">
          {#if !v?.desk.reachable}
            <StateChip chip={CHIP.fault} />
            <span class="small">the desk is not answering</span>
          {:else if playout.connected}
            <!-- In auto mode the log is what listeners hear, so the panel's own chip reads ON AIR (ADR-0183). -->
            <StateChip chip={auto ? CHIP.onAir : CHIP.standby} />
            <span class="small">{auto ? 'Auto · the log · ' : ''}SRT connected {playout.for_ms !== null ? dur(playout.for_ms) : ''}</span>
          {:else}
            <StateChip chip={CHIP.playoutDown} />
            <span class="small"
              >{playout.for_ms === null ? 'never connected' : `SRT link lost ${dur(playout.for_ms)} ago`} · room alive</span
            >
          {/if}
        </div>
        {#if v?.default_fallback}
          <div class="plamp" data-testid="default-loop">
            <StateChip chip={CHIP.defaultLoop} />
            <span class="small">on the stream</span>
          </div>
          <div class="extra warn">
            Listeners hear the loop WeaveSignal ships, because this station has no fallback files of its own. Put some music
            in the station's fallback folder and it takes over within seconds.
          </div>
        {/if}
        <div class="q"><span>Playout's queue and hard posts are not shown here yet.</span></div>
        {#if v?.desk.reachable && !playout.connected}
          <div class="extra">Listeners hear nothing of the room. Restart playout to bring them back.</div>
          <div class="acts">
            <button type="button" class="btn primary" onclick={() => run({ type: 'restart-playout' })}
              >Restart playout <kbd>⌃R</kbd></button
            >
          </div>
        {/if}
      </section>
    </div>

    <!-- centre: the stage -->
    <main class="stage" data-panel="desk.stage" aria-label="On air">
      <header class="stage-head">
        <Lamp state={lamp} />
        <div class="title">
          <h1 data-panel-title>{v?.show.title || 'No show set'}</h1>
          {#if sub}
            <div class="sub">{sub}</div>
          {:else if auto}
            <div class="sub"><b>Automation holds the air</b> · {openCount} open to the room · listeners hear the log, {behind}</div>
          {:else if up}
            <div class="sub"><b>{openCount} on air</b> · listeners {behind}</div>
          {:else}
            <div class="sub"><b>Room alive</b> · {openCount} open to the room · listeners hear nothing of it</div>
          {/if}
        </div>
        <div class="delay" data-panel="desk.delay">
          {#if delay && delay.target_ms > 0}
            <svg class="arc" viewBox="0 0 48 48" aria-hidden="true">
              <circle class="bg" cx="24" cy="24" r="20" />
              <circle
                class="fg"
                class:warn={delay.rebuilding && delay.dumps > 0}
                cx="24"
                cy="24"
                r="20"
                stroke-dasharray="{(125.7 * Math.min(1, delay.delay_ms / delay.target_ms)).toFixed(1)} 125.7"
              />
              <text x="24" y="28" text-anchor="middle">{secs(delay.delay_ms)}</text>
            </svg>
          {/if}
          <div class="lbl">
            {#if !delay}
              <span class="state hollow hollow-standby" data-panel-title
                ><svg aria-hidden="true"><use href="#g-delay" /></svg>Delay</span
              >
              <span class="small">unknown · the desk is not answering</span>
            {:else if delay.target_ms === 0}
              <span class="state hollow hollow-standby" data-panel-title
                ><svg aria-hidden="true"><use href="#g-delay" /></svg>Delay off</span
              >
              <span class="small">nothing is delayed</span>
            {:else if delay.rebuilding && delay.dumps > 0}
              <StateChip chip={CHIP.dumped} data-panel-title />
              <span class="small"
                >rebuilding {secs(delay.delay_ms)} s{#if delay.dumped_at_ms !== null}&nbsp;· {hms(delay.dumped_at_ms)}{/if} · listeners
                now {secs(delay.delay_ms)} s behind</span
              >
            {:else if delay.rebuilding}
              <StateChip chip={CHIP.delay} word="Delay building" data-panel-title />
              <span class="small">{secs(delay.delay_ms)} of {secs(delay.target_ms)} s held · Dump throws it away</span>
            {:else}
              <!-- Full: the sentence for DELAY 3.2 S speaks of a rebuild, so this word carries none. -->
              <span class="state hollow hollow-accent" data-panel-title
                ><svg class="g-accent" aria-hidden="true"><use href="#g-delay" /></svg>Delay {secs(delay.target_ms)} s</span
              >
              <span class="small">full · listeners {secs(delay.delay_ms)} s behind</span>
            {/if}
          </div>
          <button
            type="button"
            class="btn big"
            class:danger-hollow={dumpable}
            class:armed={dumpArmed}
            aria-disabled={dumpable ? undefined : 'true'}
            onclick={() => run(dumpArmed ? { type: 'dump' } : { type: 'arm', what: 'dump' })}
            >{dumpArmed ? 'Press again' : 'Dump'} <kbd>Del</kbd><kbd>Del</kbd></button
          >
        </div>
        <div class="acts">
          <button type="button" class="btn quiet" onclick={() => run({ type: 'end-show' })}>End show <kbd>⌃⇧E</kbd></button>
          <button type="button" class="btn quiet" onclick={() => run({ type: 'tab', tab: 'screener' })}
            >Screener <kbd>⌃2</kbd></button
          >
        </div>
      </header>

      <div class="tiles">
        {#each onStage as s (s.identity)}
          <Tile source={s} slot={s.slot} playoutUp={up} playoutForMs={playout.for_ms} {mode} />
        {:else}
          <div class="empty">
            <b>{v?.desk.reachable ? 'Nobody is open' : 'No desk'}</b>
            <span
              >{v?.desk.reachable
                ? 'A source comes here on its own On key, 1 to 5.'
                : (v?.desk.error ?? 'Waiting for the control plane.')}</span
            >
          </div>
        {/each}
      </div>

      <div class="waiting" data-panel="desk.waiting">
        <div class="wh">
          <span data-panel-title>Not on air</span><span class="rule"
            >a source moves between the stage and this row only on its own On key · the stage keeps slot order · cue,
            talk and mute never move it · its keys never change · while playout is down the stage does not change: its
            sources say OPEN (ADR-0062)</span
          >
        </div>
        {#each offStage as s (s.identity)}
          <Tile source={s} slot={s.slot} playoutUp={up} playoutForMs={playout.for_ms} {mode} />
        {/each}
        <Tile source={null} slot={6} playoutUp={up} playoutForMs={playout.for_ms} {mode} />
      </div>
    </main>

    <!-- right: the station -->
    <div class="col right">
      <section class="panel audience" data-panel="desk.audience" aria-label="Listeners">
        <div class="ph"><h2>Listeners</h2><span class="spacer"></span></div>
        {#if listeners === null}
          <div class="count num">— <small>not counted yet</small></div>
          <div class="split">counts by mount are not shown here yet</div>
        {:else}
          <div class="count num">{listeners.toLocaleString('en-GB')} <small>{listeners === 1 ? 'listener' : 'listeners'} now</small></div>
          <div class="split">on the station's stream; counts by mount are not shown here yet</div>
        {/if}
        {#if cdn}
          <p class="cdn-warning" role="status" data-warning="cdn">
            <svg class="g-warning" aria-hidden="true"><use href="#g-fault" /></svg>
            <span><b>{cdn.title}.</b> {cdn.message}</span>
          </p>
        {/if}
        <div class="st">
          {#if auto && v?.desk.reachable}
            <!-- ON AUTOMATION (ADR-0183): what listeners hear while automation holds the air. -->
            <span class="state hollow hollow-auto"><svg class="g-auto" aria-hidden="true"><use href="#g-auto" /></svg>On automation</span>
          {:else if up}
            <StateChip chip={CHIP.allGood} word="Hearing the desk" />
          {:else}
            <StateChip chip={CHIP.playoutDown} />
          {/if}
        </div>
      </section>

      <section class="panel video" data-panel="desk.picture" aria-label="Picture">
        <div class="ph"><h2>Picture</h2></div>
        {#if pic}
          <div class="pic" aria-label={pic.shot === 'slate' ? 'The slate is on air' : `${shotName} is on air`}>
            {#if pic.shot === 'slate'}
              <div class="slate"><b>{v?.station || 'WeaveSignal'}</b><span>{v?.show.title}</span><span class="pill">slate</span></div>
            {:else}
              <div class="slate"><b>{shotName}</b><span>on camera</span><span class="pill">{pic.mode.mode === 'auto' ? 'follows the audio' : 'held'}</span></div>
            {/if}
          </div>
          <div class="sw" role="group" aria-label="Picture source">
            <button type="button" class="btn" aria-pressed={pic.mode.mode !== 'slate'} onclick={() => pic.mode.mode === 'slate' && run({ type: 'picture' })}
              >Live picture <kbd>F7</kbd></button
            >
            <button type="button" class="btn" aria-pressed={pic.mode.mode === 'slate'} onclick={() => pic.mode.mode !== 'slate' && run({ type: 'picture' })}
              >Slate <kbd>F7</kbd></button
            >
          </div>
          <p class="small">{pic.cameras.length} {pic.cameras.length === 1 ? 'camera' : 'cameras'}{pic.hls ? '' : ' · encoders restarting'}</p>
          <Cameras
            cameras={pic.cameras}
            onair={pic.shot === 'person' ? (pic.identity ?? null) : null}
            label={(id) => {
              const s = v?.sources.find((x) => x.identity === id);
              return s ? firstName(s.name, id) : id;
            }}
          />
          <SlateStill on={!!pic.still} />
        {:else}
          <div class="pic" aria-label="No programme picture">
            <div class="slate"><b>{v?.station || 'WeaveSignal'}</b><span>{v?.show.title}</span><span class="pill">video off</span></div>
          </div>
          <div class="sw" role="group" aria-label="Picture source">
            <button type="button" class="btn" aria-disabled="true" onclick={() => run({ type: 'picture' })}
              >Live picture <kbd>F7</kbd></button
            >
            <button type="button" class="btn" aria-disabled="true" onclick={() => run({ type: 'picture' })}
              >Slate <kbd>F7</kbd></button
            >
          </div>
        {/if}
      </section>

      <section class="panel" data-panel="desk.carts" aria-label="Carts">
        <div class="ph">
          <h2>Carts{#if glanceBank}&nbsp;· {glanceBank.name}{/if}</h2>
          <span class="spacer"></span><span class="small">bank {studio.bank} · <kbd>F1</kbd>–<kbd>F6</kbd></span>
        </div>
        <div class="carts" style="--cart-colour:{glanceBank ? `var(--cart-${glanceBank.colour_slot})` : 'var(--deemph)'}">
          {#each glance as { n, cart, now: c } (n)}
            {#if !cart}
              <button type="button" class="cart empty" aria-label="F{n}, empty slot" onclick={() => run({ type: 'cart', n })}
                ><span class="label">Empty</span><span class="dur num">no cart</span><kbd>F{n}</kbd></button
              >
            {:else}
              <button
                type="button"
                class="cart"
                class:playing={!!c.sounding && c.state !== 'cued'}
                class:cued={c.state === 'cued'}
                class:ending={c.state === 'ending'}
                class:missing={c.state === 'missing'}
                aria-label="F{n}, {cart.label}{c.state === 'missing'
                  ? ', file missing, the key does nothing'
                  : c.state === 'cued'
                    ? ', cued, in your headphones'
                    : c.sounding
                    ? cart.mode === 'bed'
                      ? ', looping'
                      : `, playing, ${dur((c.leftMs ?? 0) + 999)} left`
                    : `, ${dur(cart.duration_ms + 999)}`}"
                aria-disabled={c.state === 'missing' || undefined}
                onclick={() => run({ type: 'cart', n })}
                ><span class="label">{cart.label}</span><span class="dur num"
                  >{#if c.state === 'missing'}file missing{:else if c.sounding}<b
                      >{cart.mode === 'bed' ? 'loop' : `−${dur((c.leftMs ?? 0) + 999)}`}</b
                    >{:else}{dur(cart.duration_ms + 999)}{/if}</span
                >{#if c.sounding}<svg class="ring" viewBox="0 0 24 24" aria-hidden="true"
                    ><circle class="bg" cx="12" cy="12" r="9" /><circle
                      class="fg"
                      cx="12"
                      cy="12"
                      r="9"
                      stroke-dasharray={RING_C.toFixed(1)}
                      stroke-dashoffset={(RING_C * (1 - c.progress)).toFixed(1)}
                    /></svg
                  >{/if}<kbd>F{n}</kbd></button
              >
            {/if}
          {/each}
        </div>
      </section>

      <div class="slot">
        <section class="panel chat" data-panel="desk.chat" aria-label="Chat">
          <div class="ph">
            <h2>Chat</h2>
            <span class="spacer"></span>
          </div>
          <div class="msgs"><div class="msg">The chat is on the Screener tab; it is not shown at the desk yet.</div></div>
        </section>
        {#if toast}
          <div class="toast {toast.tone}" data-panel="desk.toast" role={toast.tone === 'good' ? 'status' : 'alert'}>
            <div class="stripe"></div>
            <div class="body">
              <svg aria-hidden="true"><use href="#{toast.glyph}" /></svg><span class="title" data-panel-title>{toast.title}</span>
              <span class="msg">{toast.msg}</span>
              {#if toast.acts.length}
                <span class="acts">
                  {#each toast.acts as a (a.label)}
                    <button type="button" class="btn" class:primary={a.primary} onclick={a.go}
                      >{a.label}
                      {#each a.keys as k, i (i)}<kbd>{k}</kbd>{/each}</button
                    >
                  {/each}
                </span>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>

  <footer class="statusbar" data-panel="desk.status" aria-label="Station status">
    <span class="item" data-panel-title
      ><svg class={v?.desk.reachable ? 'g-good' : 'g-critical'} aria-hidden="true"
        ><use href={v?.desk.reachable ? '#g-good' : '#g-fault'} /></svg
      >Room <b>LiveKit</b> · {peopleIn} in</span
    >
    {#if up}
      <span class="item"><svg class="g-good" aria-hidden="true"><use href="#g-good" /></svg>To playout <b>SRT connected</b></span>
    {:else}
      <span class="item"
        ><svg class="g-serious" aria-hidden="true"><use href="#g-playout" /></svg>To playout
        <b>no link{playout.for_ms !== null ? ` ${dur(playout.for_ms)}` : ''}</b> ·
        <button
          type="button"
          class="btn quiet"
          style="min-height:1.5rem;padding:0 var(--s-1)"
          onclick={() => run({ type: 'restart-playout' })}>Restart <kbd>⌃R</kbd></button
        ></span
      >
    {/if}
    <span class="item"><svg class="g-muted" aria-hidden="true"><use href="#g-standby" /></svg>Listeners <b>icecast.server</b> · not watched yet</span>
    {#if degradedOnAir}
      <span class="item"
        ><svg class="g-warning" aria-hidden="true"><use href="#g-link-degraded" /></svg>Link degraded ·
        <b>{firstName(degradedOnAir.name, degradedOnAir.identity)}</b></span
      >
    {/if}
    <span class="sep"></span>
    <span class="item"
      >Talk <b>A</b>–<b>H</b> held, not latched ·
      <button
        type="button"
        class="btn quiet"
        class:lit-talk={!!screenerSeat && studio.talking.includes(screenerSeat)}
        style="min-height:1.5rem;padding:0 var(--s-1)"
        aria-label="Talk to the screener; held"
        onpointerdown={() => run({ type: 'talk-seat', held: true })}
        onpointerup={() => run({ type: 'talk-seat', held: false })}
        >Talk to {screenerName ?? 'the screener'} · hold <kbd>⌃T</kbd></button
      ></span
    >
  </footer>
</div>
