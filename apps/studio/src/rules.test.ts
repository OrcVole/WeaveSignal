// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The rules the studio app keeps, tested without a page: the keymap (ADR-0063, keymap.md
// section 1), the state words and the master lamp (ADR-0062, ADR-0069, ADR-0079), and how
// every join page captures a voice.
import { describe, expect, it } from 'vitest';
import { ARM_MS, KeyEngine, cartKey, slotKeys, type KeyContext, type KeyInput } from '../../../packages/ui/keymap.ts';
import { ENDING_MS, cartNow, moveSlot, type Cart, type CartsView } from '../../../packages/ui/carts.ts';
import { CHIP, LAMP, callerChips, listenersHear, masterLamp, sourceChip, tipFor, type CallerFacts } from '../../../packages/ui/states.ts';
import { ECHO_TEST_CAPTURE, VOICE_CAPTURE } from '../../../packages/ui/media.ts';

const key = (code: string, mods: Partial<KeyInput> = {}): KeyInput => ({
  code,
  ctrl: false,
  shift: false,
  alt: false,
  meta: false,
  repeat: false,
  ...mods,
});
const desk: KeyContext = { tab: 'desk', inTextField: false };
const screener: KeyContext = { tab: 'screener', inTextField: false };
const typing = (tab: KeyContext['tab']): KeyContext => ({ tab, inTextField: true });

