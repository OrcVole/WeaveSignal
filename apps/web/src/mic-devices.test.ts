// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Which microphone a join page uses (1.3.3): the list as Chromium and Firefox give it, the
// choice kept on the device and what happens when it is gone, what the browser is asked for
// (always with the join pages' rules), and the sentences.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ECHO_TEST_CAPTURE } from '../../../packages/ui/media.ts';
import { type Listed, type Mic, MIC_STOPPED, captureMic, chosen, forgetUsed, inUse, micCapture, micOptions, mics, missingWords, movedWords, notThere, nowUsing, remember, sameCapture, switchProblem, used, wanted } from './lib/mic-devices.ts';

const input = (deviceId: string, label: string, groupId = `g-${deviceId}`): Listed => ({ kind: 'audioinput', deviceId, label, groupId });
// A desk in Chromium on Windows: the system's microphone three times, a headset, and a camera.
const CHROMIUM: Listed[] = [
  input('default', 'Default - Microphone (Built-in Audio)', 'g-built-in'),
  input('communications', 'Communications - Microphone (Built-in Audio)', 'g-built-in'),
  input('aa11', 'Microphone (Built-in Audio)', 'g-built-in'),
  input('bb22', 'USB headset', 'g-headset'),
  { kind: 'videoinput', deviceId: 'cam', label: 'HD Webcam', groupId: 'g-cam' },
  { kind: 'audiooutput', deviceId: 'out', label: 'Speakers (Built-in Audio)', groupId: 'g-built-in' },
];
// The same desk in Firefox: no second entries, the system's microphone first.
const FIREFOX: Listed[] = [input('ff-1', 'Microphone (Built-in Audio)'), input('ff-2', 'USB headset')];
const BUILT_IN_MIC: Mic = { id: 'aa11', label: 'Microphone (Built-in Audio)' };
const HEADSET: Mic = { id: 'bb22', label: 'USB headset' };

describe('the microphones a page offers', () => {
  it('offers nothing before the browser names them (no permission yet)', () => {
    // Both browsers list one nameless input until the page has been given a microphone.
    expect(mics([{ kind: 'audioinput', deviceId: '', label: '', groupId: '' }])).toEqual([]);
    expect(mics([{ kind: 'audioinput', deviceId: 'x', label: '', groupId: 'g' }])).toEqual([]);
    expect(mics([])).toEqual([]);
  });

  it('offers each microphone once: Chromium’s Default and Communications entries are left out when the microphone itself is listed', () => {
    expect(mics(CHROMIUM)).toEqual([BUILT_IN_MIC, HEADSET]);
    expect(mics(FIREFOX)).toEqual([
      { id: 'ff-1', label: 'Microphone (Built-in Audio)' },
      { id: 'ff-2', label: 'USB headset' },
    ]);
  });

  it('keeps a Default entry that stands for nothing else in the list (headless Chromium’s fake devices)', () => {
    const fake = [input('default', 'Fake Default Audio Input', 'g-d'), input('f1', 'Fake Audio Input 1', 'g-1'), input('f2', 'Fake Audio Input 2', 'g-2')];
    expect(mics(fake).map((m) => m.label)).toEqual(['Fake Default Audio Input', 'Fake Audio Input 1', 'Fake Audio Input 2']);
  });

  it('names the microphone a track is on: Chromium’s "default" is the listed microphone of the same group', () => {
    expect(inUse({ deviceId: 'default', groupId: 'g-built-in' }, 'Default - Microphone (Built-in Audio)', CHROMIUM)).toEqual(BUILT_IN_MIC);
    expect(inUse({ deviceId: 'bb22', groupId: 'g-headset' }, 'USB headset', CHROMIUM)).toEqual(HEADSET);
    expect(inUse({ deviceId: 'ff-2' }, 'USB headset', FIREFOX)).toEqual({ id: 'ff-2', label: 'USB headset' });
    // No id from the browser: by the track's own label.
    expect(inUse({}, 'USB headset', FIREFOX)).toEqual({ id: 'ff-2', label: 'USB headset' });
    // A microphone the list no longer has (just unplugged) still has its name.
    expect(inUse({ deviceId: 'zz99' }, 'USB Microphone', FIREFOX)).toEqual({ id: 'zz99', label: 'USB Microphone' });
    expect(inUse({}, '', [])).toBeNull();
  });
});

