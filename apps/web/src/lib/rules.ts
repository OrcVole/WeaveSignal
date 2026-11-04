// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The room rules every join page keeps, apart from the browser so they can be tested.
//
// - The microphone: echo cancelling and noise suppression on, automatic gain control off
//   (packages/ui/media.ts; part two's live run heard a guest come back 0.3 s late). Only the echo
//   test page turns echo cancelling off.
// - Hearing: the room itself keeps private audio private (ADR-0118). A page lets no one subscribe
//   to its microphone or camera (the desk hears it through connections LiveKit lets past that),
//   and each of the desk's returns lets only its own person subscribe. A page still never
//   auto-subscribes, and takes only its own return, the desk's `<desk>-mm-<identity>`.
// - The relay: GrapheneOS's browser and strict networks block WebRTC's UDP, so a page can join
//   through LiveKit's TURN over TLS only (`iceTransportPolicy: 'relay'`); it does so on request,
//   and by itself after a direct attempt fails.
// - The held place: a caller whose link drops keeps their place for 5 minutes (ADR-0101), so the
//   page rejoins under the same identity until then.

import { VOICE_CAPTURE, type VoiceCapture } from '../../../../packages/ui/media.ts';

/** The constraints every join page captures the microphone with. */
export function voiceCapture(): VoiceCapture {
  return { ...VOICE_CAPTURE };
}

/** The part of a local participant that sets who may subscribe to its tracks. */
export interface Publisher {
  setTrackSubscriptionPermissions(allParticipantsAllowed: boolean, participantTrackPermissions?: unknown[]): void;
}

/**
 * Let no one in the room subscribe to the page's own tracks (ADR-0118): set once connected and
 * before anything is published, so no track is ever open; livekit-client sends it again after
 * each reconnection. The desk's listening connections carry LiveKit's recorder grant, which
 * passes this; another page in the room, modified or not, cannot.
 */
export function limitSubscribers(me: Publisher): void {
  me.setTrackSubscriptionPermissions(false, []);
}

/** Whether a track from this participant is one to hear: only one's own mix-minus. */
export function shouldHear(participantIdentity: string, hear: string): boolean {
  return participantIdentity === hear;
}

/**
 * Whether a picture from this participant is one to watch: only the desk's programme picture
 * (ADR-0028). Guests never subscribe to each other's cameras, so one guest's browser without
 * AV1 cannot drop every camera to VP8; the desk's picture carries their thumbnails.
 */
export function shouldSee(participantIdentity: string, see: string | null | undefined): boolean {
  return !!see && participantIdentity === see;
}

/** Whether the page offers the camera: while the desk has video on (it names a picture to
 * watch), to the people who appear on the show. Callers are heard, not seen, in release one;
 * screeners and producers never air. */
export function offersCamera(role: string, see: string | null | undefined): boolean {
  return !!see && (role === 'presenter' || role === 'dj' || role === 'guest');
}

/** Whether the join page shows a person their own picture before they enter (ADR-0171): the
 * people who appear on the show, while the station has video on. The call's own check
 * (`offersCamera`) still decides what is sent, from the token the join returns. */
export function wantsCameraPreview(role: string, videoOn: boolean): boolean {
  return videoOn && (role === 'presenter' || role === 'dj' || role === 'guest');
}

/** The camera as the page captures it: the desk's picture size and rate (480p20, ADR-0027). */
export function cameraCapture(): { resolution: { width: number; height: number; frameRate: number } } {
  return { resolution: { width: 854, height: 480, frameRate: 20 } };
}

/** How the camera is published: AV1 where the browser encodes it (Chromium), VP8 elsewhere;
 * one layer, since only the desk watches it. */
export const CAMERA_PUBLISH = {
  videoCodec: 'av1',
  backupCodec: { codec: 'vp8' },
  simulcast: false,
  videoEncoding: { maxBitrate: 900_000, maxFramerate: 20 },
} as const;

/** LiveKit's connect options: never auto-subscribe; through the relay alone when asked. */
export function connectOptions(relay: boolean): { autoSubscribe: false; rtcConfig?: RTCConfiguration; peerConnectionTimeout: number } {
  return relay
    ? { autoSubscribe: false, rtcConfig: { iceTransportPolicy: 'relay' }, peerConnectionTimeout: 20_000 }
    : { autoSubscribe: false, peerConnectionTimeout: 12_000 };
}

/** A dropped caller's place is held this long (ADR-0101). */
export const PLACE_HELD_MS = 5 * 60 * 1000;

/** Until when a dropped link may rejoin: a caller's held place; anyone else while their pass lasts. */
export function rejoinUntil(role: string, lostAt: number, passExpiresAt: number): number {
  return role === 'caller' ? Math.min(lostAt + PLACE_HELD_MS, passExpiresAt) : passExpiresAt;
}

/** The wait before the n-th rejoin attempt: 1, 2, 4, 8 s, then every 10 s. */
export function backoff(attempt: number): number {
  return Math.min(10_000, 1000 * 2 ** Math.max(0, attempt));
}
