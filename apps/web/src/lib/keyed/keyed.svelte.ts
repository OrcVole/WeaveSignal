// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A listener's key on this device (keyed shows, 1.3.2): the key kept in the browser's storage
// for next time, the token it earns kept in memory only and renewed while the page lives, the
// devices that hold the key, and what the station said when the key did not work. The page
// asks for nothing but the key: no account, no password.
//
// Browser storage can be refused (private windows, blocked site data): every read and write is
// guarded, and without it the key lasts as long as the page.

import * as routes from './routes.ts';
import { Refusal, type Device, type KeyedShows, type ListenToken } from './routes.ts';

const STORE = 'weavesignal.key';

/** A key as this device keeps it: its text and this browser's id on it. Not its name: that is
 *  the one staff gave the key to find it again, which the page never shows and so does not keep
 *  (a name kept by an earlier build is dropped the next time the key is written). */
export interface KeptKey {
  text: string;
  device: string | null;
}

export function readKept(raw?: string | null): KeptKey | null {
  let text = raw;
  if (text === undefined) {
    try {
      text = localStorage.getItem(STORE);
    } catch {
      return null;
    }
  }
  if (!text) return null;
  try {
    const k = JSON.parse(text) as KeptKey;
    return typeof k.text === 'string' && k.text ? { text: k.text, device: typeof k.device === 'string' ? k.device : null } : null;
  } catch {
    return null;
  }
}

function writeKept(k: KeptKey | null): void {
  try {
    if (k) localStorage.setItem(STORE, JSON.stringify(k));
    else localStorage.removeItem(STORE);
  } catch {
    // Refused: the key lasts as long as the page.
  }
}

/** The codes after which the key is not worth keeping: it will never work again as it is. */
const FORGET: readonly string[] = ['not_recognised', 'ended', 'revoked'];

export class Keyed {
  /** The station's keyed shows; null while unread, and from a station that has none of this. */
  shows = $state<KeyedShows | null>(null);
  /** Whether the keyed shows have been asked for at least once. */
  asked = $state(false);
  kept = $state<KeptKey | null>(null);
  /** The key as the station accepted it, with its token; null without one. */
  session = $state<ListenToken | null>(null);
  /** Why the key does not work, as the station said. */
  refusal = $state<Refusal | null>(null);
  /** The show the listener wanted when the key was refused (id or slug). */
  refusedFor = $state<string | null>(null);
  busy = $state(false);
  /** The key was accepted just now from a link or the box: the welcome is new. */
  fresh = $state(false);
  /** The devices that hold the key, once asked for. */
  devices = $state<Device[] | null>(null);
  /** How long a device holds its place on the key, as the station said (seconds). */
  holdS = $state<number | null>(null);
  devicesProblem = $state(false);

  /** The show the token is for (id), so a change of show asks again. */
  #show: string | null = null;
  #timer: ReturnType<typeof setTimeout> | null = null;
  #stopped = false;
  /** Told when a key that was working stops: the player stops the keyed stream and says why. */
  onCut: (() => void) | null = null;

  constructor(kept: KeptKey | null = readKept()) {
    this.kept = kept;
  }

  /** The keyed shows, read again: which shows need a key now. */
  async loadShows(): Promise<void> {
    this.shows = await routes.keyedShows();
    this.asked = true;
  }

  /** Use a key: from the link, the box, or the one kept. `show` is the keyed show that is on
   *  (its id), so a key that does not open it is told so. True when the station accepted it. */
  async enter(text: string, show: string | null = null, from: 'typed' | 'kept' = 'typed'): Promise<boolean> {
    const key = text.trim();
    if (!key || this.busy) return false;
    this.busy = true;
    const device = this.kept?.text === key ? this.kept.device : null;
    try {
      const s = await routes.redeem(key, { show, device });
      this.#accept(key, s, show);
      this.fresh = from === 'typed';
      return true;
    } catch (e) {
      const r = e instanceof Refusal ? e : new Refusal('no_answer', 0);
      this.#refuse(key, r, show);
      return false;
    } finally {
      this.busy = false;
    }
  }

  /** The kept key, used again as the page opens or the show changes. */
  resume(show: string | null = null): Promise<boolean> {
    if (!this.kept) return Promise.resolve(false);
    return this.enter(this.kept.text, show, 'kept');
  }

  /** The keyed show on now changed: a token is for one show's window, so a key held for another
   *  show is asked again for this one. When no keyed show is on, the token held stays: it is
   *  for its show's next window, and renewing moves it on. */
  async follow(show: string | null): Promise<void> {
    if (!this.kept || this.busy || !show) return;
    if (this.session ? this.session.show.id === show : show === this.#show) return;
    if (!this.session && !this.refusal) return;
    await this.resume(show);
  }

  /** Ask the station now whether the key still opens its show (a keyed stream stopped, which
   *  the station does at once when a key is revoked or paused). True while the key works; when
   *  it does not, the refusal is said as at any renewal. */
  async check(): Promise<boolean> {
    if (!this.session) return false;
    this.#clearTimer();
    await this.#renew();
    return this.session !== null;
  }

  #accept(key: string, s: ListenToken, show: string | null): void {
    this.session = s;
    this.refusal = null;
    this.refusedFor = null;
    this.#show = show;
    this.kept = { text: key, device: s.device };
    writeKept(this.kept);
    this.#schedule(s);
  }

