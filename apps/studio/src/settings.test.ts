// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Settings > Retention rendered on the server from a view: the Jurisdiction drop-down with the
// fourteen presets (ADR-0114), every period with its source and floor, the No retention
// warning setup must show (ADR-0113), and the holds (ADR-0116); and the words a period prints.
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import { SETTINGS, panel } from '../../../packages/ui/panels.ts';
import Settings from './settings/Settings.svelte';
import {
  daysWords,
  goneWords,
  periodWords,
  underFloor,
  holdKeepUntil,
  holdPastDate,
  type Hold,
  type Kind,
  type Period,
  type PresetView,
  type RetentionView,
  type Row,
} from './lib/retention.ts';

const KINDS: [Kind, string][] = [
  ['programme_recordings', 'Programme recordings and logger'],
  ['per_play_log', 'Per-play music log'],
  ['chat_messages', 'Chat messages'],
  ['moderation_records', 'Moderation records'],
  ['consent', 'Callers’ consent'],
  ['call_recordings', 'Off-air call audio'],
  ['listener_statistics', 'Listener statistics'],
  ['access_logs', 'Server access logs (IP addresses)'],
];

const PRESETS: [string, string][] = [
  ['none', 'No retention'],
  ['uk_ofcom', 'UK, Ofcom-licensed'],
  ['uk_internet', 'UK, internet-only'],
  ['us_webcaster', 'US webcaster (statutory licence)'],
  ['us_nce', 'US noncommercial educational webcaster'],
  ['eu_gdpr', 'EU, GDPR only'],
  ['ie_licensed', 'Ireland, licensed'],
  ['ie_internet', 'Ireland, internet-only (audio)'],
  ['de', 'Germany'],
  ['ca_licensed', 'Canada, licensed'],
  ['ca_internet', 'Canada, internet-only'],
  ['au_licensed', 'Australia, licensed'],
  ['au_internet', 'Australia, internet-only'],
  ['custom', 'Custom'],
];

const days = (d: number): Period => ({ keep: 'days', days: d });

function rows(logger: number, floor: number | null): Row[] {
  const period = (k: Kind): Period =>
    k === 'programme_recordings'
      ? days(logger)
      : k === 'consent'
        ? logger
          ? { keep: 'after_recordings', days: 42 }
          : { keep: 'with_call' }
        : k === 'listener_statistics'
          ? { keep: 'no_limit' }
          : k === 'chat_messages' && logger
            ? { keep: 'with_archive' }
            : days(logger ? 30 : 0);
  return KINDS.map(([kind, label]) => ({
    kind,
    label,
    period: period(kind),
    source: `Source for ${label}.`,
    floor: kind === 'programme_recordings' && floor ? { days: floor, rule: 'Ofcom licence condition 7(2)(a)' } : null,
    swept: ['programme_recordings', 'consent', 'call_recordings'].includes(kind),
  }));
}

function view(preset: string): RetentionView {
  const presets: PresetView[] = PRESETS.map(([key, name]) => ({
    key,
    name,
    summary: `${name}: its rule in a line.`,
    licensed: key === 'uk_ofcom',
    complaint_hold: key === 'ca_licensed' ? { days: 56, rule: 'Radio Regulations, 1986 s.8(5)(b)' } : null,
    rows: key === 'none' || key === 'custom' ? rows(0, null) : rows(42, key === 'uk_ofcom' ? 42 : null),
  }));
  const p = presets.find((x) => x.key === preset)!;
  return {
    setting: {
      preset,
      based_on: null,
      broadcast_licence: null,
      us_statutory_licence: null,
      updated_by: preset === 'none' ? null : 'Morag Ross',
      updated_at_ms: preset === 'none' ? null : Date.UTC(2026, 9, 26, 14, 2),
    },
    rows: p.rows,
    warnings:
      preset === 'none'
        ? [{ kind: null, level: 'serious', title: 'No retention does not meet a broadcast or music licence', message: 'Choose your jurisdiction.', source: 'ADR-0113 and ADR-0114; recce 27' }]
        : [],
    complaint_hold: p.complaint_hold,
    presets,
    presets_as_of: '2026-10-25',
    last_sweep: null,
    sweep_every_s: 3600,
  };
}

const hold: Hold = {
  id: 7,
  reason: 'Ofcom fairness complaint about the 14:00 phone-in',
  placed_by: 'Iain Gunn',
  placed_at_ms: Date.UTC(2026, 9, 26, 15, 0),
  from_ms: Date.UTC(2026, 9, 26, 14, 0),
  to_ms: Date.UTC(2026, 9, 26, 15, 0),
  records: [],
  released_by: null,
  released_at_ms: null,
  release_reason: null,
  extended_until_ms: null,
  extended_by: null,
  extended_at_ms: null,
  extend_reason: null,
  recordings: 1,
  consents: 2,
};

