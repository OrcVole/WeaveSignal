// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The studio keymap (docs/reference/design/keymap.md, adopted by ADR-0063) as a pure engine:
// a key event and its context in, an action out. No DOM, so its rules are tested on their own.
//
// The rules it keeps (keymap section 1):
//  1. Two layers: global chords, F1 to F7, `Del Del` and Esc fire on every tab; unmodified keys
//     (and ⇧ with a letter) fire only on their own tab.
//  3. Text fields own the unmodified keys: while one has focus, single keys, ⇧ letters and
//     two-press keys never fire; Esc leaves the field; single modifier chords still fire.
//  4. Two presses for anything that changes the room or a person's standing: the first press
//     arms for 3 s, the second (the same key) fires, any other key or Esc disarms.
//  5. A modal dialog owns the keyboard (the statement of reasons): its own keys only, ⌃T still
//     talks, and in its note field the first Esc leaves the field and the second closes it.
//  7. Held, not latched: talk keys act while held; a key repeat is not a new press, and
//     everything held is let go when the window loses focus.
// Keys are read by their physical position (`KeyboardEvent.code`), as the keymap's rows are
// the keyboard's rows: `1`–`6` over `Q`–`Y` over `A`–`H` over `Z`–`N`.

export type Tab = 'desk' | 'screener' | 'carts' | 'chat' | 'schedule';
export const TABS: readonly Tab[] = ['desk', 'screener', 'carts', 'chat', 'schedule'];

/** The two-press actions (rule 4). `end-pass` is armed by its button on a desk tile only: it
 *  has no key, because a key is a change to the keymap and so a new ADR (keymap.md), and the
 *  tile it was pressed on says whose pass it ends. */
export type TwoPress = 'dump' | 'drop' | 'influx' | 'fade-all' | 'ban' | 'end-pass';

/** The statement dialog's reasons, on `A` `S` `T` `I` `M` `E` (screener section 3), and `C` for a
 *  picture suspected of being child sexual abuse material (ADR-0257; the dialog offers it for a
 *  picture only). */
export type SorReason = 'abuse' | 'spam' | 'off_topic' | 'illegal' | 'intimate_image' | 'other' | 'suspected_csam';
/** What happens to the chatter besides the hide, on `1` `2` `3`. */
export type SorThen = 'none' | 'timeout' | 'ban';

const SOR_REASON_CODES: Record<string, SorReason> = {
  KeyA: 'abuse',
  KeyS: 'spam',
  KeyT: 'off_topic',
  KeyI: 'illegal',
  KeyM: 'intimate_image',
  KeyE: 'other',
  KeyC: 'suspected_csam',
};
const SOR_THEN: readonly SorThen[] = ['none', 'timeout', 'ban'];