  #refuse(key: string, r: Refusal, show: string | null): void {
    const was = this.session !== null;
    this.#clearTimer();
    this.session = null;
    this.refusal = r;
    this.refusedFor = show;
    this.#show = show;
    if (FORGET.includes(r.code)) {
      // A key typed wrong is not kept; one that has ended for good is let go of.
      if (this.kept?.text === key) {
        this.kept = null;
        writeKept(null);
      }
    } else if (r.code !== 'no_answer' && r.code !== 'slow_down' && r.code !== 'not_set_up') {
      // It may work later (not started, paused, too many devices): keep it for then.
      this.kept = { text: key, device: this.kept?.text === key ? this.kept.device : null };
      writeKept(this.kept);
    } else if (!this.kept) {
      // The station did not answer: keep what was typed for this page, so Try again has it.
      this.kept = { text: key, device: null };
    }
    if (r.code === 'too_many_devices') {
      this.devices = r.devices;
      if (r.hold_s != null) this.holdS = r.hold_s;
    }
    // A show that has ended with nothing of the key's next on the schedule is not a key that
    // stopped working: the page goes back to the public programme without a word of alarm.
    if (was && r.code !== 'not_scheduled') this.onCut?.();
  }

  #clearTimer(): void {
    if (this.#timer) clearTimeout(this.#timer);
    this.#timer = null;
  }

  #schedule(s: ListenToken): void {
    this.#clearTimer();
    if (this.#stopped) return;
    // Renew when the station says, and never later than a little before the token ends.
    const left = s.expires_ms - Date.now();
    const wait = Math.max(5_000, Math.min(s.renew_in_s * 1000, left - 10_000));
    this.#timer = setTimeout(() => void this.#renew(), wait);
  }

  async #renew(): Promise<void> {
    const s = this.session;
    const key = this.kept?.text;
    if (!s || !key || this.#stopped) return;
    try {
      const next = await routes.renew(s.token);
      if (this.session !== s) return;
      this.session = next;
      this.kept = { text: key, device: next.device };
      writeKept(this.kept);
      this.#schedule(next);
    } catch (e) {
      if (this.session !== s) return;
      const r = e instanceof Refusal ? e : new Refusal('no_answer', 0);
      if (r.code === 'token_ended' || r.code === 'token_invalid') {
        // The page slept past the token's life: the key itself is asked again, and nothing is said.
        this.session = null;
        const ok = await this.enter(key, this.#show, 'kept');
        if (!ok) this.onCut?.();
        return;
      }
      if (r.code === 'not_scheduled') {
        // This show's windows are over; the key may open another that is on the schedule.
        this.session = null;
        if (await this.enter(key, null, 'kept')) return;
        if (this.refusal?.code !== 'not_scheduled') this.onCut?.();
        return;
      }
      if (r.code === 'no_answer' || r.code === 'slow_down') {
        // A dropped connection is not a refusal: try again shortly while the token still lives.
        if (s.expires_ms - Date.now() > 8_000) {
          this.#timer = setTimeout(() => void this.#renew(), 5_000);
          return;
        }
      }
      this.#refuse(key, r, this.#show);
    }
  }

  /** The devices that hold the key. */
  async loadDevices(): Promise<void> {
    if (!this.kept) return;
    this.devicesProblem = false;
    try {
      const d = await routes.devices(this.kept.text);
      this.devices = d.devices;
      if (d.hold_s != null) this.holdS = d.hold_s;
    } catch {
      this.devicesProblem = true;
    }
  }

  /** Sign a device out of the key. Signing out one that was in the way uses the key here at
   *  once; signing out this one lets go of the key on this device. */
  async signOut(device: string): Promise<void> {
    const key = this.kept?.text;
    if (!key) return;
    this.devicesProblem = false;
    try {
      await routes.endDevice(key, device);
    } catch {
      this.devicesProblem = true;
      return;
    }
    if (device === this.kept?.device) return this.forget();
    if (this.refusal?.code === 'too_many_devices') {
      await this.enter(key, this.#show, 'kept');
      if (this.session) this.fresh = true;
    }
    await this.loadDevices();
  }

  /** Let go of the key on this device. The key itself still works elsewhere. */
  forget(): void {
    const was = this.session !== null;
    this.#clearTimer();
    this.session = null;
    this.refusal = null;
    this.refusedFor = null;
    this.devices = null;
    this.kept = null;
    this.fresh = false;
    writeKept(null);
    if (was) this.onCut?.();
  }

  /** The page is going away: no more renewals. */
  stop(): void {
    this.#stopped = true;
    this.#clearTimer();
  }
}