describe('Settings > Retention', () => {
  it('draws its panels under their registered names', () => {
    const { body } = render(Settings, { props: { initial: view('none'), initialHolds: [] } });
    // The page's own panels, in order; its dialogs carry names too (ADR-0245), checked below.
    const all = [...body.matchAll(/<(\w+)[^>]*?data-panel="([^"]+)"/g)];
    const drawn = all.filter((m) => m[1] !== 'dialog').map((m) => m[2]);
    expect(drawn).toEqual(SETTINGS.filter((p) => p.name.split('.').length === 2).map((p) => p.name));
    for (const m of all) expect(panel(m[2]).name).toBe(m[2]);
    expect(all.filter((m) => m[1] === 'dialog').map((m) => m[2])).toEqual(['settings.periods.confirm', 'settings.holds.extend', 'settings.holds.release']);
  });

  it('offers the fourteen presets under the Jurisdiction label, No retention as the default', () => {
    const { body } = render(Settings, { props: { initial: view('none'), initialHolds: [] } });
    expect(body).toContain('<label for="jurisdiction" class="lbl');
    const options = [...body.matchAll(/<option value="([a-z_]+)"[^>]*>([^<]*)</g)].filter(([, v]) => PRESETS.some(([k]) => k === v));
    expect(options.map((o) => o[1])).toEqual(PRESETS.map(([k]) => k));
    expect(options[0][2]).toBe('No retention (default)');
    for (const group of ['United Kingdom', 'United States', 'European Union', 'Canada', 'Australia'])
      expect(body).toContain(`<optgroup label="${group}"`);
    // Setup says plainly that a licensed broadcaster must choose (ADR-0113).
    expect(body).toContain('No retention does not meet a broadcast or music licence');
    expect(body).toContain('Not chosen yet: a new station starts on No retention.');
  });

  it('shows every period with its source, and a floor with its rule', () => {
    const { body } = render(Settings, { props: { initial: view('uk_ofcom'), initialHolds: [] } });
    for (const [, label] of KINDS) {
      expect(body).toContain(label);
      expect(body).toContain(`Source for ${label}.`);
    }
    expect(body).toContain('The rule requires at least 42 days.');
    expect(body).toContain('Set by Morag Ross');
    // Anonymous counts are not editable (ADR-0115), and kinds not kept yet say so.
    expect(body).not.toContain('id="p-listener_statistics"');
    expect(body).toContain('id="p-programme_recordings"');
    expect(body).toContain('WeaveSignal doesn’t keep these yet');
    // Each day field has a label.
    for (const m of body.matchAll(/<input id="(p-[a-z_]+)"/g)) expect(body).toContain(`<label for="${m[1]}"`);
  });

  it('lists open holds with who and why, and says so when there are none', () => {
    const empty = render(Settings, { props: { initial: view('ca_licensed'), initialHolds: [] } }).body;
    expect(empty).toContain('No open holds.');
    expect(empty).toContain('Radio Regulations, 1986 s.8(5)(b)');
    const { body } = render(Settings, { props: { initial: view('ca_licensed'), initialHolds: [hold] } });
    expect(body).toContain('Ofcom fairness complaint about the 14:00 phone-in');
    expect(body).toContain('Placed by Iain Gunn');
    expect(body).toContain('Keeps 1 recording and 2 consent records');
    expect(body).toContain('Release hold…');
    // The 56-day rule shows when to release no earlier than.
    expect(body).toContain('Keep until at least');
    expect(body).toMatch(/<legend[^>]*>Place a hold<\/legend>/);
    expect(body).toContain('15:00 · ');
    expect(body).toContain('Placed by Iain Gunn');
  });
});