export type Action =
  | { type: 'tab'; tab: Tab }
  /** Arm a two-press action; `identity` is whose, for an action aimed at one person's tile. */
  | { type: 'arm'; what: TwoPress; identity?: string }
  | { type: 'disarm' }
  | { type: 'dump' }
  | { type: 'drop' }
  /** End this person's pass (ADR-0119): off air, out of the room, no new tokens. */
  | { type: 'end-pass'; identity: string }
  | { type: 'influx' }
  | { type: 'restart-playout' }
  | { type: 'restart-encoder' }
  | { type: 'backup-stream' }
  | { type: 'end-show' }
  | { type: 'talk-seat'; held: boolean }
  /** F1 to F6: cart n of the current bank's top row, from any tab. */
  | { type: 'cart'; n: number }
  /** ⌃F1 to ⌃F5: the cart bank, from any tab. */
  | { type: 'bank'; n: number }
  | { type: 'picture' }
  | { type: 'escape' }
  | { type: 'leave-field' }
  // desk tab
  | { type: 'on'; slot: number }
  | { type: 'cue'; slot: number }
  | { type: 'mute'; slot: number }
  | { type: 'talk'; slot: number; held: boolean }
  | { type: 'handover' }
  // carts tab
  | { type: 'cart-key'; slot: number }
  | { type: 'cart-cue'; slot: number }
  | { type: 'cart-stop-last' }
  | { type: 'fade-all' }
  | { type: 'cart-move'; dx: number; dy: number }
  | { type: 'cart-enter' }
  // screener tab
  | { type: 'queue-move'; delta: number }
  | { type: 'queue-jump'; n: number }
  | { type: 'screen-talk'; held: boolean }
  | { type: 'send' }
  | { type: 'back' }
  | { type: 'focus-name' }
  | { type: 'release-place' }
  | { type: 'mode'; mode: 'slow' | 'hold_new' | 'links_trusted' }
  // screener tab: the chat (M6)
  | { type: 'mod-hide' }
  | { type: 'mod-timeout' }
  | { type: 'ban' }
  | { type: 'mod-leave' }
  | { type: 'mod-release' }
  | { type: 'release-all' }
  | { type: 'hide-all' }
  | { type: 'acknowledge' }
  | { type: 'escalate' }
  | { type: 'feature' }
  | { type: 'pin' }
  | { type: 'invite' }
  | { type: 'reply' }
  | { type: 'pass' }
  | { type: 'newest' }
  | { type: 'post' }
  /** Take down a chatter's approved picture (ADR-0235): a button only, no key. */
  | { type: 'avatar-remove'; account: string; name: string; picture: string; report?: string }
  /** A picture or document posted in the chat and shown, taken down with its message (ADR-0262). */
  | { type: 'attachment-remove'; attachment: string; kind: 'picture' | 'document'; account: string; name: string; report?: string }
  // schedule tab: the calendar's toolbar (calendar.md §5): the arrows move a week, T is today
  | { type: 'week'; delta: number }
  | { type: 'today' }
  // the statement of reasons dialog
  | { type: 'sor-then'; then: SorThen }
  | { type: 'sor-reason'; reason: SorReason }
  | { type: 'sor-send' }
  | { type: 'sor-cancel' };

export interface KeyInput {
  /** `KeyboardEvent.code`: the key's place, not its letter. */
  code: string;
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  meta: boolean;
  /** The browser's auto-repeat while the key is held down. */
  repeat: boolean;
}

export interface KeyContext {
  tab: Tab;
  /** A text field (input, textarea, contenteditable) has focus. */
  inTextField: boolean;
  /** A modal dialog is open and owns the keyboard (rule 5). */
  dialog?: 'statement' | null;
}

/** How long an armed two-press key waits for its second press. */
export const ARM_MS = 3000;

const SLOT_ROWS = {
  on: ['Digit1', 'Digit2', 'Digit3', 'Digit4', 'Digit5', 'Digit6'],
  cue: ['KeyQ', 'KeyW', 'KeyE', 'KeyR', 'KeyT', 'KeyY'],
  talk: ['KeyA', 'KeyS', 'KeyD', 'KeyF', 'KeyG', 'KeyH'],
  mute: ['KeyZ', 'KeyX', 'KeyC', 'KeyV', 'KeyB', 'KeyN'],
} as const;

/** The cart wall's 24 keys, slot 1 to 24: the left hand's four rows (keymap section 4). */
export const CART_CODES: readonly string[] = [
  ...SLOT_ROWS.on,
  ...SLOT_ROWS.cue,
  ...SLOT_ROWS.talk,
  ...SLOT_ROWS.mute,
];

/** The printed key cap for cart slot 1 to 24. */
export function cartKey(slot: number): string {
  return (CART_CODES[slot - 1] ?? '').replace(/^(Digit|Key)/, '');
}

/** The printed key caps for a slot's four buttons (desk section 3). */
export function slotKeys(slot: number): { on: string; cue: string; talk: string; mute: string } {
  const cap = (code: string) => code.replace(/^(Digit|Key)/, '');
  const i = slot - 1;
  return {
    on: cap(SLOT_ROWS.on[i]),
    cue: cap(SLOT_ROWS.cue[i]),
    talk: cap(SLOT_ROWS.talk[i]),
    mute: cap(SLOT_ROWS.mute[i]),
  };
}

