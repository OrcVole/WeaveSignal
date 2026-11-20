// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The stand-in station's 1.2 routes that are not a host's own studio (station.mjs has those):
// the telephone line (17f: crates/control/src/phone/api.rs), the setup page's contact
// (crates/control/src/setup.rs), consent erasure, and the presenter kit (17g:
// crates/control/src/kit.rs). It answers as r120's control plane does, field for field, and
// keeps what the studio sent so a driver can check it. The phone line answers as r121's does:
// `consent_words` and `default_voice` in its state, `PUT /api/v1/phone/consent`, and the two
// shipped voices at `/api/v1/phone/consent/default/{alba,edinburgh}.opus` (the files in
// assets/phone).
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HostStation } from './station.mjs';

const ASSETS = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'assets', 'phone');
const VOICE_HELP =
  'Which recording of the phone words plays when the station has not recorded its own: alba, a Scottish woman\'s voice (the default), or edinburgh, a man\'s voice from Edinburgh; both say "You\'re through to the studio, and your call may go on air and be recorded. Press any key to carry on." Choose own to play only the station\'s own recording: until there is one, the line takes no calls while the words are on. The station\'s own recording, once made, always plays instead of either.';

export const CONSENT_HELP =
  "Before a phone caller joins the queue, play them a few words: who they have reached, that the call may go on air and be recorded, and to press a key to carry on. The station's own recording is played if it has one, otherwise a short recording that comes with WeaveSignal. Leave this on if you record or keep calls (the archive, a podcast, clips), if your callers may not know the call can be broadcast, or if the law where you or your callers are asks for consent before recording (many places do, for example the UK, the EU and several US states). You may turn it off if you never record or keep calls and callers ring a number that is only ever announced on air as a live phone-in, so that calling it is itself agreeing to be on air. With it off, callers go straight into the queue, and the station's records say the words were not played.";
const DEFAULT_WORDS = "You're through to the studio, and your call may go on air and be recorded. Press any key to carry on.";

const PRESETS = [
  { name: 'studio_lan', words: "Studio PC on the network: its VBAN stands by as a show key's source, and the live programme goes back to the studio's monitors (before the delay)." },
  { name: 'transmitter', words: 'Transmitter feed: the programme after the profanity delay, what listeners hear, to a transmitter or a recorder.' },
  { name: 'local_guest', words: 'Local guest: the first VBAN in stands by, and the desk sends it back the monitor mix it makes for that source (everyone but itself).' },
  { name: 'custom', words: 'Custom: the rows as they are.' },
];

export class Station12 extends HostStation {
  constructor(o = {}) {
    super(o);
    /** The phone line. `on`: a trunk is set. `words`: the consent words play. `voice`: the
     *  shipped voice (`alba`, `edinburgh` or `own`). `hasDefault`: the shipped recordings are in
     *  the installation. `routes: false` is a station older than 1.2. */
    this.phone = { routes: true, on: false, words: true, hasDefault: true, lines: 8, atGate: 0, recording: null, bans: [], consents: { '01314960123': [41, 37] }, voice: 'alba', key: true };
    this.contact = { value: 'calls@kettle.example', source: 'settings', settings_value: 'calls@kettle.example' };
    this.stationName = { value: 'Kettle FM', source: 'page', settings_value: 'This station' };
    /** Callers in the queue, as the view's `callers` rows. */
    this.queue = [];
    this.erased = [];
    /** The kit. `allowed: false` with `cloudron` is the Cloudron package; without, VBAN is off. */
    this.kitAllowed = true;
    this.kitCloudron = false;
    this.kit = { preset: 'custom', bind: null, port: 6980, allow_public: false, vban_in: [], vban_out: [] };
    this.clock = { rate: 48_000, sources: [], vban_dropped: {}, vban_out: [] };
  }