describe('the keymap', () => {
  it('maps the desk rows to slots by position', () => {
    const k = new KeyEngine();
    expect(k.down(key('Digit4'), desk, 0).action).toEqual({ type: 'on', slot: 4 });
    expect(k.down(key('KeyR'), desk, 0).action).toEqual({ type: 'cue', slot: 4 });
    expect(k.down(key('KeyV'), desk, 0).action).toEqual({ type: 'mute', slot: 4 });
    expect(k.down(key('KeyY'), desk, 0).action).toEqual({ type: 'cue', slot: 6 });
    expect(slotKeys(3)).toEqual({ on: '3', cue: 'E', talk: 'D', mute: 'C' });
  });

  it('keeps tab letters on their own tab: T is slot 5 cue on the desk, talk on the screener', () => {
    const k = new KeyEngine();
    expect(k.down(key('KeyT'), desk, 0).action).toEqual({ type: 'cue', slot: 5 });
    expect(k.down(key('KeyT'), screener, 0).action).toEqual({ type: 'screen-talk', held: true });
    expect(k.down(key('Digit4'), { tab: 'carts', inTextField: false }, 0).action).toEqual({ type: 'cart-key', slot: 4 });
  });

  it('never fires an unmodified, shifted or two-press key in a text field', () => {
    const k = new KeyEngine();
    for (const [code, mods] of [
      ['Digit1', {}],
      ['KeyP', {}],
      ['KeyG', {}],
      ['KeyC', { shift: true }],
      ['Delete', {}],
      ['F1', {}],
      ['ArrowDown', {}],
      ['KeyH', {}],
      ['KeyO', {}],
      ['KeyD', {}],
      ['Enter', {}],
      ['End', {}],
      ['KeyB', { shift: true }],
      ['KeyX', { shift: true }],
      ['KeyM', { shift: true }],
    ] as const) {
      for (const tab of ['desk', 'screener'] as const) {
        const out = k.down(key(code, mods), typing(tab), 0);
        expect(out.action, `${code} on ${tab}`).toBeNull();
        expect(out.handled, `${code} on ${tab} must reach the field`).toBe(false);
      }
    }
    // ⌃D ⌃D is a two-press chord: not even the first press arms from a field.
    expect(k.down(key('KeyD', { ctrl: true }), typing('screener'), 0).action).toBeNull();
    expect(k.down(key('KeyD', { ctrl: true }), typing('screener'), 10).action).toBeNull();
  });

  it('still fires single modifier chords in a text field, and Esc leaves the field', () => {
    const k = new KeyEngine();
    expect(k.down(key('Digit2', { ctrl: true }), typing('desk'), 0).action).toEqual({ type: 'tab', tab: 'screener' });
    expect(k.down(key('KeyR', { ctrl: true }), typing('screener'), 0).action).toEqual({ type: 'restart-playout' });
    expect(k.down(key('KeyT', { ctrl: true }), typing('screener'), 0).action).toEqual({ type: 'talk-seat', held: true });
    expect(k.down(key('Escape'), typing('screener'), 0).action).toEqual({ type: 'leave-field' });
  });

  it('arms a two-press key on the first press and fires on the second within 3 s', () => {
    const k = new KeyEngine();
    const ctrlD = key('KeyD', { ctrl: true });
    expect(k.down(ctrlD, desk, 0).action).toEqual({ type: 'arm', what: 'drop' });
    expect(k.armedNow(1000)).toBe('drop');
    expect(k.down(ctrlD, desk, 2000).action).toEqual({ type: 'drop' });
    expect(k.armedNow(2001)).toBeNull();
    // Too late: the second press arms again instead.
    expect(k.down(key('KeyG'), screener, 0).action).toEqual({ type: 'arm', what: 'influx' });
    expect(k.down(key('KeyG'), screener, ARM_MS + 1).action).toEqual({ type: 'arm', what: 'influx' });
    expect(k.down(key('KeyG'), screener, ARM_MS + 2).action).toEqual({ type: 'influx' });
    expect(k.down(key('Delete'), desk, 0).action).toEqual({ type: 'arm', what: 'dump' });
    expect(k.down(key('Delete'), desk, 1).action).toEqual({ type: 'dump' });
  });

  it('disarms on Esc or any other key', () => {
    const k = new KeyEngine();
    k.down(key('KeyD', { ctrl: true }), desk, 0);
    expect(k.down(key('Escape'), desk, 1).action).toEqual({ type: 'disarm' });
    expect(k.down(key('KeyD', { ctrl: true }), desk, 2).action).toEqual({ type: 'arm', what: 'drop' });
    expect(k.down(key('Digit1'), desk, 3).action).toEqual({ type: 'on', slot: 1 });
    expect(k.armedNow(4)).toBeNull();
    expect(k.down(key('KeyD', { ctrl: true }), desk, 5).action).toEqual({ type: 'arm', what: 'drop' });
  });

  it('holds talk while the key is down, ignores repeats, and lets go on blur', () => {
    const k = new KeyEngine();
    expect(k.down(key('KeyF'), desk, 0).action).toEqual({ type: 'talk', slot: 4, held: true });
    expect(k.down(key('KeyF', { repeat: true }), desk, 50).action).toBeNull();
    expect(k.up(key('KeyF')).action).toEqual({ type: 'talk', slot: 4, held: false });
    expect(k.up(key('KeyF')).action).toBeNull();
    k.down(key('KeyT', { ctrl: true }), desk, 0);
    k.down(key('KeyA'), desk, 0);
    expect(k.releaseAll()).toEqual([
      { type: 'talk-seat', held: false },
      { type: 'talk', slot: 1, held: false },
    ]);
  });

  it('gives the screener its keys and the desk its handover', () => {
    const k = new KeyEngine();
    expect(k.down(key('KeyP'), screener, 0).action).toEqual({ type: 'send' });
    expect(k.down(key('KeyW'), screener, 0).action).toEqual({ type: 'back' });
    expect(k.down(key('Digit3'), screener, 0).action).toEqual({ type: 'queue-jump', n: 3 });
    expect(k.down(key('ArrowUp'), screener, 0).action).toEqual({ type: 'queue-move', delta: -1 });
    expect(k.down(key('KeyC', { shift: true }), screener, 0).action).toEqual({ type: 'release-place' });
    expect(k.down(key('KeyK'), screener, 0).action).toEqual({ type: 'mode', mode: 'hold_new' });
    expect(k.down(key('Enter', { shift: true }), desk, 0).action).toEqual({ type: 'handover' });
    expect(k.down(key('Enter', { shift: true }), screener, 0).handled).toBe(false);
    expect(k.down(key('F7'), screener, 0).action).toEqual({ type: 'picture' });
    expect(k.down(key('F3'), desk, 0).action).toEqual({ type: 'cart', n: 3 });
    expect(k.down(key('KeyE', { ctrl: true, shift: true }), desk, 0).action).toEqual({ type: 'end-show' });
  });
});

