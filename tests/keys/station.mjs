// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// A stand-in control plane for keyed shows (1.3.2): Kettle FM on Friday 20 November 2026 (the
// hosts stand-in's day), with three keyed shows, a family of named keys, a family of tickets and
// keys in every state, answering the routes of crates/control/src/access_keys.rs (staff) and
// access_listen.rs (a listener's key) as the control plane answers them: the same field names
// (checked against docs/openapi/weavesignal-v1.json by apps/studio/src/keys.test.ts and by the
// drivers here), the same rules for who may, the same codes and sentences when it refuses.
// Test tooling, not a second implementation: it signs nothing (a token here is a label), counts
// no addresses (a driver sets what the leak report says) and sends no mail.
//
// From 1.3.3 the station holds a keyed show's podcast feed and episode files back, and the studio
// words what a tick does by the station's version (lib/keyed.ts `holdsFiles`). The stand-in says
// the app's own version, as the hosts' does; `version: '1.3.2'` is the one release with keyed
// shows that held nothing back, which an app of 1.3.3 or later words differently.
//
// Sample people are from the Tartan list (ADR-0071): Isla Fraser, Callum Reid, Katie Ross, Neil
// Kennedy, Margaret Anderson, Hugh Wallace, Ewan Graham and the family's patrons; Fiona Kennedy
// presents Friday Supporters' Hour.

import { Station12 } from '../hosts/station-12.mjs';

const ALPHABET = 'ABCDEFGHJKMNPQRSTUVWXYZ23456789';
const DAY = 86_400_000;
const HOUR = 3_600_000;

/** A key's text from a number: the same number, the same text, so every picture is the same. */
export function textOf(seed) {
  let x = (seed * 2654435761) >>> 0;
  let out = '';
  for (let i = 0; i < 12; i++) {
    x = (Math.imul(x ^ (x >>> 15), 2246822519) + 0x9e3779b9) >>> 0;
    out += ALPHABET[x % ALPHABET.length];
  }
  return out;
}
export const written = (t) => `${t.slice(0, 4)}-${t.slice(4, 8)}-${t.slice(8)}`;
/** A typed text as the station compares it: no case, spaces or dashes. */
export const norm = (typed) => String(typed ?? '').toUpperCase().replace(/[^A-Z0-9]/g, '');

const REFUSALS = {
  not_recognised: [404, 'That key is not recognised. Check it: letters and dashes are ignored, and capitals do not matter.'],
  not_yet_valid: [403, 'This key has not started yet.'],
  ended: [403, 'This key has ended. Ask the station for a new one.'],
  paused: [403, 'This key is switched off for now. Ask the station.'],
  revoked: [403, 'This key was ended by the station. Ask the station for a new one.'],
  does_not_open_show: [403, 'This key does not open this show.'],
  not_scheduled: [403, 'This key works, but none of its shows is on the schedule now or in the next fortnight.'],
  too_many_devices: [409, 'This key is already in use on its most devices. Sign out of one.'],
  people_limit: [409, 'This key has reached the number of people it is for.'],
  listens_used_up: [409, "This key's listens are used up."],
  device_signed_out: [403, 'This device was signed out. Enter the key again.'],
};

/** The leak report's score and reasons, as `leak_score` in crates/model/src/listener_keys.rs. */
export function leakScore(k, s) {
  const per = Math.max(1, k.max_devices ?? 3);
  const allow = per * (k.kind === 'group' ? Math.max(1, k.max_people ?? 10) : 1);
  let score = 0;
  const reasons = [];
  if (s.addresses_7d > allow) {
    score += (s.addresses_7d - allow) * 2;
    reasons.push(`${s.addresses_7d} addresses in the last 7 days, where about ${allow} is usual`);
  }
  if (s.devices > allow) {
    score += (s.devices - allow) * 5;
    reasons.push(`${s.devices} devices have used it, where about ${allow} is usual`);
  }
  if (s.peak_new_addresses_in_an_hour >= 3) {
    score += 10 + 2 * (s.peak_new_addresses_in_an_hour - 3);
    reasons.push(`${s.peak_new_addresses_in_an_hour} new addresses within one hour`);
  }
  return { score, reasons };
}

