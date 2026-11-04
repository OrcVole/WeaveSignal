// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The sign-in's check of a station's address (ADR-0139). Mirrors the Rust tests of `clean_url`.
import { describe, expect, it } from 'vitest';
import { checkAddress, isNearby, isTailscaleHttp, NO_SCHEME, NOT_AN_ORIGIN, PLAIN_HTTP_REFUSED } from './lib/address.ts';

const ok = (u: string) => {
  const r = checkAddress(u);
  expect(r.ok, u).toBe(true);
  return r.ok ? r.base : '';
};
const refused = (u: string) => {
  const r = checkAddress(u);
  expect(r.ok, u).toBe(false);
  return r.ok ? '' : r.message;
};

describe('a station address', () => {
  it('takes https for any host and port', () => {
    for (const u of ['https://radio.example.org', 'https://radio.example.org:8443', 'https://203.0.113.9', 'https://[2001:db8::1]:9443', 'https://100.64.0.1']) {
      expect(ok(u)).toBe(u);
    }
    expect(ok(' https://radio.example.org/ ')).toBe('https://radio.example.org');
  });

  it('takes plain http for this computer and private networks', () => {
    for (const u of [
      'http://localhost:8080', 'http://127.0.0.1:8080', 'http://127.5.5.5', 'http://[::1]:8080',
      'http://10.0.0.1', 'http://10.255.255.255', 'http://172.16.0.1', 'http://172.31.255.254', 'http://192.168.1.20:8080',
      'http://169.254.10.10', 'http://[fe80::1]', 'http://[febf::1]',
      'http://[fc00::1]', 'http://[fd12:3456:789a::1]:8080',
      'http://100.64.0.1', 'http://100.101.102.103:8080', 'http://100.127.255.254',
      'http://[::ffff:192.168.1.5]',
    ]) {
      expect(ok(u)).toBe(u);
    }
  });

  it('refuses plain http beyond the neighbourhood, and says so', () => {
    for (const u of [
      'http://203.0.113.9', 'http://8.8.8.8:8080', 'http://11.0.0.1', 'http://172.15.255.255', 'http://172.32.0.1',
      'http://192.167.1.1', 'http://169.253.1.1', 'http://100.63.255.255', 'http://100.128.0.1',
      'http://[2001:db8::1]', 'http://[fe00::1]', 'http://[fec0::1]', 'http://[::ffff:8.8.8.8]',
      'http://radio.example.org', 'http://radio.local', 'http://localhost.example.org', 'http://printer', 'http://127.0.0.1.example.org',
    ]) {
      expect(refused(u), u).toBe(PLAIN_HTTP_REFUSED);
    }
    expect(PLAIN_HTTP_REFUSED).toContain('use https://');
  });

  it('refuses what is not an origin', () => {
    expect(refused('')).toMatch(/Enter/);
    for (const u of ['radio.example.org', 'ftp://radio.example.org']) expect(refused(u)).toBe(NO_SCHEME);
    for (const u of ['https://radio.example.org/studio', 'https://user:pw@radio.example.org', 'https://radio.example.org?x=1', 'https://radio.example.org/#x']) {
      expect(refused(u), u).toBe(NOT_AN_ORIGIN);
    }
  });

  it('knows nearby from far', () => {
    expect(isNearby('localhost')).toBe(true);
    expect(isNearby('[::1]')).toBe(true);
    expect(isNearby('localhost.')).toBe(false);
  });
});

describe('the Tailscale sentence (ADR-0197)', () => {
  it('shows for plain http in 100.64.0.0/10 only', () => {
    for (const u of ['http://100.64.0.1', 'http://100.101.102.103:7310', 'http://100.127.255.254', 'http://[::ffff:100.64.0.9]']) {
      expect(isTailscaleHttp(u), u).toBe(true);
    }
  });
  it('never shows for https, loopback, private ranges or the edges of the range', () => {
    for (const u of ['https://100.64.0.1', 'http://127.0.0.1:7310', 'http://localhost', 'http://192.168.1.20', 'http://10.0.0.5', 'http://172.16.0.1', 'http://[::1]', 'http://100.63.255.255', 'http://100.128.0.1', 'http://radio.example.org', 'not an address']) {
      expect(isTailscaleHttp(u), u).toBe(false);
    }
  });
});
