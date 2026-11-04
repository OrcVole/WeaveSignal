// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The relay's port. LiveKit's built-in TURN (v1.13.10) always tells clients its TURN over TLS is
// at `turns:<domain>:443?transport=tcp`, whatever `turn.tls_port` says: it assumes a load
// balancer on 443 (the binary's format string is `turns:%s:443?transport=tcp`). Our stations
// listen on 5349 by default and 15349 on Cloudron (ADR-0080, ADR-0106), so the page puts the
// station's real port (`relay.turn_tls_port`, from the control plane) into those URLs before
// the browser sees them. Where the port is 443 nothing changes.

/** The ICE servers with LiveKit's TURN-over-TLS URLs moved to `port`. */
export function withTurnPort(servers: RTCIceServer[] | undefined, port: number): RTCIceServer[] | undefined {
  if (!servers || !port || port === 443) return servers;
  const fix = (u: string) => u.replace(/^(turns:[^:?]+):443(\?transport=tcp)?$/i, `$1:${port}$2`);
  return servers.map((s) => ({ ...s, urls: Array.isArray(s.urls) ? s.urls.map(fix) : fix(s.urls) }));
}

let installed: number | null = null;

/**
 * Make every peer connection this page opens use the station's TURN-over-TLS port: once, before
 * the room connects. livekit-client builds its RTCConfiguration from the join response and
 * sets it again on reconnect, so both the constructor and setConfiguration are covered.
 */
export function useTurnPort(port: number): void {
  if (installed !== null || !port || port === 443 || typeof RTCPeerConnection === 'undefined') return;
  installed = port;
  const Native = RTCPeerConnection;
  const fix = (c?: RTCConfiguration): RTCConfiguration | undefined => (c ? { ...c, iceServers: withTurnPort(c.iceServers, port) } : c);
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
