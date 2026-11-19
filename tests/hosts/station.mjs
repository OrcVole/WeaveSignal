// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// A stand-in control plane for the hosts driver (shoot.mjs): Kettle FM on Friday 20 November
// 2026, the design's own day (hosts-in-their-own-studios.md section 1), answering the routes of
// ADR-0280 as crates/control/src/sources.rs answers them: the same field names (checked against
// docs/openapi/weavesignal-v1.json by apps/studio/src/hosts.test.ts), the same rules for who
// may, and the same sentences when it refuses. It keeps a desk of its own, so the studio's view
// carries people and sources and the desk's commands move them. Test tooling, not a second
// implementation: it does not check a key against anything, because nothing connects to it; a
// driver says what is connected.
//
// The post (`post` in the studio's view, crates/model/src/post.rs): the stand-in works out the
// view the station would send from its own desk and schedule (whose post it is, how its source
// stands, the grace's and the late count's ends, who holds it, a lost link), and answers Hold, Go
// on air now, Not yet and End show as the routes do. It never acts by itself: the station's rule
// puts people on air at the count's end, and here the driver does (`cover`, `comeBack`, `letGo`).
//
// The Long Lunch (Duncan Boyd, in the room, 12:00 to 14:00) is followed by Steeped (Finlay
// Grant, from his own studio over SRT, 14:00 to 16:00) and Kettle Drive (Heather Douglas, from
// her own studio over Icecast, 16:00 to 18:00). Shona Campbell is the producer, Morag Campbell
// the admin. Sample people are the designs' (ADR-0071).

import { Station, instant } from '../newshow/station.mjs';
import { readFileSync } from 'node:fs';

// The stand-in station runs the same version as the studio app under test.
const APP_VERSION = JSON.parse(readFileSync(new URL('../../apps/studio/src-tauri/tauri.conf.json', import.meta.url), 'utf8')).version;

export { instant };

const ALPHABET = 'ABCDEFGHJKMNPQRSTVWXYZ23456789';
/** A fixed run of characters, so every picture of a secret is the same picture. */
function fixed(seed, length) {
  let x = seed * 2654435761;
  let out = '';
  for (let i = 0; i < length; i++) {
    x = (Math.imul(x ^ (x >>> 15), 2246822519) + 0x9e3779b9) >>> 0;
    out += ALPHABET[x % ALPHABET.length];
  }
  return out;
}

/** The rules a new station has (crates/model/src/sources.rs, `SourceSettings::default`). */
export const DEFAULT_RULES = { door_before_min: 30, grace_s: 120, late_count_s: 20, test_outside: true, test_max_min: 15, steady_s: 5, silence_s: 4, place_kept_s: 300, fade_ms: 2000, port_icecast: 8005, port_srt: 9710 };

const STAFF = {
  admin: { id: 'a-morag', username: 'morag', name: 'Morag Campbell', role: 'admin' },
  producer: { id: 'a-shona', username: 'shona', name: 'Shona Campbell', role: 'producer' },
  presenter: { id: 'a-finlay', username: 'finlay', name: 'Finlay Grant', role: 'presenter' },
  screener: { id: 'a-isla', username: 'isla', name: 'Isla Robertson', role: 'screener' },
};

