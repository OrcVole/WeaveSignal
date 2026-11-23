// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// A stand-in for livekit-client, for the join page's layout driver only (tests/m3/layout.mjs builds
// the join page with this in livekit-client's place: apps/web/vite.joinstand.config.ts). It has the
// names lib/call.svelte.ts uses and no network: a room "connects" after a moment (or never, when the
// room's address says `?mode=silent`), takes whatever is published, and carries nobody. The
// microphone is the browser's real (fake-device) one, so the page's level bar works.
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
export const Track = { Kind: { Audio: 'audio', Video: 'video' }, Source: { Microphone: 'microphone', Camera: 'camera' } };
export const DisconnectReason = { UNKNOWN_REASON: 0, CLIENT_INITIATED: 1, PARTICIPANT_REMOVED: 4, ROOM_DELETED: 5 };

export class Room {
  canPlaybackAudio = true;
  remoteParticipants = new Map();
  localParticipant = { setTrackSubscriptionPermissions() {}, async publishTrack() {} };
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

export async function createLocalAudioTrack() {
  const s = await navigator.mediaDevices.getUserMedia({ audio: true });
  const t = s.getAudioTracks()[0];
  return { mediaStreamTrack: t, stop: () => t.stop() };
}

export async function createLocalVideoTrack() {
  const s = await navigator.mediaDevices.getUserMedia({ video: true });
  const t = s.getVideoTracks()[0];
  return { mediaStreamTrack: t, stop: () => t.stop(), mute: async () => {}, unmute: async () => {} };
}
