<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // A chatter's own avatar (ADR-0235): choose one of the 72 that come with the station, or use
  // a picture of their own, which they centre and zoom here and which nobody else sees until a
  // moderator approves it. The sheet always says what other people see now, in words, since
  // the chatter's own view differs while a picture is waiting. A real <dialog>: focus moves to
  // its heading on opening and back to the button that opened it.
  import { tick } from 'svelte';
  import Avatar from '../../../../../packages/ui/Avatar.svelte';
  import { STOCK, stockLabel, stockUrl } from '../../../../../packages/ui/avatars.ts';
  import type { ChatInfo, Me, RoomRules } from '../../../../../packages/ui/chat.ts';
  import * as api from './api.ts';
  import AvatarCrop from './AvatarCrop.svelte';

  let {
    open,
    info,
    rules = null,
    session,
    me,
    onme,
    onclose,
  }: { open: boolean; info: ChatInfo | null; rules?: RoomRules | null; session: string | null; me: Me; onme: (me: Me) => void; onclose: () => void } = $props();

  let dialog = $state<HTMLDialogElement | undefined>();
  let heading = $state<HTMLElement | undefined>();
  let file = $state<HTMLInputElement | undefined>();
  let opener: HTMLElement | null = null;
  let picture = $state<ImageBitmap | null>(null);
  let busy = $state(false);
  let problem = $state<string | null>(null);
  let cropProblem = $state<string | null>(null);
  let note = $state('');
  /** The chatter's own picture as only they may fetch it (waiting, else approved). */
  let own = $state<string | null>(null);
  let confirmRemove = $state(false);

  // The studio's switch (ADR-0259) reaches an open page in the rules; the info is the page's first reading.
  const uploads = $derived(rules?.avatar_uploads ?? info?.avatars?.uploads ?? false);
  const guest = $derived(me.kind === 'guest');
  const held = $derived(me.avatar_held ?? null);
  const approved = $derived(me.avatar?.kind === 'upload' ? me.avatar.id : null);
  const stock = $derived(me.avatar_stock ?? null);
  const stockName = $derived(stockLabel(stock));
  /** What other people see beside this chatter's messages, in words. */
  const others = $derived(approved ? 'your own picture' : stockName ? `the ${stockName.toLowerCase()}` : `your initial, ${[...me.name][0]?.toLocaleUpperCase() ?? ''}`);
  const blocked = $derived(me.banned || (me.timed_out_until_ms ?? 0) > Date.now());

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) {
      opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      problem = cropProblem = null;
      note = '';
      confirmRemove = false;
      dialog.showModal();
      void tick().then(() => heading?.focus());
    } else if (!open && dialog.open) {
      dialog.close();
    }
  });

  // The chatter's own picture, fetched with their session whenever which one it is changes.
  let ownFor: string | null = null;
  $effect(() => {
    const id = held ?? approved;
    if (!open || id === ownFor) return;
    ownFor = id;
    if (own) URL.revokeObjectURL(own);
    own = null;
    if (id && session) void api.ownAvatar(session).then((u) => (ownFor === id ? (own = u) : u && URL.revokeObjectURL(u)));
  });

  function closed() {
    picture?.close();
    picture = null;
    if (own) URL.revokeObjectURL(own);
    own = null;
    ownFor = null;
    opener?.focus();
    opener = null;
    onclose();
  }

  let saving: ReturnType<typeof setTimeout> | undefined;
  /** Choosing applies itself; arrowing through the set saves once it rests. */
  function choose(id: string | null) {
    if (!session) return;
    problem = null;
    onme({ ...me, avatar_stock: id, avatar: approved ? me.avatar : id ? { kind: 'stock', id } : null });
    clearTimeout(saving);
    saving = setTimeout(async () => {
      try {
        onme((await api.chooseStock(session, id)).me);
        note = id ? `Saved. Others see the ${stockLabel(id)?.toLowerCase() ?? 'avatar'}${approved ? ' if your own picture is removed' : ''}.` : 'Saved. Others see your initial.';
      } catch (err) {
        problem = err instanceof Error ? err.message : String(err);
        try {
          onme((await api.me(session)).me);
        } catch {
          // The next welcome puts it right.
        }
      }
    }, 350);
  }

  async function picked() {
    const f = file?.files?.[0];
    if (file) file.value = '';
    if (!f) return;
    problem = cropProblem = null;
    try {
      const bitmap = await createImageBitmap(f);
      if (Math.min(bitmap.width, bitmap.height) < 32) {
        bitmap.close();
        problem = 'That picture is too small to see. Choose one at least 32 pixels on each side.';
        return;
      }
      picture = bitmap;
      await tick();
      heading?.focus();
    } catch {
      problem = "That file isn't a picture this browser can open. Choose a PNG or JPEG photo or drawing.";
    }
  }

  async function send(square: Blob) {
    if (!session) return;
    busy = true;
    cropProblem = null;
    try {
      onme((await api.uploadAvatar(session, square)).me);
      picture?.close();
      picture = null;
      note = 'Sent. A moderator will look at your picture.';
      await tick();
      heading?.focus();
    } catch (err) {
      cropProblem = err instanceof Error ? err.message : String(err);
    } finally {
      busy = false;
    }
  }

  function back() {
    picture?.close();
    picture = null;
    cropProblem = null;
    void tick().then(() => document.getElementById('av-pick')?.focus());
  }

  async function remove() {
    if (!session) return;
    if (!confirmRemove) {
      confirmRemove = true;
      // Said aloud too: a button's new words are not announced while it keeps focus.
      note = 'Press again to delete your picture.';
      return;
    }
    busy = true;
    problem = null;
    try {
      onme((await api.removeAvatar(session)).me);
      note = 'Your picture is gone from the chat.';
      confirmRemove = false;
      await tick();
      heading?.focus();
    } catch (err) {
      problem = err instanceof Error ? err.message : String(err);
    } finally {
      busy = false;
    }
  }