export class HostStation extends Station {
  /** `role`: who the window is (`admin`, `producer`, `presenter`: Finlay, of Steeped, or
   *  `screener`). `routes: false` is a station older than 1.1. `host: null` is a station whose
   *  settings give no address for its stream; `publicUrl: null` one with no public address to
   *  make a monitor link from. `preparing` is a window that is preparing a show (ADR-0276). */
  constructor({ today = '2026-11-20', nowMinutes = 13 * 60 + 52, role = 'producer', routes = true, host = 'kettle.example', step = 'guests', preparing = false, ...o } = {}) {
    super({ today, nowMinutes, version: routes ? APP_VERSION : '1.0.5', role, ...o });
    this.shows = [];
    this.episodes = [];
    const daily = (start, minutes) => ({ starts_on: '2026-09-07', start, minutes, repeat: { kind: 'weekly', every: 1, weekdays: [1, 2, 3, 4, 5] }, end: { kind: 'never' }, skip: [], moves: [] });
    this.add({ slug: 'the-long-lunch', name: 'The Long Lunch', colour_slot: 6, presenter: 'Duncan Boyd', hosts: ['Duncan Boyd'], rule: daily(720, 120) });
    this.add({ slug: 'steeped', name: 'Steeped', colour_slot: 3, presenter: 'Finlay Grant', hosts: ['Finlay Grant'], presenter_account: 'a-finlay', rule: daily(840, 120) });
    this.add({ slug: 'kettle-drive', name: 'Kettle Drive', colour_slot: 2, presenter: 'Heather Douglas', hosts: ['Heather Douglas'], rule: daily(960, 120) });
    this.add({ slug: 'saturday-night', name: 'Saturday Night Kettle', colour_slot: 9, presenter: 'Catriona MacKenzie-Buchanan', hosts: ['Catriona MacKenzie-Buchanan'], rule: { starts_on: '2026-09-05', start: 1200, minutes: 180, repeat: { kind: 'weekly', every: 1, weekdays: [6] }, end: { kind: 'never' }, skip: [], moves: [] } });
    // Today's episodes, where today is a weekday (the packaged app's run is on the real day), and
    // the Saturday after.
    const weekday = ((new Date(`${today}T12:00:00Z`).getUTCDay() + 6) % 7) + 1;
    if (weekday <= 5) for (const s of ['the-long-lunch', 'steeped', 'kettle-drive']) this.draft(s, today);
    this.draft('saturday-night', new Date(Date.parse(`${today}T12:00:00Z`) + ((6 - weekday + 7) % 7) * 86_400_000).toISOString().slice(0, 10));

    this.routes = routes;
    this.host = host;
    this.preparing = preparing;
    /** The database and the token key are there; false answers 503 as the control plane does. */
    this.database = true;
    /** The ingest is running and reporting. */
    this.listening = true;
    this.rules = { ...DEFAULT_RULES };
    this.step = step;
    this.mode = 'live';
    /** Every key, newest first, with the fields the station never sends (`show`, the secrets). */
    this.keys = [];
    /** Where each show comes from, as the keys leave it: `push` once one is made. */
    this.from = new Map();
    this.made = 0;
    /** What is connected, by key id: `{ way, format, since, test, level_db, leveller_db, title }`. */
    this.conns = new Map();
    /** A key's last refusal, by key id. */
    this.refusals = new Map();
    /** The desk: the people in the room and the sources, in the order they came. */
    this.people = [];
    this.slots = [null, null, null, null, null];
    /** Who holds the handover at the post (the log's label: a username), or null. */
    this.heldBy = null;
    /** The keys the post has been dealt with for this airing (on air, off air, Not yet). */
    this.acted = new Set();
    /** A lost link: `{ key_id, name, show, lost_ms, covered }`, or null. */
    this.lost = null;
    /** A station without the playout layer has no automation, and no mode. */
    this.playout = true;
    /** The view follows Off air one view late, as the real station's `post` does. */
    this.lag = false;
    this.letGoAfterNextView = false;
    /** What the view says of the audience and of the listener delay (issue 17): the stand-in's
     *  own, which a driver replaces to see each state the desk draws. The delay is a default
     *  1.1.1 station's: the mount's 12 s and a 0.5 s player allowance named apart (ADR-0289). */
    this.audience = { listeners: 138, by_mount: [{ mount: '/programme.opus', listeners: 138 }], error: null, edges: false, scope: 'origin' };
    this.listenerDelay = { ms: 21_000, measured: false, basis: 'estimate', desk_ms: 8000, process_ms: 500, rest_ms: 12_500, read_ms: 8500, set_ms: 12_500, mount_hold_ms: 12_000, player_ms: 500 };
  }

  // ---- the driver's hands ----

  /** Someone joins the room and takes a strip, as the studio seats them. */
  join(identity, name, role, { on_air = role === 'presenter', lufs = -22.4 } = {}) {
    const order = { presenter: [1, 2, 3, 5, 4], guest: [2, 3, 1, 4, 5], caller: [4, 3, 2, 5, 1], dj: [5, 1, 2, 3, 4] }[role];
    const slot = order.find((n) => !this.slots[n - 1]);
    this.slots[slot - 1] = identity;
    this.people.push({ identity, name, role, on_air, mute: false, cue: false, talk: false, talk_to: null, present: true, away_ms: null, linked: true, lufs, lufs_peak: lufs + 3.1 });
    return slot;
  }

  /** A key as the station made it, without the round trip (a station that already has hosts). */
  give(show, label, hint, o = {}) {
    return this.makeKey(show, { label, hint, staff_id: o.staff_id ?? null }, o.by ?? 'Shona Campbell', o).key;
  }

  /** A host's software connects with a key. In its slot's door it stands by on the desk (strip
   *  5, then 1, 2, 3); outside it, it is a private test and never on the desk. */
  connect(key_id, way, format, { ago_s = 291, leveller_db = -7.4, level_db = -15.2, lufs = -23.6, test = false, title = null, monitor = false } = {}) {
    const key = this.keys.find((k) => k.key_id === key_id);
    this.conns.set(key_id, { way, format, since: this.now * 1000 - ago_s * 1000, test, level_db, leveller_db, title, monitor });
    key.last_used_ms = this.now * 1000 - ago_s * 1000;
    if (test) return null;
    const identity = `source-${key_id.toLowerCase()}`;
    const slot = [5, 1, 2, 3].find((n) => !this.slots[n - 1]);
    this.slots[slot - 1] = identity;
    this.people.push({ identity, name: key.label, role: 'source', on_air: false, mute: false, cue: false, talk: false, talk_to: null, present: true, away_ms: null, linked: true, lufs, lufs_peak: lufs + 2.2 });
    return slot;
  }

