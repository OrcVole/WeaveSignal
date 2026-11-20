// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The telephone line (1.2, PLAN 17f): where it stands, a phone caller's card, bans, the
// station's contact, and a recording made in the window.
import { describe, expect, it } from 'vitest';
import { BAN_LENGTHS, DEFAULT_VOICES, WORDS_TIP, banProblem, banUntil, callerName, canSetConsent, contactProblem, deskName, isPhonePerson, lengthWords, lineWords, phoneLine, previewPath, voiceInForce, wordsLine, type PhoneStatus } from './lib/phone.ts';
import { canRecord, encodeWav } from './lib/wav.ts';
import { deskCallers } from './lib/steps.ts';
import type { Caller, View } from './lib/api.ts';

const status = (o: Partial<PhoneStatus> = {}): PhoneStatus => ({
  open: true,
  closed: null,
  lines: 8,
  at_gate: 0,
  in_queue: 0,
  words: { kind: 'default', version: 'd1', text: "You're through to the studio, and your call may go on air and be recorded. Press any key to carry on." },
  words_to_record: 'You’ll be live on Kettle FM. Press any key to carry on.',
  recording: null,
  consent_words_help: 'Before a phone caller joins the queue, play them a few words.',
  ...o,
});

describe('where the phone line stands', () => {
  it('is off until a trunk is set, and says what a trunk is', () => {
    const w = lineWords(status({ open: false, closed: 'off', lines: 0, words: null, words_to_record: null }));
    expect(w).toMatchObject({ state: 'off', word: 'Off', line: 'The phone line is off until a trunk is set.' });
    expect(w.fix).toMatch(/phone provider’s connection/);
  });

  it('open: the lines, and who is at the words and in the queue', () => {
    expect(lineWords(status()).line).toBe('Taking calls on 8 lines.');
    expect(lineWords(status({ lines: 1, at_gate: 2, in_queue: 1 })).line).toBe('Taking calls on 1 line · 2 hearing the words · 1 in the queue.');
  });

  it('closed says why, and what puts it right', () => {
    expect(lineWords(status({ open: false, closed: { words: 'recording_out_of_date' } }))).toMatchObject({ state: 'closed', fix: 'Record the words again below. The line opens as soon as you do.' });
    expect(lineWords(status({ open: false, closed: { words: 'default_missing' } })).line).toMatch(/missing from this installation/);
    expect(lineWords(status({ open: false, closed: { words: 'no_recording' } }))).toMatchObject({ state: 'closed', fix: 'Record the station’s own words below, or choose Alba or Edinburgh.' });
    expect(lineWords(status({ open: false, closed: 'no_key' })).fix).toMatch(/station key or the token key/);
    expect(lineWords(status({ open: false })).line).toBe('The line is on, and call-in is closed just now.');
  });

  it('says which words a caller hears', () => {
    expect(wordsLine(status())).toBe('The short recording that comes with WeaveSignal.');
    expect(wordsLine(status({ words: { kind: 'station', version: 's', text: 'x' } }))).toBe('The station’s own recording.');
    expect(wordsLine(status({ words: { kind: 'off', version: '', text: '' } }))).toMatch(/^Nothing is played/);
    expect(WORDS_TIP).toMatch(/^An internet-only station may not need it\./);
  });
});

describe('which recording a caller hears', () => {
  it('offers Alba, the default, and Edinburgh, with a preview each', () => {
    expect(DEFAULT_VOICES.map((v) => `${v.name} (${v.who})`)).toEqual(['Alba (a Scottish woman)', 'Edinburgh (a man)']);
    expect(previewPath('alba')).toBe('/api/v1/phone/consent/default/alba.opus');
    expect(previewPath('edinburgh')).toBe('/api/v1/phone/consent/default/edinburgh.opus');
  });
  it('is the station’s own recording whenever it has one, else the voice it names', () => {
    expect(voiceInForce(status({ default_voice: 'edinburgh' }))).toBe('edinburgh');
    expect(voiceInForce(status())).toBeNull();
    expect(voiceInForce(status({ default_voice: 'alba', words: { kind: 'station', version: 's', text: 'x' } }))).toBe('own');
    expect(voiceInForce(status({ default_voice: 'edinburgh', words: { kind: 'off', version: '', text: '' } }))).toBe('edinburgh');
  });
  it('is changed from here on a station that says what is in force, with the line on', () => {
    expect(canSetConsent(status())).toBe(false);
    expect(canSetConsent(status({ consent_words: true, default_voice: 'alba' }))).toBe(true);
    expect(canSetConsent(status({ consent_words: true, default_voice: 'alba', closed: 'off' }))).toBe(false);
    expect(canSetConsent(null)).toBe(false);
  });
});

describe('a phone caller’s card', () => {
  it('shows the last four digits at most, never a key or a number', () => {
    const c = { identity: 'phone-x7k2', name: '', phone: { number: '···0123', withheld: false, can_ban: true } };
    expect(phoneLine(c)).toBe('Phone ···0123');
    expect(callerName(c)).toBe('Phone ···0123');
    expect(callerName({ ...c, name: 'Jessie' })).toBe('Jessie');
    expect(callerName({ identity: 'caller-jessie', name: '', phone: null })).toBe('caller-jessie');
  });
  it('a withheld number says so', () => {
    const c = { identity: 'phone-q1', name: '', phone: { number: 'Withheld', withheld: true, can_ban: false } };
    expect(phoneLine(c)).toBe('Withheld number');
    expect(callerName(c)).toBe('Withheld number');
  });
});