describe('the chat keys on the screener (keymap section 5, M6)', () => {
  const sor: KeyContext = { tab: 'screener', inTextField: false, dialog: 'statement' };

  it('gives each moderation action its letter, keeping the caller keys as they were', () => {
    const k = new KeyEngine();
    const at = (code: string, mods: Partial<KeyInput> = {}) => k.down(key(code, mods), screener, 0).action;
    expect(at('KeyH')).toEqual({ type: 'mod-hide' });
    expect(at('KeyO')).toEqual({ type: 'mod-timeout' });
    expect(at('KeyD')).toEqual({ type: 'mod-leave' });
    expect(at('Enter')).toEqual({ type: 'mod-release' });
    expect(at('KeyU', { shift: true })).toEqual({ type: 'release-all' });
    expect(at('KeyX', { shift: true })).toEqual({ type: 'hide-all' });
    expect(at('KeyA')).toEqual({ type: 'acknowledge' });
    expect(at('KeyE', { shift: true })).toEqual({ type: 'escalate' });
    expect(at('KeyF')).toEqual({ type: 'feature' });
    expect(at('KeyP', { shift: true })).toEqual({ type: 'pin' });
    expect(at('KeyI')).toEqual({ type: 'invite' });
    expect(at('KeyR')).toEqual({ type: 'reply' });
    expect(at('KeyM', { shift: true })).toEqual({ type: 'pass' });
    expect(at('End')).toEqual({ type: 'newest' });
    // The caller keys keep their meaning beside them.
    expect(at('KeyP')).toEqual({ type: 'send' });
    expect(at('KeyS')).toEqual({ type: 'mode', mode: 'slow' });
    expect(at('KeyC', { shift: true })).toEqual({ type: 'release-place' });
    // ⇧⏎ stays the desk's handover, and nothing on the screener.
    expect(k.down(key('Enter', { shift: true }), screener, 0).handled).toBe(false);
    // The same letters mean the desk's strips on the Desk tab.
    expect(k.down(key('KeyH'), desk, 0).action).toEqual({ type: 'talk', slot: 6, held: true });
  });

  it('bans only on a second ⇧B within 3 s (rule 4), and any other key disarms it', () => {
    const k = new KeyEngine();
    const shiftB = key('KeyB', { shift: true });
    expect(k.down(shiftB, screener, 0).action).toEqual({ type: 'arm', what: 'ban' });
    expect(k.armedNow(100)).toBe('ban');
    expect(k.down(shiftB, screener, 2000).action).toEqual({ type: 'ban' });
    k.down(shiftB, screener, 10_000);
    expect(k.down(key('KeyH'), screener, 10_100).action).toEqual({ type: 'mod-hide' });
    expect(k.down(shiftB, screener, 10_200).action).toEqual({ type: 'arm', what: 'ban' });
    expect(k.down(shiftB, screener, 10_200 + ARM_MS + 1).action).toEqual({ type: 'arm', what: 'ban' });
    // ⌃⇧B is still the backup stream, on every tab.
    expect(k.down(key('KeyB', { ctrl: true, shift: true }), screener, 0).action).toEqual({ type: 'backup-stream' });
  });

  it('posts as staff with ⌃⏎ from the composer, and only on the screener', () => {
    const k = new KeyEngine();
    expect(k.down(key('Enter', { ctrl: true }), typing('screener'), 0).action).toEqual({ type: 'post' });
    expect(k.down(key('NumpadEnter', { ctrl: true }), screener, 0).action).toEqual({ type: 'post' });
    expect(k.down(key('Enter', { ctrl: true }), desk, 0).handled).toBe(false);
  });

  it('lets the statement dialog own the keyboard (rule 5)', () => {
    const k = new KeyEngine();
    const at = (code: string, mods: Partial<KeyInput> = {}) => k.down(key(code, mods), sor, 0);
    expect(at('Digit2').action).toEqual({ type: 'sor-then', then: 'timeout' });
    expect(at('Digit3').action).toEqual({ type: 'sor-then', then: 'ban' });
    expect(at('Digit1').action).toEqual({ type: 'sor-then', then: 'none' });
    expect(at('KeyA').action).toEqual({ type: 'sor-reason', reason: 'abuse' });
    expect(at('KeyS').action).toEqual({ type: 'sor-reason', reason: 'spam' });
    expect(at('KeyT').action).toEqual({ type: 'sor-reason', reason: 'off_topic' });
    expect(at('KeyI').action).toEqual({ type: 'sor-reason', reason: 'illegal' });
    expect(at('KeyM').action).toEqual({ type: 'sor-reason', reason: 'intimate_image' });
    expect(at('KeyE').action).toEqual({ type: 'sor-reason', reason: 'other' });
    // ADR-0257: C is the reason for a picture suspected of being child sexual abuse material (the
    // dialog accepts it for a picture only).
    expect(at('KeyC').action).toEqual({ type: 'sor-reason', reason: 'suspected_csam' });
    expect(at('Enter').action).toEqual({ type: 'sor-send' });
    expect(at('Escape').action).toEqual({ type: 'sor-cancel' });
    // Nothing behind it leaks: not the screener's letters, not a cart, not a dump or a drop.
    for (const [code, mods] of [
      ['KeyH', {}],
      ['KeyP', {}],
      ['KeyG', {}],
      ['Digit5', {}],
      ['F1', {}],
      ['Delete', {}],
      ['KeyB', { shift: true }],
      ['KeyD', { ctrl: true }],
      ['Digit1', { ctrl: true }],
      ['Enter', { ctrl: true }],
    ] as const) {
      const out = at(code, mods);
      expect(out.action, `${code} in the dialog`).toBeNull();
      expect(out.handled, `${code} is the dialog's`).toBe(true);
    }
    expect(k.armedNow(1)).toBeNull();
    // Tab, Space and the arrows stay the browser's, for the dialog's own controls.
    for (const code of ['Tab', 'Space', 'ArrowDown']) expect(at(code).handled, code).toBe(false);
    // Talkback still works while it is open.
    expect(at('KeyT', { ctrl: true }).action).toEqual({ type: 'talk-seat', held: true });
    expect(k.up(key('KeyT')).action).toEqual({ type: 'talk-seat', held: false });
  });

  it('types in the dialog note field; the first Esc leaves the field, the second closes', () => {
    const k = new KeyEngine();
    const field: KeyContext = { ...sor, inTextField: true };
    expect(k.down(key('KeyA'), field, 0).handled).toBe(false);
    expect(k.down(key('Digit2'), field, 0).handled).toBe(false);
    expect(k.down(key('Escape'), field, 0).action).toEqual({ type: 'leave-field' });
    expect(k.down(key('Escape'), sor, 0).action).toEqual({ type: 'sor-cancel' });
  });
});