  /** A caller in the queue. `phone`: `{ tail }` for a number, `{ withheld: true }` for none. */
  ring(identity, { name = '', state = 'waiting', topic = '', phone = null, consent = 'caller', banned = null, waited_s = 95 } = {}) {
    const now = this.now * 1000;
    this.queue.push({
      identity,
      name,
      ordinal: this.queue.length + 1,
      state,
      since_ms: now - waited_s * 1000,
      arrived_ms: now - waited_s * 1000,
      line: '',
      note: '',
      name_on_air: '',
      declined: null,
      banned_before: banned,
      phone: phone ? (phone.withheld ? { number: 'Withheld', withheld: true, can_ban: false } : { number: `···${phone.tail}`, withheld: false, can_ban: true }) : null,
      topic,
      consent: consent ? { at_ms: now - waited_s * 1000, version: 'v7', by: consent } : null,
      dropped_ms: null,
      slot: null,
      on_air: false,
      cue: false,
      linked: true,
      present: true,
      lufs: -24,
    });
  }

  viewNow() {
    const v = super.viewNow();
    return { ...v, callers: this.queue.map((c) => ({ ...c })), seats: { ...v.seats, screener: 'isla', screener_name: 'Isla Robertson' } };
  }

  phoneStatus() {
    const p = this.phone;
    const station = "You'll be live on Kettle FM, and we may keep your call in our archive and podcast. To have it taken down, contact " + (this.contact.value || 'the station') + '. Press any key to carry on.';
    if (!p.on) return { open: false, closed: 'off', lines: 0, at_gate: 0, in_queue: this.queue.filter((c) => c.phone).length, words: null, words_to_record: null, recording: null, consent_words_help: CONSENT_HELP, consent_words: false, default_voice: 'alba', default_voice_help: VOICE_HELP };
    // The station's own recording plays whenever it has one, instead of either voice.
    const own = !!p.recording;
    const kind = !p.words ? 'off' : own ? 'station' : 'default';
    let closed = null;
    if (!p.key) closed = 'no_key';
    else if (p.words && own && p.recording.out_of_date) closed = { words: 'recording_out_of_date' };
    else if (p.words && !own && p.voice === 'own') closed = { words: 'no_recording' };
    else if (p.words && !own && !p.hasDefault) closed = { words: 'default_missing' };
    return {
      open: closed === null,
      closed,
      lines: p.lines,
      at_gate: p.atGate,
      in_queue: this.queue.filter((c) => c.phone).length,
      words: { kind, version: kind === 'default' ? 'd1' : 'v7', text: kind === 'off' ? 'No words were played to this caller: the station has its phone words off.' : kind === 'station' ? station : DEFAULT_WORDS },
      words_to_record: station,
      recording: p.recording,
      consent_words_help: CONSENT_HELP,
      consent_words: p.words,
      default_voice: p.voice,
      default_voice_help: VOICE_HELP,
    };
  }

  kitView() {
    const why = this.kitCloudron ? 'VBAN is never offered on Cloudron: it is a protocol for one local network, and a Cloudron server is on the internet.' : 'VBAN is off for this installation: set `[kit] vban = true` in the settings file (Compose: KIT_VBAN=true) and restart.';
    const allowed = this.kitAllowed && !this.kitCloudron;
    const pub = (a) => !!a && !/^(10\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.|127\.)/.test(a);
    return {
      allowed,
      why_not: allowed ? null : why,
      warning: this.kit.allow_public && pub(this.kit.bind) ? 'VBAN is on a public address. It has no password and no encryption: anyone on the way can listen or send.' : null,
      kit: JSON.parse(JSON.stringify(this.kit)),
      presets: PRESETS,
      out_sources: '`programme`, `programme_live`, `monitor:<key id>` or `mix_minus:<identity>`',
    };
  }

  checkKit(kit) {
    const ip = (a) => /^\d{1,3}(\.\d{1,3}){3}$/.test(String(a ?? '').trim());
    const pub = (a) => !/^(10\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.|127\.)/.test(a);
    const active = kit.vban_in.length || kit.vban_out.some((o) => o.enabled);
    if (active && !kit.bind) return ['bind', "the station's own address on the network VBAN uses (192.168.1.10, say)"];
    if (kit.bind && !ip(kit.bind)) return ['bind', 'an IP address'];
    for (const [i, v] of kit.vban_in.entries()) {
      if (!ip(v.address)) return [`vban_in[${i}].address`, 'an IP address'];
      if (pub(v.address) && !kit.allow_public) return [`vban_in[${i}].address`, "a public address: VBAN is for the station's own network"];
      if (!/^[\x21-\x7e]{1,16}$/.test(v.name)) return [`vban_in[${i}].name`, '1 to 16 characters of plain ASCII'];
      if (!/^[A-Z2-9a-z]{8}$/.test(v.key_id)) return [`vban_in[${i}].key_id`, "a show key's id (8 characters)"];
    }
    for (const [i, o] of kit.vban_out.entries()) {
      if (!ip(o.address)) return [`vban_out[${i}].address`, 'an IP address'];
      if (/\.255$/.test(o.address)) return [`vban_out[${i}].address`, "one receiver's address: never broadcast or multicast"];
      if (pub(o.address) && !kit.allow_public) return [`vban_out[${i}].address`, "a public address: VBAN is for the station's own network"];
      if (!/^[\x21-\x7e]{1,16}$/.test(o.name)) return [`vban_out[${i}].name`, '1 to 16 characters of plain ASCII'];
    }
    return null;
  }