</script>

<dialog class="sheet avatar-sheet" bind:this={dialog} onclose={closed} aria-labelledby="av-h" data-chat="avatar-sheet">
  {#if open}
    <div class="inner">
      <h2 id="av-h" tabindex="-1" bind:this={heading}>{picture ? 'Fit your picture' : 'Your avatar'}</h2>
      {#if picture}
        <AvatarCrop {picture} name={me.name} {busy} problem={cropProblem} onuse={send} onback={back} />
      {:else}
        <div class="now">
          <Avatar avatar={me.avatar} name={me.name} size={64} label="Your avatar as others see it: {others}" />
          <div class="words">
            <p data-chat="avatar-others">Other people see <b>{others}</b> beside your messages.</p>
            {#if held}
              <p class="waiting" data-chat="avatar-waiting">
                <Avatar avatar={null} name={me.name} size={40} src={own} label="Your picture, waiting for a moderator" />
                <span><b>Your picture is waiting for a moderator.</b> Only you, the station's staff and its moderators can see it until one approves it.</span>
              </p>
            {/if}
          </div>
        </div>
        <div class="sr-only" role="status" aria-live="polite">{note}</div>
        {#if note}<p class="small saved" aria-hidden="true">{note}</p>{/if}
        {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
        {#if blocked}<p class="small">You can't change your avatar while you're timed out or banned.</p>{/if}

        {#if uploads}
          <section class="own" aria-labelledby="av-own">
            <h3 id="av-own">Your own picture</h3>
            {#if guest}
              <p class="small">Sign in with an email address to use a picture of your own. As a guest you can choose one from the set below.</p>
            {:else}
              <p class="small" id="av-own-hint">A photo or drawing, PNG or JPEG. You centre and zoom it next, and a moderator looks at it before anyone else sees it.</p>
              <input bind:this={file} class="sr-only" id="av-file" type="file" accept="image/png,image/jpeg" tabindex="-1" aria-hidden="true" onchange={picked} data-chat="avatar-file" />
              <div class="row">
                <button type="button" class="btn" id="av-pick" disabled={blocked || busy} aria-describedby="av-own-hint" data-chat="avatar-pick" onclick={() => file?.click()}
                  >{held || approved ? 'Choose another picture…' : 'Choose a picture…'}</button
                >
                {#if held || approved}
                  <button type="button" class="btn quiet" class:danger-hollow={confirmRemove} disabled={busy} data-chat="avatar-remove" onclick={remove}
                    >{confirmRemove ? 'Delete it? Press again' : 'Delete my picture'}</button
                  >
                {/if}
              </div>
            {/if}
          </section>
        {/if}

        <fieldset class="set" disabled={blocked}>
          <legend>{uploads && !guest ? 'Or choose one of ours' : 'Choose one'}</legend>
          {#if approved}<p class="small">Shown if your own picture is ever removed.</p>{/if}
          <div class="grid" data-chat="avatar-set">
            <label class="pick none" title="None: your initial">
              <input class="sr-only" type="radio" name="av-stock" checked={stock === null} onchange={() => choose(null)} />
              <Avatar avatar={null} name={me.name} size={44} />
              <span class="sr-only">None: your initial</span>
            </label>
            {#each STOCK as a (a.id)}
              <label class="pick" title={a.label}>
                <input class="sr-only" type="radio" name="av-stock" value={a.id} checked={stock === a.id} onchange={() => choose(a.id)} data-avatar-id={a.id} />
                <img src={stockUrl(a.id)} alt="" width="44" height="44" loading="lazy" decoding="async" draggable="false" />
                <span class="sr-only">{a.label}</span>
              </label>
            {/each}
          </div>
        </fieldset>

        <button type="button" class="btn primary wide" data-chat="avatar-done" onclick={() => dialog?.close()}>Done</button>
      {/if}
    </div>
  {/if}
</dialog>

<style>
  .now {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--s-3);
    align-items: start;
  }
  .words {
    display: grid;
    gap: var(--s-2);
    align-content: center;
    min-height: 64px;
  }
  .waiting {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--s-2);
    align-items: center;
    padding: var(--s-2) var(--s-3);
    border-left: 3px solid var(--status-warning);
    background: var(--surface-2);
    border-radius: var(--r-1);
    font-size: var(--fs-14);
    line-height: var(--lh-label);
  }
  .saved {
    color: var(--ink-2);
  }
  .own {
    display: grid;
    gap: var(--s-2);
  }
  h3,
  legend {
    font-size: var(--fs-16);
    font-weight: 650;
    padding: 0;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-2);
  }
  .set {
    border: 0;
    margin: 0;
    padding: 0;
    min-width: 0;
    display: grid;
    gap: var(--s-2);
  }
  .set legend {
    margin-bottom: var(--s-2);
  }
  .set[disabled] {
    opacity: 0.55;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(52px, 1fr));
    gap: var(--s-1);
    max-height: min(40dvh, 17rem);
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: var(--s-1);
    border: 1px solid var(--hairline);
    border-radius: var(--r-2);
    background: var(--plane);
  }
  .pick {
    position: relative;
    display: grid;
    place-items: center;
    aspect-ratio: 1;
    min-height: 52px;
    border-radius: var(--r-2);
    cursor: pointer;
  }
  .pick img {
    width: 44px;
    height: 44px;
    border-radius: 50%;
    display: block;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--ink) 14%, transparent);
  }
  .pick:hover {
    background: var(--surface-2);
  }
  /* The chosen one: a ring in the accent and a filled ground, not colour alone on the picture. */
  .pick:has(input:checked) {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    box-shadow: inset 0 0 0 2px var(--accent);
  }
  .pick:has(input:focus-visible) {
    outline: var(--focus-ring);
    outline-offset: 1px;
  }
</style>