describe('the master lamp and the state words', () => {
  const up = { deskReachable: true, playoutConnected: true };

  it('is ON AIR only while listeners can hear an open source', () => {
    expect(masterLamp({ ...up, anyOnAir: true })).toBe('on-air');
    expect(masterLamp({ ...up, anyOnAir: false })).toBe('standby');
  });

  it('is PLAYOUT DOWN while playout is gone, whoever is open, and never OPEN', () => {
    expect(masterLamp({ deskReachable: true, playoutConnected: false, anyOnAir: true })).toBe('playout-down');
    expect(masterLamp({ deskReachable: true, playoutConnected: false, anyOnAir: false })).toBe('playout-down');
  });

  it('is a fault when the desk does not answer', () => {
    expect(masterLamp({ deskReachable: false, playoutConnected: true, anyOnAir: true })).toBe('fault');
  });

  it('says OPEN, the white lamp, on a source the room hears while playout is down', () => {
    const on = { on_air: true, mute: false, cue: false };
    expect(sourceChip(on, true)).toBe(CHIP.onAir);
    expect(sourceChip(on, false)).toBe(CHIP.open);
    expect(CHIP.open.cls).toBe('fill-open');
    expect(sourceChip({ ...on, mute: true }, true)).toBe(CHIP.muted);
    expect(sourceChip({ on_air: false, mute: false, cue: true }, false)).toBe(CHIP.cue);
    expect(sourceChip({ on_air: false, mute: false, cue: false, ready: true }, true)).toBe(CHIP.ready);
    expect(sourceChip({ on_air: false, mute: false, cue: false }, true)).toBe(CHIP.standby);
  });

  it('marks a caller DESK BUSY and BANNED BEFORE beside the state, never in place of it', () => {
    const base: CallerFacts = {
      state: 'ready',
      on_air: false,
      cue: false,
      present: true,
      linked: true,
      dropped_ms: null,
      declined: { at_ms: 1, reason: 'in a two-way' },
      banned_before: { date: '2026-09-12', reason: 'Abuse on air' },
    };
    expect(callerChips(base, true).map((c) => c.word)).toEqual(['Ready', 'Desk busy', 'Banned before']);
    expect(CHIP.deskBusy.cls).toContain('hollow-warning');
    expect(CHIP.bannedBefore.cls).toContain('hollow-serious');
    const dropped = { ...base, declined: null, banned_before: null, dropped_ms: 5 };
    expect(callerChips(dropped, true).map((c) => c.word)).toEqual(['Dropped']);
    const screening = { ...dropped, dropped_ms: null, state: 'screening' as const, linked: false };
    expect(callerChips(screening, true).map((c) => c.word)).toEqual(['Screening', 'Link degraded']);
  });
});