describe('the choice kept on this device', () => {
  beforeEach(() => forgetUsed());

  it('reads only a whole choice, and nothing from a browser that refuses storage', () => {
    expect(chosen(JSON.stringify(HEADSET))).toEqual(HEADSET);
    expect(chosen(null)).toBeNull();
    expect(chosen('')).toBeNull();
    expect(chosen('not json')).toBeNull();
    expect(chosen('{"id":""}')).toBeNull();
    expect(chosen('{"label":"x"}')).toBeNull();
    // Node has no localStorage here: the guarded read gives nothing, and the write does not throw.
    expect(chosen()).toBeNull();
    expect(() => remember(HEADSET)).not.toThrow();
  });

  it('asks for the chosen microphone while the browser names nothing, and when it is listed', () => {
    expect(wanted([], HEADSET, null)).toEqual(HEADSET);
    expect(wanted(mics(CHROMIUM), HEADSET, null)).toEqual(HEADSET);
  });

  it('falls back cleanly when the chosen microphone is gone: nothing is asked for by name', () => {
    expect(wanted([BUILT_IN_MIC], HEADSET, null)).toBeNull();
    // The microphone this page last used is asked for instead, if it is still there.
    expect(wanted([BUILT_IN_MIC], HEADSET, BUILT_IN_MIC)).toEqual(BUILT_IN_MIC);
    expect(wanted([BUILT_IN_MIC], HEADSET, { id: 'gone', label: 'Gone' })).toBeNull();
  });

  it('finds the chosen microphone by name when its id has changed', () => {
    const renamed = { id: 'new-id', label: HEADSET.label };
    expect(wanted([BUILT_IN_MIC, renamed], HEADSET, null)).toEqual(renamed);
  });

  it('with no choice, asks for the microphone the page last used (what was checked is what joins)', () => {
    expect(wanted([], null, null)).toBeNull();
    used(HEADSET);
    expect(wanted([BUILT_IN_MIC, HEADSET], null)).toEqual(HEADSET);
    used(null);
    expect(wanted([BUILT_IN_MIC, HEADSET], null)).toEqual(HEADSET);
    // A choice comes before it.
    expect(wanted([BUILT_IN_MIC, HEADSET], BUILT_IN_MIC)).toEqual(BUILT_IN_MIC);
  });
});

describe('what the browser is asked for', () => {
  const RULES = { echoCancellation: true, noiseSuppression: true, autoGainControl: false };

  afterEach(() => vi.unstubAllGlobals());

  it('always the join pages’ rules: echo cancelling and noise suppression on, gain control off', () => {
    expect(micCapture(null)).toEqual(RULES);
    expect(micCapture(HEADSET)).toEqual({ ...RULES, deviceId: { exact: 'bb22' } });
    expect(micOptions(null)).toEqual(RULES);
    expect(micOptions(HEADSET)).toEqual({ ...RULES, deviceId: 'bb22' });
  });

  it('a microphone is asked for exactly, never as a preference: Chromium ignores a preferred id', () => {
    expect(micCapture(HEADSET).deviceId).toEqual({ exact: 'bb22' });
    expect(JSON.stringify([micCapture(HEADSET), micOptions(HEADSET), sameCapture({}, HEADSET)])).not.toMatch(/ideal/);
  });

  it('a microphone that is not there is asked for once, then the browser’s own choice, with the rules both times', async () => {
    const asked: unknown[] = [];
    const getUserMedia = vi.fn(async (c: { audio: MediaTrackConstraints }) => {
      asked.push(c.audio);
      if (c.audio.deviceId) throw new DOMException('gone', 'OverconstrainedError');
      return 'the browser’s own' as unknown as MediaStream;
    });
    vi.stubGlobal('navigator', { mediaDevices: { getUserMedia } });
    expect(await captureMic(HEADSET)).toBe('the browser’s own');
    expect(asked).toEqual([{ ...RULES, deviceId: { exact: 'bb22' } }, RULES]);
    // With nothing wanted, one question.
    asked.length = 0;
    await captureMic(null);
    expect(asked).toEqual([RULES]);
  });

  it('a refusal is not asked about twice: only "not there" falls back', async () => {
    const getUserMedia = vi.fn(async () => {
      throw new DOMException('no', 'NotAllowedError');
    });
    vi.stubGlobal('navigator', { mediaDevices: { getUserMedia } });
    await expect(captureMic(HEADSET)).rejects.toThrow('no');
    expect(getUserMedia).toHaveBeenCalledTimes(1);
    expect(notThere(new DOMException('x', 'OverconstrainedError'))).toBe(true);
    expect(notThere(new DOMException('x', 'NotFoundError'))).toBe(true);
    expect(notThere(new DOMException('x', 'NotReadableError'))).toBe(false);
  });

  it('a change during a call keeps what the track in use was captured with, the rules over it, and only the microphone new', () => {
    const was = { deviceId: { ideal: 'default' }, echoCancellation: true, noiseSuppression: true, autoGainControl: false, voiceIsolation: true } as MediaTrackConstraints;
    expect(sameCapture(was, HEADSET)).toEqual({ ...was, deviceId: { exact: 'bb22' } });
    // Whatever the old track said, the rules hold.
    expect(sameCapture({ autoGainControl: true, echoCancellation: false, noiseSuppression: false }, HEADSET)).toEqual({ ...RULES, deviceId: { exact: 'bb22' } });
  });

  it('never the echo test’s settings: that page keeps its own', () => {
    expect(ECHO_TEST_CAPTURE).toEqual({ echoCancellation: false, noiseSuppression: false, autoGainControl: false });
    expect(micCapture(HEADSET).echoCancellation).toBe(true);
  });
});

