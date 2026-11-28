// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// One person in the room from a browser: a caller from the player page, or a guest, remote DJ
// or presenter from an invite. It publishes the microphone with the join pages' constraints,
// hears only its own mix-minus, follows its own state from the control plane, offers the relay
// (TURN over TLS), and rejoins under the same identity when the link drops: within the held
// 5 minutes for a caller (ADR-0101), while the pass lasts for anyone else.
//
// The microphone can be changed during the call (1.3.3). The new one is captured first and then
// put in the old one's place on the same published track (`replaceTrack`): the room sees one
// publication carry on, so the desk's subscription is never made again, and a microphone that
// cannot be had leaves the one in use running. A rejoin publishes that same track again.

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
  TrackEvent,
  createLocalAudioTrack,
  createLocalVideoTrack,
} from 'livekit-client';
import * as api from './api.ts';
import { CAMERA_PUBLISH, backoff, cameraCapture, connectOptions, limitSubscribers, offersCamera, rejoinUntil, shouldHear, shouldSee, voiceCapture } from './rules.ts';
import { cameraProblem, micProblem } from './mic.ts';
import { type Mic, MIC_STOPPED, chosen, micOptions, missingWords, movedWords, nowUsing, remember, sameCapture, switchProblem, used, wanted } from './mic-devices.ts';
import { MicList } from './mics.svelte.ts';
import { MOVE_RETRY_MS, adoptInBrowser, type Moved } from './move.ts';
import type { CameraHandover } from './camera-preview.svelte.ts';
import * as store from './store.ts';
import { installStationIce } from './turn.ts';
import type { Link } from './words.ts';

/** What a call is told besides its pass. */
export interface CallOptions {
  /** Where the link leads (1.3). A rehearsal link joins the rehearsal room and, when the presenter
   *  moves everyone, moves on to the live studio. */
  studio?: api.Studio;
  /** The rehearsal this person was moved from, when the page learned it before joining. */
  moved?: Moved | null;
  /** Called with the new pass when a move swaps it in; by default the page's own fragment is
   *  swapped (`history.replaceState`). */
  onPass?: (pass: string) => void;
}

export class Call {
  link = $state<Link>('idle');
  /** Where this call is: the rehearsal studio until a move, then the live studio. */
  studio = $state<api.Studio>('live');
  /** Whether they have been on air in the live studio since they joined. `off_air` alone cannot
   *  tell "standing by" (every invitation arrives so) from "taken off". */
  wasOnAir = $state(false);
  /** The rehearsal they were moved from, until they are on air. */
  moved = $state<Moved | null>(null);
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
  /** The browser's microphones, named once it has given us one, and kept current. */
  mics = new MicList();
  /** The microphone in use, as the list names it. */
  micInUse = $state<Mic | null>(null);
  /** A sentence about the microphone: which one is in use after a change, or why a change did
   *  not happen (`micTrouble`). */
  micNote = $state<string | null>(null);
  micTrouble = $state(false);
  /** A change of microphone is under way. */
  micChanging = $state(false);
  /** No microphone is sending: the one in use stopped and no other could be started. */
  micStopped = $state(false);
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
  /** A move is under way: a second `move` frame is ignored (the server sends it on every frame). */
  #moving = false;
  /** Counts the passes this call has followed; a loop or a state frame of an older one stops. */
  #gen = 0;
  #onPass: (pass: string) => void;
  #heartbeat: ReturnType<typeof setInterval> | undefined;

  /** The camera the join page already showed the person (ADR-0171), taken at start. */
  #handover: CameraHandover | null;
  /** The microphone in use ended (unplugged, taken back), until the next one is read. */
  #micEnded = false;
  /** The captures whose end is already listened for. */
  #heard = new WeakSet<MediaStreamTrack>();

  constructor(joined: api.Joined, relay = false, camera: CameraHandover | null = null, opts: CallOptions = {}) {
    this.joined = joined;
    this.relay = relay;
    this.#handover = camera;
    this.studio = opts.studio ?? 'live';
    this.moved = opts.moved ?? null;
    this.#onPass = opts.onPass ?? adoptInBrowser;
  }

  get role(): api.Role {
    return this.joined.role;
  }

