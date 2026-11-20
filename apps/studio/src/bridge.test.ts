// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The bridge's words and the kit's clock badge (Phase 17g).

import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Bridge from './settings/Bridge.svelte';
import { clockWords, keyProblem, uplinkWords, type BridgeStatus } from './lib/bridge.ts';

const status = (s: Partial<BridgeStatus>): BridgeStatus => ({
  uplink: 'off',
  software: null,
  format: null,
  station_state: null,
  station_message: null,
  problem: null,
  retry_in_s: null,
  sent_bytes: 0,
  dropped_bytes: 0,
  kbps: 0,
  too_fast: false,
  ...s,
});

describe('the bridge', () => {
  it('says where it stands', () => {
    expect(uplinkWords(status({ uplink: 'waiting' }))).toMatch(/Waiting for your software/);
    expect(uplinkWords(status({ uplink: 'up', station_state: 'standing_by' }))).toBe('Standing by at the desk.');
    expect(uplinkWords(status({ uplink: 'up', station_state: 'test' }))).toMatch(/private test/);
    expect(uplinkWords(status({ uplink: 'retrying', retry_in_s: 4, problem: 'the station closed the connection' }))).toBe(
      'Lost the station (the station closed the connection). Trying again in 4 s.',
    );
  });

  it('takes a key as its Icecast password', () => {
    expect(keyProblem('')).toBeNull();
    expect(keyProblem('abcd2345-thesecret')).toBeNull();
    expect(keyProblem('abcd2345')).toMatch(/hyphen/);
    expect(keyProblem('ABCD2345-x')).toMatch(/hyphen/);
  });

  it('counts drop-outs for the clock badge', () => {
    const c = {
      rate: 48_000,
      sources: [{ identity: 'source-abcd2345', name: 'Finlay Grant', way: 'vban', ppm: 41, replaced: 1, lost: 2 }],
      vban_dropped: {},
      vban_out: [],
    };
    expect(clockWords(c)).toBe('One clock, 48 kHz · 3 drop-outs');
    expect(clockWords({ ...c, sources: [] })).toBe('One clock, 48 kHz · 0 drop-outs');
  });
});

describe('the ways at the desk', () => {
  it('names the bridge and VBAN, and says the bridge is behind like Icecast', async () => {
    const { WAY_WORD, detailWay, tileLink } = await import('./lib/hosts.ts');
    expect(WAY_WORD.bridge).toBe('Bridge');
    expect(WAY_WORD.vban).toBe('VBAN');
    expect(detailWay('bridge')).toBe('icecast');
    expect(detailWay('srt')).toBe('srt');
    const s = { way: 'bridge' as const, format: 'mp3 44.1 kHz stereo', key: 'abcd2345', show: 'steeped', connected_ms: 60_000, leveller_db: 0 };
    expect(tileLink(s)).toMatch(/^Bridge · steady .* · about 2 s behind$/);
    expect(tileLink({ ...s, way: 'vban' })).not.toMatch(/behind/);
  });
});

describe('the bridge’s screen (17j)', () => {
  const view = (o: Partial<BridgeStatus>, running = true) => ({ saved: { url: 'https://kettle.example', key_id: 'abcd2345', icecast_port: 8100, srt_port: 9100 }, has_key: true, running, local_password: 'local-pass-1', status: status(o) });
  it('says where it stands as a chip with a word', async () => {
    const { bridgeChip } = await import('./lib/bridge.ts');
    expect(bridgeChip(view({}, false)).word).toBe('Off');
    expect(bridgeChip(view({ uplink: 'waiting' })).word).toBe('Waiting for your software');
    expect(bridgeChip(view({ uplink: 'connecting' })).word).toBe('Connecting');
    expect(bridgeChip(view({ uplink: 'up', station_state: 'standing_by' })).word).toBe('Standing by');
    expect(bridgeChip(view({ uplink: 'up', station_state: 'test' })).word).toBe('Private test');
    expect(bridgeChip(view({ uplink: 'up', station_state: 'refused' })).word).toBe('Refused');
    expect(bridgeChip(view({ uplink: 'retrying' })).word).toBe('Trying again');
  });
  it('gives the host’s software each line to copy', async () => {
    const { softwareFields, droppedKb } = await import('./lib/bridge.ts');
    const f = softwareFields(view({}));
    expect(f.icecast.map((x) => `${x.label} ${x.value}`)).toEqual(['Server 127.0.0.1', 'Port 8100', 'User source', 'Password local-pass-1']);
    expect(f.srt).toBe('srt://127.0.0.1:9100?passphrase=local-pass-1&pbkeylen=16');
    expect(droppedKb(900)).toBe('900 bytes');
    expect(droppedKb(12_800)).toBe('13 KB');
    expect(droppedKb(3_600_000)).toBe('3.4 MB');
  });
});

