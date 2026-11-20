// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The host's monitor, heard through the room's own path (ADR-0280; the design's section 4.5):
// the desk publishes the programme without this host, the cue bus and talk addressed to them in
// a room of their own, and this page joins it with a listen-only token. It publishes nothing
// and asks for no microphone.
//
// The same LiveKit client as the join pages, with their rules for the relay (lib/rules.ts,
// lib/turn.ts). The sound goes through one audio graph made inside the host's press, so a
// browser lets it play, the volume control works where an element's own volume does not, and
// the level bar reads what arrives, before the volume.

import { type RemoteParticipant, type RemoteTrack, type RemoteTrackPublication, DisconnectReason, Room, RoomEvent, Track } from 'livekit-client';
import { ApiError } from '../api.ts';
import { barPct, dbfs } from '../meter.ts';
import { backoff, connectOptions } from '../rules.ts';
import { installStationIce, type StationRelay, useStationIce } from '../turn.ts';
import { roomToken } from './api.ts';

/** Where the monitor's sound stands. */
export type Hearing =
  /** Not started: the browser needs the host's press. */
  | 'idle'
  /** Joining the monitor room. */
  | 'starting'
  /** In the room. Whether sound arrives is `sounding`. */
  | 'listening'
  /** The room dropped; the page is joining again with a fresh token. */
  | 'rejoining';

const VOLUME_KEPT = 'weavesignal.monitor.volume';
/** How long past the room client's own time an attempt to join may run. */
const JOIN_GRACE_MS = 8000;
const UNREACHED = 'This page could not reach the station for your monitor. Check your connection and try again.';

function keptVolume(): number {
  try {
    const v = Number(localStorage.getItem(VOLUME_KEPT));
    return Number.isFinite(v) && v > 0 && v <= 100 ? Math.round(v) : 80;
  } catch {
    return 80;
  }
}

export class Listen {
  hearing = $state<Hearing>('idle');
  /** The desk's mix is arriving. */
  sounding = $state(false);
  /** The level bar's fill, 0 to 100: what arrives, before the volume. */
  level = $state(0);
  /** 0 to 100. */
  volume = $state(keptVolume());
  /** The browser is holding playback until a tap. */
  needsTap = $state(false);
  /** Joined through the station's relay only (TURN over TLS). */
  relay = $state(false);
  /** One sentence about the last problem, the station's own where it gave one. */
  problem = $state<string | null>(null);
  /** The listeners' stream is playing: what the host hears while the desk's mix is silent. */
  streaming = $state(false);
  /** The browser refused the stream's playback until a tap. */
  streamBlocked = $state(false);

  #secret: string;
  #relay: () => StationRelay | undefined;
  #room: Room | null = null;
  #ctx: AudioContext | null = null;
  #tracks = new Map<RemoteTrack, { el: HTMLMediaElement; tap: MediaStreamAudioSourceNode | null; an: AnalyserNode | null }>();
  #raf = 0;
  #stopped = true;
  /** The last refusal of a token was for good (the link has ended): do not try again. */
  #fatal = false;
  /** The listeners' stream (the player's own address), and whether the page should be playing
   *  it now. The page decides from the station's view; this plays and stops. */
  #streamUrl = '';
  #want = false;
  #stream: HTMLAudioElement | null = null;
  #streamRetry: ReturnType<typeof setTimeout> | undefined;

  /** `relay` gives the station's relay facts when the press comes (`undefined`: not known; the
   *  page then asks no STUN server and leaves TURN as LiveKit says). */
  constructor(secret: string, relay: () => StationRelay | undefined) {
    this.#secret = secret;
    this.#relay = relay;
  }

