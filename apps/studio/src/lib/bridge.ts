// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The bridge (Phase 17g): the studio app stands in for the station on this computer, so the
// host's own software sends to it on loopback, and the app carries the bytes to the station over
// one WebSocket on 443. Its commands are the app's own (src-tauri/src/bridge.rs); a browser build
// has none. The presenter kit's station settings (VBAN, presets, the clock) are the station's
// routes, read and written as any settings are.

import { FieldError, base, headers, netFetch } from './api.ts';
import { CHIP, type Chip } from '../../../../packages/ui/states.ts';

/** `bridge_view`, field for field. */
export interface BridgeView {
  saved: { url: string; key_id: string; icecast_port: number; srt_port: number };
  has_key: boolean;
  running: boolean;
  local_password: string | null;
  status: BridgeStatus;
}

export interface BridgeStatus {
  uplink: 'off' | 'waiting' | 'connecting' | 'up' | 'retrying';
  software: string | null;
  format: string | null;
  station_state: 'standing_by' | 'test' | 'refused' | null;
  station_message: string | null;
  problem: string | null;
  retry_in_s: number | null;
  sent_bytes: number;
  dropped_bytes: number;
  kbps: number;
  too_fast: boolean;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

export const bridgeView = () => call<BridgeView>('bridge_view');
export const bridgeSave = (url: string, password: string | null, icecastPort: number, srtPort: number) =>
  call<BridgeView>('bridge_save', { url, password, icecastPort, srtPort });
export const bridgeStart = () => call<BridgeView>('bridge_start');
export const bridgeStop = () => call<BridgeView>('bridge_stop');
export const bridgeNewPassword = () => call<BridgeView>('bridge_new_password');
export const bridgeForget = () => call<BridgeView>('bridge_forget');

/** Where the bridged source stands at the desk, from the station's sources (`GET
 *  /api/v1/sources`), when this app can read them: `null` when it cannot (signed in to another
 *  station, or not as staff). The bridge's own messages say only how it was admitted, so after
 *  the post puts it on air they still say standing by (1.2.0, the real-kit tries' finding 5). */
export type DeskState = 'on_air' | 'standby' | 'link_lost' | null;

/** The bridged key's state among the sources of the station this app is signed in to, or null
 *  when it is not listed there (another station: a key's id is the station's own, 40 random
 *  bits, so a match is this station's key; on Cloudron the app talks to the studio's address and
 *  the bridge to the public one, so the addresses are not compared). */
export function deskStateOf(sources: readonly { key_id: string; state: string }[], keyId: string): DeskState {
  const s = sources.find((x) => x.key_id === keyId)?.state;
  return s === 'on_air' || s === 'standby' || s === 'link_lost' ? s : null;
}

/** One line on where the bridge stands. `desk` is the source's state at the desk, when known. */
export function uplinkWords(s: BridgeStatus, desk: DeskState = null): string {
  switch (s.uplink) {
    case 'off':
      return 'Off.';
    case 'waiting':
      return 'Waiting for your software to connect on this computer.';
    case 'connecting':
      return 'Connecting to the station…';
    case 'up':
      if (s.station_state === 'standing_by' && desk === 'on_air') return 'On air.';
      if (s.station_state === 'standing_by' && desk === 'standby') return 'Standing by at the desk.';
      return s.station_state === 'standing_by'
        ? 'Connected to the desk. The studio puts it on air.'
        : s.station_state === 'test'
          ? 'Connected: a private test, not heard at the desk outside your slot.'
          : 'Connected to the station.';
    case 'retrying':
      return `Lost the station${s.problem ? ` (${s.problem})` : ''}. Trying again in ${s.retry_in_s ?? 1} s.`;
  }
}

/** A key's Icecast password, as the bridge takes it: `<8-character id>-<secret>`. */
export function keyProblem(password: string): string | null {
  const p = password.trim();
  if (p === '') return null;
  return /^[a-z2-7]{8}-\S+$/.test(p) ? null : 'Paste the key’s Icecast password: its 8-character id, a hyphen, then its secret.';
}

// ---- the presenter kit at the station ----

export interface VbanIn {
  address: string;
  name: string;
  key_id: string;
  channels: [number, number];
}

export interface VbanOut {
  address: string;
  port: number;
  name: string;
  source: string;
  format: 's16' | 's24' | 'f32';
  enabled: boolean;
}

export interface Kit {
  preset: string;
  bind: string | null;
  port: number;
  allow_public: boolean;
  vban_in: VbanIn[];
  vban_out: VbanOut[];
}

/** `GET /api/v1/kit`. */
export interface KitView {
  allowed: boolean;
  why_not: string | null;
  warning: string | null;
  kit: Kit;
  presets: { name: string; words: string }[];
  out_sources: string;
}

/** `GET /api/v1/kit/clock`. */
export interface KitClock {
  rate: number;
  sources: { identity: string; name: string; way: string; ppm: number | null; replaced: number; lost: number }[];
  vban_dropped: Record<string, number>;
  vban_out: { name: string; address: string; source: string; packets: number; dropped: number; problem: string | null }[];
}

async function station<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await netFetch(`${base()}${path}`, {
    method,
    headers: headers(body !== undefined),
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new FieldError(res.status, data.error ?? `${res.status} ${res.statusText}`, data.field ?? null, data);
  return data as T;
}

export const kitView = () => station<KitView>('GET', '/api/v1/kit');
export const kitSave = (kit: Kit) => station<KitView>('PUT', '/api/v1/kit', kit);
export const kitPreset = (preset: string, address: string | null) =>
  station<KitView>('POST', '/api/v1/kit/preset', { preset, address });
export const kitClock = () => station<KitClock>('GET', '/api/v1/kit/clock');

/** The clock badge's words: "one clock 48 kHz · 0 drop-outs". */
export function clockWords(c: KitClock): string {
  const lost = c.sources.reduce((n, s) => n + s.lost + s.replaced, 0);
  return `One clock, ${c.rate / 1000} kHz · ${lost} drop-out${lost === 1 ? '' : 's'}`;
}

// ---- the kit's screen: words and places (17j) ----

/** A preset's name as its button says it, and what it sets: the station's own sentence, split
 *  at its first colon (*Transmitter feed: the programme after…*). */
export function presetWords(p: { name: string; words: string }): { title: string; does: string } {
  const at = p.words.indexOf(':');
  if (at < 0) return { title: p.name, does: p.words };
  const does = p.words.slice(at + 1).trim();
  return { title: p.words.slice(0, at).trim(), does: does.charAt(0).toUpperCase() + does.slice(1) };
}

/** Why VBAN is not offered here, in the screen's words: never on Cloudron, or off until the
 *  installation turns it on. The station's own sentence is kept for any other reason. */
export function vbanOffWords(why: string | null): { cloudron: boolean; title: string; body: string } {
  const w = why ?? '';
  if (/cloudron/i.test(w)) return { cloudron: true, title: 'VBAN is not offered on Cloudron.', body: 'VBAN is a sound link for one local network, with no password and no encryption, and a Cloudron server is on the internet. A studio PC sends to this station with a show key instead: Icecast, SRT, or the studio app’s bridge.' };
  if (/vban = true|KIT_VBAN/i.test(w)) return { cloudron: false, title: 'VBAN is off for this station.', body: 'It is turned on where the station is installed (KIT_VBAN=true with weavesignal.sh, then a restart). VBAN has no password and no encryption: turn it on only for a station on the same local network as the studio PC.' };
  return { cloudron: false, title: 'VBAN is not offered here.', body: w };
}

/** A VBAN out's source in words, for its picker and its status. */
export function outSourceWords(source: string, keyName: (keyId: string) => string | null = () => null, personName: (identity: string) => string | null = () => null): string {
  if (source === 'programme') return 'The programme, after the delay (what listeners hear)';
  if (source === 'programme_live') return 'The programme, live (before the delay)';
  if (source.startsWith('monitor:')) return `The monitor mix for ${keyName(source.slice(8)) ?? `key ${source.slice(8)}`}`;
  if (source.startsWith('mix_minus:')) return `Everyone but ${personName(source.slice(10)) ?? source.slice(10)}, for their own ear`;
  return source;
}

export const OUT_FORMAT_WORD: Record<VbanOut['format'], string> = { s16: '16-bit (the usual)', s24: '24-bit', f32: '32-bit float' };

/** Why the ingest dropped VBAN packets, in words. A reason it adds later is shown as given. */
export const DROP_WORD: Record<string, string> = {
  address: 'from an address that is not bound',
  name: 'with a stream name that is not bound',
  late: 'arrived late',
  too_fast: 'sent faster than a stream may',
  rate: 'not at 48 kHz',
  channels: 'with more channels than the station takes',
};
export function droppedWords(dropped: Record<string, number>): string[] {
  return Object.entries(dropped)
    .filter(([, n]) => n > 0)
    .map(([why, n]) => `${n.toLocaleString('en-GB')} ${DROP_WORD[why] ?? why}`);
}

/** A source's clock against the desk's: *+12 ppm*, *steady*, or *measuring* in its first 20 s. */
export function ppmWords(ppm: number | null): string {
  if (ppm === null) return 'measuring…';
  const r = Math.round(ppm);
  return r === 0 ? 'in step' : `${r > 0 ? '+' : '−'}${Math.abs(r)} ppm`;
}

/** Where a field error from the station belongs: `vban_out[0].address` is row 0's address. */
export function kitFieldId(field: string | null): string | null {
  if (!field) return null;
  const m = /^vban_(in|out)\[(\d+)\]\.(\w+)$/.exec(field);
  if (m) return `kit-${m[1]}-${m[2]}-${m[3] === 'key_id' ? 'key' : m[3]}`;
  return field === 'bind' || field === 'port' ? `kit-${field}` : null;
}

/** The station's short reason for a field, as a sentence under it. */
export function kitFieldWords(why: string): string {
  const w = why.trim();
  if (/^an IP address$/i.test(w)) return 'Give an IP address, such as 192.168.1.30.';
  if (/public address/i.test(w)) return 'That is a public address. VBAN has no password and no encryption: use an address on the station’s own network.';
  if (/1 to 16 characters/i.test(w)) return 'A stream name is 1 to 16 plain characters, exactly as the sender has it.';
  if (/already bound/i.test(w)) return 'That sender and stream name are bound already.';
  if (/never broadcast/i.test(w)) return 'Give one receiver’s address: VBAN is never sent to everyone on the network.';
  if (/show key/i.test(w)) return 'Choose the show key this stream belongs to.';
  return `${w.charAt(0).toUpperCase()}${w.slice(1)}${/[.!?]$/.test(w) ? '' : '.'}`;
}

// ---- the bridge's screen (17j) ----

/** Where the bridge stands, as a chip of the vocabulary with its own word. */
export function bridgeChip(v: BridgeView, desk: DeskState = null): { chip: Chip; word: string } {
  const s = v.status;
  if (!v.running || s.uplink === 'off') return { chip: CHIP.standby, word: 'Off' };
  if (s.uplink === 'waiting') return { chip: CHIP.waiting, word: 'Waiting for your software' };
  if (s.uplink === 'connecting') return { chip: CHIP.waiting, word: 'Connecting' };
  if (s.uplink === 'retrying') return { chip: CHIP.linkDegraded, word: 'Trying again' };
  if (s.station_state === 'refused') return { chip: CHIP.linkDegraded, word: 'Refused' };
  if (s.station_state === 'test') return { chip: CHIP.ready, word: 'Private test' };
  if (s.station_state === 'standing_by' && desk === 'on_air') return { chip: CHIP.onAir, word: 'On air' };
  if (s.station_state === 'standing_by' && desk === 'standby') return { chip: CHIP.standby, word: 'Standing by' };
  return { chip: CHIP.ready, word: 'Connected' };
}

/** *12 KB*, *3.4 MB*: what was dropped while the network fell behind. */
export function droppedKb(bytes: number): string {
  if (bytes < 1024) return `${bytes} bytes`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** What the host types into their own software: Icecast's lines, and SRT's one address. */
export function softwareFields(v: BridgeView): { icecast: { id: string; label: string; value: string }[]; srt: string } {
  const pw = v.local_password ?? '';
  return {
    icecast: [
      { id: 'ice-server', label: 'Server', value: '127.0.0.1' },
      { id: 'ice-port', label: 'Port', value: String(v.saved.icecast_port) },
      { id: 'ice-user', label: 'User', value: 'source' },
      { id: 'ice-password', label: 'Password', value: pw },
    ],
    srt: `srt://127.0.0.1:${v.saved.srt_port}?passphrase=${pw}&pbkeylen=16`,
  };
}
