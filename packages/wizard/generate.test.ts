// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { defaults, generate, keysOf, problems, readAnswersFile, type Answers, type Output } from './generate.ts';

const read = (p: string) => readFileSync(new URL(`../../${p}`, import.meta.url), 'utf8');
const weavesignalSh = read('deploy/compose/weavesignal.sh');
const composeYaml = read('deploy/compose/compose.yaml');
const startSh = read('deploy/cloudron/start.sh');

const compose = (over: Partial<Answers> = {}): Answers => ({
  ...defaults('compose'),
  name: "Paddy's Wireless",
  contact: 'calls@station.example',
  domain: 'station.example',
  ...over,
});
const cloudron = (over: Partial<Answers> = {}): Answers => ({
  ...defaults('cloudron'),
  name: 'Radio Ceilidh',
  contact: 'https://station.example/takedown',
  studioDomain: 'studio.station.example',
  publicDomain: 'station.example',
  listenDomain: 'listen.station.example',
  roomDomain: 'room.station.example',
  ...over,
});
function out(a: Answers): Output {
  const g = generate(a);
  if (!g.ok) throw new Error(`problems: ${JSON.stringify(g.problems)}`);
  return g.output;
}
const value = (text: string, key: string) => new RegExp(`^${key}=(.*)$`, 'm').exec(text)?.[1];

describe('the Compose and Quadlets settings file', () => {
  it('holds only keys weavesignal.sh keeps or Compose reads', () => {
    // `known`: the keys the script writes itself. Any other key in the file is kept at its end,
    // and takes effect only if compose.yaml substitutes it.
    const known = / known=" ([A-Z0-9_ ]+) "/.exec(weavesignalSh)![1].split(' ');
    const everything = compose({ acmeEmail: 'ops@station.example', nodeIp: '203.0.113.7', mail: 'outbox', mailFrom: 'chat@station.example', mediaPort: 52000, turnTlsPort: 15349, turnUdpPort: 3478, iceTcpPort: 7881, video: true });
    for (const route of ['compose', 'quadlets'] as const) {
      const keys = keysOf(out({ ...everything, route }).file.text);
      expect(keys.length).toBeGreaterThan(10);
      for (const k of keys) {
        expect(known.includes(k) || composeYaml.includes(`\${${k}:-`), `${k} is read by nothing`).toBe(true);
      }
      expect(new Set(keys).size).toBe(keys.length);
    }
  });
  it('never holds a secret, a derived address or the name the command gives', () => {
    const keys = keysOf(out(compose()).file.text);
    for (const k of ['CONTROL_ADMIN_TOKEN', 'POSTGRES_PASSWORD', 'LIVEKIT_API_SECRET', 'STATION_ID', 'PUBLIC_DOMAIN', 'PUBLIC_ORIGIN', 'STREAM_URL']) {
      expect(keys).not.toContain(k);
    }
  });
  it('writes each value as the script reads it: the whole line after the equals sign', () => {
    const text = out(compose({ video: true, searchListing: true })).file.text;
    expect(value(text, 'STATION_NAME')).toBe("Paddy's Wireless");
    expect(value(text, 'CONSENT_CONTACT')).toBe('calls@station.example');
    expect(value(text, 'VIDEO')).toBe('true');
    expect(value(text, 'SEARCH_LISTING')).toBe('true');
    expect(value(text, 'DIRECTORY_LISTING')).toBe('false');
    expect(value(text, 'MAIL_MODE')).toBe('off');
    expect(value(text, 'MAIL_FROM')).toBeUndefined();
  });
  it('leaves the default ports to the script and names only the ones changed', () => {
    expect(keysOf(out(compose()).file.text)).not.toContain('LIVEKIT_MEDIA_PORT');
    const text = out(compose({ mediaPort: 52000, turnUdpPort: 3478 })).file.text;
    expect(value(text, 'LIVEKIT_MEDIA_PORT')).toBe('52000');
    expect(value(text, 'TURN_UDP_PORT')).toBe('3478');
    expect(value(text, 'TURN_TLS_PORT')).toBeUndefined();
  });
  it('has no public settings on a station for this machine only', () => {
    const o = out(compose({ isPublic: false, domain: '', acmeEmail: 'ops@station.example', mediaPort: 52000 }));
    expect(keysOf(o.file.text)).not.toContain('ACME_EMAIL');
    expect(keysOf(o.file.text)).not.toContain('LIVEKIT_MEDIA_PORT');
    expect(o.dns).toEqual([]);
    expect(o.ports).toEqual([]);
    expect(o.setupUrl).toBe('http://127.0.0.1:7310/setup/');
    expect(o.steps.map((s) => s.command ?? '').join('\n')).not.toContain('--public');
  });
});