describe('voice capture on join pages', () => {
  it('cancels echo and suppresses noise, with the browser gain control off', () => {
    expect(VOICE_CAPTURE).toEqual({ echoCancellation: true, noiseSuppression: true, autoGainControl: false });
    expect(ECHO_TEST_CAPTURE.echoCancellation).toBe(false);
    expect(ECHO_TEST_CAPTURE.autoGainControl).toBe(false);
    expect(Object.isFrozen(VOICE_CAPTURE)).toBe(true);
  });
});

const carts: KeyContext = { tab: 'carts', inTextField: false };

describe('the cart wall keys (keymap section 4)', () => {
  it('maps the four rows to slots 1 to 24, and only on the Carts tab', () => {
    const k = new KeyEngine();
    expect(k.down(key('Digit1'), carts, 0).action).toEqual({ type: 'cart-key', slot: 1 });
    expect(k.down(key('KeyY'), carts, 0).action).toEqual({ type: 'cart-key', slot: 12 });
    expect(k.down(key('KeyA'), carts, 0).action).toEqual({ type: 'cart-key', slot: 13 });
    expect(k.down(key('KeyN'), carts, 0).action).toEqual({ type: 'cart-key', slot: 24 });
    expect([1, 7, 13, 19, 24].map(cartKey)).toEqual(['1', 'Q', 'A', 'Z', 'N']);
    // The same letters are the desk's strips on the Desk tab, and nothing on the screener's 7.
    expect(k.down(key('KeyA'), desk, 0).action).toEqual({ type: 'talk', slot: 1, held: true });
    k.up({ code: 'KeyA' });
    expect(k.down(key('KeyU'), carts, 0).handled).toBe(false);
  });

  it('fires the top row with F1 to F6 from any tab, never from a text field', () => {
    const k = new KeyEngine();
    for (const ctx of [desk, screener, carts]) {
      expect(k.down(key('F1'), ctx, 0).action).toEqual({ type: 'cart', n: 1 });
      expect(k.down(key('F6'), ctx, 0).action).toEqual({ type: 'cart', n: 6 });
    }
    expect(k.down(key('F7'), carts, 0).action).toEqual({ type: 'picture' });
    expect(k.down(key('F2'), typing('carts'), 0).handled).toBe(false);
    expect(k.down(key('Digit2'), typing('carts'), 0).handled).toBe(false);
  });

  it('switches banks with ⌃F1 to ⌃F5 on every tab, and never from a text field (ADR-0104)', () => {
    const k = new KeyEngine();
    for (const ctx of [desk, screener, carts]) {
      expect(k.down(key('F3', { ctrl: true }), ctx, 0).action).toEqual({ type: 'bank', n: 3 });
    }
    expect(k.down(key('F6', { ctrl: true }), carts, 0).handled).toBe(false);
  });

  it('does nothing with ⌃F1 to ⌃F5 or Space while a text field has focus, on any tab (ADR-0104)', () => {
    const k = new KeyEngine();
    for (const tab of ['desk', 'screener', 'carts'] as const) {
      for (const n of [1, 2, 3, 4, 5]) {
        const out = k.down(key(`F${n}`, { ctrl: true }), typing(tab), 0);
        expect(out.action, `⌃F${n} on ${tab}`).toBeNull();
        expect(out.handled, `⌃F${n} on ${tab} reaches the field`).toBe(false);
      }
      const space = k.down(key('Space'), typing(tab), 0);
      expect(space.action, `Space on ${tab}`).toBeNull();
      expect(space.handled, `Space on ${tab} types a space`).toBe(false);
    }
    // Out of the field, they work as before.
    expect(k.down(key('Space'), carts, 0).action).toEqual({ type: 'cart-stop-last' });
    expect(k.down(key('F5', { ctrl: true }), screener, 0).action).toEqual({ type: 'bank', n: 5 });
  });

  it('cues with ⇧ and the key, stops the last with Space, fades all only on a second ⇧Space', () => {
    const k = new KeyEngine();
    expect(k.down(key('KeyW', { shift: true }), carts, 0).action).toEqual({ type: 'cart-cue', slot: 8 });
    expect(k.down(key('Space'), carts, 0).action).toEqual({ type: 'cart-stop-last' });
    expect(k.down(key('Space', { shift: true }), carts, 0).action).toEqual({ type: 'arm', what: 'fade-all' });
    expect(k.armedNow(10)).toBe('fade-all');
    expect(k.down(key('Space', { shift: true }), carts, 1000).action).toEqual({ type: 'fade-all' });
    // Too late, or another key between: it arms again rather than firing.
    k.down(key('Space', { shift: true }), carts, 0);
    expect(k.down(key('Space', { shift: true }), carts, ARM_MS + 1).action).toEqual({ type: 'arm', what: 'fade-all' });
    k.down(key('Space', { shift: true }), carts, 10_000);
    k.down(key('Digit1'), carts, 10_100);
    expect(k.down(key('Space', { shift: true }), carts, 10_200).action).toEqual({ type: 'arm', what: 'fade-all' });
    // Never from a text field.
    expect(k.down(key('Space', { shift: true }), typing('carts'), 0).handled).toBe(false);
  });

  it('moves between carts with the arrows without firing, and fires with Enter', () => {
    const k = new KeyEngine();
    expect(k.down(key('ArrowRight'), carts, 0).action).toEqual({ type: 'cart-move', dx: 1, dy: 0 });
    expect(k.down(key('Enter'), carts, 0).action).toEqual({ type: 'cart-enter' });
    expect(moveSlot(1, -1, 0)).toBe(1);
    expect(moveSlot(1, 1, 1)).toBe(8);
    expect(moveSlot(24, 1, 1)).toBe(24);
    expect(moveSlot(6, 0, 3)).toBe(24);
  });
});