export class KeysStation extends Station12 {
  /** `keysUp`: `true`; `'no-database'` and `'no-station-key'` answer the two 503s; `'older'` is
   *  a station before 1.3.2, with none of these routes. `seed: false` is a station with keyed
   *  support and nothing made yet; `shows: false` one with no keyed show. `mail` is whether the
   *  station sends mail. `onAir` puts a show on air now, by slug (for the player and the desk). */
  constructor({ keysUp = true, seed = true, shows = true, role = 'producer', onAir = null, mail = true, playoutKeyed = true, ...o } = {}) {
    super({ role, step: 'full', ...o });
    this.keysUp = keysUp;
    /** Playout has keyed shows on. False: the station refuses to key a show (409), as the
     *  control plane does when playout's `keyed.enabled` is off: it fails closed. */
    this.playoutKeyed = playoutKeyed;
    this.mail = mail;
    this.onAirSlug = onAir;
    this.getAKey = { text: 'Kettle FM’s supporters get a key by email at the start of each season. Join at kettle.example/supporters, or ask at the desk on a Friday.', url: 'https://kettle.example/supporters' };
    /** Show id to its rule. */
    this.access = new Map();
    this.lkeys = [];
    this.families = [];
    /** Key id to its devices, log and leak figures. */
    this.devs = new Map();
    this.logs = new Map();
    this.stats = new Map();
    this.seq = 0;
    this.lostAsked = [];
    this.tokens = new Map();
    /** How often a player is told to renew, in seconds (a driver shortens it to see a key cut). */
    this.renewIn = 60;
    /** The station's `keys.ends_soon_days` and `keys.device_hold_s`. */
    this.soonDays = 7;
    this.holdS = 360;
    /** Whether playout keeps the keyed show on air from the public (false: the studio's alarm). */
    this.gateKept = true;
    /** What the public hears meanwhile, as playout says it. */
    this.publicHears = 'fallback';
    /** Whether playout answered the station's last look (1.3.2). False: the studio's third state,
     *  not confirmed; the station then reports the listener process's fallback as kept. */
    this.gateConfirmed = true;
    /** A guest's invitation, for the join page: the show it is for, by slug. */
    this.inviteFor = null;
    const weekly = (weekdays, start, minutes) => ({ starts_on: '2026-09-04', start, minutes, repeat: { kind: 'weekly', every: 1, weekdays }, end: { kind: 'never' }, skip: [], moves: [] });
    this.add({ slug: 'friday-supporters', name: 'Friday Supporters’ Hour', colour_slot: 8, presenter: 'Fiona Kennedy', hosts: ['Fiona Kennedy'], rule: weekly([5], 19 * 60, 60) });
    this.add({ slug: 'the-archive-hour', name: 'The Archive Hour', colour_slot: 11, presenter: 'Magnus Henderson', hosts: ['Magnus Henderson'], rule: weekly([7], 15 * 60, 60) });
    this.add({ slug: 'ceilidh-night-live', name: 'Ceilidh Night Live', colour_slot: 4, presenter: 'Shona Campbell', hosts: ['Shona Campbell'], rule: { starts_on: '2026-11-28', start: 20 * 60, minutes: 150, repeat: { kind: 'once' }, end: { kind: 'never' }, skip: [], moves: [] } });
    if (shows) {
      const at = this.now * 1000 - 9 * DAY;
      this.access.set('id-friday-supporters', { rule: 'key', opens_at_ms: null, slate: 'announcement', slate_note: 'Supporters hear this show. Join at kettle.example/supporters.', updated_by: 'Shona Campbell', updated_ms: at });
      this.access.set('id-the-archive-hour', { rule: 'key', opens_at_ms: Date.UTC(2026, 10, 29, 15, 0), slate: 'fallback', slate_note: '', updated_by: 'Shona Campbell', updated_ms: at });
      this.access.set('id-ceilidh-night-live', { rule: 'key', opens_at_ms: null, slate: 'teaser', slate_note: 'Tickets at kettle.example/ceilidh.', updated_by: 'Morag Campbell', updated_ms: at });
    }
    if (seed && shows) this.seed();
  }

  // ---- the driver's hands ----