describe('the Cloudron settings file', () => {
  const text = out(cloudron({ streamPassword: true, video: true, nodeIp: '203.0.113.7' })).file.text;
  it('names every setting start.sh expects before it adds its own', () => {
    // start.sh sources the file under `set -u`: a setting of its first file that is missing
    // here would stop the app at start. The ones `setting` adds are appended if absent.
    const first = /cat > "\$data\/weavesignal\.env" <<'ENV'\n([\s\S]*?)\nENV\n/.exec(startSh)![1];
    const needed = keysOf(first);
    expect(needed).toContain('ROOM_NAME');
    for (const k of needed) expect(keysOf(text), `${k} is missing`).toContain(k);
  });
  it('holds only settings start.sh knows, and never a domain, a public port or a secret', () => {
    const first = keysOf(/<<'ENV'\n([\s\S]*?)\nENV\n/.exec(startSh)![1]);
    const added = [...startSh.matchAll(/^setting ([A-Z_]+) /gm)].map((m) => m[1]);
    const keys = keysOf(text);
    for (const k of keys) expect([...first, ...added], `${k} is not a setting of start.sh`).toContain(k);
    expect(new Set(keys).size).toBe(keys.length);
    for (const k of ['LISTENERS_DOMAIN', 'ROOM_DOMAIN', 'PUBLIC_DOMAIN', 'LIVEKIT_MEDIA_PORT', 'TURN_TLS_PORT', 'TURN_UDP_PORT', 'LIVEKIT_TCP_PORT', 'CONTROL_ADMIN_TOKEN', 'LISTENERS_PASSWORD', 'STATION_ID']) {
      expect(keys).not.toContain(k);
    }
  });
  it('quotes what the shell would split, and carries the answers', () => {
    expect(value(text, 'STATION_NAME')).toBe('"Radio Ceilidh"');
    expect(value(text, 'CONSENT_CONTACT')).toBe('"https://station.example/takedown"');
    expect(value(text, 'LISTENERS_AUTH')).toBe('on');
    expect(value(text, 'VIDEO')).toBe('true');
    expect(value(text, 'MAIL_MODE')).toBe('cloudron');
    expect(value(text, 'NODE_IP')).toBe('203.0.113.7');
  });
  it('says the player is silent behind the stream password', () => {
    expect(out(cloudron({ streamPassword: true })).notes.join(' ')).toMatch(/player page stays silent/);
    expect(out(cloudron()).notes.join(' ')).not.toMatch(/player page stays silent/);
  });
});

describe('hostile and mistaken answers', () => {
  const fields = (a: Answers) => problems(a).map((p) => p.field);
  it.each(['Kettle "FM"', 'Kettle\\FM', 'Kettle $(reboot)', 'Kettle `id`', 'Kettle # FM', 'Kettle\nFM', 'K\u0000', "'Kettle' FM", '', '   ', 'x'.repeat(201)])('refuses the name %j on both routes', (name) => {
    expect(fields(compose({ name }))).toContain('name');
    expect(fields(cloudron({ name }))).toContain('name');
    expect(generate(compose({ name })).ok).toBe(false);
  });
  it.each(['"; rm -rf /', 'calls@station.example; id', 'a b@c.example', 'calls@station.example\nVIDEO=true', '$HOME', 'javascript:alert(1)', 'not an address', 'http://x/#y'])('refuses the contact %j', (contact) => {
    expect(fields(compose({ contact }))).toContain('contact');
    expect(fields(cloudron({ contact }))).toContain('contact');
  });
  it('takes an empty contact, and says call-in stays closed', () => {
    expect(out(compose({ contact: '' })).notes.join(' ')).toMatch(/Call-in stays closed/);
  });
  it.each(['https://station.example', 'Station.Example', 'station', 'station.example/path', 'station.example:8443', '-a.example', 'a b.example', ''])('refuses the domain %j', (domain) => {
    expect(fields(compose({ domain }))).toContain('domain');
    expect(fields(cloudron({ publicDomain: domain }))).toContain('publicDomain');
  });
  it('refuses two Cloudron names that are the same', () => {
    expect(fields(cloudron({ roomDomain: 'station.example' }))).toContain('roomDomain');
  });
  it("refuses the ports Cloudron's own TURN holds, and 443 for TURN beside the proxy", () => {
    expect(fields(cloudron({ turnTlsPort: 5349 }))).toContain('turnTlsPort');
    expect(fields(cloudron({ turnUdpPort: 3478 }))).toContain('turnUdpPort');
    expect(fields(cloudron({ mediaPort: 50500 }))).toContain('mediaPort');
    expect(fields(compose({ turnTlsPort: 443 }))).toContain('turnTlsPort');
    expect(fields(compose({ mediaPort: 0 }))).toContain('mediaPort');
    expect(fields(compose({ mediaPort: 70000 }))).toContain('mediaPort');
    expect(fields(compose({ mediaPort: 7882.5 }))).toContain('mediaPort');
  });
  it('refuses a node address that is not IPv4, a jurisdiction not in the list, a colour that is none', () => {
    expect(fields(compose({ nodeIp: '2001:db8::1' }))).toContain('nodeIp');
    expect(fields(compose({ nodeIp: '999.1.1.1' }))).toContain('nodeIp');
    expect(fields(compose({ jurisdiction: 'custom' }))).toContain('jurisdiction');
    expect(fields(compose({ accent: 'red' }))).toContain('accent');
    expect(fields(compose({ mail: 'outbox', mailFrom: '' }))).toContain('mailFrom');
    expect(fields(compose({ mail: 'cloudron' }))).toContain('mail');
  });
  it('has no problem with honest answers', () => {
    expect(problems(compose())).toEqual([]);
    expect(problems(cloudron())).toEqual([]);
    expect(problems(compose({ isPublic: false, domain: '' }))).toEqual([]);
  });
});

