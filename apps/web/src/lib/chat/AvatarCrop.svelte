<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Fit a picture to the avatar's circle (ADR-0235): drag it to centre it, zoom with the slider,
  // the wheel, a pinch, or + and −; the arrow keys move it. What is inside the circle is what
  // the chat shows; the square around it is what is sent. The picture is drawn on a canvas
  // from the file itself, so it never leaves this page until "Use this picture" is pressed, and
  // what is sent is a fresh drawing: nothing the file carried beside its pixels goes with it.
  import { onDestroy, onMount } from 'svelte';
  import { GROUND, MAX_ZOOM, MIN_ZOOM, SEND_SIDE, ZOOM_STEP, centred, clamp, describe, dragBy, nudge, source, zoomTo, type Crop } from './crop.ts';

  let {
    picture,
    name,
    busy,
    problem,
    onuse,
    onback,
  }: {
    picture: ImageBitmap;
    name: string;
    busy: boolean;
    problem: string | null;
    onuse: (square: Blob) => void;
    onback: () => void;
  } = $props();

  const img = $derived({ width: picture.width, height: picture.height });
  let crop = $state<Crop>({ cx: 0, cy: 0, zoom: 1 });
  let stage = $state<HTMLCanvasElement | undefined>();
  let frame = $state<HTMLElement | undefined>();
  let small = $state<HTMLCanvasElement | undefined>();
  let stageSide = $state(280);
  let say = $state('');
  let failed = $state<string | null>(null);
  const zoomPct = $derived(Math.round(crop.zoom * 100));

  $effect(() => {
    crop = centred({ width: picture.width, height: picture.height });
  });

  function paint(canvas: HTMLCanvasElement | undefined, cssSide: number) {
    if (!canvas) return;
    const px = Math.round(cssSide * Math.min(globalThis.devicePixelRatio || 1, 2));
    if (canvas.width !== px) canvas.width = canvas.height = px;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const { sx, sy, s } = source(crop, img);
    ctx.fillStyle = GROUND;
    ctx.fillRect(0, 0, px, px);
    ctx.imageSmoothingQuality = 'high';
    ctx.drawImage(picture, sx, sy, s, s, 0, 0, px, px);
  }
  $effect(() => {
    paint(stage, stageSide);
    paint(small, 28);
  });

  let sayTimer: ReturnType<typeof setTimeout> | undefined;
  function moved(next: Crop) {
    crop = clamp(next, img);
    clearTimeout(sayTimer);
    sayTimer = setTimeout(() => (say = describe(crop, img)), 400);
  }

  // ---- pointer: one finger or the mouse drags, two fingers pinch ----
  const pointers = new Map<number, { x: number; y: number }>();
  let pinch = 0;
  function spread(): number {
    const [a, b] = [...pointers.values()];
    return a && b ? Math.hypot(a.x - b.x, a.y - b.y) : 0;
  }
  function down(e: PointerEvent) {
    frame?.setPointerCapture(e.pointerId);
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
    pinch = spread();
  }
  function move(e: PointerEvent) {
    const was = pointers.get(e.pointerId);
    if (!was) return;
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pointers.size === 1) {
      moved(dragBy(crop, img, e.clientX - was.x, e.clientY - was.y, stageSide));
    } else if (pointers.size === 2) {
      const now = spread();
      if (pinch > 0 && now > 0) moved(zoomTo(crop, img, crop.zoom * (now / pinch)));
      pinch = now;
    }
  }
  function up(e: PointerEvent) {
    pointers.delete(e.pointerId);
    pinch = spread();
  }
  function wheel(e: WheelEvent) {
    e.preventDefault();
    moved(zoomTo(crop, img, crop.zoom * (e.deltaY < 0 ? 1.08 : 1 / 1.08)));
  }
  let watch: ResizeObserver | undefined;
  onMount(() => {
    // Not passive: the wheel zooms the picture instead of scrolling the sheet under it.
    frame?.addEventListener('wheel', wheel, { passive: false });
    if (frame && 'ResizeObserver' in globalThis) {
      watch = new ResizeObserver(() => frame && (stageSide = frame.clientWidth || 280));
      watch.observe(frame);
    }
  });
  onDestroy(() => {
    frame?.removeEventListener('wheel', wheel);
    watch?.disconnect();
    clearTimeout(sayTimer);
  });

  function keys(e: KeyboardEvent) {
    const step = e.shiftKey ? 0.2 : 0.05;
    const act: Record<string, () => Crop> = {
      ArrowLeft: () => nudge(crop, img, -step, 0),
      ArrowRight: () => nudge(crop, img, step, 0),
      ArrowUp: () => nudge(crop, img, 0, -step),
      ArrowDown: () => nudge(crop, img, 0, step),
      '+': () => zoomTo(crop, img, crop.zoom + ZOOM_STEP),
      '=': () => zoomTo(crop, img, crop.zoom + ZOOM_STEP),
      '-': () => zoomTo(crop, img, crop.zoom - ZOOM_STEP),
      '0': () => centred(img),
    };
    const f = act[e.key];
    if (!f || e.ctrlKey || e.metaKey || e.altKey) return;
    e.preventDefault();
    moved(f());
  }

  function use() {
    failed = null;
    const out = document.createElement('canvas');
    out.width = out.height = SEND_SIDE;
    const ctx = out.getContext('2d');
    if (!ctx) {
      failed = "This browser couldn't prepare the picture. Try another browser, or choose an avatar from the set.";
      return;
    }
    const { sx, sy, s } = source(crop, img);
    ctx.fillStyle = GROUND;
    ctx.fillRect(0, 0, SEND_SIDE, SEND_SIDE);
    ctx.imageSmoothingQuality = 'high';
    ctx.drawImage(picture, sx, sy, s, s, 0, 0, SEND_SIDE, SEND_SIDE);
    out.toBlob(
      (blob) => {
        if (blob) onuse(blob);
        else failed = "This browser couldn't prepare the picture. Try another browser, or choose an avatar from the set.";
      },
      'image/jpeg',
      0.9,
    );
  }