/** A two-press key's identity: the same chord pressed again within ARM_MS fires it. */
interface Armed {
  what: TwoPress;
  chord: string;
  at: number;
}

/** What a key press did: the action to run, if any, and whether the event was the keymap's
 *  (the caller then stops the browser doing its own thing with it, as `⌃D` bookmarks). */
export interface Outcome {
  action: Action | null;
  handled: boolean;
}

const NONE: Outcome = { action: null, handled: false };
const handled = (action: Action | null): Outcome => ({ action, handled: true });

function chordOf(k: KeyInput): string {
  return `${k.ctrl || k.meta ? 'C-' : ''}${k.alt ? 'A-' : ''}${k.shift ? 'S-' : ''}${k.code}`;
}

export class KeyEngine {
  private armed: Armed | null = null;
  /** Held keys by code, with the release each one owes. */
  private held = new Map<string, Action>();

  /** What is armed now (for the armed fill and its countdown), or null. */
  armedNow(now: number): TwoPress | null {
    if (this.armed && now - this.armed.at > ARM_MS) this.armed = null;
    return this.armed?.what ?? null;
  }

  disarm(): void {
    this.armed = null;
  }

  /** A two-press key: arm on the first press, fire on the second. */
  private twoPress(what: TwoPress, k: KeyInput, now: number, fire: Action): Outcome {
    const chord = chordOf(k);
    if (this.armed && this.armed.chord === chord && now - this.armed.at <= ARM_MS) {
      this.armed = null;
      return handled(fire);
    }
    this.armed = { what, chord, at: now };
    return handled({ type: 'arm', what });
  }

  private hold(k: KeyInput, press: Action, release: Action): Outcome {
    if (this.held.has(k.code)) return handled(null);
    this.held.set(k.code, release);
    return handled(press);
  }

  down(k: KeyInput, ctx: KeyContext, now: number): Outcome {
    const mod = k.ctrl || k.meta;
    // A key held down repeats; a repeat is never a new press (rule 7, and two-press keys).
    if (k.repeat) return this.held.has(k.code) || mod || this.armed ? handled(null) : NONE;
    if (k.code.startsWith('Shift') || k.code.startsWith('Control') || k.code.startsWith('Alt') || k.code.startsWith('Meta')) {
      return NONE;
    }

    if (k.code === 'Escape') {
      if (this.armed) {
        this.armed = null;
        return handled({ type: 'disarm' });
      }
      if (ctx.inTextField) return handled({ type: 'leave-field' });
      return handled(ctx.dialog ? { type: 'sor-cancel' } : { type: 'escape' });
    }

    // A modal dialog owns the keyboard (rule 5): nothing global fires behind it.
    if (ctx.dialog) {
      this.armed = null;
      return this.statement(k, ctx, mod);
    }

    const wasArmed = this.armed;
    const out = this.resolve(k, ctx, now, mod);
    // Any other key disarms a key that was armed (rule 4).
    if (wasArmed && this.armed === wasArmed && out.action?.type !== 'arm') this.armed = null;
    return out;
  }