describe('the presenter kit’s screen (17j)', () => {
  it('a preset’s button is named from the station’s own sentence', async () => {
    const { presetWords } = await import('./lib/bridge.ts');
    expect(presetWords({ name: 'transmitter', words: 'Transmitter feed: the programme after the profanity delay, what listeners hear, to a transmitter or a recorder.' })).toEqual({ title: 'Transmitter feed', does: 'The programme after the profanity delay, what listeners hear, to a transmitter or a recorder.' });
    expect(presetWords({ name: 'x', words: 'No colon here' })).toEqual({ title: 'x', does: 'No colon here' });
  });
  it('VBAN is never offered on Cloudron, and says how it is turned on elsewhere', async () => {
    const { vbanOffWords } = await import('./lib/bridge.ts');
    const c = vbanOffWords('VBAN is never offered on Cloudron: it is a protocol for one local network, and a Cloudron server is on the internet.');
    expect(c).toMatchObject({ cloudron: true, title: 'VBAN is not offered on Cloudron.' });
    expect(c.body).toMatch(/show key instead/);
    const o = vbanOffWords('VBAN is off for this installation: set `[kit] vban = true` in the settings file (Compose: KIT_VBAN=true) and restart.');
    expect(o).toMatchObject({ cloudron: false, title: 'VBAN is off for this station.' });
    expect(o.body).toMatch(/KIT_VBAN=true/);
    expect(vbanOffWords('another reason').body).toBe('another reason');
  });
  it('says what a VBAN out carries, a clock’s drift and dropped packets in words', async () => {
    const { outSourceWords, ppmWords, droppedWords } = await import('./lib/bridge.ts');
    expect(outSourceWords('programme')).toBe('The programme, after the delay (what listeners hear)');
    expect(outSourceWords('programme_live')).toBe('The programme, live (before the delay)');
    expect(outSourceWords('monitor:abcd2345', () => 'Finlay Grant')).toBe('The monitor mix for Finlay Grant');
    expect(outSourceWords('mix_minus:eilidh', () => null, () => 'Eilidh Fraser')).toBe('Everyone but Eilidh Fraser, for their own ear');
    expect(ppmWords(null)).toBe('measuring…');
    expect(ppmWords(0.2)).toBe('in step');
    expect(ppmWords(41.3)).toBe('+41 ppm');
    expect(ppmWords(-12)).toBe('−12 ppm');
    expect(droppedWords({ address: 1200, late: 3, name: 0, other: 2 })).toEqual(['1,200 from an address that is not bound', '3 arrived late', '2 other']);
  });
  it('puts the station’s refusal under the field it names', async () => {
    const { kitFieldId, kitFieldWords } = await import('./lib/bridge.ts');
    expect(kitFieldId('vban_out[0].address')).toBe('kit-out-0-address');
    expect(kitFieldId('vban_in[2].key_id')).toBe('kit-in-2-key');
    expect(kitFieldId('bind')).toBe('kit-bind');
    expect(kitFieldId('preset')).toBeNull();
    expect(kitFieldWords('an IP address')).toBe('Give an IP address, such as 192.168.1.30.');
    expect(kitFieldWords('a public address: VBAN is for the station\'s own network')).toMatch(/^That is a public address/);
    expect(kitFieldWords('at most 4 inputs')).toBe('At most 4 inputs.');
  });
});

