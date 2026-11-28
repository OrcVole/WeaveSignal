// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Which microphone a join page uses (1.3.3): the list a person chooses from, the choice kept on
// this device, what the browser is asked for, and the sentences about it. Pure apart from the
// guarded reads of browser storage and of the browser's own list, so the rules are tested.
//
// - A browser names its microphones only once the page has been given one, so there is no list
//   before that. The remembered microphone is asked for exactly (`exact`), and when the browser
//   says it is not there the page asks again for the browser's own choice. Not as a preference
//   (`ideal`): Chromium takes its default microphone whatever id is preferred (tried 2026-11-27
//   with every form of a non-exact id), so a preference would quietly be ignored.
// - A change the person makes is asked for exactly too: that microphone or none, so the one in
//   use stays in use when the new one cannot be had.
// - Every capture keeps the join pages' rules (rules.ts: echo cancelling and noise suppression
//   on, automatic gain control off), whichever microphone it is. The echo test page has its own
//   settings and takes whatever the browser gives it.

import type { VoiceCapture } from '../../../../packages/ui/media.ts';
import { voiceCapture } from './rules.ts';

/** A microphone as the page offers it. */
export interface Mic {
  id: string;
  label: string;
}

/** What of the browser's device list these rules read. */
export type Listed = Pick<MediaDeviceInfo, 'kind' | 'deviceId' | 'label' | 'groupId'>;

/** Chromium lists the system's microphone a second time under these ids ("Default - …"; on
 *  Windows also "Communications - …"). */
const ALIAS = new Set(['default', 'communications']);

/** The microphones to choose from: the audio inputs the browser has named. Before the page has
 *  been given a microphone the browser names none, and the list is empty. Chromium's second
 *  entries for the system's microphone are left out when the microphone itself is listed (the
 *  same group), so nothing is offered twice. */
export function mics(devices: readonly Listed[]): Mic[] {
  const named = devices.filter((d) => d.kind === 'audioinput' && d.deviceId && d.label);
  const real = named.filter((d) => !ALIAS.has(d.deviceId));
  return named.filter((d) => !ALIAS.has(d.deviceId) || !real.some((r) => r.groupId && r.groupId === d.groupId)).map((d) => ({ id: d.deviceId, label: d.label }));
}

/** The microphone a track is on, as the list names it. Chromium says `default` for the system's
 *  microphone: that is the listed one of the same group. A browser that gives no id is matched
 *  by the track's label. */
export function inUse(settings: { deviceId?: string; groupId?: string }, label: string, devices: readonly Listed[]): Mic | null {
  const list = mics(devices);
  const id = settings.deviceId ?? '';
  if (id && !ALIAS.has(id)) return list.find((m) => m.id === id) ?? (label ? { id, label } : null);
  if (id) {
    const listed = list.find((m) => m.id === id);
    if (listed) return listed;
    const group = settings.groupId ?? devices.find((d) => d.kind === 'audioinput' && d.deviceId === id)?.groupId;
    const real = devices.find((d) => d.kind === 'audioinput' && !ALIAS.has(d.deviceId) && d.label && group && d.groupId === group);
    if (real) return { id: real.deviceId, label: real.label };
  }
  const byLabel = label ? list.find((m) => m.label === label) : undefined;
  return byLabel ?? (id && label ? { id, label } : null);
}

const KEY = 'weavesignal.mic';

/** The microphone the person chose on this device, if they ever chose one here. Browser storage
 *  can be refused (private windows, blocked site data): then nothing is remembered and the page
 *  works as before. */
export function chosen(raw?: string | null): Mic | null {
  let text = raw;
  if (text === undefined) {
    try {
      text = localStorage.getItem(KEY);
    } catch {
      return null;
    }
  }
  if (!text) return null;
  try {
    const m = JSON.parse(text) as Partial<Mic> | null;
    return m && typeof m.id === 'string' && m.id && typeof m.label === 'string' ? { id: m.id, label: m.label } : null;
  } catch {
    return null;
  }
}

/** Keep the person's choice on this device. Only a choice they made is kept, never the
 *  microphone the browser happened to give. */
export function remember(m: Mic): void {
  try {
    localStorage.setItem(KEY, JSON.stringify({ id: m.id, label: m.label }));
  } catch {
    // Refused: the choice lasts while the page does.
  }
}

// The microphone this page last captured, until it is reloaded: what was checked before joining
// is what the call asks for, and a rejoin asks for the one the call was on.
let last: Mic | null = null;

/** Note the microphone a capture gave. */
export function used(m: Mic | null): void {
  if (m) last = m;
}

/** Forget the page's own note (tests). */
export function forgetUsed(): void {
  last = null;
}

/**
 * The microphone to ask the browser for, or null for the browser's own choice. The person's
 * choice first: by id, or by name when its id has changed (a browser gives new ids when its
 * site data is cleared). A choice that is not in the list is not asked for; the list is empty
 * while the browser names nothing, and then the choice is asked for as a preference anyway.
 * Failing that, the microphone this page last used.
 */
