// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// A stand-in for livekit-client, for the join page's layout driver only (tests/m3/layout.mjs builds
// the join page with this in livekit-client's place: apps/web/vite.joinstand.config.ts). It has the
// names lib/call.svelte.ts uses and no network: a room "connects" after a moment (or never, when the
// room's address says `?mode=silent`), takes whatever is published, and carries nobody. The
// microphone is the browser's real (fake-device) one, so the page's level bar works and the
// microphone chooser's driver (tests/m3/microphone.mjs) reads real captures.
//
// It proves how the page lays out in each state of a call. It proves nothing about LiveKit, and
// nothing about the move's order against a real room: that is the vitest suite's (the order) and
// the stack's (the room).

export const RoomEvent = {
  TrackPublished: 'trackPublished',
  TrackSubscribed: 'trackSubscribed',
  TrackUnsubscribed: 'trackUnsubscribed',
  AudioPlaybackStatusChanged: 'audioPlaybackChanged',
  Reconnecting: 'reconnecting',
  Reconnected: 'reconnected',
  Disconnected: 'disconnected',
};
export const TrackEvent = { Restarted: 'restarted', Muted: 'muted' };
export const Track = { Kind: { Audio: 'audio', Video: 'video' }, Source: { Microphone: 'microphone', Camera: 'camera' } };
export const DisconnectReason = { UNKNOWN_REASON: 0, CLIENT_INITIATED: 1, PARTICIPANT_REMOVED: 4, ROOM_DELETED: 5 };

export class Room {
  canPlaybackAudio = true;
  remoteParticipants = new Map();
  localParticipant = { activeDeviceMap: new Map(), setTrackSubscriptionPermissions() {}, async publishTrack() {} };
  on() {
    return this;
  }
  async connect(url) {
    await new Promise((r) => setTimeout(r, 60));
    if (new URL(url).searchParams.get('mode') === 'silent') await new Promise(() => {});
  }
  async disconnect() {}
  async startAudio() {}
}

/** A local audio track as far as the call uses one (1.3.3: the microphone can be changed). The
 *  capture is the browser's real one, asked for with the page's own constraints, so the driver
 *  can read what the page asked for from the track itself. `replaceTrack` does what
 *  livekit-client does for a track it manages: the new capture takes the old one's place and the
 *  old one stops. The driver reaches the track at `window.__mic` to end or mute it as the room
 *  client would. */
class LocalAudio {
  isMuted = false;
  sender = {};
  #on = new Map();
  constructor(t) {
    this.mediaStreamTrack = t;
  }
  on(e, fn) {
    if (!this.#on.has(e)) this.#on.set(e, new Set());
    this.#on.get(e).add(fn);
    return this;
  }
  off(e, fn) {
    this.#on.get(e)?.delete(fn);
    return this;
  }
  emit(e) {
    for (const fn of this.#on.get(e) ?? []) fn();
  }
  async replaceTrack(t) {
    const old = this.mediaStreamTrack;
    this.mediaStreamTrack = t;
    old.stop();
    return this;
  }
  async unmute() {
    this.isMuted = false;
    return this;
  }
  stop() {
    this.mediaStreamTrack.stop();
  }
}

// As livekit-client does with a bare device id: that microphone exactly, and when the browser
// says it is not there, the same again with the id as a preference only.
export async function createLocalAudioTrack(options) {
  const id = typeof options?.deviceId === 'string' ? options.deviceId : null;
  const ask = (audio) => navigator.mediaDevices.getUserMedia({ audio });
  let s;
  try {
    s = await ask(id ? { ...options, deviceId: { exact: id } } : (options ?? true));
  } catch (e) {
    if (!id || (e.name !== 'OverconstrainedError' && e.name !== 'NotFoundError')) throw e;
    s = await ask({ ...options, deviceId: { ideal: id } });
  }
  const t = new LocalAudio(s.getAudioTracks()[0]);
  globalThis.__mic = t;
  return t;
}

export async function createLocalVideoTrack() {
  const s = await navigator.mediaDevices.getUserMedia({ video: true });
  const t = s.getVideoTracks()[0];
  return { mediaStreamTrack: t, stop: () => t.stop(), mute: async () => {}, unmute: async () => {} };
}