describe('number bans and the contact', () => {
  it('a ban needs a reason, and lasts 7, 30 or 90 days or until lifted', () => {
    expect(banProblem('  ')).toMatch(/^Say why/);
    expect(banProblem('Abusive on air')).toBeNull();
    expect(BAN_LENGTHS.map((b) => b.days)).toEqual([7, 30, 90, null]);
    expect(banUntil({ ban_key: 'pb_1', tail: '0123', date: '2026-11-20', reason: 'x', expires_ms: Date.UTC(2026, 11, 20) })).toBe('until 20 December 2026');
    expect(banUntil({ ban_key: 'pb_1', tail: '0123', date: '2026-11-20', reason: 'x', expires_ms: null })).toBe('until it is lifted');
  });
  it('the contact is one line of at most 200 characters; empty hands it back to the settings', () => {
    expect(contactProblem('calls@kettle.example')).toBeNull();
    expect(contactProblem('')).toBeNull();
    expect(contactProblem('a'.repeat(201))).toMatch(/200/);
    expect(contactProblem('a\nb')).toBe('Keep it on one line.');
  });
  it('a recording’s length in words', () => {
    expect(lengthWords(7_400)).toBe('7 s');
    expect(lengthWords(65_000)).toBe('1 min 05 s');
  });
});

describe('a recording made in the window', () => {
  it('is a 16-bit mono WAV the station reads', () => {
    const wav = encodeWav([new Float32Array([0, 1, -1]), new Float32Array([0.5])], 48_000);
    const v = new DataView(wav.buffer);
    const text = (at: number) => String.fromCharCode(...wav.slice(at, at + 4));
    expect([text(0), text(8), text(12), text(36)]).toEqual(['RIFF', 'WAVE', 'fmt ', 'data']);
    expect(wav.length).toBe(44 + 8);
    expect(v.getUint32(4, true)).toBe(36 + 8);
    expect([v.getUint16(20, true), v.getUint16(22, true), v.getUint32(24, true), v.getUint16(34, true)]).toEqual([1, 1, 48_000, 16]);
    expect(v.getUint32(40, true)).toBe(8);
    expect([v.getInt16(44, true), v.getInt16(46, true), v.getInt16(48, true), v.getInt16(50, true)]).toEqual([0, 32767, -32768, 16384]);
  });
  it('is offered only where the window can reach a microphone', () => {
    expect(canRecord({} as typeof globalThis)).toBe(false);
    expect(canRecord({ navigator: { mediaDevices: { getUserMedia: () => {} } }, AudioContext: function () {} } as unknown as typeof globalThis)).toBe(true);
    expect(canRecord({ navigator: { mediaDevices: {} }, AudioContext: function () {} } as unknown as typeof globalThis)).toBe(false);
  });
});

describe('a phone caller in the desk’s queue', () => {
  const caller = (o: Partial<Caller>): Caller => ({ identity: 'c', name: '', ordinal: 1, state: 'waiting', since_ms: 0, arrived_ms: 0, line: '', note: '', name_on_air: '', declined: null, banned_before: null, topic: '', consent: { at_ms: 0, version: 'v', by: 'caller' }, dropped_ms: null, slot: null, on_air: false, cue: false, linked: true, present: true, lufs: null, ...o });
  it('is their line on the row and on Take, never their key; a typed name on air wins', () => {
    const v = { callers: [caller({ identity: 'phone-x7k2', phone: { number: '···0123', withheld: false, can_ban: true } }), caller({ identity: 'caller-dougal', name: 'Dougal Thomson', arrived_ms: 5 })], sources: [] } as unknown as View;
    const q = deskCallers(v);
    expect(q.rows.map((r) => r.name)).toEqual(['Phone ···0123', 'Dougal Thomson']);
    expect(q.next).toEqual({ identity: 'phone-x7k2', name: 'Phone ···0123' });
    const named = deskCallers({ ...v, callers: [caller({ identity: 'phone-x7k2', name_on_air: 'Morven', phone: { number: 'Withheld', withheld: true, can_ban: false } })] } as unknown as View);
    expect(named.next?.name).toBe('Morven');
    expect(named.rows[0].name).toBe('Withheld number');
  });
});

describe('a phone caller at the desk', () => {
  it('keeps the whole of the name the desk has for them, and is never their key', () => {
    expect(isPhonePerson('phone-x7k2')).toBe(true);
    expect(isPhonePerson('phone-')).toBe(false);
    expect(isPhonePerson('caller-jessie')).toBe(false);
    expect(deskName({ name: 'Phone 4471', identity: 'phone-x7k2' })).toBe('Phone 4471');
    expect(deskName({ name: '', identity: 'phone-x7k2' })).toBe('the phone caller');
    expect(deskName({ name: 'Phone 4471', identity: 'phone-x7k2' }, 'Morven')).toBe('Morven');
    expect(deskName({ name: 'Dougal Thomson', identity: 'caller-dougal' })).toBe('Dougal');
    expect(deskName({ name: '', identity: 'caller-dougal' })).toBe('caller-dougal');
  });
});
