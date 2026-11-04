// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// One person in the room from a browser: a caller from the player page, or a guest, remote DJ
// or presenter from an invite. It publishes the microphone with the join pages' constraints,
// hears only its own mix-minus, follows its own state from the control plane, offers the relay
// (TURN over TLS), and rejoins under the same identity when the link drops: within the held
// 5 minutes for a caller (ADR-0101), while the pass lasts for anyone else.

import {
  type LocalAudioTrack,
  type LocalVideoTrack,
  type VideoCodec,
  type RemoteParticipant,
  type RemoteTrack,
  type RemoteTrackPublication,
  DisconnectReason,
  Room,
  RoomEvent,
  Track,
  createLocalAudioTrack,
  createLocalVideoTrack,
} from 'livekit-client';
import * as api from './api.ts';
import { CAMERA_PUBLISH, backoff, cameraCapture, connectOptions, limitSubscribers, offersCamera, rejoinUntil, shouldHear, shouldSee, voiceCapture } from './rules.ts';
import { cameraProblem, micProblem } from './mic.ts';
import type { CameraHandover } from './camera-preview.svelte.ts';
import * as store from './store.ts';
import type { Link } from './words.ts';

export class Call {
  link = $state<Link>('idle');
  me = $state<api.MeState | null>(null);
  /** The studio's sound (our mix-minus) is arriving. */
  hearing = $state(false);
  /** The browser blocked playback until a tap. */
  needsTap = $state(false);
  /** Joined through the relay only (TURN over TLS). */
  relay = $state(false);
  /** A sentence about the last problem, for the page. */
  problem = $state<string | null>(null);
  /** Until when a dropped link may rejoin. */
  heldUntil = $state<number | null>(null);
  /** Our own microphone, for the level bar. */
  mic = $state<MediaStreamTrack | null>(null);
  /** The desk's programme picture, live (ADR-0028), while the station has video on. */
  picture = $state<MediaStreamTrack | null>(null);
  /** Our own camera, for its preview; null when we send none. */
  camera = $state<MediaStreamTrack | null>(null);
  cameraOn = $state(false);
  /** Why the camera did not start (we are heard without it). */
  cameraProblem = $state<string | null>(null);
  joined: api.Joined;

  #room: Room | null = null;
  #track: LocalAudioTrack | null = null;
  #camera: LocalVideoTrack | null = null;
  #stopMe: (() => void) | null = null;
  #audio: HTMLMediaElement[] = [];
  #leaving = false;
  #heartbeat: ReturnType<typeof setInterval> | undefined;

  /** The camera the join page already showed the person (ADR-0171), taken at start. */
  #handover: CameraHandover | null;

  constructor(joined: api.Joined, relay = false, camera: CameraHandover | null = null) {
    this.joined = joined;
    this.relay = relay;
    this.#handover = camera;
  }

  get role(): api.Role {
    return this.joined.role;
  }