describe('the cart states (cart-wall.md section 3)', () => {
  const cart = (over: Partial<Cart> = {}): Cart => ({
    id: 'c1',
    bank: 1,
    slot: 1,
    label: 'Applause',
    detail: '',
    mode: 'one_shot',
    file: 'f.flac',
    duration_ms: 12_000,
    gain_db: -7,
    loudness_lufs: -16,
    true_peak_dbtp: -1.5,
    ramp_ms: null,
    origin: '',
    licence: '',
    missing: false,
    ...over,
  });
  const view = (over: Partial<CartsView> = {}): CartsView => ({
    version: 1,
    desk: true,
    desk_error: null,
    sounding: [],
    ducked: false,
    mic_open: false,
    missing: [],
    importing: null,
    ...over,
  });
  const sounding = (ends: number | null, stopping = false, cue = false) => ({
    cart: 'c1',
    pool: 'shot' as const,
    player: 1,
    mode: 'one_shot' as const,
    cue,
    play: 1,
    started_ms: 0,
    ends_ms: ends,
    stopping,
    seq: 1,
  });

  it('says EMPTY, idle, playing, ENDING under 5 s, and fading', () => {
    expect(cartNow(null, view(), 0).state).toBe('empty');
    expect(cartNow(cart(), view(), 0).state).toBe('idle');
    const v = view({ sounding: [sounding(12_000)] });
    expect(cartNow(cart(), v, 2_000)).toMatchObject({ state: 'playing', leftMs: 10_000 });
    expect(cartNow(cart(), v, 12_000 - ENDING_MS + 1).state).toBe('ending');
    expect(cartNow(cart(), view({ sounding: [sounding(12_000, true)] }), 2_000).state).toBe('stopping');
    // A bed has no end and its ring stays full.
    expect(cartNow(cart({ mode: 'bed' }), view({ sounding: [sounding(null)] }), 99_000)).toMatchObject({
      state: 'playing',
      leftMs: null,
      progress: 1,
    });
  });

  it('says CUE for a cart in the headphones, never ENDING, and fading when it goes', () => {
    const v = view({ sounding: [sounding(12_000, false, true)] });
    expect(cartNow(cart(), v, 2_000)).toMatchObject({ state: 'cued', leftMs: 10_000 });
    expect(cartNow(cart(), v, 11_000).state).toBe('cued');
    expect(cartNow(cart({ mode: 'bed' }), view({ sounding: [sounding(null, false, true)] }), 9_000).state).toBe('cued');
    expect(cartNow(cart(), view({ sounding: [sounding(12_000, true, true)] }), 2_000).state).toBe('stopping');
    // Fired on air from the cue: the newest sounding wins.
    const both = view({ sounding: [sounding(12_000, true, true), { ...sounding(20_000), play: 2, seq: 2 }] });
    expect(cartNow(cart(), both, 2_000).state).toBe('playing');
  });

  it('says FILE MISSING without a file, from the catalogue or the live view', () => {
    expect(cartNow(cart({ file: null }), view(), 0).state).toBe('missing');
    expect(cartNow(cart({ missing: true }), view(), 0).state).toBe('missing');
    expect(cartNow(cart(), view({ missing: ['c1'] }), 0).state).toBe('missing');
  });
});