describe('the DNS records', () => {
  it('Compose: one A record, and no AAAA on the one name, which is the TURN name', () => {
    // compose.yaml gives LiveKit's TURN the station's own name.
    expect(composeYaml).toMatch(/turn:\n\s+enabled: .*\n\s+domain: "\$\{PUBLIC_DOMAIN:-\}"/);
    for (const ipv6 of [false, true]) {
      const dns = out(compose({ ipv6 })).dns;
      expect(dns.map((r) => [r.name, r.type, r.when])).toEqual([
        ['station.example', 'A', 'keep'],
        ['station.example', 'AAAA', 'never'],
      ]);
    }
  });
  it("Cloudron: the TURN name is the app's own domain, not the room's", () => {
    expect(startSh).toContain('echo "  domain: $CLOUDRON_APP_DOMAIN"');
    const dns = out(cloudron()).dns;
    expect(dns.filter((r) => r.type === 'A').map((r) => r.name)).toEqual(['studio.station.example', 'station.example', 'listen.station.example', 'room.station.example']);
    expect(dns.filter((r) => r.type === 'AAAA')).toMatchObject([{ name: 'studio.station.example', when: 'never' }]);
  });
  it('Cloudron with IPv6: every name gets an AAAA for the install, and the studio loses it after', () => {
    const o = out(cloudron({ ipv6: true }));
    const aaaa = o.dns.filter((r) => r.type === 'AAAA');
    expect(aaaa.map((r) => [r.name, r.when])).toEqual([
      ['studio.station.example', 'install-only'],
      ['station.example', 'keep'],
      ['listen.station.example', 'keep'],
      ['room.station.example', 'keep'],
    ]);
    const titles = o.steps.map((s) => s.title);
    expect(titles.indexOf('Set the domain to manual DNS')).toBe(0);
    expect(titles.indexOf('Install the app')).toBeLessThan(titles.indexOf('Delete the studio name’s AAAA record'));
    expect(out(cloudron()).steps.map((s) => s.title)).not.toContain('Delete the studio name’s AAAA record');
  });
});

