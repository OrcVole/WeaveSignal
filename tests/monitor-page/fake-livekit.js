// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// A stand-in for livekit-client, for the monitor page's driver only (tests/monitor-page/shoot.mjs
// builds the page with this in livekit-client's place: apps/web/vite.standin.config.ts). It has
// the few names lib/monitor/listen.svelte.ts uses and no network: `connect` succeeds or fails as
// the room's address says, and a room that is joined "publishes" one track, a real tone made in
// the page's own audio graph, so the page's level bar and volume control work as they would.
// Everything it is asked to do is written to `window.__rooms` for the driver to read.
//
// It proves the page's own behaviour round the room. It proves nothing about LiveKit: the sound
// path is `just test-monitor`'s, with a real desk and a real room.

export const RoomEvent = {
  TrackSubscribed: 'trackSubscribed',
  TrackUnsubscribed: 'trackUnsubscribed',
  AudioPlaybackStatusChanged: 'audioPlaybackChanged',
  Reconnecting: 'reconnecting',
  Reconnected: 'reconnected',
  Disconnected: 'disconnected',
};
export const Track = { Kind: { Audio: 'audio', Video: 'video' } };
export const DisconnectReason = { UNKNOWN_REASON: 0, CLIENT_INITIATED: 1, PARTICIPANT_REMOVED: 4, ROOM_DELETED: 5 };

const rooms = (globalThis.__rooms ??= []);

export class Room {
  canPlaybackAudio = true;
  #on = new Map();
  #track = null;
  #osc = null;

  constructor(options) {
    this.options = options;
    /** What the driver reads: each connect, the volume set, whether it was left. */
    this.log = { connects: [], volume: null, left: false, joined: false, webAudio: !!options?.webAudioMix };
    rooms.push(this);
  }

  on(event, fn) {
    this.#on.set(event, fn);
    return this;
  }

  #emit(event, ...args) {
    this.#on.get(event)?.(...args);
  }

  async connect(url, token, opts) {
    const mode = new URL(url).searchParams.get('mode') ?? 'ok';
    const relay = opts?.rtcConfig?.iceTransportPolicy === 'relay';
    this.log.connects.push({ url, token, relay, autoSubscribe: opts?.autoSubscribe });
    await new Promise((r) => setTimeout(r, 60));
    if (mode === 'fail' || (mode === 'relayonly' && !relay)) throw new Error('could not establish pc connection');
    this.log.joined = true;
    if (mode === 'silent') return;
    setTimeout(() => this.#publish(), 150);
  }

  #publish() {
    if (this.log.left) return;
    const ctx = this.options?.webAudioMix?.audioContext ?? new AudioContext();
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    const out = ctx.createMediaStreamDestination();
    osc.frequency.value = 440;
    gain.gain.value = 0.2;
    osc.connect(gain).connect(out);
    osc.start();
    this.#osc = osc;
    const log = this.log;
    const track = {
      kind: 'audio',
      mediaStreamTrack: out.stream.getAudioTracks()[0],
      attach() {
        const el = document.createElement('audio');
        el.muted = true;
        return el;
      },
      detach() {
        return [];
      },
      setVolume(v) {
        log.volume = v;
      },
    };
    this.#track = track;
    this.#emit(RoomEvent.TrackSubscribed, track, {}, { identity: 'desk-monitor' });
  }

  async startAudio() {
    this.canPlaybackAudio = true;
  }

  async disconnect() {
    this.log.left = true;
    try {
      this.#osc?.stop();
    } catch {
      // not started
    }
  }

  /** The driver's: the room drops under the page. */
  drop(reason = DisconnectReason.UNKNOWN_REASON) {
    if (this.#track) this.#emit(RoomEvent.TrackUnsubscribed, this.#track);
    this.#emit(RoomEvent.Disconnected, reason);
  }
}