  /** The host opens their monitor link, or closes it: the desk makes their monitor mix, or stops. */
  listen(key_id, on = true) {
    const conn = this.conns.get(key_id);
    if (conn) conn.monitor = on;
  }

  /** The connection closes, and its place on the desk goes with it. */
  drop(key_id) {
    this.conns.delete(key_id);
    const identity = `source-${key_id.toLowerCase()}`;
    this.people = this.people.filter((p) => p.identity !== identity);
    this.slots = this.slots.map((s) => (s === identity ? null : s));
    const key = this.keys.find((k) => k.key_id === key_id);
    if (key?.ends_on_disconnect && key.revoked_ms === null) this.endKey(key);
  }

  /** A source on air loses its link: it leaves the desk, and its place is kept. */
  lose(key_id, { ago_s = 0 } = {}) {
    const key = this.keys.find((k) => k.key_id === key_id);
    this.drop(key_id);
    this.lost = { key_id, name: key.label, show: key.show, lost_ms: this.now * 1000 - ago_s * 1000, covered: false };
  }

  /** The silence is over with nobody else on air: automation covers (the rule's `Cover`). */
  cover() {
    // Without the playout layer there is no automation and the rule never covers: the
    // listener process's own fallback does, and the station's view goes on saying `covered: false`.
    if (!this.playout) return;
    this.mode = 'auto';
    this.lost.covered = true;
  }

  /** The lost source's link has held steady again: back on air, as the rule's `Back`. */
  comeBack() {
    const identity = `source-${this.lost.key_id.toLowerCase()}`;
    this.people.find((p) => p.identity === identity).on_air = true;
    if (this.playout) this.mode = 'live';
    this.lost = null;
  }

  /** The place's time is up: it is let go. */
  letGo() {
    this.lost = null;
  }

  /** The station refused a connection with this key: its row says so. */
  refuse(key_id, reason, { ago_s = 660, way = 'icecast' } = {}) {
    this.refusals.set(key_id, { at_ms: this.now * 1000 - ago_s * 1000, key_id, event: 'refused', reason, detail: '', way });
  }

  // ---- what the control plane works out ----

  me() {
    const a = STAFF[this.role] ?? STAFF.producer;
    return { kind: 'staff', role: a.role, account: { ...a, disabled_ms: null, created_ms: 1 }, preparing: this.preparing };
  }

  /** An admin, a producer or the show's presenter (`may_manage` in sources.rs). */
  mayManage(show) {
    const me = this.me();
    return me.role === 'admin' || me.role === 'producer' || show.presenter_account === me.account.id;
  }

  /** The slot's door (crates/model/src/sources.rs, `door`): open from so long before today's
   *  episode until two hours after its end. */
  door(slug) {
    const ep = this.episodes.find((e) => e.slug === slug && e.occurrence === this.today);
    const s = this.shows.find((x) => x.slug === slug);
    if (!ep || !s?.rule) return null;
    const open = ep.air_at - this.rules.door_before_min * 60;
    const close = ep.air_at + s.rule.minutes * 60 + 7200;
    return this.now >= open && this.now < close ? { close } : null;
  }

  details(key, secret) {
    const shown = secret ?? '(shown once, when the key was made)';
    const { port_icecast: ice, port_srt: srt } = this.rules;
    return {
      host: this.host,
      icecast: { port: ice, mount: '/live', user: 'source', password: `${key.key_id}-${shown}` },
      srt: { port: srt, streamid: key.key_id, passphrase: shown, pbkeylen: 16, url: this.host && secret ? `srt://${this.host}:${srt}?streamid=${key.key_id}&passphrase=${secret}&pbkeylen=16` : null },
    };
  }

  /** The key as the station sends it: never the show's slug, never a secret. */
  wire(k) {
    const { show: _show, secret: _secret, monitor: _monitor, ...key } = k;
    return key;
  }