describe('the ports and the commands', () => {
  it('Compose: the proxy, media and TURN; the optional ones only when on', () => {
    expect(out(compose()).ports.map((p) => `${p.port}/${p.proto}`)).toEqual(['80/TCP', '443/TCP', '7882/UDP', '5349/TCP']);
    expect(out(compose({ turnUdpPort: 3478, iceTcpPort: 7881 })).ports.map((p) => `${p.port}/${p.proto}`)).toEqual(['80/TCP', '443/TCP', '7882/UDP', '5349/TCP', '3478/UDP', '7881/TCP']);
    expect(out(cloudron()).ports.map((p) => `${p.port}/${p.proto}`)).toEqual(['7882/UDP', '15349/TCP']);
  });
  it('Compose: the script is run with the name, after the file is saved', () => {
    const commands = out(compose()).steps.map((s) => s.command ?? '');
    const save = commands.findIndex((c) => c.includes('chmod 600 deploy/compose/.env'));
    const run = commands.findIndex((c) => c === 'deploy/compose/weavesignal.sh --public station.example');
    expect(save).toBeGreaterThanOrEqual(0);
    expect(run).toBeGreaterThan(save);
    // every option used exists in the script
    for (const flag of ['--public', '--no-start', '--env-file']) expect(weavesignalSh).toContain(`${flag})`);
  });
  it('Quadlets: the settings are completed without starting, then the units are generated', () => {
    const o = out(compose({ route: 'quadlets' }));
    const all = o.steps.map((s) => s.command ?? '').join('\n');
    expect(all).toContain('deploy/compose/weavesignal.sh --no-start --env-file ~/.config/weavesignal/station.env --public station.example');
    expect(all).toContain('deploy/quadlets/generate.sh --env-file ~/.config/weavesignal/station.env');
    expect(all).toContain('systemctl --user enable --now weavesignal.target');
    expect(o.file.name).toBe('station.env');
  });
  it("Cloudron: the dialog's rows and the command carry the names and ports, and the file is put in place after", () => {
    const o = out(cloudron({ turnUdpPort: 3479 }));
    expect(o.dialog.map((d) => d.value)).toEqual(['studio.station.example', 'listen.station.example', 'room.station.example', 'station.example', '7882', '15349', '3479', 'off']);
    const install = o.steps.find((s) => s.title === 'Install the app')!.command!;
    expect(install).toContain('--location studio.station.example');
    expect(install).toContain('LISTENERS_DOMAIN=listen.station.example,ROOM_DOMAIN=room.station.example,PUBLIC_DOMAIN=station.example');
    expect(install).toContain('LIVEKIT_MEDIA_PORT=7882,TURN_TLS_PORT=15349,TURN_UDP_PORT=3479');
    // the names the manifest gives its domains and ports
    const manifest = read('deploy/cloudron/CloudronManifest.json');
    for (const k of ['LISTENERS_DOMAIN', 'ROOM_DOMAIN', 'PUBLIC_DOMAIN', 'LIVEKIT_MEDIA_PORT', 'TURN_TLS_PORT', 'TURN_UDP_PORT', 'LIVEKIT_TCP_PORT']) expect(manifest).toContain(`"${k}"`);
    expect(o.setupUrl).toBe('https://studio.station.example/setup/');
  });
  it('ends every route on the setup page, with the station key read first', () => {
    for (const a of [compose(), compose({ route: 'quadlets' }), cloudron()]) {
      const titles = out(a).steps.map((s) => s.title);
      expect(titles.at(-1)).toBe('Finish on the station’s setup page');
      expect(titles.at(-2)).toBe('Read the station key');
    }
  });
  it('never puts an answer where a shell would run it', () => {
    // Names and contacts reach only the file; commands carry domains and ports, which are checked.
    const o = out(compose({ name: "Paddy's; Wireless", contact: 'calls@station.example' }));
    expect(o.steps.map((s) => `${s.command ?? ''}`).join('\n')).not.toContain('Paddy');
  });
});

describe('the answers file', () => {
  it('carries what only the setup page stores, and reads back', () => {
    const o = out(compose({ jurisdiction: 'uk_internet', accent: 'B4532A' }));
    expect(o.answers.name).toBe('weavesignal-setup.json');
    expect(readAnswersFile(o.answers.text)).toEqual({ name: "Paddy's Wireless", contact: 'calls@station.example', jurisdiction: 'uk_internet', accent: '#b4532a' });
  });
  it('holds no secret and no domain', () => {
    expect(Object.keys(JSON.parse(out(cloudron()).answers.text)).sort()).toEqual(['accent', 'contact', 'jurisdiction', 'name', 'weavesignal_setup']);
  });
  it.each(['', 'not json', '[]', 'null', '{}', '{"weavesignal_setup":2}'])('refuses %j', (text) => {
    expect(() => readAnswersFile(text)).toThrow(/not an answers file/);
  });
  it('drops what it does not know and cuts what is too long', () => {
    const got = readAnswersFile(JSON.stringify({ weavesignal_setup: 1, name: `A\u0007${'x'.repeat(500)}`, contact: 7, jurisdiction: 'custom', accent: 'javascript:1', extra: { deep: true } }));
    expect(got.name.length).toBeLessThanOrEqual(200);
    expect(got.name).not.toContain('\u0007');
    expect(got).toMatchObject({ contact: '', jurisdiction: '', accent: '' });
  });
});
