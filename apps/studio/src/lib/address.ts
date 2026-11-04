// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The station's address as the sign-in screen checks it (ADR-0139): https for any host and
// port, plain http only for this computer and private networks, so the key and the password
// never cross the internet in clear. The Rust side (`clean_url` in `src-tauri/src/station.rs`)
// is the authority and applies the same rule; this catches a slip before the round trip, with
// the same sentences. No DNS lookup decides anything: a name other than `localhost` is never
// nearby, because a name can point anywhere.

export const NO_SCHEME = 'The address starts with http:// or https://, for example https://radio.example.org.';
export const NOT_AN_ORIGIN = 'The address is only the station’s, like https://radio.example.org, with no path.';
export const PLAIN_HTTP_REFUSED =
  'Plain http:// is only for this computer and private networks, like http://192.168.1.20:8080. ' +
  'For a station on the internet, use https://, like https://radio.example.org.';

// ADR-0197: a plain http address in 100.64.0.0/10 is accepted because Tailscale encrypts the link
// itself; the sign-in says so, in those terms, the first time only (the operator's words).
export const TAILSCALE_NOTE =
  'It looks like you are connecting through Tailscale, which encrypts the link itself, so this ' +
  'station’s plain http address is accepted. If you are not using Tailscale, ask your station for ' +
  'an https address.';

/** `a.b.c.d` as four numbers, or null if it is not a dotted IPv4 address. */
function v4(host: string): [number, number, number, number] | null {
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(host);
  if (!m) return null;
  const o = m.slice(1).map(Number);
  return o.every((n) => n <= 255) ? (o as [number, number, number, number]) : null;
}

function nearbyV4([a, b]: [number, number, number, number]): boolean {
  return (
    a === 127 || // this computer
    a === 10 || // RFC 1918
    (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 168) ||
    (a === 169 && b === 254) || // link-local
    (a === 100 && b >= 64 && b <= 127) // carrier-grade NAT: Tailscale
  );
}

/** The eight 16-bit groups of an IPv6 address as the URL parser writes it (no brackets), or null. */
function groups6(host: string): number[] | null {
  if (!/^[0-9a-f:.]+$/i.test(host) || !host.includes(':')) return null;
  let h = host.toLowerCase();
  // A dotted tail (::ffff:1.2.3.4) is two groups.
  const tail = /:(\d+\.\d+\.\d+\.\d+)$/.exec(h);
  if (tail) {
    const o = v4(tail[1]);
    if (!o) return null;
    h = h.slice(0, -tail[1].length) + ((o[0] << 8) | o[1]).toString(16) + ':' + ((o[2] << 8) | o[3]).toString(16);
  }
  const halves = h.split('::');
  if (halves.length > 2) return null;
  const part = (s: string) => (s === '' ? [] : s.split(':'));
  const head = part(halves[0]);
  const rest = halves.length === 2 ? part(halves[1]) : [];
  const fill = halves.length === 2 ? 8 - head.length - rest.length : 0;
  if (fill < (halves.length === 2 ? 1 : 0)) return null;
  const all = [...head, ...Array<string>(fill).fill('0'), ...rest];
  if (all.length !== 8 || !all.every((g) => /^[0-9a-f]{1,4}$/.test(g))) return null;
  return all.map((g) => parseInt(g, 16));
}

/** Whether plain http may be spoken to this host (a URL's `hostname`). */
export function isNearby(hostname: string): boolean {
  const host = hostname.replace(/^\[|\]$/g, '');
  if (host === 'localhost') return true;
  const four = v4(host);
  if (four) return nearbyV4(four);
  const g = groups6(host);
  if (!g) return false;
  if (g.slice(0, 5).every((x) => x === 0) && g[5] === 0xffff) {
    return nearbyV4([g[6] >> 8, g[6] & 255, g[7] >> 8, g[7] & 255]); // an IPv4 address inside IPv6
  }
  if (g.slice(0, 7).every((x) => x === 0) && g[7] === 1) return true; // ::1
  return (g[0] & 0xfe00) === 0xfc00 || (g[0] & 0xffc0) === 0xfe80; // RFC 4193, link-local
}

/** Whether an address is plain http to a carrier-grade NAT address (100.64.0.0/10), Tailscale's
 * range: the one case the sign-in explains (ADR-0197). Never for loopback or private ranges. */
export function isTailscaleHttp(raw: string): boolean {
  const r = checkAddress(raw);
  if (!r.ok) return false;
  const u = new URL(r.base);
  if (u.protocol !== 'http:') return false;
  const host = u.hostname.replace(/^\[|\]$/g, '');
  let four = v4(host);
  if (!four) {
    const g = groups6(host);
    if (g && g.slice(0, 5).every((x) => x === 0) && g[5] === 0xffff) {
      four = [g[6] >> 8, g[6] & 255, g[7] >> 8, g[7] & 255];
    }
  }
  return four !== null && four[0] === 100 && four[1] >= 64 && four[1] <= 127;
}

/** The address as an origin, or the sentence for its field. */
export function checkAddress(raw: string): { ok: true; base: string } | { ok: false; message: string } {
  const base = raw.trim().replace(/\/+$/, '');
  if (!base) return { ok: false, message: 'Enter the station’s address.' };
  if (!/^https?:\/\//i.test(base)) return { ok: false, message: NO_SCHEME };
  let u: URL;
  try {
    u = new URL(base);
  } catch {
    return { ok: false, message: NO_SCHEME };
  }
  if (!u.hostname || u.username || u.password || u.search || u.hash || u.pathname !== '/' || base.includes('?') || base.includes('#')) {
    return { ok: false, message: NOT_AN_ORIGIN };
  }
  if (u.protocol === 'http:' && !isNearby(u.hostname)) return { ok: false, message: PLAIN_HTTP_REFUSED };
  return { ok: true, base };
}