  makeKey(slug, body, by, o = {}) {
    const show = this.shows.find((s) => s.slug === slug);
    const n = ++this.made;
    const key = {
      id: `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`,
      show_id: show.id,
      show: slug,
      label: body.label.trim(),
      staff_id: body.staff_id ?? null,
      key_id: fixed(n, 8),
      hint: body.hint,
      created_by: by,
      created_ms: this.now * 1000 - (o.made_days_ago ?? 0) * 86_400_000,
      last_used_ms: o.last_used_days_ago !== undefined ? this.now * 1000 - o.last_used_days_ago * 86_400_000 + 6_480_000 : null,
      revoked_ms: null,
      ends_on_disconnect: false,
      turned_away_until_ms: null,
      turned_away_by: null,
      secret: fixed(n + 100, 24),
      monitor: fixed(n + 200, 24),
    };
    this.keys.unshift(key);
    this.from.set(slug, 'push');
    const d = this.details(key, key.secret);
    const host = d.host ?? "(the station's address)";
    // The control plane's own text (`as_text` in sources.rs), which the studio does not use.
    const as_text = `${key.label}'s details for ${show.name}\n\nIcecast (most radio software)\n  Server: ${host}\n  Port: ${d.icecast.port}\n  Mount: ${d.icecast.mount}\n  User: ${d.icecast.user}\n  Password: ${d.icecast.password}\n\nSRT (OBS and others; needed for callers or a co-host)\n  Server: ${host}\n  Port: ${d.srt.port}\n  Stream ID: ${d.srt.streamid}\n  Passphrase: ${d.srt.passphrase}\n  Key length: 16 (AES-128)\n\nMonitor link secret: ${key.monitor}\n\nPut these into your software, open the monitor link, and press connect. You can test at any time. You go on air at your show's time, not when you connect.\n`;
    return { key: this.wire(key), secret: key.secret, monitor_secret: key.monitor, details: d, as_text };
  }

  endKey(key) {
    if (key.revoked_ms !== null) return false;
    key.revoked_ms = this.now * 1000;
    this.conns.has(key.key_id) && this.drop(key.key_id);
    if (!this.keys.some((k) => k.show === key.show && k.revoked_ms === null)) this.from.set(key.show, 'room');
    return true;
  }

  keyList(show) {
    return {
      show: show.slug,
      source: this.from.get(show.slug) ?? 'room',
      keys: this.keys.filter((k) => k.show === show.slug).map((k) => ({ ...this.wire(k), last_refusal: this.refusals.get(k.key_id) ?? null, details: this.details(k, null) })),
    };
  }

  /** `GET /sources` (`sources_now` in sources.rs): the live keys that are connected or due. */
  sourcesNow() {
    const now = this.now * 1000;
    const sources = [];
    for (const k of this.keys) {
      if (k.revoked_ms !== null) continue;
      const show = this.shows.find((s) => s.slug === k.show);
      const identity = `source-${k.key_id.toLowerCase()}`;
      const conn = this.conns.get(k.key_id);
      const at = this.people.find((p) => p.identity === identity);
      const door = this.door(k.show);
      const state = at ? (at.on_air ? 'on_air' : 'standby') : conn ? (conn.test ? 'test' : 'standby') : door ? 'not_connected' : null;
      if (!state) continue;
      sources.push({
        key_id: k.key_id,
        identity,
        label: k.label,
        show: show.slug,
        show_name: show.name,
        state,
        slot: at ? this.slots.indexOf(identity) + 1 : null,
        way: conn?.way ?? null,
        format: conn?.format || null,
        level: at ? at.lufs : (conn?.level_db ?? null),
        leveller_db: at ? conn.leveller_db : null,
        steady_ms: conn ? now - conn.since : null,
        monitor_open: !!conn?.monitor,
        door_closes_ms: door ? door.close * 1000 : null,
        title: conn?.title ?? null,
        turned_away_until_ms: k.turned_away_until_ms !== null && k.turned_away_until_ms > now ? k.turned_away_until_ms : null,
      });
    }
    return { listening: this.listening, sources };
  }

  /** The airing on air by the schedule, and the next to start. */
  airings() {
    const all = this.episodes.map((e) => ({ e, s: this.shows.find((x) => x.slug === e.slug) })).filter((x) => x.s?.rule);
    const cur = all.filter((x) => x.e.air_at <= this.now && this.now < x.e.air_at + x.s.rule.minutes * 60).sort((a, b) => b.e.air_at - a.e.air_at)[0] ?? null;
    const next = all.filter((x) => x.e.air_at > this.now).sort((a, b) => a.e.air_at - b.e.air_at)[0] ?? null;
    return { cur, next };
  }

  /** A key's source at the desk, and how far it has got. */
  subject(k) {
    const identity = `source-${k.key_id.toLowerCase()}`;
    const at = this.people.find((p) => p.identity === identity) ?? null;
    const conn = this.conns.get(k.key_id);
    const steady = !!at && !!conn && this.now * 1000 - conn.since >= this.rules.steady_s * 1000;
    const state = at?.on_air ? 'on_air' : steady ? 'standing_by' : at ? 'connecting' : this.lost?.key_id === k.key_id ? 'link_lost' : 'not_connected';
    return { k, identity, at, conn, state, rank: ['on_air', 'standing_by', 'connecting', 'link_lost', 'not_connected'].indexOf(state) };
  }