  private resolve(k: KeyInput, ctx: KeyContext, now: number, mod: boolean): Outcome {
    // ---- the global layer: single modifier chords fire everywhere, even in a text field ----
    if (mod && !k.alt) {
      const digit = /^Digit([1-5])$/.exec(k.code);
      if (digit && !k.shift) return handled({ type: 'tab', tab: TABS[Number(digit[1]) - 1] });
      if (k.code === 'KeyR') {
        if (k.shift) return handled({ type: 'restart-encoder' });
        return handled({ type: 'restart-playout' });
      }
      if (k.code === 'KeyB' && k.shift) return handled({ type: 'backup-stream' });
      // ⌃⏎ posts as staff from the Room's composer: a chord, so it fires from the field.
      if ((k.code === 'Enter' || k.code === 'NumpadEnter') && !k.shift && ctx.tab === 'screener') {
        return handled({ type: 'post' });
      }
      if (k.code === 'KeyE' && k.shift) return handled({ type: 'end-show' });
      if (k.code === 'KeyT' && !k.shift) {
        return this.hold(k, { type: 'talk-seat', held: true }, { type: 'talk-seat', held: false });
      }
      if (k.code === 'KeyD' && !k.shift) {
        // A two-press chord: never from a text field (screener section 3).
        if (ctx.inTextField) return handled(null);
        return this.twoPress('drop', k, now, { type: 'drop' });
      }
      // ⌃F1 to ⌃F5: the cart bank, on every tab (nothing else uses it), but never from a text
      // field: F-keys there do nothing, chord or not (ADR-0104, as the operator decided).
      const bank = /^F([1-5])$/.exec(k.code);
      if (bank && !k.shift) return ctx.inTextField ? NONE : handled({ type: 'bank', n: Number(bank[1]) });
      return NONE;
    }
    if (mod || k.alt) return NONE;

    // ---- a text field owns every unmodified key (rule 3) ----
    if (ctx.inTextField) return NONE;

    // ---- the rest of the global layer ----
    const f = /^F([1-7])$/.exec(k.code);
    if (f && !k.shift) {
      const n = Number(f[1]);
      return handled(n === 7 ? { type: 'picture' } : { type: 'cart', n });
    }
    if (k.code === 'Delete' && !k.shift) return this.twoPress('dump', k, now, { type: 'dump' });

    // ---- the tab layer ----
    if (ctx.tab === 'desk') return this.desk(k);
    if (ctx.tab === 'screener') return this.screener(k, now);
    if (ctx.tab === 'carts') return this.carts(k, now);
    if (ctx.tab === 'schedule') return this.schedule(k);
    return NONE;
  }

  /** The schedule tab (calendar.md §5): `←` `→` the previous and next week, `T` today. Nothing
   *  here is live, so no key is two-press; the global layer (F1–F6, Del Del, ⌃D ⌃D) still fires. */
  private schedule(k: KeyInput): Outcome {
    if (k.shift) return NONE;
    switch (k.code) {
      case 'ArrowLeft':
        return handled({ type: 'week', delta: -1 });
      case 'ArrowRight':
        return handled({ type: 'week', delta: 1 });
      case 'KeyT':
        return handled({ type: 'today' });
      default:
        return NONE;
    }
  }

  private carts(k: KeyInput, now: number): Outcome {
    const slot = CART_CODES.indexOf(k.code) + 1;
    if (k.code === 'Space') {
      return k.shift ? this.twoPress('fade-all', k, now, { type: 'fade-all' }) : handled({ type: 'cart-stop-last' });
    }
    if (slot > 0) return handled({ type: k.shift ? 'cart-cue' : 'cart-key', slot });
    if (k.shift) return NONE;
    switch (k.code) {
      case 'ArrowLeft':
        return handled({ type: 'cart-move', dx: -1, dy: 0 });
      case 'ArrowRight':
        return handled({ type: 'cart-move', dx: 1, dy: 0 });
      case 'ArrowUp':
        return handled({ type: 'cart-move', dx: 0, dy: -1 });
      case 'ArrowDown':
        return handled({ type: 'cart-move', dx: 0, dy: 1 });
      case 'Enter':
      case 'NumpadEnter':
        return handled({ type: 'cart-enter' });
      default:
        return NONE;
    }
  }

  private desk(k: KeyInput): Outcome {
    if (k.shift) {
      return k.code === 'Enter' || k.code === 'NumpadEnter' ? handled({ type: 'handover' }) : NONE;
    }
    for (const [row, codes] of Object.entries(SLOT_ROWS)) {
      const i = (codes as readonly string[]).indexOf(k.code);
      if (i < 0) continue;
      const slot = i + 1;
      if (row === 'talk') {
        return this.hold(k, { type: 'talk', slot, held: true }, { type: 'talk', slot, held: false });
      }
      return handled({ type: row as 'on' | 'cue' | 'mute', slot });
    }
    return NONE;
  }