  seed() {
    const now = this.now * 1000;
    const patrons = this.family('Autumn patrons', 'Supporters who joined from September to November.', now - 30 * DAY);
    const fri = ['id-friday-supporters', 'id-the-archive-hour'];
    const k = (o) => this.put({ shows: fri, all_shows: false, created_ms: now - 30 * DAY, ...o });
    // The docs' own example text is Isla's.
    this.isla = k({ label: 'Isla Fraser', email: 'isla.fraser@mail.example', family: patrons, text: 'KETM7QMP2D4X', valid_until_ms: Date.UTC(2026, 10, 25, 22, 0), used: { first: now - 28 * DAY, last: now - 26 * HOUR, listens: 9 } });
    this.device(this.isla, 'Firefox on Linux', { first: now - 28 * DAY, last: now - 26 * HOUR, show: 'id-friday-supporters' });
    this.device(this.isla, 'Safari on iPhone', { first: now - 20 * DAY, last: now - 7 * DAY, show: 'id-friday-supporters' });
    this.setStats(this.isla, { addresses: 5, addresses_24h: 0, addresses_7d: 2, devices: 2, peak: 1 });
    this.callum = k({ label: 'Callum Reid', email: 'callum.reid@mail.example', family: patrons, used: { first: now - 21 * DAY, last: now - 7 * DAY, listens: 4 } });
    this.device(this.callum, 'Chrome on Windows', { first: now - 21 * DAY, last: now - 7 * DAY, show: 'id-friday-supporters' });
    this.setStats(this.callum, { addresses: 2, addresses_24h: 0, addresses_7d: 1, devices: 1, peak: 1 });
    this.katie = k({ label: 'Katie Ross', email: 'katie.ross@mail.example', family: patrons, state: 'paused', state_reason: 'Membership payment is late', note: 'Renewal due 1 November', used: { first: now - 25 * DAY, last: now - 14 * DAY, listens: 3 } });
    this.setStats(this.katie, { addresses: 1, addresses_24h: 0, addresses_7d: 0, devices: 1, peak: 1 });
    // A key passed on: found on a forum, replaced, and the new one sent to the same person.
    this.hugh = k({ label: 'Hugh Wallace', email: 'hugh.wallace@mail.example', family: patrons, used: { first: now - 19 * DAY, last: now - 40 * 60_000, listens: 61 } });
    for (const [i, label] of ['Chrome on Android', 'Firefox on Windows', 'Safari on iPhone', 'Chrome on Windows', 'Edge on Windows', 'Chrome on macOS', 'Firefox on Linux', 'Safari on iPad', 'Chrome on Android'].entries()) this.device(this.hugh, label, { first: now - (i < 2 ? 19 * DAY : 2 * DAY), last: now - (i < 2 ? 40 * 60_000 : (i + 2) * HOUR), show: 'id-friday-supporters' });
    this.setStats(this.hugh, { addresses: 44, addresses_24h: 33, addresses_7d: 41, devices: 9, peak: 17 });
    for (const [i, name] of ['Margaret Anderson', 'Jamie Henderson', 'Rosie MacIntosh', 'Kenneth Bruce', 'Wendy Armstrong', 'Lewis Gordon', 'Grace MacMillan', 'Fergus Thomson'].entries()) {
      const x = k({ label: name, email: i % 3 === 2 ? null : `${name.toLowerCase().replace(' ', '.')}@mail.example`, family: patrons, ...(i === 0 ? { valid_until_ms: Date.UTC(2026, 10, 15, 22, 0) } : {}), ...(i > 4 ? {} : { used: { first: now - (20 - i) * DAY, last: now - (i + 2) * DAY, listens: 2 + i } }) });
      if (i <= 4) this.setStats(x, { addresses: 1 + (i % 2), addresses_24h: 0, addresses_7d: i % 2, devices: 1, peak: 1 });
      if (i === 0) this.margaret = x;
    }
    // A leak dealt with a fortnight ago: Neil's first key revoked and replaced.
    const neilOld = k({ label: 'Neil Kennedy', email: 'neil.kennedy@mail.example', family: patrons, state: 'revoked', state_reason: 'replaced', created_ms: now - 30 * DAY, used: { first: now - 29 * DAY, last: now - 15 * DAY, listens: 22 } });
    this.neil = k({ label: 'Neil Kennedy', email: 'neil.kennedy@mail.example', family: patrons, created_ms: now - 14 * DAY, replaces: neilOld, used: { first: now - 13 * DAY, last: now - 6 * DAY, listens: 2 } });
    this.setStats(neilOld, { addresses: 12, addresses_24h: 0, addresses_7d: 0, devices: 6, peak: 5 });
    this.setStats(this.neil, { addresses: 1, addresses_24h: 0, addresses_7d: 1, devices: 1, peak: 1 });

    const tickets = this.family('Ceilidh Night tickets', 'Sold at the hall and at kettle.example/ceilidh.', now - 6 * DAY);
    for (let i = 1; i <= 120; i++) {
      const used = i === 14 ? { first: now - 2 * DAY, last: now - 3 * HOUR, listens: 1 } : null;
      const x = this.put({ label: `Ticket ${String(i).padStart(3, '0')}`, family: tickets, shows: ['id-ceilidh-night-live'], all_shows: false, max_devices: 1, valid_from_ms: Date.UTC(2026, 10, 28, 19, 30), valid_until_ms: Date.UTC(2026, 10, 29, 2, 0), created_ms: now - 6 * DAY + i, created_by: 'Morag Campbell', ...(used ? { used } : {}) });
      if (i === 14) this.ticket = x;
    }

    this.choir = this.put({ kind: 'group', label: 'Dunfermline Gaelic Choir', email: 'secretary@gaelic-choir.example', note: 'One key for the choir: 40 singers', shows: ['id-friday-supporters'], all_shows: false, max_people: 40, max_devices: 2, created_ms: now - 12 * DAY, used: { first: now - 11 * DAY, last: now - 6 * DAY, listens: 31 } });
    this.setStats(this.choir, { addresses: 23, addresses_24h: 0, addresses_7d: 19, devices: 21, peak: 2 });
    this.relay = this.put({ kind: 'machine', label: 'Radio Ross relay', note: 'Their Friday rebroadcast. Rotate at new year.', all_shows: true, shows: [], max_devices: null, created_ms: now - 40 * DAY, used: { first: now - 39 * DAY, last: now - 6 * DAY, listens: 6 } });
    this.setStats(this.relay, { addresses: 1, addresses_24h: 0, addresses_7d: 1, devices: 1, peak: 1 });
    this.press = this.put({ label: 'Ewan Graham', email: 'ewan.graham@courier.example', note: 'Press preview, the Courier', shows: ['id-the-archive-hour'], all_shows: false, max_devices: 1, max_listens: 2, valid_from_ms: Date.UTC(2026, 10, 27, 9, 0), valid_until_ms: Date.UTC(2026, 10, 29, 15, 0), created_ms: now - 2 * DAY });
    return this;
  }

  family(label, note = '', created_ms = this.now * 1000) {
    const f = { id: `fam-${this.families.length + 1}`, label, note, created_by: 'Shona Campbell', created_ms };
    this.families.unshift(f);
    return f.id;
  }

  /** A key as the station keeps it, without the round trip. */
  put({ label, kind = 'personal', email = null, note = '', media = 'both', all_shows = true, shows = [], valid_from_ms = null, valid_until_ms = null, max_devices = kind === 'machine' ? null : 2, max_listens = null, max_people = null, state = 'active', state_reason = '', family = null, text = null, created_ms = this.now * 1000, created_by = 'Shona Campbell', used = null, replaces = null }) {
    const n = ++this.seq;
    const k = { id: `key-${String(n).padStart(4, '0')}`, kind, label, note, email, media, all_shows, shows: [...shows], valid_from_ms, valid_until_ms, max_devices, max_listens, max_people, state, state_reason, listens_used: used?.listens ?? 0, first_used_ms: used?.first ?? null, last_used_ms: used?.last ?? null, family_id: family, replaces_id: null, replaced_by_id: null, created_by, created_ms, updated_ms: created_ms, text: text ?? textOf(n), sealed: true };
    if (replaces) {
      k.replaces_id = replaces.id;
      replaces.replaced_by_id = k.id;
    }
    this.lkeys.unshift(k);
    this.devs.set(k.id, []);
    this.logs.set(k.id, [{ action: 'created', actor: created_by, at_ms: created_ms, detail: {} }]);
    if (state === 'paused') this.log(k, 'paused', { reason: state_reason }, created_ms + 5 * DAY);
    if (state === 'revoked') this.log(k, state_reason === 'replaced' ? 'replaced' : 'revoked', state_reason === 'replaced' ? {} : { reason: state_reason }, this.now * 1000 - 14 * DAY);
    return k;
  }

  device(k, label, { first = this.now * 1000, last = this.now * 1000, show = null, ended = null } = {}) {
    const list = this.devs.get(k.id);
    const d = { id: `dev-${k.id.slice(4)}-${list.length + 1}`, label, first_seen_ms: first, last_seen_ms: last, ended_ms: ended, last_show_id: show };
    list.push(d);
    return d;
  }