</script>

<div class="crop" data-chat="avatar-crop">
  <p class="small" id="crop-how">Drag the picture so the part you want sits inside the circle. Zoom with the slider.</p>
  <!-- An application region: the arrow keys reach it under a screen reader, and its name says what they do. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    bind:this={frame}
    class="frame"
    tabindex="0"
    role="application"
    aria-label="Your picture. Arrow keys move it, plus and minus zoom, zero centres it."
    aria-describedby="crop-how"
    data-chat="avatar-stage"
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
    onkeydown={keys}
  >
    <canvas bind:this={stage} class="stage"></canvas>
    <span class="ring" aria-hidden="true"></span>
  </div>

  <div class="zoom">
    <label for="crop-zoom">Zoom</label>
    <button type="button" class="btn step" aria-label="Zoom out" disabled={crop.zoom <= MIN_ZOOM} onclick={() => moved(zoomTo(crop, img, crop.zoom - ZOOM_STEP))}>−</button>
    <input
      id="crop-zoom"
      type="range"
      min={MIN_ZOOM}
      max={MAX_ZOOM}
      step={ZOOM_STEP}
      value={crop.zoom}
      aria-valuetext="{zoomPct}%"
      data-chat="avatar-zoom"
      oninput={(e) => moved(zoomTo(crop, img, Number(e.currentTarget.value)))}
    />
    <button type="button" class="btn step" aria-label="Zoom in" disabled={crop.zoom >= MAX_ZOOM} onclick={() => moved(zoomTo(crop, img, crop.zoom + ZOOM_STEP))}>+</button>
    <output class="num small" for="crop-zoom">{zoomPct}%</output>
  </div>

  <div class="as">
    <span class="small">In the chat:</span>
    <canvas bind:this={small} class="mini" aria-hidden="true"></canvas>
    <b class="who">{name}</b>
    <span class="spacer"></span>
    <button type="button" class="btn quiet centre" data-chat="avatar-centre" onclick={() => moved(centred(img))}>Centre</button>
  </div>

  <p class="small">A moderator looks at your picture before anyone else sees it. Use one you have the right to use, and not a picture of someone else without asking them.</p>

  <div class="sr-only" role="status" aria-live="polite">{say}</div>
  {#if failed || problem}<p class="problem" role="alert">{failed ?? problem}</p>{/if}
  <button type="button" class="btn primary wide" disabled={busy} aria-busy={busy} data-chat="avatar-use" onclick={use}>{busy ? 'Sending…' : 'Use this picture'}</button>
  <button type="button" class="btn quiet wide" disabled={busy} data-chat="avatar-back" onclick={onback}>Back</button>
</div>

<style>
  .crop {
    display: grid;
    gap: var(--s-3);
  }
  .frame {
    position: relative;
    width: min(18rem, 100%);
    aspect-ratio: 1;
    justify-self: center;
    border-radius: var(--r-2);
    overflow: hidden;
    background: var(--surface-3);
    cursor: grab;
    touch-action: none;
  }
  .frame:active {
    cursor: grabbing;
  }
  .frame:focus-visible {
    outline: var(--focus-ring);
    outline-offset: var(--focus-offset);
  }
  .stage {
    display: block;
    width: 100%;
    height: 100%;
  }
  /* The circle the chat shows: outside it the square is dimmed, never hidden, so the edge of
     what is sent stays in sight. Two rings, dark and light, read on any picture. */
  .ring {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    pointer-events: none;
    box-shadow:
      0 0 0 1px rgba(0, 0, 0, 0.55),
      inset 0 0 0 2px rgba(255, 255, 255, 0.92),
      0 0 0 999px rgba(0, 0, 0, 0.5);
  }
  .zoom {
    display: grid;
    grid-template-columns: auto auto 1fr auto 5ch;
    align-items: center;
    gap: var(--s-2);
  }
  .zoom label {
    font-weight: 650;
  }
  .zoom input {
    width: 100%;
    min-height: var(--target-touch);
    accent-color: var(--accent);
  }
  .zoom output {
    text-align: right;
  }
  .step {
    width: var(--target-touch);
    padding: 0;
    font-size: var(--fs-20);
    line-height: 1;
  }
  .as {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    min-width: 0;
  }
  .as .who {
    font-weight: 650;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .as .spacer {
    flex: 1;
  }
  .mini {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    flex: none;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--ink) 14%, transparent);
  }
  .centre {
    flex: none;
  }
</style>
