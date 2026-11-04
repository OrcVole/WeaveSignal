// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The sandbox's sound (ADR-0212): Web Audio in the page, following the simulated station.
//
// Two ways to listen, one at a time: the studio (what the presenter hears: everyone open, the
// carts, what is cued, and a caller's booth while it is screened) and the listeners (the
// programme only, behind the profanity delay; the default loop while playout is down). The
// listeners' path is made of its own copies of every sound, started `delay` later; a dump stops
// those copies and starts them again from where the studio is now, so what was held is never
// heard. Levels for the meters are read from each person's voice before the fader, as the
// desk's are, and are only approximate (RMS, not K-weighted loudness).

import type { Catalogue } from '../../../../packages/ui/carts.ts';
import { DELAY_TARGET_MS, fileOf, LINES, type Speaking, type Station } from './station.ts';

export type Hear = 'studio' | 'listeners';

const MEDIA = ((import.meta.env.VITE_SANDBOX_MEDIA as string | undefined) ?? '/media/sandbox/').replace(/\/?$/, '/');

function ext(): 'opus' | 'm4a' {
  try {
    const a = document.createElement('audio');
    return a.canPlayType('audio/ogg; codecs=opus') ? 'opus' : 'm4a';
  } catch {
    return 'm4a';
  }
}

interface Voice {
  /** Before the fader: the meter reads here. */
  meter: AnalyserNode;
  studio: GainNode;
  listen: GainNode;
  /** What the gains were last set to, so only a change schedules anything. */
  was: { studio: number; listen: number };
}

/** A sound playing on the programme, for a dump to restart its listeners' copy. */
interface Playing {
  buffer: AudioBuffer;
  /** When it started on the studio's path (context time) and whether it loops. */
  at: number;
  loop: boolean;
  /** Where its listeners' copy goes. */
  dest: AudioNode;
  studio: AudioBufferSourceNode;
  listen: AudioBufferSourceNode | null;
  /** The cart play's number, for a cart. */
  play?: number;
  /** The bed's own gain (for ducking and fades) on each path, for a cart. */
  gains?: { studio: GainNode; listen: GainNode };
}

export class Sound {
  readonly ctx: AudioContext;
  private readonly kind = ext();
  private readonly studioOut: GainNode;
  private readonly listenOut: GainNode;
  /** The listeners' programme, and the fallback they hear while playout is down. */
  private readonly programme: GainNode;
  private readonly fallback: GainNode;
  private readonly booth: GainNode;
  private readonly voices = new Map<string, Voice>();
  private readonly buffers = new Map<string, Promise<AudioBuffer>>();
  private playing: Playing[] = [];
  private hear: Hear = 'studio';
  private dumps = 0;
  private playout = true;
  private fallbackSrc: AudioBufferSourceNode | null = null;
  private ducked = false;
  private readonly data = new Float32Array(2048);