  log(k, action, detail = {}, at_ms = this.now * 1000, actor = this.me().account.name) {
    this.logs.get(k.id).unshift({ action, actor, at_ms, detail });
  }

  setStats(k, { addresses, addresses_24h, addresses_7d, devices, peak }) {
    this.stats.set(k.id, { addresses, addresses_24h, addresses_7d, devices, peak_new_addresses_in_an_hour: peak });
  }

  /** The key's text can no longer be read: the station's token key changed since it was made. */
  unseal(k) {
    k.sealed = false;
  }

  // ---- the views ----

  /** A key as staff see it (`ListenerKey`): never its text or its email address. */
  keyView(k) {
    const { text: _t, email, sealed: _s, ...rest } = k;
    return { ...rest, has_email: !!email };
  }

  link(k) {
    return `${this.publicUrl ?? 'https://kettle.example'}/listen/#key=${written(k.text)}`;
  }

  familyView(f) {
    return { ...f, keys: this.lkeys.filter((k) => k.family_id === f.id).length };
  }

  showBy(idOrSlug) {
    return this.shows.find((s) => s.id === idOrSlug || s.slug === idOrSlug) ?? null;
  }

  accessView(show) {
    const a = this.access.get(show.id);
    return a ? { show_id: show.id, ...a } : { show_id: show.id, rule: 'public', opens_at_ms: null, slate: 'announcement', slate_note: '', updated_by: '', updated_ms: 0 };
  }

  keyedShows() {
    const now = this.now * 1000;
    const rows = [...this.access.entries()]
      .filter(([, a]) => a.rule === 'key')
      .map(([id, a]) => {
        const s = this.shows.find((x) => x.id === id);
        return { id, slug: s.slug, name: s.name, now: a.opens_at_ms !== null && a.opens_at_ms <= now ? 'public' : 'key', opens_at_ms: a.opens_at_ms, slate: a.slate, slate_note: a.slate_note };
      });
    const on = this.keyedOnAir();
    return { shows: rows, on_now: on?.id ?? null, get_a_key_text: this.getAKey.text, get_a_key_url: this.getAKey.url, mail: this.mail };
  }

  leaks(family = null) {
    const entries = this.lkeys
      .filter((k) => !family || k.family_id === family)
      .map((k) => {
        const s = this.stats.get(k.id) ?? { addresses: 0, addresses_24h: 0, addresses_7d: 0, devices: 0, peak_new_addresses_in_an_hour: 0 };
        const { score, reasons } = leakScore(k, s);
        return { key: this.keyView(k), family: this.families.find((f) => f.id === k.family_id)?.label ?? null, first_seen_ms: k.first_used_ms, last_seen_ms: k.last_used_ms, listens: k.listens_used, ...s, spike: s.peak_new_addresses_in_an_hour >= 3, score, reasons };
      });
    const used = (e) => (e.first_seen_ms === null ? 1 : 0);
    entries.sort((a, b) => used(a) - used(b) || b.score - a.score || b.addresses_7d - a.addresses_7d || (b.last_seen_ms ?? 0) - (a.last_seen_ms ?? 0));
    return { generated_ms: this.now * 1000, entries };
  }

  /** The show on air now, for the player's facts and the studio's view. */
  onAirShow() {
    return this.onAirSlug ? this.shows.find((s) => s.slug === this.onAirSlug) : null;
  }

  /** The show on air when its keyed window is open: keyed by its rule and not opened to all. */
  keyedOnAir() {
    const s = this.onAirShow();
    const a = s && this.access.get(s.id);
    return a && a.rule === 'key' && !(a.opens_at_ms !== null && a.opens_at_ms <= this.now * 1000) ? s : null;
  }

  viewNow() {
    const v = super.viewNow();
    const s = this.onAirShow();
    if (!s) return v;
    const k = this.keyedOnAir();
    const keyed = !k ? null : this.gateConfirmed ? { show: k.id, kept: this.gateKept, confirmed: true, public_hears: this.gateKept ? this.publicHears : null } : { show: k.id, kept: this.gateKept, confirmed: false, public_hears: this.gateKept ? 'listener_fallback' : null };
    return { ...v, show: { ...v.show, title: s.name }, step: { ...v.step, show: { slug: s.slug, name: s.name }, on_air: true }, keyed };
  }

  player() {
    const s = this.onAirShow();
    return {
      station: 'Kettle FM',
      show: s?.name ?? '',
      show_id: s?.id ?? null,
      keyed: !!this.keyedOnAir(),
      stream_url: 'https://stream.kettle.example/programme.opus',
      streams: [
        { codec: 'opus', kbps: 96, url: 'https://stream.kettle.example/programme.opus' },
        { codec: 'mp3', kbps: 128, url: 'https://stream.kettle.example/programme.mp3' },
      ],
      presenter: s?.presenter ?? '',
      presenter_live: !!s,
      show_on_air: !!s,
      call_in: { open: false, why_not: s ? 'This show takes no calls.' : 'The lines open when a show is on air.', consent_text: 'Your call may be broadcast and recorded.', consent_version: 'v1', name_max: 40, topic_max: 140 },
      relay: { turn_tls_port: 5349, stun_servers: [] },
      video: null,
    };
  }

  // ---- the rules ----