describe('the bridge’s panel, drawn (17j)', () => {
  const saved = { url: 'https://kettle.example', key_id: 'abcd2345', icecast_port: 8100, srt_port: 9100 };
  const VIEWS = {
    new: { saved: { ...saved, url: '', key_id: '' }, has_key: false, running: false, local_password: null, status: status({}) },
    off: { saved, has_key: true, running: false, local_password: 'tay-forth-clyde-dee', status: status({}) },
    waiting: { saved, has_key: true, running: true, local_password: 'tay-forth-clyde-dee', status: status({ uplink: 'waiting' }) },
    standing: { saved, has_key: true, running: true, local_password: 'tay-forth-clyde-dee', status: status({ uplink: 'up', station_state: 'standing_by', station_message: 'Standing by for Steeped, which starts at 14:00.', software: 'butt 1.45', format: 'mp3 44.1 kHz stereo', kbps: 192, sent_bytes: 4_000_000 }) },
    toofast: { saved, has_key: true, running: true, local_password: 'tay-forth-clyde-dee', status: status({ uplink: 'up', station_state: 'test', software: 'OBS 31', format: 'mpegts/h264+aac', kbps: 2600, too_fast: true, dropped_bytes: 3_600_000 }) },
    retrying: { saved, has_key: true, running: true, local_password: 'tay-forth-clyde-dee', status: status({ uplink: 'retrying', retry_in_s: 4, problem: 'the station closed the connection' }) },
  } as const;
  const draw = (v: (typeof VIEWS)[keyof typeof VIEWS]) => render(Bridge, { props: { initial: v } }).body;

  it('a browser build with no view draws nothing', () => {
    expect(render(Bridge).body).not.toContain('data-panel="settings.bridge"');
  });

  it('says where it stands, with Start or Stop beside it', () => {
    expect(draw(VIEWS.off)).toContain('Start the bridge');
    expect(draw(VIEWS.off)).not.toContain('Stop the bridge');
    const up = draw(VIEWS.standing);
    expect(up).toContain('Stop the bridge');
    expect(up).toContain('Standing by at the desk.');
    expect(up).toContain('butt 1.45 · mp3 44.1 kHz stereo · 192 kbit/s');
    expect(draw(VIEWS.retrying)).toContain('Lost the station (the station closed the connection). Trying again in 4 s.');
  });

  it('before a key is saved Start is drawn off and says why, and no details are given', () => {
    const b = draw(VIEWS.new);
    expect(b).toMatch(/aria-disabled="true"[^>]*title="Save the station’s address and a show key first\."/);
    expect(b).toContain('The details for your software appear here.');
    expect(b).not.toContain('New local password');
  });

  it('gives each line of the details with Copy, and says Cloudron’s address is the public one', () => {
    const b = draw(VIEWS.off);
    for (const label of ['Copy the Icecast server', 'Copy the Icecast port', 'Copy the Icecast user', 'Copy the Icecast password', 'Copy the SRT address']) expect(b, label).toContain(`aria-label="${label}"`);
    expect(b).toContain('srt://127.0.0.1:9100?passphrase=tay-forth-clyde-dee&amp;pbkeylen=16');
    expect(b).toContain('On Cloudron that is the public pages’ address, not the studio’s');
    expect(b).toContain('Saved: abcd2345-…');
  });

  it('while it runs the settings are locked, and New local password is off with the reason', () => {
    const b = draw(VIEWS.waiting);
    expect(b).toMatch(/id="bridge-url"[^>]*disabled/);
    expect(b).toMatch(/aria-disabled="true"[^>]*title="Stop the bridge first: your software is using this one\."/);
  });

  it('says Send sound only when the software sends more than the station takes', () => {
    const b = draw(VIEWS.toofast);
    expect(b).toContain('data-testid="bridge-too-fast"');
    expect(b).toContain('Send sound only.');
    expect(b).toContain('3.4 MB dropped while the network fell behind.');
    expect(draw(VIEWS.standing)).not.toContain('bridge-too-fast');
  });

  // For tests/hosts/shoot-12.mjs: the panel's markup in each state, to draw in a real browser
  // beside the built app's styles (the packaged app alone has the bridge).
  // The package has no Node types: the module's name is given as a value, as the other tests do.
  const shotDir = (globalThis as unknown as { process?: { env: Record<string, string | undefined> } }).process?.env.SHOT_HTML_DIR;
  it.runIf(!!shotDir)('writes its states for the screenshots', async () => {
    const fsModule = 'node:fs';
    const fs = (await import(/* @vite-ignore */ fsModule)) as { mkdirSync(p: string, o: { recursive: boolean }): void; writeFileSync(p: string, s: string): void };
    fs.mkdirSync(shotDir!, { recursive: true });
    for (const [name, v] of Object.entries(VIEWS)) fs.writeFileSync(`${shotDir}/bridge-${name}.html`, draw(v));
  });
});