  constructor() {
    const Ctx = (globalThis.AudioContext ?? (globalThis as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext) as typeof AudioContext;
    this.ctx = new Ctx({ latencyHint: 'interactive' });
    const g = (v: number, to: AudioNode) => {
      const n = this.ctx.createGain();
      n.gain.value = v;
      n.connect(to);
      return n;
    };
    this.studioOut = g(1, this.ctx.destination);
    this.listenOut = g(0, this.ctx.destination);
    this.programme = g(1, this.listenOut);
    this.fallback = g(0, this.listenOut);
    this.booth = g(0.9, this.studioOut);
  }

  /** Fetch every voice line (about 2 MB) and the bank's top row, so the first presses answer. */
  async preload(cat: Catalogue): Promise<void> {
    const top = cat.carts.filter((c) => c.bank === 1 && c.slot <= 6).map((c) => this.cartUrl(c.file));
    await Promise.allSettled([...LINES.map((l) => this.buffer(this.lineUrl(l.id))), ...top.map((u) => this.buffer(u)), this.buffer(`${MEDIA}default-loop.${this.kind}`)]);
  }

  async resume(): Promise<void> {
    if (this.ctx.state !== 'running') await this.ctx.resume();
  }

  setHear(h: Hear): void {
    this.hear = h;
    const t = this.ctx.currentTime;
    this.studioOut.gain.setTargetAtTime(h === 'studio' ? 1 : 0, t, 0.03);
    this.listenOut.gain.setTargetAtTime(h === 'listeners' ? 1 : 0, t, 0.03);
  }

  hearing(): Hear {
    return this.hear;
  }

  private lineUrl(id: string): string {
    return `${MEDIA}voices/${id}.${this.kind}`;
  }

  private cartUrl(file: string | null): string {
    const base = (file ?? '').replace(/\.[a-z0-9]+$/, '');
    return `${MEDIA}carts/${base}.${this.kind}`;
  }

  private buffer(url: string): Promise<AudioBuffer> {
    let b = this.buffers.get(url);
    if (!b) {
      b = fetch(url)
        .then((r) => {
          if (!r.ok) throw new Error(`${r.status} for ${url}`);
          return r.arrayBuffer();
        })
        .then((a) => this.ctx.decodeAudioData(a));
      b.catch(() => this.buffers.delete(url));
      this.buffers.set(url, b);
    }
    return b;
  }

  private voice(id: string): Voice {
    let v = this.voices.get(id);
    if (!v) {
      const meter = this.ctx.createAnalyser();
      meter.fftSize = 2048;
      const studio = this.ctx.createGain();
      studio.gain.value = 0;
      studio.connect(this.studioOut);
      const listen = this.ctx.createGain();
      listen.gain.value = 0;
      listen.connect(this.programme);
      v = { meter, studio, listen, was: { studio: 0, listen: 0 } };
      this.voices.set(id, v);
    }
    return v;
  }

  /** The delay listeners are behind the studio now, in seconds. */
  private behind(st: Station): number {
    return Math.max(0, st.delay.delay_ms) / 1000;
  }

  /** A line starts: on the desk (and, a delay later, to listeners) or in the booth. */
  async say(sp: Speaking, st: Station): Promise<void> {
    const buf = await this.buffer(this.lineUrl(sp.line.id)).catch(() => null);
    if (!buf) return;
    const v = this.voice(sp.line.who);
    const now = this.ctx.currentTime;
    const src = this.ctx.createBufferSource();
    src.buffer = buf;
    src.connect(v.meter);
    if (sp.line.heard === 'booth') {
      src.connect(this.booth);
      src.start(now);
      return;
    }
    src.connect(v.studio);
    src.start(now);
    const listen = this.ctx.createBufferSource();
    listen.buffer = buf;
    listen.connect(v.listen);
    listen.start(now + this.behind(st));
    const p: Playing = { buffer: buf, at: now, loop: false, dest: v.listen, studio: src, listen };
    this.playing.push(p);
    src.onended = () => {
      this.playing = this.playing.filter((x) => x !== p || x.listen !== null);
    };
    listen.onended = () => {
      if (p.listen === listen) this.playing = this.playing.filter((x) => x !== p);
    };
  }

  /** Follow the station: faders, carts, a dump, playout, the bed's ducking. Cheap when nothing
   *  changed. */
  follow(st: Station): void {
    const now = this.ctx.currentTime;
    const later = now + this.behind(st);
    for (const p of st.people) {
      const v = this.voice(p.identity);
      const open = st.open(p.identity);
      const studio = open || p.cue ? 1 : 0;
      const listen = open ? 1 : 0;
      if (studio !== v.was.studio) {
        v.studio.gain.setTargetAtTime(studio, now, 0.015);
        v.was.studio = studio;
      }
      if (listen !== v.was.listen) {
        v.listen.gain.setTargetAtTime(listen, later, 0.015);
        v.was.listen = listen;
      }
    }
    this.carts(st, now, later);
    if (st.delay.dumps !== this.dumps) {
      this.dumps = st.delay.dumps;
      this.dump(st, now);
    }
    if (st.playout !== this.playout) {
      this.playout = st.playout;
      void this.setPlayout(st.playout);
    }
    const duck = st.ducked();
    if (duck !== this.ducked) {
      this.ducked = duck;
      for (const p of this.playing) {
        if (!p.gains || !p.loop) continue;
        p.gains.studio.gain.setTargetAtTime(duck ? 0.3 : 1, now, 0.12);
        p.gains.listen.gain.setTargetAtTime(duck ? 0.3 : 1, later, 0.12);
      }
    }
  }

  private carts(st: Station, now: number, later: number): void {
    const plays = new Map(st.sounding.map((s) => [s.play, s]));
    // Stopped or faded on the desk: fade here too (the listeners' copy a delay later).
    for (const p of this.playing) {
      if (p.play === undefined || !p.gains) continue;
      const s = plays.get(p.play);
      if (s && !s.stopping) continue;
      const fade = s ? Math.max(0.05, ((s.ends_ms ?? st.clock) - st.clock) / 1000) : 0.05;
      p.gains.studio.gain.setTargetAtTime(0, now, fade / 4);
      p.studio.stop(now + fade + 0.1);
      if (p.listen) {
        p.gains.listen.gain.setTargetAtTime(0, later, fade / 4);
        p.listen.stop(later + fade + 0.1);
      }
      p.play = undefined;
    }
    for (const s of st.sounding) {
      if (s.stopping || this.playing.some((p) => p.play === s.play) || this.starting.has(s.play)) continue;
      this.starting.add(s.play);
      void this.startCart(st, s.play, s.cart, s.cue, s.mode === 'bed');
    }
  }

  private readonly starting = new Set<number>();

  private async startCart(st: Station, play: number, cart: string, cue: boolean, loop: boolean): Promise<void> {
    const buf = await this.buffer(this.cartUrl(fileOf(st.catalogue, cart))).catch(() => null);
    this.starting.delete(play);
    // Stopped before it loaded, or never loads: nothing to play.
    if (!buf || !st.sounding.some((s) => s.play === play && !s.stopping)) return;
    const now = this.ctx.currentTime;
    const gs = this.ctx.createGain();
    gs.connect(cue ? this.booth : this.studioOut);
    const src = this.ctx.createBufferSource();
    src.buffer = buf;
    src.loop = loop;
    src.connect(gs);
    src.start(now);
    const gl = this.ctx.createGain();
    gl.connect(this.programme);
    let listen: AudioBufferSourceNode | null = null;
    if (!cue) {
      listen = this.ctx.createBufferSource();
      listen.buffer = buf;
      listen.loop = loop;
      listen.connect(gl);
      listen.start(now + this.behind(st));
    }
    const p: Playing = { buffer: buf, at: now, loop, dest: gl, studio: src, listen, play, gains: { studio: gs, listen: gl } };
    if (loop && this.ducked) {
      gs.gain.value = 0.3;
      gl.gain.value = 0.3;
    }
    this.playing.push(p);
    const end = () => {
      this.playing = this.playing.filter((x) => x !== p);
    };
    (listen ?? src).onended = end;
  }

  /** The dump: the listeners' copies held in the delay are thrown away, and the programme
   *  carries on for them from where the studio is now, with no delay (it builds back). */
  private dump(st: Station, now: number): void {
    for (const v of this.voices.values()) {
      v.listen.gain.cancelScheduledValues(now);
      v.listen.gain.setValueAtTime(v.was.listen, now);
    }
    for (const p of this.playing) {
      if (!p.listen) continue;
      p.listen.onended = null;
      try {
        p.listen.stop(now);
      } catch {
        /* not started yet */
      }
      const into = now - p.at;
      if (!p.loop && into >= p.buffer.duration) {
        p.listen = null;
        continue;
      }
      const src = this.ctx.createBufferSource();
      src.buffer = p.buffer;
      src.loop = p.loop;
      src.connect(p.dest);
      src.start(now, p.loop ? into % p.buffer.duration : into);
      p.listen = src;
      src.onended = () => {
        this.playing = this.playing.filter((x) => x !== p);
      };
    }
    void st;
  }

  private async setPlayout(on: boolean): Promise<void> {
    const now = this.ctx.currentTime;
    this.programme.gain.setTargetAtTime(on ? 1 : 0, now, 0.05);
    this.fallback.gain.setTargetAtTime(on ? 0 : 0.8, now, 0.3);
    if (!on && !this.fallbackSrc) {
      const buf = await this.buffer(`${MEDIA}default-loop.${this.kind}`).catch(() => null);
      if (!buf || this.playout) return;
      const src = this.ctx.createBufferSource();
      src.buffer = buf;
      src.loop = true;
      src.connect(this.fallback);
      src.start();
      this.fallbackSrc = src;
    } else if (on && this.fallbackSrc) {
      this.fallbackSrc.stop(now + 1);
      this.fallbackSrc = null;
    }
  }

  /** Each voice's level now, LUFS-like (RMS in dBFS less 0.7); null when it is silent. */
  levels(): Record<string, number | null> {
    const out: Record<string, number | null> = {};
    for (const [id, v] of this.voices) {
      v.meter.getFloatTimeDomainData(this.data);
      let sum = 0;
      for (let i = 0; i < this.data.length; i++) sum += this.data[i] * this.data[i];
      const rms = Math.sqrt(sum / this.data.length);
      const db = rms > 0 ? 20 * Math.log10(rms) - 0.7 : -Infinity;
      out[id] = db > -70 ? Math.round(db * 10) / 10 : null;
    }
    return out;
  }

  /** The target delay, for the guide's words. */
  static readonly delayS = DELAY_TARGET_MS / 1000;
}