  problemsOf(b, { needLabel = true } = {}) {
    if (needLabel && !(typeof b.label === 'string' && b.label.trim().length >= 1 && [...b.label.trim()].length <= 120)) return ['label', 'who the key is for: 1 to 120 characters'];
    if (b.note !== undefined && [...String(b.note)].length > 500) return ['note', 'a note is up to 500 characters'];
    if (b.email && !/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(b.email)) return ['email', 'that is not an email address'];
    if (b.shows && b.shows.length > 32) return ['shows', 'a key opens at most 32 named shows'];
    for (const s of b.shows ?? []) if (!this.showBy(s)) return ['shows', `show \`${s}\` does not exist`];
    if (b.valid_from_ms != null && b.valid_until_ms != null && b.valid_until_ms <= b.valid_from_ms) return ['valid_until_ms', 'the end is after the start'];
    for (const f of ['max_devices', 'max_listens', 'max_people']) if (b[f] != null && !(Number.isInteger(b[f]) && b[f] >= 1)) return [f, 'a whole number from 1, or null for any number'];
    return null;
  }

  make(b, by) {
    const shows = (b.shows ?? []).map((s) => this.showBy(s).id);
    const kind = b.kind ?? 'personal';
    const k = this.put({ label: b.label.trim(), kind, email: b.email ?? null, note: b.note ?? '', media: b.media ?? 'both', all_shows: b.all_shows ?? shows.length === 0, shows, valid_from_ms: b.valid_from_ms ?? null, valid_until_ms: b.valid_until_ms ?? null, max_devices: 'max_devices' in b ? b.max_devices : kind === 'machine' ? null : 2, max_listens: b.max_listens ?? null, max_people: b.max_people ?? null, family: b.family ?? null, created_by: by });
    return { key: this.keyView(k), text: written(k.text), link: this.link(k) };
  }

  /** Why a key does not open now, as `Refusal` in crates/model/src/listener_keys.rs; null when it does. */
  refusalOf(k, show, device) {
    const now = this.now * 1000;
    if (!k) return ['not_recognised'];
    if (k.state === 'revoked') return ['revoked'];
    if (k.state === 'paused') return ['paused'];
    if (k.valid_from_ms !== null && now < k.valid_from_ms) return ['not_yet_valid', { from_ms: k.valid_from_ms }];
    if (k.valid_until_ms !== null && now >= k.valid_until_ms) return ['ended', { until_ms: k.valid_until_ms }];
    if (show && !k.all_shows && !k.shows.includes(show.id)) return ['does_not_open_show'];
    const devs = this.devs.get(k.id);
    const mine = devs.find((d) => d.id === device && d.ended_ms === null);
    if (mine) return null;
    if (k.max_listens !== null && k.listens_used >= k.max_listens) return ['listens_used_up'];
    if (k.kind === 'group' && k.max_people !== null && devs.length >= k.max_people) return ['people_limit'];
    const live = devs.filter((d) => d.ended_ms === null && now - d.last_seen_ms <= this.holdS * 1000);
    if (k.kind !== 'machine' && k.max_devices !== null && live.length >= k.max_devices) return ['too_many_devices', { devices: live, max_devices: k.max_devices, device_hold_s: this.holdS }];
    return null;
  }

  refuse([code, extra = {}]) {
    const [status, error] = REFUSALS[code];
    return [status, { error, code, ...extra }];
  }

  listenKey(k) {
    const now = this.now * 1000;
    return { id: k.id, kind: k.kind, media: k.media, all_shows: k.all_shows, shows: k.all_shows ? [] : k.shows.map((id) => this.shows.find((s) => s.id === id)).filter(Boolean).map((s) => ({ id: s.id, slug: s.slug, name: s.name })), valid_from_ms: k.valid_from_ms, valid_until_ms: k.valid_until_ms, ends_soon: k.valid_until_ms !== null && k.valid_until_ms - now <= this.soonDays * DAY };
  }

  /** The show a token is for, as the station picks it: the one asked for, else the keyed show on
   *  now that the key opens, else the first keyed show it opens (the stand-in has no windows). */
  tokenShow(k, asked) {
    const opens = (s) => k.all_shows || k.shows.includes(s.id);
    if (asked) return asked;
    const on = this.keyedOnAir();
    if (on && opens(on)) return on;
    return [...this.access.entries()].filter(([, a]) => a.rule === 'key').map(([id]) => this.shows.find((s) => s.id === id)).find((s) => s && opens(s)) ?? null;
  }

  /** A token for one show's window (`ks1.…`, as the control plane's, not signed here). */
  token(k, device, show) {
    const token = `ks1.${Buffer.from(JSON.stringify({ k: k.id, s: show.id, d: device.id, n: this.tokens.size + 1 })).toString('base64url')}.stand-in`;
    const now = this.now * 1000;
    this.tokens.set(token, { key: k.id, device: device.id, show: show.id, expires_ms: now + 3_600_000 });
    return {
      token,
      device: device.id,
      starts_ms: now - 3_000,
      expires_ms: now + 3_600_000,
      show: { id: show.id, slug: show.slug, name: show.name },
      // A sound key's HLS is the sound's playlist, as the control plane's (19f).
      urls: { hls: `/hls/k/${show.id}/${token}/${k.media === 'sound' ? 'audio' : 'live'}.m3u8`, opus: `/listen/k/${show.id}/${token}/programme-keyed.opus`, mp3: `/listen/k/${show.id}/${token}/programme-keyed.mp3` },
      renew_in_s: this.renewIn,
      key: this.listenKey(k),
    };
  }

  // ---- the routes ----