describe('Settings > Holds past their date', () => {
  const rule = { days: 56, rule: 'Radio Regulations, 1986 s.8(5)(b)' };
  const day = 86_400_000;

  it('flags an open hold after its keep-until date, never a released or a dateless one (ADR-0156)', () => {
    const h = { from_ms: 10 * day, extended_until_ms: null, released_at_ms: null };
    expect(holdKeepUntil(h, rule)).toBe(66 * day);
    expect(holdPastDate(h, rule, 66 * day)).toBe(false);
    expect(holdPastDate(h, rule, 66 * day + 1)).toBe(true);
    expect(holdPastDate({ ...h, released_at_ms: 1 }, rule, 500 * day)).toBe(false);
    expect(holdPastDate({ ...h, from_ms: null }, rule, 500 * day)).toBe(false);
    expect(holdPastDate(h, { days: null, rule: 'r' }, 500 * day)).toBe(false);
    expect(holdPastDate(h, null, 500 * day)).toBe(false);
  });

  it('an extension replaces the rule’s date and clears the flag until its own (ADR-0156)', () => {
    const h = { from_ms: 10 * day, extended_until_ms: 200 * day, released_at_ms: null };
    expect(holdKeepUntil(h, rule)).toBe(200 * day);
    expect(holdPastDate(h, rule, 100 * day)).toBe(false);
    expect(holdPastDate(h, rule, 200 * day + 1)).toBe(true);
    // A hold the rule gives no date has one once extended.
    expect(holdKeepUntil({ ...h, from_ms: null }, null)).toBe(200 * day);
    expect(holdPastDate({ ...h, from_ms: null }, null, 300 * day)).toBe(true);
    expect(holdPastDate({ ...h, released_at_ms: 1 }, rule, 300 * day)).toBe(false);
  });

  it('offers to extend an open hold, and shows who extended it and why', () => {
    const old = { ...hold, from_ms: Date.UTC(2020, 0, 1), to_ms: Date.UTC(2020, 0, 1, 1) };
    const plain = render(Settings, { props: { initial: view('ca_licensed'), initialHolds: [old] } }).body;
    expect(plain).toContain('Extend hold…');
    expect(plain).toContain('Release it if the complaint is closed, or extend it.');
    expect(plain).toContain('>Extend this hold?</h2>');
    const extended = {
      ...old,
      extended_until_ms: Date.UTC(2099, 0, 1),
      extended_by: 'Morag Ross',
      extended_at_ms: Date.UTC(2026, 10, 2, 9, 0),
      extend_reason: 'The regulator asked for the tapes again',
    };
    const { body } = render(Settings, { props: { initial: view('ca_licensed'), initialHolds: [extended] } });
    expect(body).toContain('Extended to');
    expect(body).toContain('by Morag Ross');
    expect(body).toContain('The regulator asked for the tapes again');
    expect(body).not.toContain('Past its date');
    expect(body).not.toContain('Keep until at least');
  });

  it('shows the flag on an old open hold, with the release advice', () => {
    const old = { ...hold, from_ms: Date.UTC(2020, 0, 1), to_ms: Date.UTC(2020, 0, 1, 1) };
    const { body } = render(Settings, { props: { initial: view('ca_licensed'), initialHolds: [old] } });
    expect(body).toContain('Past its date');
    expect(body).toContain('still open after its keep-until date');
  });

  it('says when a floor applies, where it applies only in some cases (ADR-0155)', () => {
    const v = view('uk_ofcom');
    const conditional = (r: Row): Row =>
      r.kind === 'programme_recordings' ? { ...r, floor: { days: 183, rule: 'DSA Art 20', condition: 'the platform is larger than a small enterprise' } } : r;
    v.rows = v.rows.map(conditional);
    for (const p of v.presets) p.rows = p.rows.map(conditional);
    const { body } = render(Settings, { props: { initial: v, initialHolds: [] } });
    expect(body).toContain('The rule requires at least 183 days, where the platform is larger than a small enterprise.');
  });
});

describe('period words', () => {
  it('words each kind of period plainly', () => {
    expect(periodWords(days(0))).toBe('Not kept after the show');
    expect(periodWords(days(42))).toBe('42 days');
    expect(periodWords(days(1096))).toBe('1,096 days (3 years)');
    expect(periodWords({ keep: 'with_archive' })).toBe('With the programme’s archive');
    expect(periodWords({ keep: 'after_recordings', days: 42 })).toBe('While a recording of the call exists, then 42 days');
    expect(periodWords({ keep: 'with_call' })).toBe('Until the call ends');
    expect(periodWords({ keep: 'no_limit' })).toBe('Kept, as anonymous totals only');
    expect(daysWords(1)).toBe('1 day');
    expect(goneWords({ recordings_deleted: 12, consents_deleted: 1 })).toBe('12 recordings and 1 caller’s consent record');
    expect(goneWords({ recordings_deleted: 0, consents_deleted: 0 })).toBe('nothing');
  });

  it('marks a period under its floor, chat by its archive', () => {
    const [logger, , chat] = rows(42, 42);
    const floored = { ...chat, floor: { days: 42, rule: 'r' } };
    expect(underFloor(logger, days(41), days(41))).toBe(true);
    expect(underFloor(logger, days(42), days(42))).toBe(false);
    expect(underFloor(floored, { keep: 'with_archive' }, days(28))).toBe(true);
    expect(underFloor(floored, { keep: 'with_archive' }, days(90))).toBe(false);
  });
});