  /** `binary` is the request's body when it is not JSON (a recording). */
  handle(method, path, body) {
    const p = new URL(path, 'http://station').pathname;
    const me = this.me();
    const admin = me.role === 'admin';
    const log = () => this.calls.push({ method, path, body });
    let m;

    // ---- the telephone line ----
    if (p.startsWith('/api/v1/phone')) {
      log();
      if (!this.phone.routes) return [404, { error: 'not found' }];
      if (p === '/api/v1/phone' && method === 'GET') return [200, this.phoneStatus()];
      if (p === '/api/v1/phone/words') {
        if (!admin) return [403, { error: 'Only an admin does this.' }];
        if (!this.phone.on) return [409, { error: 'The phone line is off.' }];
        if (method === 'PUT') {
          const size = body?.length ?? body?.byteLength ?? 0;
          if (size < 2000) return [422, { error: 'The recording should be between a second and a half and two minutes long.' }];
          this.phone.recording = { duration_ms: 7400, recorded_by: me.account.username, recorded_at_ms: this.now * 1000, out_of_date: false };
          return [200, this.phoneStatus()];
        }
        if (method === 'DELETE') {
          this.phone.recording = null;
          return [200, this.phoneStatus()];
        }
      }
      if (p === '/api/v1/phone/consent' && method === 'PUT') {
        if (!admin) return [403, { error: 'Only an admin does this.' }];
        if (!this.phone.on) return [409, { error: 'The phone line is off.' }];
        if (body?.consent_words === undefined && body?.default_voice === undefined) return [422, { error: 'Say what to change: `consent_words`, `default_voice`, or both.' }];
        if (body.default_voice !== undefined && !['alba', 'edinburgh', 'own'].includes(body.default_voice)) return [422, { error: 'unknown variant' }];
        if (body.consent_words !== undefined) this.phone.words = !!body.consent_words;
        if (body.default_voice !== undefined) this.phone.voice = body.default_voice;
        return [200, this.phoneStatus()];
      }
      if ((m = /^\/api\/v1\/phone\/consent\/default\/([^/]+)$/.exec(p))) {
        if (!this.phone.on) return [409, { error: 'The phone line is off.' }];
        const file = { 'alba.opus': 'phone-words-alba.opus', 'edinburgh.opus': 'phone-words-edinburgh.opus' }[m[1]];
        if (!file) return [404, { error: 'There is no such voice.' }];
        if (!this.phone.hasDefault) return [404, { error: "This station's image has no copy of that voice." }];
        // A Buffer: the driver sends it as audio/ogg.
        return [200, readFileSync(join(ASSETS, file))];
      }
      if (p === '/api/v1/phone/bans' && method === 'GET') return [200, this.phone.bans];
      if (p === '/api/v1/phone/bans' && method === 'POST') {
        if (!String(body.reason ?? '').trim()) return [422, { error: 'a ban needs a reason', field: 'reason' }];
        let tail;
        if (body.identity) {
          const c = this.queue.find((x) => x.identity === body.identity);
          if (!c?.phone) return [404, { error: 'no phone caller in the queue has that key' }];
          if (!c.phone.can_ban) return [409, { error: 'Cannot ban a withheld number' }];
          tail = c.phone.number.replace(/\D/g, '');
        } else {
          if (!admin) return [403, { error: 'Only an admin bans a number as typed.' }];
          tail = String(body.number).replace(/\D/g, '').slice(-4);
        }
        const ban = { ban_key: `pb_${String(this.phone.bans.length + 1).padStart(32, '0')}`, tail, date: this.today, reason: body.reason.trim(), expires_ms: body.days ? this.now * 1000 + body.days * 86_400_000 : null };
        this.phone.bans = [ban, ...this.phone.bans];
        return [200, [ban]];
      }
      if ((m = /^\/api\/v1\/phone\/bans\/([^/]+)$/.exec(p)) && method === 'DELETE') {
        if (!admin) return [403, { error: 'Only an admin does this.' }];
        const had = this.phone.bans.some((b) => b.ban_key === m[1]);
        this.phone.bans = this.phone.bans.filter((b) => b.ban_key !== m[1]);
        return had ? [204, null] : [404, { error: 'There is no ban with that key.' }];
      }
      if (p === '/api/v1/phone/takedown' && method === 'POST') {
        if (!admin) return [403, { error: 'Only an admin does this.' }];
        if (!this.phone.on) return [409, { error: 'The phone line is off.' }];
        return [200, { consents: this.phone.consents[String(body.number).replace(/\D/g, '')] ?? [] }];
      }
      return [404, { error: `the stand-in station has no ${method} ${p}` }];
    }
    if ((m = /^\/api\/v1\/retention\/consents\/(\d+)\/erase$/.exec(p)) && method === 'POST') {
      log();
      if (!admin) return [403, { error: 'Only an admin does this.' }];
      this.erased.push({ id: Number(m[1]), by: body.by });
      return [200, { erased: 1 }];
    }

    // ---- the setup page's name and contact (an admin's) ----
    if (p === '/api/v1/setup') {
      log();
      if (!admin) return [403, { error: 'Only an admin does this.' }];
      if (method === 'PUT') {
        // Whatever is left out is handed back to the settings file (`save` in setup.rs).
        this.stationName = body.name ? { ...this.stationName, value: body.name, source: 'page' } : { ...this.stationName, value: this.stationName.settings_value, source: 'settings' };
        this.contact = body.contact ? { ...this.contact, value: body.contact, source: 'page' } : { ...this.contact, value: this.contact.settings_value, source: 'settings' };
      }
      return [200, { status: 'done', done_by: 'page', missing: [], completed_at_ms: 1, name: this.stationName, contact: this.contact, stream_password: { available: false, enabled: null, asked: null, restart_needed: false }, install: {}, branding: {}, updated_by: null, updated_at_ms: null }];
    }

    // ---- the presenter kit (an admin's; the clock is any staff's) ----
    if (p.startsWith('/api/v1/kit')) {
      log();
      if (p === '/api/v1/kit/clock' && method === 'GET') return [200, this.clock];
      if (!admin) return [403, { error: 'Only an admin does this.' }];
      if (p === '/api/v1/kit' && method === 'GET') return [200, this.kitView()];
      if (p === '/api/v1/kit' && method === 'PUT') {
        const bad = this.checkKit(body);
        if (bad) return [422, { error: bad[1], field: bad[0] }];
        if ((body.vban_in.length || body.vban_out.some((o) => o.enabled)) && !this.kitView().allowed) return [409, { error: this.kitView().why_not }];
        this.kit = body;
        return [200, this.kitView()];
      }
      if (p === '/api/v1/kit/preset' && method === 'POST') {
        const to = body.address;
        if (body.preset !== 'custom' && !to) return [422, { error: 'an address to send to', field: 'address' }];
        const out = (source, name) => ({ address: to, port: 6980, name, source, format: 's16', enabled: true });
        if (body.preset === 'studio_lan') this.kit = { ...this.kit, preset: 'studio_lan', vban_out: [out('programme_live', 'Monitors')] };
        else if (body.preset === 'transmitter') this.kit = { ...this.kit, preset: 'transmitter', vban_out: [out('programme', 'Programme')] };
        else if (body.preset === 'local_guest') this.kit = { ...this.kit, preset: 'local_guest', vban_out: [out(`monitor:${this.kit.vban_in[0]?.key_id ?? 'ABCD2345'}`, 'Return')] };
        else this.kit = { ...this.kit, preset: 'custom' };
        return [200, this.kitView()];
      }
    }
    return super.handle(method, path, body);
  }
}