  handle(method, path, body) {
    const url = new URL(path, 'http://station');
    const p = url.pathname;
    let m;
    const mine = p.startsWith('/api/v1/listener-keys') || p.startsWith('/api/v1/keyed/') || p === '/api/v1/public/keyed-shows' || /^\/api\/v1\/shows\/[^/]+\/access$/.test(p) || p === '/api/v1/player' || p === '/api/v1/pass';
    if (!mine) return super.handle(method, path, body);
    this.calls.push({ method, path, body });
    if (p === '/api/v1/player') return [200, this.player()];
    // A guest's invitation (the join page): the pass's own facts.
    if (p === '/api/v1/pass') {
      const s = this.inviteFor ? this.shows.find((x) => x.slug === this.inviteFor) : null;
      if (!s) return [401, { error: 'This link no longer works.' }];
      return [200, { kind: 'invite', role: 'guest', identity: 'guest-gregor', name: 'Gregor Stewart', station: 'Kettle FM', show: s.name, expires_at_ms: this.now * 1000 + 4 * 3_600_000, ended: false, relay: { turn_tls_port: 5349, stun_servers: [] }, studio: 'live', show_id: s.id, keyed: (() => { const a = this.access.get(s.id); return !!a && a.rule === 'key' && !(a.opens_at_ms !== null && a.opens_at_ms <= this.now * 1000); })() }];
    }
    // A station older than 1.3.2 has none of these routes.
    if (this.keysUp === 'older') return [404, { error: 'not found' }];

    // ---- anyone ----
    if (p === '/api/v1/public/keyed-shows' && method === 'GET') return [200, this.keyedShows()];
    if (p.startsWith('/api/v1/keyed/')) return this.listen(method, p, body ?? {});

    // ---- staff ----
    const me = this.me();
    const by = me.account.name;
    if (me.role !== 'admin' && me.role !== 'producer') return [403, { error: "Only a producer or an admin can manage keys. Ask the station's admin." }];
    if (this.keysUp === 'no-database') return [503, { error: 'Keys are kept in the database.' }];
    if (this.keysUp === 'no-station-key') return [503, { error: 'Keys need a station key to seal them under: set WEAVESIGNAL_CONTROL_TOKEN_KEY (or the station key).' }];

    if ((m = /^\/api\/v1\/shows\/([^/]+)\/access$/.exec(p))) {
      const show = this.showBy(decodeURIComponent(m[1]));
      if (!show) return [404, { error: `show \`${m[1]}\` does not exist` }];
      if (method === 'GET') return [200, this.accessView(show)];
      if (body.rule !== 'public' && body.rule !== 'key') return [422, { error: '`public` or `key`', field: 'rule' }];
      if (body.slate !== undefined && !['announcement', 'fallback', 'teaser'].includes(body.slate)) return [422, { error: '`announcement`, `fallback` or `teaser`', field: 'slate' }];
      if ([...(body.slate_note ?? '')].length > 500) return [422, { error: 'a line of at most 500 characters', field: 'slate_note' }];
      if (body.rule === 'key' && !this.playoutKeyed) return [409, { error: "Playout has keyed shows off (its data's `keyed.enabled`), so nothing would keep this show from the public. Turn them on and restart playout first." }];
      this.access.set(show.id, { rule: body.rule, opens_at_ms: body.opens_at_ms ?? null, slate: body.slate ?? 'announcement', slate_note: body.slate_note ?? '', updated_by: by, updated_ms: this.now * 1000 });
      return [200, this.accessView(show)];
    }

    if (p === '/api/v1/listener-keys' && method === 'GET') {
      const q = url.searchParams;
      const show = q.get('show') ? this.showBy(q.get('show')) : null;
      const needle = (q.get('q') ?? '').trim().toLowerCase();
      let keys = this.lkeys.filter((k) => (!q.get('family') || k.family_id === q.get('family')) && (!q.get('state') || k.state === q.get('state')) && (!q.get('kind') || k.kind === q.get('kind')) && (!show || k.all_shows || k.shows.includes(show.id)) && (!needle || k.label.toLowerCase().includes(needle) || k.note.toLowerCase().includes(needle)));
      const total = keys.length;
      const offset = Number(q.get('offset') ?? 0);
      keys = keys.slice(offset, offset + Number(q.get('limit') ?? 100));
      return [200, { keys: keys.map((k) => this.keyView(k)), total }];
    }
    if (p === '/api/v1/listener-keys' && method === 'POST') {
      const bad = this.problemsOf(body);
      if (bad) return [422, { error: bad[1], field: bad[0] }];
      if (body.family && !this.families.some((f) => f.id === body.family)) return [422, { error: 'no such family', field: 'family' }];
      return [201, this.make(body, by)];
    }
    if (p === '/api/v1/listener-keys/find' && method === 'POST') {
      const k = this.lkeys.find((x) => x.text === norm(body.text));
      return k ? [200, this.keyView(k)] : [404, { error: 'no such key' }];
    }
    if (p === '/api/v1/listener-keys/leaks' && method === 'GET') return [200, this.leaks(url.searchParams.get('family'))];
    if (p === '/api/v1/listener-keys/signing' && method === 'GET') return [200, { alg: 'p256', prefix: 'ks1', public_key: 'c3RhbmQtaW4tcHVibGljLWtleS1ub3QtYS1yZWFsLW9uZQ', token_ttl_s: 120, renew_every_s: 60, clock_skew_s: 5, ends_soon_days: this.soonDays, device_hold_s: this.holdS }];
    if (p === '/api/v1/listener-keys/families' && method === 'GET') return [200, { families: this.families.map((f) => this.familyView(f)) }];
    if (p === '/api/v1/listener-keys/families' && method === 'POST') {
      const label = String(body.label ?? '').trim();
      if (!label || [...label].length > 120) return [422, { error: "the family's name: 1 to 120 characters", field: 'label' }];
      if (this.families.some((f) => f.label.toLowerCase() === label.toLowerCase())) return [409, { error: 'a family has that name already', field: 'label' }];
      const named = body.recipients ?? [];
      const n = named.length || body.count || 0;
      if (n < 1) return [422, { error: 'name the recipients, or say how many keys', field: 'recipients' }];
      if (n > 1000) return [422, { error: 'at most 1000 keys a call', field: named.length ? 'recipients' : 'count' }];
      const bad = this.problemsOf(body.template ?? {}, { needLabel: false });
      if (bad) return [422, { error: bad[1], field: bad[0] }];
      for (const r of named) {
        const b2 = this.problemsOf({ ...r });
        if (b2) return [422, { error: `${r.label || 'a recipient'}: ${b2[1]}`, field: 'recipients' }];
      }
      const fam = this.family(label, body.note ?? '');
      this.families[0].created_by = by;
      const prefix = (body.prefix ?? label).trim();
      const width = Math.max(3, String(n).length);
      const made = [];
      for (let i = 0; i < n; i++) {
        const r = named[i] ?? { label: `${prefix} ${String(i + 1).padStart(width, '0')}` };
        made.push(this.make({ ...body.template, label: r.label, email: r.email ?? null, note: r.note ?? '', family: fam }, by));
      }
      // Newest first in the list means the last made first: keep the answer in the recipients' order.
      return [201, { family: this.familyView(this.families[0]), keys: made }];
    }
    if ((m = /^\/api\/v1\/listener-keys\/families\/([^/]+)(\/export)?$/.exec(p)) && method === 'GET') {
      const f = this.families.find((x) => x.id === m[1]);
      if (!f) return [404, { error: 'no such family' }];
      if (!m[2]) return [200, this.familyView(f)];
      const keys = this.lkeys.filter((k) => k.family_id === f.id).reverse();
      if (keys.some((k) => !k.sealed)) return [409, { error: "the station's token key changed since these keys were made: their text can no longer be read; replace the keys" }];
      const cell = (v) => {
        let s = String(v ?? '');
        if (/^[=+\-@\t\r]/.test(s)) s = `'${s}`;
        return /[",\r\n']/.test(s) ? `"${s.replaceAll('"', '""')}"` : s;
      };
      for (const k of keys) this.log(k, 'text_shown', { export: f.id });
      // A calendar and a CSV are text: the driver answers a string as it is.
      return [200, `name,email,note,key,link,state\r\n${keys.map((k) => [k.label, k.email, k.note, written(k.text), this.link(k), k.state].map(cell).join(',')).join('\r\n')}\r\n`];
    }

    if ((m = /^\/api\/v1\/listener-keys\/([^/]+)(?:\/([a-z]+)(?:\/([^/]+))?)?$/.exec(p))) {
      const k = this.lkeys.find((x) => x.id === decodeURIComponent(m[1]));
      if (!k) return [404, { error: 'no such key' }];
      const [, , verb, sub] = m;
      const now = this.now * 1000;
      if (!verb && method === 'GET') return [200, this.keyView(k)];
      if (!verb && method === 'DELETE') {
        if (me.role !== 'admin') return [403, { error: 'Only an admin does this.' }];
        this.lkeys = this.lkeys.filter((x) => x !== k);
        for (const x of this.lkeys) {
          if (x.replaces_id === k.id) x.replaces_id = null;
          if (x.replaced_by_id === k.id) x.replaced_by_id = null;
        }
        return [204, null];
      }
      if (!verb && method === 'PATCH') {
        if (k.state === 'revoked') return [409, { error: 'a revoked key cannot be changed: replace it instead' }];
        const bad = this.problemsOf({ ...body, valid_from_ms: 'valid_from_ms' in body ? body.valid_from_ms : k.valid_from_ms, valid_until_ms: 'valid_until_ms' in body ? body.valid_until_ms : k.valid_until_ms }, { needLabel: 'label' in body });
        if (bad) return [422, { error: bad[1], field: bad[0] }];
        const fields = [];
        for (const [f, name] of [['label', 'label'], ['note', 'note'], ['email', 'email'], ['media', 'media'], ['all_shows', 'all_shows'], ['shows', 'shows'], ['valid_from_ms', 'valid_from'], ['valid_until_ms', 'valid_until'], ['max_devices', 'max_devices'], ['max_listens', 'max_listens'], ['max_people', 'max_people']]) {
          if (!(f in body)) continue;
          fields.push(name);
          k[f] = f === 'shows' ? (body.shows ?? []).map((s) => this.showBy(s).id) : typeof body[f] === 'string' ? body[f].trim() : body[f];
        }
        k.updated_ms = now;
        this.log(k, 'changed', { fields });
        return [200, this.keyView(k)];
      }
      if (verb === 'extend' && method === 'POST') {
        if (k.state === 'revoked') return [409, { error: 'a revoked key cannot be extended: replace it instead' }];
        if (k.valid_until_ms === null) return [422, { error: 'this key has no end: there is nothing to extend', field: 'by_days' }];
        if ((body.by_days == null) === (body.until_ms == null)) return [422, { error: 'send `by_days` or `until_ms`, not both', field: 'by_days' }];
        const from = k.valid_until_ms;
        const to = body.until_ms ?? Math.max(from, now) + body.by_days * DAY;
        if (to <= now) return [422, { error: 'the new end is in the past', field: 'until_ms' }];
        k.valid_until_ms = to;
        k.updated_ms = now;
        this.log(k, 'extended', { from_ms: from, to_ms: to });
        return [200, this.keyView(k)];
      }
      if ((verb === 'pause' || verb === 'resume' || verb === 'revoke') && method === 'POST') {
        const reason = String(body?.reason ?? '').trim();
        if ([...reason].length > 200) return [422, { error: 'a reason is at most 200 characters', field: 'reason' }];
        if (k.state === 'revoked' && verb !== 'revoke') return [409, { error: `a revoked key cannot be ${verb === 'pause' ? 'paused' : 'resumed'}` }];
        const to = { pause: 'paused', resume: 'active', revoke: 'revoked' }[verb];
        if (k.state !== to) {
          k.state = to;
          k.state_reason = verb === 'resume' ? '' : reason;
          k.updated_ms = now;
          this.log(k, { pause: 'paused', resume: 'resumed', revoke: 'revoked' }[verb], reason ? { reason } : {});
        }
        return [200, this.keyView(k)];
      }
      if (verb === 'replace' && method === 'POST') {
        if (k.state === 'revoked') return [409, { error: 'a revoked key cannot be replaced' }];
        k.state = 'revoked';
        k.state_reason = 'replaced';
        k.updated_ms = now;
        const fresh = this.put({ label: k.label, kind: k.kind, email: k.email, note: k.note, media: k.media, all_shows: k.all_shows, shows: k.shows, valid_from_ms: k.valid_from_ms, valid_until_ms: k.valid_until_ms, max_devices: k.max_devices, max_listens: k.max_listens, max_people: k.max_people, family: k.family_id, created_by: by, replaces: k });
        fresh.listens_used = k.listens_used;
        this.log(k, 'replaced', { by_key: fresh.id });
        return [200, { old: this.keyView(k), new: this.keyView(fresh), text: written(fresh.text), link: this.link(fresh) }];
      }
      if (verb === 'text' && method === 'POST') {
        if (!k.sealed) return [409, { error: "the station's token key changed since this key was made: its text can no longer be read; replace the key" }];
        this.log(k, 'text_shown', {});
        return [200, { text: written(k.text), link: this.link(k), email: k.email }];
      }
      if (verb === 'log' && method === 'GET') return [200, { entries: this.logs.get(k.id).slice(0, 200) }];
      if (verb === 'devices' && !sub && method === 'GET') {
        const live = (d) => (d.ended_ms === null && now - d.last_seen_ms <= 360_000 ? 0 : 1);
        return [200, { devices: [...this.devs.get(k.id)].sort((a, b) => live(a) - live(b) || b.last_seen_ms - a.last_seen_ms).slice(0, 200) }];
      }
      if (verb === 'devices' && sub && method === 'DELETE') {
        const d = this.devs.get(k.id).find((x) => x.id === decodeURIComponent(sub));
        if (!d) return [404, { error: 'no such device on that key' }];
        d.ended_ms = now;
        this.log(k, 'device_ended', { device: d.id });
        return [204, null];
      }
    }
    return [404, { error: 'not found' }];
  }

  /** A listener's key: no bearer, the key is the credential. */
  listen(method, p, body) {
    if (method !== 'POST') return [405, { error: 'method not allowed' }];
    if (this.keysUp !== true) return [503, { error: 'Keys are kept in the database.' }];
    const now = this.now * 1000;
    if (p === '/api/v1/keyed/lost') {
      // The same answer whether an address has a key or not.
      this.lostAsked.push(body.email);
      for (const k of this.lkeys.filter((x) => x.email && x.email.toLowerCase() === String(body.email).toLowerCase())) this.log(k, 'lost_key_asked', { sent: !!this.mail }, now, 'the holder');
      // 1.3.2: the station's words for whether it sends mail, and the fact itself; about the
      // station, never the address.
      return [202, this.mail ? { message: 'If a key was made for that address, the station is sending it now. Check your inbox and your spam folder.', mail: true } : { message: "This station does not send email itself. If a key was made for that address, the station's staff can see your request and will send it to you. If you do not hear from them, ask them.", mail: false }];
    }
    if (p === '/api/v1/keyed/redeem') {
      const k = this.lkeys.find((x) => x.text === norm(body.key));
      const asked = body.show ? this.showBy(body.show) : null;
      if (body.show && !asked) return [404, { error: 'There is no such show.' }];
      const no = this.refusalOf(k, asked, body.device);
      if (no) return this.refuse(no);
      const show = this.tokenShow(k, asked);
      if (!show) return this.refuse(['not_scheduled', {}]);
      const devs = this.devs.get(k.id);
      let d = devs.find((x) => x.id === body.device && x.ended_ms === null);
      if (!d) {
        d = this.device(k, body.device_label ?? 'Chromium on Linux', { show: show?.id ?? null });
        k.listens_used += 1;
        k.first_used_ms ??= now;
      }
      d.last_seen_ms = now;
      k.last_used_ms = now;
      return [200, this.token(k, d, show)];
    }
    if (p === '/api/v1/keyed/renew') {
      const t = this.tokens.get(body.token);
      if (!t) return [401, { error: 'That token is not valid.', code: 'token_invalid' }];
      if (t.expires_ms <= now) return [401, { error: 'That token has ended. Enter the key again.', code: 'token_ended' }];
      const k = this.lkeys.find((x) => x.id === t.key);
      const d = k && this.devs.get(k.id).find((x) => x.id === t.device);
      if (k && d && d.ended_ms !== null) return this.refuse(['device_signed_out']);
      const show = this.showBy(t.show);
      const no = this.refusalOf(k, show, t.device);
      if (no) return this.refuse(no);
      d.last_seen_ms = now;
      return [200, this.token(k, d, show)];
    }
    if (p === '/api/v1/keyed/devices' || p === '/api/v1/keyed/devices/end') {
      const k = this.lkeys.find((x) => x.text === norm(body.key));
      if (!k) return this.refuse(['not_recognised']);
      if (p.endsWith('/end')) {
        const d = this.devs.get(k.id).find((x) => x.id === body.device);
        if (!d) return [404, { error: 'no such device on that key' }];
        d.ended_ms = now;
        this.log(k, 'device_ended', { device: d.id }, now, 'a listener');
        return [204, null];
      }
      const live = (d) => (d.ended_ms === null && now - d.last_seen_ms <= this.holdS * 1000 ? 0 : 1);
      return [200, { devices: [...this.devs.get(k.id)].sort((a, b) => live(a) - live(b) || b.last_seen_ms - a.last_seen_ms), device_hold_s: this.holdS }];
    }
    return [404, { error: 'not found' }];
  }
}