export function wanted(list: readonly Mic[], choice: Mic | null = chosen(), lastUsed: Mic | null = last): Mic | null {
  const there = (m: Mic) => list.length === 0 || list.some((x) => x.id === m.id);
  if (choice) {
    if (there(choice)) return choice;
    const renamed = choice.label ? list.find((m) => m.label === choice.label) : undefined;
    if (renamed) return renamed;
  }
  if (lastUsed && there(lastUsed)) return lastUsed;
  return null;
}

/** What the page captures with: the join pages' rules, and exactly the microphone named, or
 *  the browser's own choice when none is. */
export function micCapture(want: Pick<Mic, 'id'> | null): VoiceCapture & { deviceId?: { exact: string } } {
  const rules = voiceCapture();
  return want ? { ...rules, deviceId: { exact: want.id } } : rules;
}

/** The same for the room client's own capture (`createLocalAudioTrack`), which takes a bare id
 *  as exact and asks again without it when the browser says that microphone is not there. */
export function micOptions(want: Pick<Mic, 'id'> | null): VoiceCapture & { deviceId?: string } {
  const rules = voiceCapture();
  return want ? { ...rules, deviceId: want.id } : rules;
}

/** Whether a capture failed because the microphone asked for is not there (unplugged since it
 *  was chosen), rather than refused or busy. */
export function notThere(e: unknown): boolean {
  const err = e instanceof DOMException ? e.name : ((e as { name?: string } | null)?.name ?? '');
  return err === 'OverconstrainedError' || err === 'NotFoundError';
}

/** Capture for the microphone check: the microphone wanted, exactly; if it is not there, the
 *  browser's own choice. Any other refusal is the caller's to say. */
export async function captureMic(want: Pick<Mic, 'id'> | null): Promise<MediaStream> {
  try {
    return await navigator.mediaDevices.getUserMedia({ audio: micCapture(want) });
  } catch (e) {
    if (!want || !notThere(e)) throw e;
    return navigator.mediaDevices.getUserMedia({ audio: micCapture(null) });
  }
}

/** The capture for a change of microphone during a call: everything the track in use was
 *  captured with, the join pages' rules over it, and exactly the new microphone. */
export function sameCapture(inUse: MediaTrackConstraints, to: Pick<Mic, 'id'>): MediaTrackConstraints {
  return { ...inUse, ...voiceCapture(), deviceId: { exact: to.id } };
}

const name = (label: string | undefined, fallback: string): string => label?.trim() || fallback;

/** After a capture: the sentence when the microphone chosen before was not there. Nothing when
 *  it is listed (the person, or their browser's own question, picked another this time). */
export function missingWords(choice: Mic | null, using: Mic | null, list: readonly Mic[]): string | null {
  if (!choice || !using || list.length === 0) return null;
  if (using.id === choice.id || (choice.label && using.label === choice.label)) return null;
  if (list.some((m) => m.id === choice.id || (choice.label && m.label === choice.label))) return null;
  return `The microphone you chose before, ${name(choice.label, 'your usual one')}, is not connected. This page is using ${name(using.label, 'another microphone')} instead.`;
}

/** After the person changed microphone. */
export function nowUsing(to: Mic, inCall: boolean): string {
  const which = name(to.label, 'the microphone you chose');
  return inCall ? `Now using ${which}. The studio hears you through it.` : `Now using ${which}. This page will use it next time too.`;
}

/** Why a change did not happen, and that the microphone in use carries on. */
export function switchProblem(e: unknown, to: Mic, kept: Mic | null): string {
  const err = e instanceof DOMException ? e.name : ((e as { name?: string } | null)?.name ?? '');
  const which = name(to.label, 'That microphone');
  const still = kept ? `this page is still using ${name(kept.label, 'the microphone it had')}` : 'nothing has changed';
  if (err === 'NotAllowedError' || err === 'SecurityError') return `The browser did not allow ${which}, so ${still}. Allow it when the browser asks, then choose it again.`;
  if (err === 'NotFoundError' || err === 'OverconstrainedError') return `${which} is no longer connected, so ${still}.`;
  if (err === 'NotReadableError' || err === 'AbortError') return `Another app is using ${which}, so ${still}. Close that app, then choose it again.`;
  return `${which} could not be started, so ${still}.`;
}

/** The browser moved the call to another microphone by itself: the one in use was unplugged or
 *  taken away (`stopped`), or the system's own choice changed. A microphone that stopped and
 *  could be started again (a loose plug, a headset that reconnected) is said as that. */
export function movedWords(from: Mic | null, to: Mic, stopped: boolean): string {
  const now = name(to.label, 'another microphone');
  if (stopped && from && from.id === to.id) return `${now} stopped for a moment. This page is using it again.`;
  if (stopped && from) return `${name(from.label, 'Your microphone')} stopped, so this page is now using ${now}.`;
  return `This page is now using ${now}.`;
}

/** No microphone is sending: the one in use stopped and no other could be started. */
export const MIC_STOPPED = 'Your microphone has stopped, so the studio cannot hear you. Plug it back in, or choose another one here.';

/** The browser's device list, guarded: a browser without one gives an empty list. */
export async function listed(): Promise<Listed[]> {
  try {
    return (await navigator.mediaDevices.enumerateDevices()) as Listed[];
  } catch {
    return [];
  }
}
