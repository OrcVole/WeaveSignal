// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Settings > Send to another Icecast server (ADR-0226): the words for each state, and the panel
// rendered from a view on the server, with no password anywhere in it.
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import { panel } from '../../../packages/ui/panels.ts';
import IcecastOut from './settings/IcecastOut.svelte';
import { MODES, errorWords, sizeWords, statusWords, targetWords, type IcecastOut as View } from './lib/icecastOut.ts';

const base: View = {
  configured: true,
  mode: 'auto',
  wanted: false,
  reason: 'the desk is not on air',
  connected: false,
  host: 'radio.example.org',
  port: 8005,
  mount: '/',
  tls: false,
  bitrate: 128,
  bytes_sent: 0,
  connected_since_ms: null,
  failures: 0,
  last_error: '',
  last_error_at_ms: null,
};

describe('the words', () => {
  it('says where it sends', () => {
    expect(targetWords(base)).toBe('radio.example.org:8005/');
    expect(targetWords({ ...base, mount: 'dj', tls: true })).toBe('https://radio.example.org:8005/dj');
  });

  it('says what is happening in each state', () => {
    expect(statusWords({ ...base, configured: false }).text).toBe('Not set up on this station.');
    expect(statusWords(base)).toEqual({ tone: 'plain', text: 'Not sending (the desk is not on air).' });
    expect(statusWords({ ...base, wanted: true })).toEqual({ tone: 'warning', text: 'Connecting to radio.example.org:8005/…' });
    expect(statusWords({ ...base, wanted: true, failures: 2 }).tone).toBe('serious');
    expect(statusWords({ ...base, wanted: true, connected: true })).toEqual({
      tone: 'good',
      text: 'Sending to radio.example.org:8005/.',
    });
  });

  it('shows the last error only while it matters', () => {
    const e = { ...base, last_error: 'the server refused the user name or password (401)' };
    expect(errorWords(e)).toContain('refused');
    expect(errorWords({ ...e, connected: true })).toBe('');
  });

  it('writes sizes', () => {
    expect(sizeWords(900)).toBe('900 bytes');
    expect(sizeWords(2048)).toBe('2 KB');
    expect(sizeWords(5 * 1024 * 1024)).toBe('5.0 MB');
  });

  it('offers the three choices, the default first', () => {
    expect(MODES.map((m) => m.mode)).toEqual(['auto', 'on', 'off']);
  });
});

describe('the panel', () => {
  it('is registered under its name', () => {
    expect(panel('settings.icecast-out').what).toContain('Icecast');
  });

  it('explains how to turn it on when the station has not', () => {
    const { body } = render(IcecastOut, { props: { initial: { ...base, configured: false } } });
    expect(body).toContain('data-panel="settings.icecast-out"');
    expect(body).toContain('ICECAST_OUT');
    expect(body).not.toContain('type="radio"');
  });

  it('tells a DJ to ask the admin rather than to set a setting they cannot reach (ADR-0246)', () => {
    const { body } = render(IcecastOut, { props: { initial: { ...base, configured: false }, admin: false } });
    expect(body).toContain('Ask your station’s admin to turn it on');
    expect(body).not.toContain('ICECAST_OUT');
  });

  it('shows the state and the three choices, with the current one checked', () => {
    const { body } = render(IcecastOut, {
      props: { initial: { ...base, wanted: true, failures: 3, last_error: 'nothing is listening at that address and port' } },
    });
    expect(body).toContain('Cannot reach radio.example.org:8005/');
    expect(body).toContain('Last error: nothing is listening');
    expect((body.match(/type="radio"/g) ?? []).length).toBe(3);
    expect(body).toMatch(/value="auto"[^>]*checked/);
  });
});