describe('the sentences', () => {
  it('says when the microphone chosen before is not connected, and which one is used instead', () => {
    expect(missingWords(HEADSET, BUILT_IN_MIC, [BUILT_IN_MIC])).toBe('The microphone you chose before, USB headset, is not connected. This page is using Microphone (Built-in Audio) instead.');
  });

  it('says nothing when the chosen microphone is in use, is listed, or the list is not known', () => {
    expect(missingWords(HEADSET, HEADSET, [BUILT_IN_MIC, HEADSET])).toBeNull();
    // Listed, and another is in use: the person, or Firefox's own question, picked it this time.
    expect(missingWords(HEADSET, BUILT_IN_MIC, [BUILT_IN_MIC, HEADSET])).toBeNull();
    expect(missingWords(HEADSET, { id: 'new-id', label: HEADSET.label }, [BUILT_IN_MIC])).toBeNull();
    expect(missingWords(null, BUILT_IN_MIC, [BUILT_IN_MIC])).toBeNull();
    expect(missingWords(HEADSET, null, [BUILT_IN_MIC])).toBeNull();
    expect(missingWords(HEADSET, BUILT_IN_MIC, [])).toBeNull();
  });

  it('after a change: before joining it is kept for next time, during a call the studio hears it', () => {
    expect(nowUsing(HEADSET, false)).toBe('Now using USB headset. This page will use it next time too.');
    expect(nowUsing(HEADSET, true)).toBe('Now using USB headset. The studio hears you through it.');
  });

  it('a change that did not happen says why, and that the microphone in use carries on', () => {
    const say = (name: string) => switchProblem(new DOMException('x', name), HEADSET, BUILT_IN_MIC);
    expect(say('NotAllowedError')).toBe('The browser did not allow USB headset, so this page is still using Microphone (Built-in Audio). Allow it when the browser asks, then choose it again.');
    expect(say('OverconstrainedError')).toBe('USB headset is no longer connected, so this page is still using Microphone (Built-in Audio).');
    expect(say('NotFoundError')).toBe(say('OverconstrainedError'));
    expect(say('NotReadableError')).toBe('Another app is using USB headset, so this page is still using Microphone (Built-in Audio). Close that app, then choose it again.');
    expect(switchProblem(new Error('x'), HEADSET, BUILT_IN_MIC)).toBe('USB headset could not be started, so this page is still using Microphone (Built-in Audio).');
    expect(switchProblem(new DOMException('x', 'NotAllowedError'), HEADSET, null)).toMatch(/so nothing has changed\./);
  });

  it('a microphone the browser moved by itself, and one that stopped with nothing to take its place', () => {
    expect(movedWords(HEADSET, BUILT_IN_MIC, true)).toBe('USB headset stopped, so this page is now using Microphone (Built-in Audio).');
    expect(movedWords(HEADSET, BUILT_IN_MIC, false)).toBe('This page is now using Microphone (Built-in Audio).');
    expect(movedWords(null, BUILT_IN_MIC, true)).toBe('This page is now using Microphone (Built-in Audio).');
    // Stopped and started again on the same one (a loose plug): not "X stopped, so now using X".
    expect(movedWords(HEADSET, HEADSET, true)).toBe('USB headset stopped for a moment. This page is using it again.');
    expect(MIC_STOPPED).toMatch(/the studio cannot hear you/);
  });
});