  /**
   * Start listening. `ctx` is the audio graph made inside the host's press (a browser may refuse
   * one made later); null where the browser has no Web Audio, and the room then plays through
   * its own elements, without the level bar.
   */
  async start(ctx: AudioContext | null): Promise<void> {
    if (this.hearing !== 'idle') {
      void ctx?.close().catch(() => {});
      return;
    }
    this.problem = null;
    this.#stopped = false;
    this.hearing = 'starting';
    // Inside the press, so the browser lets the stream play without another.
    this.#sync();
    if (this.#ctx && this.#ctx !== ctx) void this.#ctx.close().catch(() => {});
    this.#ctx = ctx;
    useStationIce(this.#relay());
    const direct = !this.relay;
    const why = await this.#join();
    if (why === null || this.#stopped) return;
    await this.#quiet();
    // The next press tries a direct connection again, unless the host asked for the relay.
    if (direct) this.relay = false;
    this.problem = why;
    this.hearing = 'idle';
    this.#sync();
  }

  /** Leave the room for good (the link has ended, or the page is closing). */
  async stop(): Promise<void> {
    this.#stopped = true;
    await this.#quiet();
    this.hearing = 'idle';
    this.#sync();
    this.problem = null;
    void this.#ctx?.close().catch(() => {});
    this.#ctx = null;
  }

  /** Leave and join again through the station's relay only. */
  async useRelay(): Promise<void> {
    if (this.hearing === 'idle') {
      this.relay = true;
      return;
    }
    this.relay = true;
    this.problem = null;
    await this.#quiet();
    this.hearing = 'rejoining';
    void this.#rejoin();
  }

  setVolume(v: number): void {
    this.volume = Math.round(Math.min(100, Math.max(0, v)));
    for (const t of this.#tracks.keys()) (t as RemoteTrack & { setVolume?: (v: number) => void }).setVolume?.(this.volume / 100);
    if (this.#stream) this.#stream.volume = this.volume / 100;
    try {
      localStorage.setItem(VOLUME_KEPT, String(this.volume));
    } catch {
      // a private window: the volume is asked for again next time
    }
  }

  /** Let the browser play after a tap. */
  async tap(): Promise<void> {
    void this.#ctx?.resume().catch(() => {});
    await this.#room?.startAudio().catch(() => {});
    this.needsTap = !(this.#room?.canPlaybackAudio ?? true);
    // A tap is a press: the stream may play now.
    this.streamBlocked = false;
    this.#sync();
  }

  /** Where the listeners' stream is (the player's own address); empty where the station has none. */
  setStream(url: string): void {
    this.#streamUrl = url;
    this.#sync();
  }

  /**
   * Whether the host should hear the listeners' stream now: while they stand by and the desk's
   * mix has nothing in it (automation holds the air, and that is downstream of the desk). The
   * page says so from each view, and says no at once when the host goes on air or when views
   * stop arriving, so the host never hears themselves a few seconds late.
   */
  wantStream(want: boolean): void {
    this.#want = want;
    this.#sync();
  }

  /** Play or stop the stream to match what is wanted; only once the host has pressed Start. */
  #sync(): void {
    if (this.#want && this.#streamUrl && this.hearing !== 'idle') void this.#playStream();
    else this.#haltStream();
  }

  async #playStream(): Promise<void> {
    clearTimeout(this.#streamRetry);
    if (this.#stream && !this.#stream.paused) return;
    let el = this.#stream;
    if (!el) {
      el = new Audio();
      el.preload = 'none';
      el.dataset.monitor = 'stream';
      document.body.appendChild(el);
      const dropped = () => {
        this.streaming = false;
        // A live stream that stops is tried again while it is wanted.
        if (this.#stream === el && this.#want && this.hearing !== 'idle') this.#streamRetry = setTimeout(() => this.#sync(), 4000);
      };
      el.addEventListener('playing', () => (this.streaming = true));
      el.addEventListener('error', dropped);
      el.addEventListener('ended', dropped);
      this.#stream = el;
    }
    // A fresh address each time, so it joins the live stream and not what was buffered.
    el.src = this.#streamUrl;
    el.volume = this.volume / 100;
    try {
      await el.play();
      this.streamBlocked = false;
    } catch (e) {
      if (this.#stream !== el) return;
      if (e instanceof DOMException && e.name === 'NotAllowedError') this.streamBlocked = true;
      else this.#streamRetry = setTimeout(() => this.#sync(), 4000);
    }
  }

  #haltStream(): void {
    clearTimeout(this.#streamRetry);
    this.streaming = false;
    this.streamBlocked = false;
    const el = this.#stream;
    if (!el) return;
    this.#stream = null;
    el.pause();
    // Drop the connection, not only the sound.
    el.removeAttribute('src');
    el.load();
    el.remove();
  }

  /** One attempt: a token, then the room, directly and then through the relay. Null when it is
   *  in the room; otherwise the sentence for why not. `#fatal` is set when trying again cannot help. */
  async #join(): Promise<string | null> {
    let token;
    try {
      token = await roomToken(this.#secret);
    } catch (e) {
      // The station's own sentence: two browsers listen already (409), the link has ended (401,
      // 410), the room is not set up (503).
      if (e instanceof ApiError) {
        this.#fatal = e.status === 401 || e.status === 410;
        if (e.message) return e.message;
      }
      return UNREACHED;
    }
    if (await this.#connect(token.url, token.token)) return null;
    if (!this.relay) {
      this.relay = true;
      // A token joins once: the relay's attempt needs its own.
      try {
        token = await roomToken(this.#secret);
      } catch (e) {
        return (e instanceof ApiError && e.message) || UNREACHED;
      }
      if (await this.#connect(token.url, token.token)) return null;
    }
    return 'This page could not join your monitor, directly or through the station’s relay. Check your connection and try again.';
  }

  async #connect(url: string, token: string): Promise<boolean> {
    const ctx = this.#ctx;
    installStationIce();
    const room = new Room({ adaptiveStream: false, dynacast: false, disconnectOnPageLeave: true, webAudioMix: ctx ? { audioContext: ctx } : false });
    this.#room = room;
    room
      .on(RoomEvent.TrackSubscribed, (track: RemoteTrack, _pub: RemoteTrackPublication, _who: RemoteParticipant) => {
        if (track.kind !== Track.Kind.Audio || this.#room !== room) return;
        const el = track.attach();
        el.dataset.monitor = 'mix';
        document.body.appendChild(el);
        (track as RemoteTrack & { setVolume?: (v: number) => void }).setVolume?.(this.volume / 100);
        let tap: MediaStreamAudioSourceNode | null = null;
        let an: AnalyserNode | null = null;
        if (ctx) {
          try {
            tap = ctx.createMediaStreamSource(new MediaStream([track.mediaStreamTrack]));
            an = ctx.createAnalyser();
            an.fftSize = 2048;
            tap.connect(an);
          } catch {
            tap = an = null;
          }
        }
        this.#tracks.set(track, { el, tap, an });
        this.sounding = true;
        this.#meter();
      })
      .on(RoomEvent.TrackUnsubscribed, (track: RemoteTrack) => this.#forget(track))
      .on(RoomEvent.AudioPlaybackStatusChanged, () => (this.needsTap = !room.canPlaybackAudio))
      .on(RoomEvent.Reconnecting, () => (this.hearing = 'rejoining'))
      .on(RoomEvent.Reconnected, () => (this.hearing = 'listening'))
      .on(RoomEvent.Disconnected, (reason?: DisconnectReason) => {
        if (this.#room !== room || this.#stopped) return;
        void this.#lost(reason);
      });
    const opts = connectOptions(this.relay);
    let late: ReturnType<typeof setTimeout> | undefined;
    try {
      // The only publisher in a monitor room is the desk, with this host's mix: take what is there.
      // The room client can try for ever against an address that answers and is not a room, so
      // the page gives it its own time and a little more, then calls the attempt failed.
      await Promise.race([
        room.connect(url, token, { ...opts, autoSubscribe: true }),
        new Promise<never>((_, refuse) => (late = setTimeout(() => refuse(new Error('the room did not answer in time')), opts.peerConnectionTimeout + JOIN_GRACE_MS))),
      ]);
      clearTimeout(late);
      if (this.#stopped || this.#room !== room) {
        await room.disconnect().catch(() => {});
        return true;
      }
      this.needsTap = !room.canPlaybackAudio;
      this.hearing = 'listening';
      this.problem = null;
      return true;
    } catch {
      clearTimeout(late);
      if (this.#room === room) this.#room = null;
      await room.disconnect().catch(() => {});
      return false;
    }
  }

  #forget(track: RemoteTrack): void {
    const t = this.#tracks.get(track);
    if (!t) return;
    this.#tracks.delete(track);
    t.tap?.disconnect();
    for (const el of track.detach()) el.remove();
    t.el.remove();
    this.sounding = this.#tracks.size > 0;
  }

  /** Leave the room and drop its sound, keeping the audio graph for a rejoin. */
  async #quiet(): Promise<void> {
    const room = this.#room;
    this.#room = null;
    for (const t of [...this.#tracks.keys()]) this.#forget(t);
    cancelAnimationFrame(this.#raf);
    this.#raf = 0;
    this.sounding = false;
    this.level = 0;
    this.needsTap = false;
    await room?.disconnect().catch(() => {});
  }

  /** The room dropped under the page: join again with a fresh token, while the link is good. */
  async #lost(_reason?: DisconnectReason): Promise<void> {
    await this.#quiet();
    if (this.#stopped) return;
    this.hearing = 'rejoining';
    await this.#rejoin();
  }

  async #rejoin(): Promise<void> {
    for (let n = 0; !this.#stopped; n++) {
      await new Promise((r) => setTimeout(r, n === 0 ? 300 : backoff(n - 1)));
      if (this.#stopped) return;
      this.#fatal = false;
      const why = await this.#join();
      if (why === null) return;
      // The station's sentence shows while the page keeps trying (a seat frees in a few seconds).
      this.problem = why;
      if (this.#fatal) {
        await this.#quiet();
        this.hearing = 'idle';
        this.#sync();
        return;
      }
    }
  }

  /** The level bar: the loudest of what arrives, before the volume. */
  #meter(): void {
    if (this.#raf) return;
    const buf = new Float32Array(2048);
    const tick = () => {
      let db = -Infinity;
      for (const t of this.#tracks.values()) {
        if (!t.an) continue;
        t.an.getFloatTimeDomainData(buf);
        db = Math.max(db, dbfs(buf));
      }
      this.level = barPct(db);
      this.#raf = this.#tracks.size ? requestAnimationFrame(tick) : 0;
    };
    this.#raf = requestAnimationFrame(tick);
  }
}