import { delayParts, delayWords, listenerDelayMs } from './lib/format.ts';

describe('the listener delay (ADR-0103, ADR-0272)', () => {
  it('is an estimate in words until the whole path is read, and a plain figure after', () => {
    expect(delayWords({ ms: 15_000, measured: false })).toBe('about 15 s behind (an estimate)');
    expect(delayWords({ ms: 9_000, measured: true })).toBe('9 s behind');
    expect(listenerDelayMs(null)).toBe(15_000);
    expect(listenerDelayMs({ ms: 12_000 })).toBe(12_000);
  });

  it('names what is read live and what is a setting', () => {
    const d = { ms: 26_000, measured: false, desk_ms: 8_000, process_ms: 3_200, rest_ms: 14_500 };
    expect(delayParts(d)).toBe(
      "Read now: 8.0 s profanity delay, 3.2 s in the listener process; about 14.5 s more is set, not read: the stream's own hold and each listener's player",
    );
    // The listener process does not answer: only the desk is read.
    expect(delayParts({ ...d, process_ms: null, rest_ms: 15_000 })).toBe(
      "Read now: 8.0 s profanity delay; about 15.0 s more is set, not read: the stream's own hold and each listener's player",
    );
    // A control plane too old to split it says nothing rather than guess.
    expect(delayParts({ ms: 15_000, measured: false })).toBe('');
    expect(delayParts(null)).toBe('');
  });
});