  /** Join: follow our state, take the microphone, connect; through the relay if direct fails. */
  async start(): Promise<void> {
    // Again after a refused microphone (Try again): the refusal's sentence goes with it.
    this.problem = null;
    this.#stopMe = api.followMe(
      this.joined.pass,
      (s) => {
        this.me = s;
        if (s.phase === 'ended') void this.#end();
      },
      (why) => {
        this.problem = why;
        void this.#end();
      },
    );
    this.link = 'connecting';
    try {
      this.#track = await createLocalAudioTrack(voiceCapture());
      this.mic = this.#track.mediaStreamTrack;
    } catch (e) {
      this.problem = micProblem(e);
      this.link = 'no-mic';
      this.#handover?.track?.stop();
      this.#handover = null;
      this.#stopMe?.();
      return;
    }
    await this.#adoptCamera();
    if (!(await this.#connect())) {
      if (!this.relay) {
        this.problem = 'A direct connection did not work, so this page is trying the station’s relay.';
        this.relay = true;
        if (await this.#connect()) return;
      }
      this.problem = 'This page could not reach the studio. Check your connection and try again.';
      this.link = 'lost';
    }
  }

  /** Leave and join again through the relay (the "trouble connecting?" option). */
  async useRelay(): Promise<void> {
    this.relay = true;
    this.problem = null;
    await this.#drop(true);
    this.link = 'connecting';
    if (!(await this.#rejoin())) this.link = 'lost';
  }

  /** Let the browser play the studio's sound after a tap. */
  async tap(): Promise<void> {
    await this.#room?.startAudio();
    this.needsTap = !(this.#room?.canPlaybackAudio ?? true);
  }

  /** Hang up (a caller leaves the queue for good) or leave (anyone else). */
  async hangUp(): Promise<void> {
    this.#leaving = true;
    if (this.role === 'caller') await api.leave(this.joined.pass).catch(() => {});
    await this.#end();
  }

  async #end(): Promise<void> {
    this.#leaving = true;
    this.#stopMe?.();
    this.#stopMe = null;
    await this.#drop();
    this.#track?.stop();
    this.mic = null;
    this.#camera?.stop();
    this.#camera = null;
    this.camera = null;
    clearInterval(this.#heartbeat);
    if (this.role === 'caller') store.forget();
    this.link = 'ended';
  }

  /**
   * Leave the room. livekit-client's `disconnect()` stops every local track unless told not
   * to; a rejoin keeps them (`keepTracks`), so it can publish the same, live microphone again.
   */
  async #drop(keepTracks = false): Promise<void> {
    const room = this.#room;
    this.#room = null;
    for (const el of this.#audio) el.remove();
    this.#audio = [];
    this.hearing = false;
    this.picture = null;
    await room?.disconnect(!keepTracks).catch(() => {});
  }

  /** At start: the camera the person was shown before joining, if there is one; otherwise take
   * it now. Nothing has been sent until `#connect` publishes it. A camera that was refused in
   * the preview is not asked for again: they join with sound. */
  async #adoptCamera(): Promise<void> {
    const h = this.#handover;
    this.#handover = null;
    if (!h) return this.#takeCamera();
    if (!offersCamera(this.role, this.joined.see)) {
      // Video went off between the preview and the join: nothing to send.
      h.track?.stop();
      return;
    }
    if (h.track) {
      this.#camera = h.track;
      this.camera = h.track.mediaStreamTrack;
      this.cameraOn = h.on;
      this.cameraProblem = null;
    } else if (h.problem) {
      this.cameraProblem = h.problem;
    } else {
      await this.#takeCamera();
    }
  }

  /** Take the camera, when the page offers it; a camera that does not start costs only the
   * picture. On a rejoin, a fresh capture, as for the microphone, kept off if they turned it off. */
  async #takeCamera(): Promise<void> {
    if (!offersCamera(this.role, this.joined.see)) return;
    const keep = this.#camera ? this.cameraOn : true;
    try {
      const t = await createLocalVideoTrack(cameraCapture());
      this.#camera?.stop();
      this.#camera = t;
      this.camera = t.mediaStreamTrack;
      this.cameraOn = keep;
      this.cameraProblem = null;
    } catch (e) {
      this.cameraProblem = cameraProblem(e, true);
    }
  }

  /** Turn our camera off or on; the studio then cuts away from us or can cut to us again. */
  async setCamera(on: boolean): Promise<void> {
    if (!this.#camera) return;
    if (on) await this.#camera.unmute();
    else await this.#camera.mute();
    this.cameraOn = on;
  }

  async #connect(): Promise<boolean> {
    const j = this.joined;
    const room = new Room({ adaptiveStream: false, dynacast: false, audioCaptureDefaults: voiceCapture(), disconnectOnPageLeave: true });
    this.#room = room;
    const take = (pub: RemoteTrackPublication, who: RemoteParticipant) => {
      if (pub.kind === Track.Kind.Audio && shouldHear(who.identity, j.hear)) pub.setSubscribed(true);
      if (pub.kind === Track.Kind.Video && shouldSee(who.identity, j.see)) pub.setSubscribed(true);
    };
    room
      .on(RoomEvent.TrackPublished, take)
      .on(RoomEvent.TrackSubscribed, (track: RemoteTrack, pub: RemoteTrackPublication, who: RemoteParticipant) => {
        if (track.kind === Track.Kind.Video) {
          if (shouldSee(who.identity, j.see)) this.picture = track.mediaStreamTrack;
          else pub.setSubscribed(false);
          return;
        }
        if (!shouldHear(who.identity, j.hear)) {
          pub.setSubscribed(false);
          return;
        }
        const el = track.attach();
        el.dataset.hear = who.identity;
        document.body.appendChild(el);
        this.#audio.push(el);
        this.hearing = true;
      })
      .on(RoomEvent.TrackUnsubscribed, (track: RemoteTrack, _pub: RemoteTrackPublication, who: RemoteParticipant) => {
        if (track.kind === Track.Kind.Video) {
          if (shouldSee(who.identity, j.see)) this.picture = null;
          return;
        }
        if (!shouldHear(who.identity, j.hear)) return;
        for (const el of track.detach()) el.remove();
        this.hearing = false;
      })
      .on(RoomEvent.AudioPlaybackStatusChanged, () => (this.needsTap = !room.canPlaybackAudio))
      .on(RoomEvent.Reconnecting, () => (this.link = 'reconnecting'))
      .on(RoomEvent.Reconnected, () => (this.link = 'connected'))
      .on(RoomEvent.Disconnected, (reason?: DisconnectReason) => {
        if (this.#room !== room || this.#leaving) return;
        if (reason === DisconnectReason.PARTICIPANT_REMOVED || reason === DisconnectReason.ROOM_DELETED) {
          // The studio took us out (the screener's drop): the state stream says it ended.
          this.link = 'reconnecting';
          void this.#afterRemoval();
          return;
        }
        void this.#lost();
      });
    try {
      await room.connect(j.url, j.token, connectOptions(this.relay));
      limitSubscribers(room.localParticipant);
      if (this.#track) await room.localParticipant.publishTrack(this.#track, { source: Track.Source.Microphone, dtx: false, red: true });
      if (this.#camera) {
        await room.localParticipant
          .publishTrack(this.#camera, {
            source: Track.Source.Camera,
            ...CAMERA_PUBLISH,
            videoCodec: CAMERA_PUBLISH.videoCodec as VideoCodec,
            backupCodec: { codec: 'vp8' },
          })
          .catch(() => (this.cameraProblem = "Your camera couldn't be sent, so the studio hears you without your picture."));
        if (!this.cameraOn) await this.#camera.mute();
      }
      for (const who of room.remoteParticipants.values()) for (const pub of who.trackPublications.values()) take(pub, who);
      this.needsTap = !room.canPlaybackAudio;
      this.link = 'connected';
      this.heldUntil = null;
      this.#seen();
      return true;
    } catch {
      if (this.#room === room) this.#room = null;
      await room.disconnect().catch(() => {});
      return false;
    }
  }

  /** Remember this call on the device, so a reload can rejoin while the place is held. */
  #seen(): void {
    if (this.role !== 'caller') return;
    const j = this.joined;
    const put = () =>
      this.link === 'connected' &&
      store.keep({ pass: j.pass, identity: j.identity, name: j.name, seen_ms: Date.now(), pass_expires_at_ms: j.pass_expires_at_ms });
    put();
    clearInterval(this.#heartbeat);
    this.#heartbeat = setInterval(put, 5000);
  }

  /**
   * A new microphone track for a rejoin, only when the old one has ended (a device unplugged,
   * the browser taking it back). The M3 relay run (2026-10-26) heard nothing after any rejoin
   * because the page's `disconnect()` had stopped the track it then published again: an ended
   * track publishes but carries no audio (review 0025, D1). The rejoin now keeps it live.
   */
  async #freshMic(): Promise<void> {
    if (this.#track && this.#track.mediaStreamTrack.readyState === 'live') return;
    try {
      const fresh = await createLocalAudioTrack(voiceCapture());
      this.#track?.stop();
      this.#track = fresh;
      this.mic = fresh.mediaStreamTrack;
    } catch {
      // Keep the old track: better a rejoin the desk may not hear than none at all.
    }
  }

  async #afterRemoval(): Promise<void> {
    // Give the state stream a moment to say "ended"; if it does not, it was a link fault.
    await new Promise((r) => setTimeout(r, 2500));
    if ((this.link as Link) !== 'ended') await this.#lost();
  }

  async #lost(): Promise<void> {
    this.link = 'reconnecting';
    this.heldUntil = rejoinUntil(this.role, Date.now(), this.joined.pass_expires_at_ms);
    if (!(await this.#rejoin()) && (this.link as Link) !== 'ended') this.link = 'lost';
  }

  /** New room tokens for the same identity until the held place or the pass runs out. */
  async #rejoin(): Promise<boolean> {
    const until = this.heldUntil ?? rejoinUntil(this.role, Date.now(), this.joined.pass_expires_at_ms);
    this.heldUntil = until;
    for (let n = 0; Date.now() < until && !this.#leaving; n++) {
      await new Promise((r) => setTimeout(r, n === 0 ? 300 : backoff(n - 1)));
      try {
        this.joined = await api.passToken(this.joined.pass);
      } catch (e) {
        if (e instanceof api.ApiError && (e.status === 410 || e.status === 401)) {
          this.problem = e.message;
          await this.#end();
          return false;
        }
        continue;
      }
      await this.#drop(true);
      await this.#freshMic();
      if (this.#camera) await this.#takeCamera();
      if (await this.#connect()) return true;
    }
    return false;
  }
}
