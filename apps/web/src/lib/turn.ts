// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The ICE servers a page may use (ADR-0290). LiveKit (v1.13.10) hands a browser two things the
// station does not want as they come, and the page puts both right before the browser sees them:
//
// - **The relay's port.** LiveKit always tells clients its TURN over TLS is at
//   `turns:<domain>:443?transport=tcp`, whatever `turn.tls_port` says: it assumes a load balancer
//   on 443 (the binary's format string is `turns:%s:443?transport=tcp`). Our stations listen on
//   5349 by default and 15349 on Cloudron (ADR-0080, ADR-0106), so the page puts the station's
//   real port (`relay.turn_tls_port`, from the control plane) into those URLs. Where the port is
//   443 nothing changes.
// - **Outside STUN servers.** With no STUN server and no UDP TURN configured, LiveKit tells every
//   client to use Twilio's and Google's (`roommanager.go`, `iceServersForParticipant`), and its
//   configuration cannot make that list empty. A station contacts no STUN server unless its
//   operator named one (`STUN_SERVERS`, passed as `relay.stun_servers`), so the page drops every
//   `stun:` and `stuns:` URL that is not on that list. TURN URLs are never touched.

/** What the control plane says about the station's relay (`relay` in the player's facts and in a
 * pass's info). */
export interface StationRelay {
  turn_tls_port?: number;
  /** `host:port` each; the only STUN servers the page may ask. Empty or absent: none. */
  stun_servers?: string[];
}

/** The ICE servers with LiveKit's TURN-over-TLS URLs moved to `port`. */
export function withTurnPort(servers: RTCIceServer[] | undefined, port: number): RTCIceServer[] | undefined {
  if (!servers || !port || port === 443) return servers;
  const fix = (u: string) => u.replace(/^(turns:[^:?]+):443(\?transport=tcp)?$/i, `$1:${port}$2`);
  return servers.map((s) => ({ ...s, urls: Array.isArray(s.urls) ? s.urls.map(fix) : fix(s.urls) }));
}

/** Whether a URL is a STUN one: judged by its scheme alone, so a URL the page cannot read is
 * still STUN, and still dropped. */
function isStun(url: string): boolean {
  return /^\s*stuns?:/i.test(url);
}

/** `host:port` of a `stun:` or `stuns:` URL, lower case, the scheme's default port filled in;
 * null where the page cannot read it, which is then not on any list. */
function stunHostPort(url: string): string | null {
  const m = /^\s*(stuns?):(?:\/\/)?([^?/\s]+)(?:\?.*)?$/i.exec(url);
  if (!m) return null;
  const hostPort = m[2].toLowerCase();
  if (/:\d+$/.test(hostPort)) return hostPort;
  return `${hostPort}:${m[1].toLowerCase() === 'stuns' ? 5349 : 3478}`;
}

/**
 * The ICE servers without any STUN server the station did not name. A server left with no URL is
 * dropped. When the station named some and none came, they are added, as one server of their own.
 */
export function withoutOutsideStun(servers: RTCIceServer[] | undefined, allowed: readonly string[]): RTCIceServer[] | undefined {
  const ok = new Set(allowed.map((a) => a.toLowerCase()));
  const kept: RTCIceServer[] = [];
  for (const s of servers ?? []) {
    const urls = (Array.isArray(s.urls) ? s.urls : [s.urls]).filter((u) => {
      if (!isStun(u)) return true;
      const hp = stunHostPort(u);
      return hp !== null && ok.has(hp);
    });
    if (urls.length > 0) kept.push({ ...s, urls: Array.isArray(s.urls) ? urls : urls[0] });
  }
  const have = new Set(kept.flatMap((s) => (Array.isArray(s.urls) ? s.urls : [s.urls]).map(stunHostPort)));
  const missing = allowed.filter((a) => !have.has(a.toLowerCase()));
  if (missing.length > 0) kept.push({ urls: missing.map((a) => `stun:${a}`) });
  return kept;
}

/** The ICE servers as a page on this station may use them: TURN over TLS at the station's own
 * port, and no STUN server the station did not name. */
export function stationIceServers(servers: RTCIceServer[] | undefined, relay: StationRelay | undefined): RTCIceServer[] | undefined {
  if (!servers) return servers;
  return withTurnPort(withoutOutsideStun(servers, relay?.stun_servers ?? []), relay?.turn_tls_port ?? 0);
}

let installed = false;
let current: StationRelay | undefined;

/**
 * Make every peer connection this page opens use the station's ICE servers, with these facts
 * about the station's relay: before the room connects, and on every call, whatever `relay` is.
 * With no `relay` (the station's facts did not arrive) the page still drops every STUN server
 * and leaves TURN as LiveKit says. livekit-client builds its RTCConfiguration from the join
 * response and sets it again on reconnect, so both the constructor and setConfiguration are
 * covered.
 */
export function useStationIce(relay: StationRelay | undefined): void {
  current = relay;
  installStationIce();
}

/**
 * Put the filter in place without touching the facts it holds: called beside every `new Room`,
 * so a page that reaches the room without having been given the station's relay (a path added
 * later, say) still asks no STUN server.
 */
export function installStationIce(): void {
  if (installed || typeof RTCPeerConnection === 'undefined') return;
  installed = true;
  const Native = RTCPeerConnection;
  const fix = (c?: RTCConfiguration): RTCConfiguration | undefined => (c ? { ...c, iceServers: stationIceServers(c.iceServers, current) } : c);
  class StationPeerConnection extends Native {
    constructor(config?: RTCConfiguration) {
      super(fix(config));
    }
    override setConfiguration(config?: RTCConfiguration): void {
      super.setConfiguration(fix(config));
    }
  }
  globalThis.RTCPeerConnection = StationPeerConnection;
}