  /** `post` as the station's rule would say it (`plan` in crates/model/src/post.rs), from this
   *  desk: the show whose post it is, its source, the clocks, the hold, a lost link. */
  postView() {
    const now = this.now * 1000;
    const { cur, next } = this.airings();
    const live = (slug) => this.keys.filter((k) => k.show === slug && k.revoked_ms === null).map((k) => this.subject(k));
    // The show on air by the schedule when it has a host to bring on and none of them is on air;
    // otherwise the next to start.
    const curSubjects = cur ? live(cur.s.slug) : [];
    const post = cur && curSubjects.length && !curSubjects.some((x) => x.state === 'on_air') ? cur : next;
    const subjects = post ? live(post.s.slug) : [];
    const post_ms = post ? post.e.air_at * 1000 : 0;
    const started = !!post && this.now >= post.e.air_at;
    const holders = this.people.filter((p) => p.on_air && ['presenter', 'dj', 'source'].includes(p.role));
    const autoHolds = this.playout ? this.mode === 'auto' || holders.length === 0 : holders.length === 0;
    const ready = subjects.filter((x) => x.state === 'standing_by' && !this.acted.has(x.k.key_id)).sort((a, b) => a.conn.since - b.conn.since)[0] ?? null;
    const shown = ready ?? [...subjects].sort((a, b) => a.rank - b.rank)[0] ?? null;
    const view = {
      show: post ? { slug: post.s.slug, name: post.s.name, start_ms: post_ms, started } : null,
      source: shown?.k.key_id ?? null,
      source_name: shown?.k.label ?? null,
      state: shown?.state ?? null,
      standing_since_ms: null,
      on_air_at_ms: null,
      grace_ends_ms: null,
      held_by: started && this.heldBy ? this.heldBy : null,
      late_ends_ms: null,
      holders: holders.map((p) => p.name),
      link_lost: null,
    };
    if (ready) {
      const since = ready.conn.since + this.rules.steady_s * 1000;
      view.standing_since_ms = since;
      if (autoHolds) {
        // Automation holds the air: at the post if it stood by before it, else the late count.
        if (since <= post_ms) view.on_air_at_ms = post_ms;
        else if (this.rules.late_count_s > 0) view.late_ends_ms = view.on_air_at_ms = since + this.rules.late_count_s * 1000;
      } else if (this.rules.grace_s > 0 && started && !view.held_by) {
        view.grace_ends_ms = Math.max(since, post_ms) + this.rules.grace_s * 1000;
      }
    }
    if (this.lost) {
      const { key_id, name, show, lost_ms, covered } = this.lost;
      view.link_lost = { key_id, name, show, lost_ms, covers_ms: lost_ms + this.rules.silence_s * 1000, place_kept_until_ms: lost_ms + this.rules.place_kept_s * 1000, covered };
    }
    return view.show || view.link_lost ? view : null;
  }

  studioView() {
    const view = this.viewNow();
    if (this.letGoAfterNextView) {
      this.letGoAfterNextView = false;
      this.lost = null;
    }
    return view;
  }

  viewNow() {
    const v = super.studioView();
    const now = this.now * 1000;
    const onAir = this.shows.find((s) => this.episodes.some((e) => e.slug === s.slug && e.occurrence === this.today && e.air_at <= this.now && this.now < e.air_at + s.rule.minutes * 60));
    const next = this.shows.map((s) => ({ s, e: this.episodes.find((e) => e.slug === s.slug && e.air_at > this.now) })).filter((x) => x.e).sort((a, b) => a.e.air_at - b.e.air_at)[0];
    const presenter = this.people.find((p) => p.role === 'presenter');
    return {
      ...v,
      // The rundown is the `studio.rundown` setting, which this station leaves empty: the next
      // show's row and the clock's post line are the schedule's (`post`), from 1.1.
      show: { title: onAir?.name ?? '', rundown: [] },
      desk: { reachable: true, error: null, playout: { connected: true, for_ms: 6_720_000 }, delay: { delay_ms: 8000, target_ms: 8000, rebuilding: false, dumps: 0, dumped_at_ms: null }, picture: null },
      seats: { presenter: presenter?.identity ?? null, screener: null, presenter_name: presenter?.name ?? '', screener_name: '' },
      sources: this.people.map((p) => {
        const conn = p.role === 'source' ? this.conns.get(this.keys.find((k) => `source-${k.key_id.toLowerCase()}` === p.identity)?.key_id) : null;
        const key = conn && this.keys.find((k) => `source-${k.key_id.toLowerCase()}` === p.identity);
        return {
          slot: this.slots.indexOf(p.identity) + 1,
          ...p,
          ...(conn ? { source: { way: conn.way, format: conn.format, key: key.key_id, show: key.show, connected_ms: now - conn.since, leveller_db: conn.leveller_db, level_db: conn.level_db, monitor: !!conn.monitor } } : {}),
          caller: null,
        };
      }),
      listener_delay: this.listenerDelay,
      audience: this.audience,
      mode: this.playout ? this.mode : null,
      ...(this.routes ? { post: this.postView() } : {}),
      step: { step: this.step, show: onAir ? { slug: onAir.slug, name: onAir.name } : null, on_air: !!onAir, next: next ? { slug: next.s.slug, name: next.s.name, start_ms: next.e.air_at * 1000, step: 'solo' } : null, may_change: true },
    };
  }