  /** Join: follow our state, take the microphone, connect; through the relay if direct fails. */
  async start(): Promise<void> {
    // Again after a refused microphone (Try again): the refusal's sentence goes with it.
    this.problem = null;
    this.#watch();
    this.link = 'connecting';
    try {
      // The microphone chosen on this device, or the one just checked; the browser's own
      // choice when it is not there.
      const want = wanted(await this.mics.refresh());
      this.#track = await createLocalAudioTrack(micOptions(want));
      await this.#ownMic(this.#track);
      this.#say(missingWords(chosen(), this.micInUse, this.mics.list));
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

  /** Follow our own state on the current pass; frames of an older pass are dropped. */
  #watch(): void {
    const gen = this.#gen;
    this.#stopMe?.();
    this.#stopMe = api.followMe(
      this.joined.pass,
      (s) => {
        if (gen === this.#gen) this.#state(s);
      },
      (why) => {
        if (gen !== this.#gen) return;
        this.problem = why;
        void this.#end();
      },
    );
  }

  #state(s: api.MeState): void {
    this.me = s;
    if (s.studio) this.studio = s.studio;
    if (s.studio !== 'rehearsal' && s.phase === 'on_air') this.wasOnAir = true;
    // The presenter moved everyone (1.3): the server offers each page its live pass, and offers it
    // again on every frame and after every reconnection, so only the first one moves the page.
    if (s.studio === 'rehearsal' && s.move && !this.#moving) {
      void this.#moveToLive(s.move);
      return;
    }
    if (s.phase === 'ended' && !this.#moving) void this.#end();
  }

  /**
   * Move from the rehearsal room to the live room, keeping the microphone. In this order:
   * the fragment takes the live pass first (a reload now lands in the live studio), the old
   * stream and anything the old pass was retrying stop, the stream follows the new pass, then
   * the room is left with the tracks kept and the live room joined as a rejoin does: a token
   * for the new pass, the microphone (a fresh one only if it ended) and the camera, which the
   * rehearsal had none of. Nothing here puts anyone on air: the live desk keeps them standing by.
   */
  async #moveToLive(move: api.MoveTo): Promise<void> {
    this.#moving = true;
    const gen = ++this.#gen;
    this.#onPass(move.pass);
    this.#stopMe?.();
    this.#stopMe = null;
    this.me = null;
    this.problem = null;
    this.link = 'moving';
    this.moved = { show: move.show, airing_start_ms: move.airing_start_ms };
    this.joined = { ...this.joined, pass: move.pass };
    this.#watch();
    await this.#drop(true);
    this.heldUntil = Date.now() + MOVE_RETRY_MS;
    const ok = await this.#rejoin();
    if (gen !== this.#gen) return;
    this.#moving = false;
    if (ok) {
      this.studio = 'live';
      this.heldUntil = null;
    } else if ((this.link as Link) !== 'ended') {
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
    this.mics.stop();
    this.#track?.off(TrackEvent.Restarted, this.#micRestarted);
    this.#track?.off(TrackEvent.Muted, this.#micMuted);
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

  /** A capture becomes the call's microphone: its level, its name, and what the room client
   *  does to it by itself (it starts the track again on another device when the one in use is
   *  unplugged, and mutes it when it cannot). */
  async #ownMic(track: LocalAudioTrack): Promise<void> {
    track.on(TrackEvent.Restarted, this.#micRestarted);
    track.on(TrackEvent.Muted, this.#micMuted);
    await this.#readMic();
    this.mics.watch(this.#micsChanged);
  }

  /** Read which microphone the track is on, from the track itself and never from what was asked
   *  for: a browser's own question can give another. */
  async #readMic(): Promise<void> {
    const t = this.#track?.mediaStreamTrack ?? null;
    this.mic = t;
    await this.mics.refresh();
    this.micInUse = this.mics.of(t);
    used(this.micInUse);
    if (t && t.readyState === 'live') {
      this.micStopped = false;
      if (!this.#heard.has(t)) {
        this.#heard.add(t);
        t.addEventListener('ended', () => {
          if (this.#track?.mediaStreamTrack === t && !this.#leaving) this.#micEnded = true;
        });
      }
    }
  }

  #say(note: string | null, trouble = false): void {
    this.micNote = note;
    this.micTrouble = !!note && trouble;
  }

  /** The room client started our track again by itself, on whatever the browser gave it. */
  #micRestarted = (): void => {
    const from = this.micInUse;
    const stopped = this.#micEnded;
    this.#micEnded = false;
    void this.#readMic().then(() => {
      if (this.micChanging || !this.micInUse) return;
      if (from?.id !== this.micInUse.id || stopped) this.#say(movedWords(from, this.micInUse, stopped));
    });
  };

  /** We never mute the microphone: a mute is the room client giving up on a track that ended. */
  #micMuted = (): void => {
    if (this.#leaving || this.micChanging) return;
    this.micStopped = true;
    this.#say(MIC_STOPPED, true);
    void this.mics.refresh();
  };

  /** A microphone was plugged in or taken away. While none is sending, take the first one that
   *  appears: nothing is being heard that a change could interrupt. A microphone chosen before
   *  that comes back while another is sending is offered in the list and not switched to. */
  #micsChanged = (): void => {
    if (!this.micStopped || this.micChanging || this.link !== 'connected') return;
    const to = wanted(this.mics.list) ?? this.mics.list[0];
    if (to) void this.useMic(to.id, false);
  };

  /**
   * Change microphone during the call. The new one is captured first, with everything the one
   * in use was captured with, and put in its place on the same published track; the room client
   * then stops the old one. Nothing is unpublished and nothing rejoins, so the desk goes on
   * hearing the same publication. If the new one cannot be had (the browser asks and is refused,
   * another app holds it, it was unplugged) the microphone in use carries on and the page says so.
   */
  async useMic(id: string, chosenByPerson = true): Promise<void> {
    const track = this.#track;
    const room = this.#room;
    const to = this.mics.list.find((m) => m.id === id);
    if (!track || !room || !to || this.micChanging || this.link !== 'connected') return;
    if (to.id === this.micInUse?.id && !this.micStopped) return;
    const from = this.micStopped ? null : this.micInUse;
    this.micChanging = true;
    this.#say(null);
    let fresh: MediaStreamTrack | null = null;
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: sameCapture(track.mediaStreamTrack.getConstraints(), to) });
      fresh = stream.getAudioTracks()[0];
      // The link dropped while the browser asked: there is nothing to put it into just now.
      if (this.#track !== track || this.#room !== room || (this.link as Link) !== 'connected') throw new Error('not connected');
      await track.replaceTrack(fresh, { userProvidedTrack: false });
      fresh = null;
      if (track.isMuted) await track.unmute();
      this.#micEnded = false;
      await this.#readMic();
      // The room client moves a microphone it believes is the system's own; tell it which this is.
      const now = track.mediaStreamTrack.getSettings().deviceId;
      if (now) room.localParticipant.activeDeviceMap.set('audioinput', now);
      if (chosenByPerson) remember(this.micInUse ?? to);
      this.#say(chosenByPerson ? nowUsing(this.micInUse ?? to, true) : movedWords(null, this.micInUse ?? to, false));
    } catch (e) {
      fresh?.stop();
      if (this.micStopped) this.#say(MIC_STOPPED, true);
      else this.#say(switchProblem(e, to, from), true);
    } finally {
      this.micChanging = false;
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
    installStationIce();
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
      const fresh = await createLocalAudioTrack(micOptions(wanted(await this.mics.refresh())));
      this.#track?.off(TrackEvent.Restarted, this.#micRestarted);
      this.#track?.off(TrackEvent.Muted, this.#micMuted);
      this.#track?.stop();
      this.#track = fresh;
      await this.#ownMic(fresh);
    } catch {
      // Keep the old track: better a rejoin the desk may not hear than none at all.
    }
  }

  async #afterRemoval(): Promise<void> {
    // Give the state stream a moment to say "ended"; if it does not, it was a link fault.
    const gen = this.#gen;
    await new Promise((r) => setTimeout(r, 2500));
    if (gen === this.#gen && (this.link as Link) !== 'ended') await this.#lost();
  }

  async #lost(): Promise<void> {
    const gen = this.#gen;
    this.link = 'reconnecting';
    this.heldUntil = rejoinUntil(this.role, Date.now(), this.joined.pass_expires_at_ms);
    // A move that came meanwhile took over (its own rejoin carries on); this one stands down.
    if (!(await this.#rejoin()) && gen === this.#gen && (this.link as Link) !== 'ended') this.link = 'lost';
  }

  /** New room tokens for the same identity until the held place or the pass runs out. */
  async #rejoin(): Promise<boolean> {
    const gen = this.#gen;
    const until = this.heldUntil ?? rejoinUntil(this.role, Date.now(), this.joined.pass_expires_at_ms);
    this.heldUntil = until;
    for (let n = 0; Date.now() < until && !this.#leaving && gen === this.#gen; n++) {
      await new Promise((r) => setTimeout(r, n === 0 ? 300 : backoff(n - 1)));
      if (gen !== this.#gen) return false;
      try {
        const fresh = await api.passToken(this.joined.pass);
        if (gen !== this.#gen) return false;
        this.joined = fresh;
      } catch (e) {
        if (gen !== this.#gen) return false;
        if (e instanceof api.ApiError && (e.status === 410 || e.status === 401)) {
          this.problem = e.message;
          await this.#end();
          return false;
        }
        // A refusal that stays (the show's step has switched this seat off, a closed rehearsal)
        // is said on the page, not left as "Reconnecting" or "Moving" for ever.
        if (e instanceof api.ApiError && e.status === 409) this.problem = e.message;
        continue;
      }
      await this.#drop(true);
      await this.#freshMic();
      // A move takes the camera it did not have in the rehearsal room (which sends none).
      if (this.#camera || this.#moving) await this.#takeCamera();
      if (gen !== this.#gen) return false;
      if (await this.#connect()) {
        this.problem = null;
        return true;
      }
    }
    return false;
  }
}