describe('AUTO (ADR-0183)', () => {
  type M = 'live' | 'auto' | null | undefined;
  const lamp = (deskReachable: boolean, playoutConnected: boolean, anyOnAir: boolean, mode: M) =>
    masterLamp({ deskReachable, playoutConnected, anyOnAir, mode });

  it('reads as live while the mode is unknown, so nothing changes before the setting is read', () => {
    for (const mode of [undefined, null, 'live'] as const) {
      expect(lamp(true, true, true, mode)).toBe('on-air');
      expect(lamp(true, true, false, mode)).toBe('standby');
      expect(lamp(true, false, true, mode)).toBe('playout-down');
      expect(lamp(false, true, true, mode)).toBe('fault');
    }
  });

  it('is AUTO in auto mode whoever is open and whether or not playout is connected', () => {
    for (const playout of [true, false]) for (const open of [true, false]) expect(lamp(true, playout, open, 'auto')).toBe('auto');
  });

  it('keeps a desk that does not answer a fault, whatever the mode', () => {
    expect(lamp(false, true, true, 'auto')).toBe('fault');
    expect(lamp(false, false, false, 'auto')).toBe('fault');
  });

  it('moves from each state to AUTO and back as the mode changes', () => {
    const from: [boolean, boolean, boolean, string][] = [
      [true, true, true, 'on-air'],
      [true, true, false, 'standby'],
      [true, false, true, 'playout-down'],
      [true, false, false, 'playout-down'],
    ];
    for (const [r, p, o, live] of from) {
      expect(lamp(r, p, o, 'live')).toBe(live);
      expect(lamp(r, p, o, 'auto')).toBe('auto');
      expect(lamp(r, p, o, 'live')).toBe(live);
    }
  });

  it('is hollow, in the mode colour, with a glyph of its own', () => {
    expect(LAMP.auto.filled).toBe(false);
    expect(LAMP.auto.chip).toBe(CHIP.auto);
    expect(CHIP.auto).toEqual({ cls: 'hollow hollow-auto', glyph: 'g-auto', glyphCls: 'g-auto', word: 'Auto' });
    expect(LAMP['on-air'].filled).toBe(true);
    const glyphs = Object.values(CHIP).filter((c) => c !== CHIP.auto).map((c) => c.glyph);
    expect(glyphs).not.toContain('g-auto');
  });

  it('says OPEN, the white lamp, on an open source in auto mode, playout up or down', () => {
    const on = { on_air: true, mute: false, cue: false };
    expect(sourceChip(on, true, 'auto')).toBe(CHIP.open);
    expect(sourceChip(on, false, 'auto')).toBe(CHIP.open);
    expect(sourceChip(on, true, 'live')).toBe(CHIP.onAir);
    expect(sourceChip(on, true, null)).toBe(CHIP.onAir);
    expect(sourceChip(on, true)).toBe(CHIP.onAir);
    // The other words do not depend on the mode.
    expect(sourceChip({ ...on, mute: true }, true, 'auto')).toBe(CHIP.muted);
    expect(sourceChip({ on_air: false, mute: false, cue: true }, true, 'auto')).toBe(CHIP.cue);
    expect(sourceChip({ on_air: false, mute: false, cue: false }, true, 'auto')).toBe(CHIP.standby);
  });

  it('says OPEN on a caller who is on air in auto mode', () => {
    const c: CallerFacts = {
      state: 'ready', on_air: true, cue: false, present: true, linked: true, dropped_ms: null, declined: null, banned_before: null,
    };
    expect(callerChips(c, true, 'auto').map((x) => x.word)).toEqual(['Open']);
    expect(callerChips(c, true, 'live').map((x) => x.word)).toEqual(['On air']);
    expect(callerChips(c, false).map((x) => x.word)).toEqual(['Open']);
    expect(callerChips({ ...c, on_air: false }, true, 'auto').map((x) => x.word)).toEqual(['Ready']);
  });

  it('listeners hear the room only with playout up and the desk in charge', () => {
    expect(listenersHear(true, 'live')).toBe(true);
    expect(listenersHear(true, null)).toBe(true);
    expect(listenersHear(true, 'auto')).toBe(false);
    expect(listenersHear(false, 'live')).toBe(false);
  });
});

describe('the tooltip sentences', () => {
  it('carry the fifteen approved words, and none of the words the design has no sentence for', () => {
    const words = [CHIP.onAir, CHIP.open, CHIP.auto, CHIP.cue, CHIP.talk, CHIP.muted, CHIP.standby, CHIP.allGood, CHIP.fault,
      CHIP.playoutDown, CHIP.linkDegraded, CHIP.delay, CHIP.dumped, CHIP.deskBusy, CHIP.bannedBefore];
    expect(words.length).toBe(15);
    for (const c of words) expect(tipFor(c), c.word).toMatch(/\S.*[.]$/);
    expect(new Set(words.map((c) => tipFor(c))).size).toBe(15);
    for (const c of [CHIP.ready, CHIP.waiting, CHIP.screening, CHIP.dropped, CHIP.defaultLoop]) expect(tipFor(c), c.word).toBeNull();
  });

  it('keeps the design system text', () => {
    expect(tipFor(CHIP.auto)).toBe(
      'Automation is playing the log and the desk never reaches listeners; pressing On opens you to the room only, and you leave auto mode before you go on air.',
    );
    expect(tipFor(CHIP.standby)).toBe('Nothing is open and playout holds the air; the next On key puts you on air at once.');
  });
});