  /** The statement of reasons (keymap section 5): `1` `2` `3` the action, `A` `S` `T` `I` `M` `E`
   *  the reason, `⏎` hide and send. Tab, Space and the arrows stay the browser's, so the
   *  dialog's own radios and buttons work; every other key is swallowed. */
  private statement(k: KeyInput, ctx: KeyContext, mod: boolean): Outcome {
    if (mod && !k.alt && !k.shift && k.code === 'KeyT') {
      return this.hold(k, { type: 'talk-seat', held: true }, { type: 'talk-seat', held: false });
    }
    if (mod || k.alt) return handled(null);
    if (ctx.inTextField) return NONE;
    if (k.code === 'Tab' || k.code === 'Space' || k.code.startsWith('Arrow')) return NONE;
    if (k.shift) return handled(null);
    const digit = /^Digit([1-3])$/.exec(k.code);
    if (digit) return handled({ type: 'sor-then', then: SOR_THEN[Number(digit[1]) - 1] });
    const reason = SOR_REASON_CODES[k.code];
    if (reason) return handled({ type: 'sor-reason', reason });
    if (k.code === 'Enter' || k.code === 'NumpadEnter') return handled({ type: 'sor-send' });
    return handled(null);
  }

  private screener(k: KeyInput, now: number): Outcome {
    if (k.shift) {
      switch (k.code) {
        case 'KeyC':
          return handled({ type: 'release-place' });
        case 'KeyB':
          return this.twoPress('ban', k, now, { type: 'ban' });
        case 'KeyU':
          return handled({ type: 'release-all' });
        case 'KeyX':
          return handled({ type: 'hide-all' });
        case 'KeyE':
          return handled({ type: 'escalate' });
        case 'KeyP':
          return handled({ type: 'pin' });
        case 'KeyM':
          return handled({ type: 'pass' });
        default:
          return NONE;
      }
    }
    const digit = /^Digit([1-9])$/.exec(k.code);
    if (digit) return handled({ type: 'queue-jump', n: Number(digit[1]) });
    switch (k.code) {
      case 'ArrowUp':
        return handled({ type: 'queue-move', delta: -1 });
      case 'ArrowDown':
        return handled({ type: 'queue-move', delta: 1 });
      case 'KeyT':
        return this.hold(k, { type: 'screen-talk', held: true }, { type: 'screen-talk', held: false });
      case 'KeyP':
        return handled({ type: 'send' });
      case 'KeyW':
        return handled({ type: 'back' });
      case 'KeyN':
        return handled({ type: 'focus-name' });
      case 'KeyS':
        return handled({ type: 'mode', mode: 'slow' });
      case 'KeyK':
        return handled({ type: 'mode', mode: 'hold_new' });
      case 'KeyL':
        return handled({ type: 'mode', mode: 'links_trusted' });
      case 'KeyG':
        return this.twoPress('influx', k, now, { type: 'influx' });
      // the chat (M6): per-item actions, on letters no room-wide mode uses (rule 6)
      case 'KeyH':
        return handled({ type: 'mod-hide' });
      case 'KeyO':
        return handled({ type: 'mod-timeout' });
      case 'KeyD':
        return handled({ type: 'mod-leave' });
      case 'Enter':
      case 'NumpadEnter':
        return handled({ type: 'mod-release' });
      case 'KeyA':
        return handled({ type: 'acknowledge' });
      case 'KeyF':
        return handled({ type: 'feature' });
      case 'KeyI':
        return handled({ type: 'invite' });
      case 'KeyR':
        return handled({ type: 'reply' });
      case 'End':
        return handled({ type: 'newest' });
      default:
        return NONE;
    }
  }

  /** A key let go: the release a held key owes, if any. */
  up(k: Pick<KeyInput, 'code'>): Outcome {
    const release = this.held.get(k.code);
    if (!release) return NONE;
    this.held.delete(k.code);
    return handled(release);
  }

  /** The window lost focus (or the tab changed): let go of everything held, so no talk key
   *  stays open because its key-up went elsewhere. */
  releaseAll(): Action[] {
    const out = [...this.held.values()];
    this.held.clear();
    this.armed = null;
    return out;
  }
}