  // ---- the routes ----

  handle(method, path, body) {
    const p = new URL(path, 'http://station').pathname;
    if (p === '/api/v1/sessions/me') {
      this.calls.push({ method, path, body });
      return [200, this.me()];
    }
    if (p === '/api/v1/staff' && method === 'GET') {
      this.calls.push({ method, path, body });
      if (this.me().role !== 'admin') return [403, { error: 'Only an admin manages staff.' }];
      return [200, { accounts: Object.values(STAFF).map((a) => ({ ...a, disabled_ms: null, created_ms: 1, has_email: false })) }];
    }
    if (p === '/api/v1/studio' && method === 'GET') {
      this.calls.push({ method, path, body });
      return [200, this.studioView()];
    }
    if (p === '/api/v1/desk/commands' && method === 'POST') {
      this.calls.push({ method, path, body });
      const who = this.people.find((x) => x.identity === body.identity);
      if (!who) return [404, { error: `nobody at the desk is \`${body.identity}\`` }];
      if (body.op === 'on_air' || body.op === 'cue' || body.op === 'mute') who[body.op] = body.on;
      // Talk to one person, held: for a host's own studio it goes into their monitor mix.
      if (body.op === 'talk_to') who.talk_to = body.on ? body.to : who.talk_to === body.to ? null : who.talk_to;
      return [200, this.studioView()];
    }
    if (p === '/api/v1/invites' && method === 'POST') {
      // An invitation for yourself (the Solo DJ's own microphone link): the answer a station gives.
      this.calls.push({ method, path, body });
      const a = this.me().account;
      const identity = `presenter-me-${a.id.replaceAll('-', '')}`;
      return [200, { pass: 'stand-in-pass', identity, role: 'presenter', name: a.name, expires_at_ms: this.now * 1000 + 4 * 3600_000, link: '/join/#stand-in-pass' }];
    }
    if (p === '/api/v1/passes/end' && method === 'POST') {
      // End pass (ADR-0119), and so Leave on your own tile: off air, out of the room.
      this.calls.push({ method, path, body });
      this.people = this.people.filter((x) => x.identity !== body.identity);
      this.slots = this.slots.map((x) => (x === body.identity ? null : x));
      return [200, this.studioView()];
    }
    if (p === '/api/v1/playout/mode' && method === 'POST') {
      this.calls.push({ method, path, body });
      this.mode = body.mode;
      return [200, this.studioView()];
    }
    if (p === '/api/v1/show/end' && method === 'POST') {
      this.calls.push({ method, path, body });
      // The next host standing by takes the air as the others leave it (`end_show` in app.rs);
      // else everyone is off air and the air goes to automation once the delay has played out.
      const post = this.routes ? this.postView() : null;
      const on = this.people.filter((x) => x.on_air);
      const next = post?.standing_since_ms != null ? this.people.find((x) => x.identity === `source-${post.source.toLowerCase()}`) : null;
      for (const x of on) x.on_air = false;
      if (next) {
        next.on_air = true;
        this.acted.add(post.source);
        if (this.playout) this.mode = 'live';
        return [200, { view: this.studioView(), off_air: on.length, automation_in_ms: null, passed_to: next.name }];
      }
      if (this.playout) this.mode = 'auto';
      return [200, { view: this.studioView(), off_air: on.length, automation_in_ms: this.playout ? 9000 : null }];
    }
    if (p === '/api/v1/air/post/hold' && this.routes) {
      this.calls.push({ method, path, body });
      const me = this.me();
      if (me.role !== 'admin' && me.role !== 'producer') return [403, { error: 'Only an admin or a producer holds a handover.' }];
      if (method === 'POST') {
        const post = this.postView();
        if (!post?.show) return [409, { error: 'there is no handover to hold' }];
        this.heldBy = me.account.username;
        return [200, { held: true, by: this.heldBy, airing_start_ms: post.show.start_ms }];
      }
      this.heldBy = null;
      return [200, { held: false }];
    }
    // The team's names (1.1): a producer's and an admin's (ADR-0289), the accounts that are on.
    if (p === '/api/v1/staff/directory' && method === 'GET') {
      this.calls.push({ method, path, body });
      if (!this.routes) return [404, { error: 'not found' }];
      if (this.me().role !== 'admin' && this.me().role !== 'producer') return [403, { error: 'Only a producer or an admin can see the team. Ask the station’s admin.' }];
      return [200, { accounts: Object.values(STAFF).map(({ id, name, role }) => ({ id, name, role })).sort((a, b) => a.name.localeCompare(b.name)) }];
    }
    // Who may name a staff account as a show's presenter (crates/control/src/shows.rs, ADR-0218):
    // an admin anyone, anyone else only themselves; naming it again unchanged is refused too,
    // which is why a save must leave the field out when it did not change.
    if (/^\/api\/v1\/shows\/[^/]+$/.test(p) && method === 'PUT' && typeof body?.presenter_account === 'string') {
      const me = this.me();
      if (me.role !== 'admin' && body.presenter_account !== me.account.id) {
        this.calls.push({ method, path, body });
        return [403, { error: 'only an admin names another account as a show\u2019s presenter' }];
      }
    }
    // ADR-0241: the presenter's name and the takedown contact are an admin's or the show's own
    // presenter's to change; a save that sends them unchanged passes.
    if (/^\/api\/v1\/shows\/[^/]+$/.test(p) && method === 'PUT') {
      const show = this.shows.find((x) => p.endsWith(`/${x.slug}`));
      const me = this.me();
      const changes = (k) => typeof body?.[k] === 'string' && body[k].trim() !== (show?.[k] ?? '').trim();
      const own = show?.presenter_account === me.account.id || body.presenter_account === me.account.id;
      if (show && me.role !== 'admin' && (changes('presenter') || changes('contact')) && !own) {
        this.calls.push({ method, path, body });
        return [403, { error: 'only an admin or the show\u2019s own presenter changes its presenter and takedown contact' }];
      }
    }
    const mine = /^\/api\/v1\/(sources|settings\/sources|shows\/[^/]+\/keys)(\/|$)/.test(p);
    if (!mine) return super.handle(method, path, body);
    this.calls.push({ method, path, body });
    // A station older than 1.1 has none of these routes.
    if (!this.routes) return [404, { error: 'not found' }];
    const me = this.me();
    const by = me.account.name;
    if (!this.database) return [503, { error: 'show keys are kept in the database: set WEAVESIGNAL_DATABASE_URL' }];
    let m;
    if (p === '/api/v1/sources' && method === 'GET') return [200, this.sourcesNow()];
    if (p === '/api/v1/settings/sources') {
      if (me.role !== 'admin') return [403, { error: 'Only an admin does this.' }];
      if (method === 'GET') return [200, { ...this.rules }];
      if (![0, 60, 120, 300].includes(body.grace_s)) return [422, { error: '0 (off), 60, 120 or 300', field: 'grace_s' }];
      if (!(body.door_before_min >= 0 && body.door_before_min <= 240)) return [422, { error: 'minutes, 0 to 240', field: 'door_before_min' }];
      if (body.port_icecast === body.port_srt) return [422, { error: 'use a different number from the Icecast port', field: 'port_srt' }];
      // A port the stand-in will not have, to show the station's own sentence under the field.
      if (body.door_before_min === 59) return [422, { error: 'the station refuses this (a test)', field: 'door_before_min' }];
      this.rules = { ...body };
      return [200, { ...this.rules }];
    }
    if ((m = /^\/api\/v1\/sources\/([^/]+)\/(go|not-yet)$/.exec(p)) && method === 'POST') {
      const key = this.keys.find((k) => k.key_id === m[1] && k.revoked_ms === null);
      if (!key) return [404, { error: `no key \`${m[1]}\`` }];
      if (me.preparing) return [409, { error: 'This window is preparing a show, so it cannot change what is on air. Switch it to Live first.' }];
      const x = this.subject(key);
      if (!x.at) return [409, { error: 'that source is not at the desk: it is not connected, or is only a test' }];
      if (m[2] === 'not-yet') {
        this.acted.add(key.key_id);
        return [200, { waiting: true }];
      }
      if (x.at.on_air) return [200, { on_air: true, already: true }];
      if (x.state !== 'standing_by') return [409, { error: 'that source is not steady yet: it stands by after a few seconds of steady sound' }];
      if (this.people.some((q) => q.on_air && q !== x.at && ['presenter', 'dj', 'source'].includes(q.role))) return [409, { error: 'someone else is presenting: the air passes at the post, after the grace, or by a press' }];
      x.at.on_air = true;
      this.acted.add(key.key_id);
      if (this.playout) this.mode = 'live';
      return [200, { on_air: true }];
    }
    if ((m = /^\/api\/v1\/sources\/([^/]+)\/off-air$/.exec(p)) && method === 'POST') {
      // Off air on a kept place (`let_go_now` in crates/control/src/post.rs): refused unless the
      // key's place is kept now; with nobody presenting and the station not on automation yet,
      // automation takes the air first, and the reply says so.
      const key = this.keys.find((k) => k.key_id === m[1] && k.revoked_ms === null);
      if (!key) return [404, { error: `no key \`${m[1]}\`` }];
      if (me.preparing) return [409, { error: 'This window is preparing a show, so it cannot change what is on air. Switch it to Live first.' }];
      if (this.lost?.key_id !== key.key_id) return [409, { error: 'no place is being kept for that source: its link has not been lost, or its place is let go already' }];
      const someone = this.people.some((q) => q.on_air && ['presenter', 'dj', 'source'].includes(q.role));
      let covered = false;
      if (!someone && this.playout && this.mode !== 'auto') {
        this.mode = 'auto';
        covered = true;
      }
      // The real station's `post` follows a decision by up to a second (`lag`): its next view
      // still carries the place, and the one after does not.
      if (this.lag) this.letGoAfterNextView = true;
      else this.lost = null;
      // It does not go back on air by itself if it reconnects: this airing is acted on.
      this.acted.add(key.key_id);
      return [200, { let_go: true, covered }];
    }
    if ((m = /^\/api\/v1\/sources\/([^/]+)\/disconnect$/.exec(p))) {
      const key = this.keys.find((k) => k.key_id === m[1]);
      if (!key) return [404, { error: `no key \`${m[1]}\`` }];
      if (method === 'POST') {
        if (me.preparing) return [409, { error: 'This window is preparing a show: it changes nothing on air.' }];
        const door = this.door(key.show);
        const until = door ? door.close * 1000 : this.now * 1000 + 7_200_000;
        key.turned_away_until_ms = until;
        key.turned_away_by = by;
        this.drop(key.key_id);
        // A kept place is let go with it (the log's `disconnected` is where the place stands).
        if (this.lost?.key_id === key.key_id) this.lost = null;
        return [200, { disconnected: true, until_ms: until }];
      }
      const show = this.shows.find((s) => s.slug === key.show);
      if (!this.mayManage(show)) return [403, { error: "Only an admin, a producer or the show's presenter lets a key back in." }];
      key.turned_away_until_ms = null;
      key.turned_away_by = null;
      return [200, { let_back: true }];
    }
    if ((m = /^\/api\/v1\/shows\/([^/]+)\/keys(?:\/([^/]+))?(\/renew)?$/.exec(p))) {
      const show = this.shows.find((s) => s.slug === m[1]);
      if (!show) return [404, { error: `show \`${m[1]}\` does not exist` }];
      if (!this.mayManage(show)) return [403, { error: "Only an admin, a producer or the show's presenter makes or ends its keys. An API token never does." }];
      if (!m[2]) {
        if (method === 'GET') return [200, this.keyList(show)];
        const known = ['label', 'staff_id', 'hint'];
        const extra = Object.keys(body ?? {}).find((k) => !known.includes(k));
        if (extra) return [422, { error: `unknown field \`${extra}\`` }];
        const label = String(body.label ?? '').trim();
        if (!label || [...label].length > 120) return [422, { error: 'who the key is for: 1 to 120 characters, no control characters', field: 'label' }];
        if (!['icecast', 'srt'].includes(body.hint)) return [422, { error: '`icecast` or `srt`', field: 'hint' }];
        if (this.keys.filter((k) => k.show === show.slug && k.revoked_ms === null).length >= 8) return [409, { error: 'a show has at most 8 keys that have not ended' }];
        return [201, this.makeKey(show.slug, { label, hint: body.hint, staff_id: body.staff_id ?? null }, by)];
      }
      const key = this.keys.find((k) => k.show === show.slug && k.key_id === m[2]);
      if (!key) return [404, { error: `${show.name} has no key \`${m[2]}\`` }];
      if (m[3]) {
        if (!['now', 'on_disconnect'].includes(body?.old)) return [422, { error: '`now` or `on_disconnect`', field: 'old' }];
        if (key.revoked_ms !== null) return [409, { error: 'that key has ended already' }];
        const made = this.makeKey(show.slug, { label: key.label, hint: key.hint, staff_id: key.staff_id }, by);
        if (body.old === 'now' || !this.conns.has(key.key_id)) this.endKey(key);
        else key.ends_on_disconnect = true;
        return [201, made];
      }
      if (method === 'DELETE') return [200, { ended: this.endKey(key) }];
    }
    return [404, { error: `the stand-in station has no ${method} ${p}` }];
  }
}
