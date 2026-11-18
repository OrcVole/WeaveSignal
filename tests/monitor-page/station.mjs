// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// A stand-in station for the monitor link's page (ADR-0280): a loopback HTTP server that serves
// the built page with the Content-Security-Policy the control plane gives its pages
// (crates/control/src/web.rs), and answers the monitor routes as crates/control/src/monitor.rs
// does: `GET /api/v1/monitor`, its event stream (a `view` at once and every second, `ended`
// when the key ends), `POST /api/v1/monitor/token`, and the six controls, each refused with 409
// and the station's words when it is not the host's to do.
//
// The view is built from a handful of facts by the same rules, in the same words, as
// crates/control/src/monitor_view.rs (`controls`, `says`, `standing_then`, `outgoing_then`,
// `lost_says`); shoot.mjs checks every view it sends against `MonitorView` in
// docs/openapi/weavesignal-v1.json, so a change to the shape there fails here. Kettle FM,
// Friday 20 November 2026, as in the design; the station's clock runs on from the time a
// scenario gives it, whatever the machine's own clock says.
import { createServer } from 'node:http';
import { existsSync, readFileSync } from 'node:fs';
import { extname, join } from 'node:path';

const ENDED = 'This link no longer works. Ask the station for a new one.';
const STEP_WORD = { solo: 'Solo', guests: 'Guests', callers: 'Callers', full: 'Full' };
const STEP_RANK = { solo: 0, guests: 1, callers: 2, full: 3 };

/** A time on the design's day, in London (GMT in November): `at('14:00:00')`. */
export const at = (hms, day = '2026-11-20') => Date.parse(`${day}T${hms.length === 5 ? `${hms}:00` : hms}Z`);

const joinNames = (n) => (n.length <= 1 ? (n[0] ?? '') : n.length === 2 ? `${n[0]} and ${n[1]}` : `${n.slice(0, -1).join(', ')} and ${n.at(-1)}`);
const isAre = (n) => (n === 1 ? 'is' : 'are');

export class MonitorStation {
  constructor(o = {}) {
    this.secret = o.secret ?? 'm7Q4MX2KDr5wzk8net2hq6mv';
    this.zone = 'Europe/London';
    this.name = 'Kettle FM';
    /** The station's time when the scenario starts; it runs on from there in real time. */
    this.base = o.now ?? at('13:57:12');
    this.t0 = Date.now();
    /** The host: Finlay Grant, Steeped, 14:00 to 16:00, over SRT. */
    this.label = 'Finlay Grant';
    this.show = 'steeped';
    this.show_name = 'Steeped';
    this.start = at('14:00');
    this.end = at('16:00');
    /** Where their software is: `desk` (in its slot), `test`, `lost` or `none`. */
    this.conn = 'desk';
    this.on_air = false;
    this.way = 'srt';
    this.format = 'mpegts/aac 48 kHz stereo';
    this.level = -22.6;
    this.leveller_db = -7.4;
    this.steady_ms = 291_000;
    this.title = null;
    this.step = 'full';
    /** Who else presents and is on air, by name. */
    this.others = [];
    /** The next host: { name, show_name, start, state, at_desk, on_air, cued, passes }. */
    this.next = null;
    /** The post as far as it is this host's (see `MonitorPost`), or null. */
    this.post = null;
    this.rows = { before: null, after: null };
    this.insert = null;
    this.refusal = null;
    this.kept_until = null;
    this.place_gone = false;
    this.behind_ms = 23_000;
    this.automation = true;
    this.auto_mode = false;
    this.playout_unknown = false;
    this.steady_s = 5;
    this.grace_s = 120;
    /** The station's look (ADR-0237): an accent of its own, to try Night against. */
    this.accent = null;
    // What the driver turns: an ended key, a silent or refused stream, a refused token or press.
    this.ended = false;
    this.silent = false;
    this.down = false;
    this.tooMany = null;
    this.tokenRefusal = null;
    this.roomMode = 'ok';
    /** The room's address, when it is not this server's own (a port where nothing listens). */
    this.roomUrl = null;
    this.refuseNext = null;
    /** Presses get no answer at all: the connection closes under them. */
    this.dropPresses = false;
    /** Presses are taken and never answered: the connection stays open, as a stalled one does. */
    this.holdPresses = false;
    this.calls = [];
    /** The last answers given, for the driver to check against the description. */
    this.lastDone = null;
    this.lastToken = null;
    this.tokens = 0;
    this.streams = new Set();
    /** The listeners' stream (`/stream/programme.wav`): requests made, and how many are open now. */
    this.listenerStream = { hits: 0, open: 0 };
    /** The station has no listener stream set up (`stream_url` empty). */
    this.noStream = false;
    this.port = 0;
  }

  now() {
    return this.base + (Date.now() - this.t0);
  }

  /** Move the station's clock: the views that follow carry the new time. */
  setNow(ms) {
    this.base = ms;
    this.t0 = Date.now();
  }

  // ---- the view, by monitor_view.rs's rules ----

  lamp() {
    if (this.conn === 'desk') return this.on_air ? ((this.automation && this.auto_mode) || this.playout_unknown ? 'open' : 'on_air') : 'standby';
    return this.conn === 'lost' ? 'link_lost' : this.conn === 'test' ? 'test' : 'not_connected';
  }

  airingOnNow() {
    const n = this.now();
    return this.start <= n && n < this.end;
  }

  controls() {
    const lamp = this.lamp();
    const onAir = this.conn === 'desk' && this.on_air;
    const yes = { may: true, why_not: null };
    const no = (why) => ({ may: false, why_not: why });
    const notOnAir = 'You are not on air.';
    const n = this.next;
    const stepNo = (what, from) => `${what} is off at ${STEP_WORD[this.step]}. It comes on at ${STEP_WORD[from]}.`;
    const nextGone = !n ? 'Nobody is standing by after you.' : !n.at_desk ? `${n.name} has not connected yet.` : null;
    const counting = !!this.post?.mine && this.post.late_ends_ms != null;

    let go;
    if (onAir) go = no('You are on air.');
    else if (lamp !== 'standby') go = no(lamp === 'test' ? "This is a test: your slot's door is not open yet." : 'Your software has not reached the station.');
    else if (!this.airingOnNow()) go = no('Your show has not started. You go on air at its time.');
    else if (this.others.length) go = no(`${joinNames(this.others)} ${isAre(this.others.length)} still on air. You take the air when they hand over, or when the schedule does.`);
    else if (this.steady_ms < this.steady_s * 1000) go = no('You are not steady yet. You can go on air after a few seconds of steady sound.');
    else go = yes;
    const not_yet = counting && lamp === 'standby' ? yes : onAir ? no('You are on air.') : no('There is no count to stop.');

    const cue = !onAir ? no(notOnAir) : nextGone ? no(nextGone) : n.on_air ? no(`${n.name} is on air already.`) : yes;
    const full = STEP_RANK[this.step] >= STEP_RANK.full;
    const bring = !onAir ? no(notOnAir) : !full ? no(stepNo('The overlap', 'full')) : nextGone ? no(nextGone) : n.on_air ? no(`${n.name} is on air already.`) : yes;
    const hand_over = !onAir ? no(notOnAir) : !full ? no(stepNo('The overlap', 'full')) : nextGone ? no(nextGone) : !n.on_air ? no(`Bring ${n.name} on air first.`) : yes;
    let end_show = no(notOnAir);
    let end_show_says = null;
    if (onAir) {
      end_show = yes;
      end_show_says = this.others.length
        ? `${joinNames(this.others)} stay${this.others.length === 1 ? 's' : ''} on air.`
        : n?.passes && !n.on_air
          ? `${n.name} is standing by: the air passes to them as you leave.`
          : this.automation
            ? 'Automation takes the air.'
            : 'The fallback takes the air.';
    }
    const dump = !onAir ? no(notOnAir) : STEP_RANK[this.step] < STEP_RANK.callers ? no(stepNo('Dump', 'callers')) : yes;
    return { go, not_yet, cue, bring, hand_over, end_show, end_show_says, dump };
  }

  outgoingThen(p) {
    if (!p || p.mine) return null;
    if (p.held_by) return { kind: 'held', words: `Handover held by ${p.held_by}. It is by a press only.`, at_ms: null };
    if (p.grace_ends_ms != null) return { kind: 'grace', words: `${p.show_name} takes the air by the schedule.${p.host ? ` ${p.host} is standing by.` : ''}`, at_ms: p.grace_ends_ms };
    if (!p.host) return null;
    if (!['standing_by', 'on_air'].includes(p.host_state)) return { kind: 'keeps_air', words: `${p.host} has not connected. You keep the air.`, at_ms: null };
    return this.grace_s === 0 && this.now() >= p.start_ms ? { kind: 'press_only', words: `${p.host} is standing by. The handover is by a press only.`, at_ms: null } : null;
  }

  standingThen(p) {
    const atPost = { kind: 'at_post', words: "You go on air at your show's time, by the schedule.", at_ms: this.start };
    if (!p || !p.mine) return atPost;
    if (p.held_by) return { kind: 'held', words: `Handover held by ${p.held_by}. It is by a press only.`, at_ms: null };
    if (p.late_ends_ms != null) return { kind: 'late_count', words: 'Automation holds your air.', at_ms: p.late_ends_ms };
    const on = p.on_air ?? [];
    const who = joinNames(on);
    if (p.grace_ends_ms != null) return { kind: 'grace', words: `${who} ${isAre(Math.max(1, on.length))} still on air. The handover is by a press or by the schedule.`, at_ms: p.grace_ends_ms };
    if (on.length) {
      return this.grace_s === 0 && this.now() >= p.start_ms
        ? { kind: 'press_only', words: `${who} ${isAre(on.length)} on air. The handover is by a press only.`, at_ms: null }
        : { kind: 'waits', words: `${who} ${isAre(on.length)} still on air.`, at_ms: p.start_ms };
    }
    if (p.on_air_at_ms != null) return { kind: 'at_post', words: 'You go on air at the post, by the schedule.', at_ms: p.on_air_at_ms };
    return atPost;
  }

  says(lamp, post) {
    const behind = this.behind_ms ? Math.round(this.behind_ms / 1000) : null;
    switch (lamp) {
      case 'on_air':
        return [behind ? `On air. Listeners hear you, about ${behind} s behind.` : 'On air. Listeners hear you.', this.outgoingThen(post)];
      case 'open':
        return [`Open. The station hears you. ${this.playout_unknown ? 'Listeners hear the fallback.' : 'Automation holds the air, so listeners do not hear you.'}`, this.outgoingThen(post)];
      case 'standby':
        return ['Standing by. The station hears you. Listeners do not.', this.standingThen(post)];
      case 'link_lost': {
        const heard = this.others.length ? '' : this.automation ? 'Listeners hear automation. ' : 'Listeners hear the fallback. ';
        return ['The station has lost your stream.', { kind: 'link_lost', words: `${heard}Reconnect your software: you are back on air ${this.steady_s} s after it is steady.`, at_ms: this.kept_until }];
      }
      case 'test':
        return ['Test. The station hears you. Nobody else does.', { kind: 'test', words: "Your slot's door is not open yet.", at_ms: null }];
      default:
        if (this.place_gone) return ['Your place was kept for 5 minutes. Connect again and you will be standing by.', { kind: 'place_gone', words: 'Connect again and you will be standing by.', at_ms: null }];
        return ['Not connected. Your software has not reached the station.', { kind: 'slot', words: `${this.show_name} is next.`, at_ms: this.start }];
    }
  }

  view() {
    const lamp = this.lamp();
    const connected = this.conn === 'desk' || this.conn === 'test';
    // The post is this host's only when it is their own show's, or they hold the air at it.
    const post = this.post && (this.post.mine || (this.conn === 'desk' && this.on_air)) ? this.post : null;
    const [says, then] = this.says(lamp, post);
    const n = this.next;
    return {
      label: this.label,
      show: this.show,
      show_name: this.show_name,
      state: lamp,
      says,
      then,
      way: connected || this.conn === 'lost' ? this.way : null,
      format: connected ? this.format : null,
      level: connected ? this.level : null,
      leveller_db: this.conn === 'desk' ? this.leveller_db : null,
      steady_ms: connected ? this.steady_ms + (Date.now() - this.t0) : null,
      title: this.title,
      monitor_open: this.tokens > 0,
      door_closes_ms: this.conn === 'desk' ? this.end : null,
      last_refusal: this.refusal,
      post: post
        ? { show_name: post.show_name, start_ms: post.start_ms, started: this.now() >= post.start_ms, mine: post.mine, host: post.host ?? null, host_state: post.host_state ?? null, on_air_at_ms: post.on_air_at_ms ?? null, late_ends_ms: post.late_ends_ms ?? null, grace_ends_ms: post.grace_ends_ms ?? null, held_by: post.held_by ?? null, on_air: post.on_air ?? [] }
        : null,
      rundown: {
        before: this.rows.before,
        you: { show_name: this.show_name, host: this.label, state: lamp, start_ms: this.start, end_ms: this.end },
        after: this.rows.after,
      },
      insert: this.insert,
      step: { step: this.step, word: STEP_WORD[this.step], dump: STEP_RANK[this.step] >= STEP_RANK.callers, handover: STEP_RANK[this.step] >= STEP_RANK.full },
      next_host: n ? { name: n.name, state: n.on_air ? 'on_air' : n.state, cued: !!n.cued } : null,
      listeners_behind_ms: this.behind_ms,
      // The desk's mix has someone in it, and listeners hear the desk: nobody on air at the desk, or
      // automation holding the air, and the mix is silent (monitor_view.rs `monitor_has_programme`).
      monitor_has_programme: this.others.length > 0 && !(this.automation && this.auto_mode) && !this.playout_unknown,
      controls: this.controls(),
      station: { now_ms: this.now(), zone: this.zone },
    };
  }

  // ---- the routes ----

  /** One request to `/api/v1/monitor…`: `[status, body, headers?]`. */
  handle(method, path, bearer, body) {
    const route = path.replace('/api/v1/monitor', '') || '/';
    this.calls.push({ method, path, bearer, body });
    if (bearer !== this.secret) return [401, { error: ENDED }];
    if (this.ended) return [410, { error: ENDED }];
    if (method === 'GET' && route === '/') return [200, this.view()];
    if (method !== 'POST') return [404, { error: 'no such route' }];
    if (route === '/token') {
      if (this.tokenRefusal) return [409, { error: this.tokenRefusal }];
      this.tokens += 1;
      return [201, (this.lastToken = { token: `standin-token-${this.tokens}`, url: this.roomUrl ?? `ws://127.0.0.1:${this.port}/room?mode=${this.roomMode}`, room: 'kettle-monitor-MREQM2YS', identity: `monitor-MREQM2YS-${((this.tokens - 1) % 2) + 1}`, ttl_s: 600 })];
    }
    const what = { '/go': 'go', '/not-yet': 'not_yet', '/cue': 'cue', '/handover': body?.step === 'hand_over' ? 'hand_over' : 'bring', '/end-show': 'end_show', '/dump': 'dump' }[route];
    if (!what) return [404, { error: 'no such route' }];
    if (this.refuseNext) {
      const words = this.refuseNext;
      this.refuseNext = null;
      return [409, { error: words }];
    }
    const c = this.controls()[what];
    if (!c.may) return [409, { error: c.why_not ?? 'That is not yours to do now.' }];
    const done = { done: what, passed_to: null, automation_in_ms: null, lost_ms: null };
    const n = this.next;
    switch (what) {
      case 'go':
        this.on_air = true;
        this.auto_mode = false;
        this.post = null;
        break;
      case 'not_yet':
        this.post = { ...this.post, late_ends_ms: null, on_air_at_ms: null };
        break;
      case 'cue':
        n.cued = body?.on ?? true;
        break;
      case 'bring':
        n.on_air = true;
        n.cued = false;
        this.others = [n.name];
        if (this.rows.after) this.rows.after = { ...this.rows.after, state: 'on_air' };
        break;
      case 'hand_over':
        this.on_air = false;
        this.post = null;
        break;
      case 'end_show':
        this.on_air = false;
        if (n?.passes && !n.on_air) {
          n.on_air = true;
          this.others = [n.name];
          done.passed_to = n.name;
        } else if (!this.others.length && this.automation) {
          this.auto_mode = true;
          done.automation_in_ms = 9000;
        }
        this.post = null;
        break;
      case 'dump':
        done.lost_ms = 8000;
        break;
    }
    return [200, (this.lastDone = { ...done, view: this.view() })];
  }

  /** End the key: every open stream says `ended` and closes, as the station's does. */
  endKey() {
    this.ended = true;
    for (const res of this.streams) {
      res.write(`event: ended\ndata: ${JSON.stringify({ error: ENDED })}\n\n`);
      res.end();
    }
    this.streams.clear();
  }

  /** Cut every open stream, as a dropped connection does. */
  cut() {
    for (const res of this.streams) res.destroy();
    this.streams.clear();
  }

  /** Send a view made earlier to every open stream, as one overtaken on the way would arrive. */
  pushOld(view) {
    for (const res of this.streams) res.write(`event: view\ndata: ${JSON.stringify(view)}\n\n`);
  }

  /** Send the view to every open stream now (a press in another browser, a change at the desk). */
  push() {
    if (this.silent) return;
    const frame = `event: view\ndata: ${JSON.stringify(this.view())}\n\n`;
    for (const res of this.streams) res.write(frame);
  }
}

/** Sixty seconds of a quiet 330 Hz tone, 8 kHz mono, as a WAV file. */
const TONE = (() => {
  const rate = 8000;
  const n = rate * 60;
  const b = Buffer.alloc(44 + n * 2);
  b.write('RIFF', 0);
  b.writeUInt32LE(36 + n * 2, 4);
  b.write('WAVEfmt ', 8);
  b.writeUInt32LE(16, 16);
  b.writeUInt16LE(1, 20);
  b.writeUInt16LE(1, 22);
  b.writeUInt32LE(rate, 24);
  b.writeUInt32LE(rate * 2, 28);
  b.writeUInt16LE(2, 32);
  b.writeUInt16LE(16, 34);
  b.write('data', 36);
  b.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) b.writeInt16LE(Math.round(3000 * Math.sin((2 * Math.PI * 330 * i) / rate)), 44 + i * 2);
  return b;
})();

const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.json': 'application/json' };

/**
 * Serve a built page and a station. `server.station` is the station that answers now; the
 * driver puts a new one there for each scenario. Every response carries the pages' own
 * Content-Security-Policy and headers, so the page is driven under the rules it ships with.
 */
export async function serve(dist, port, roomPort = port) {
  const state = { station: new MonitorStation(), violations: [] };
  // `connect-src` names the room's origin, as web.rs does from `room.public_url`.
  const csp =
    `default-src 'none'; script-src 'self'; style-src 'self'; style-src-attr 'unsafe-inline'; ` +
    `img-src 'self' data: blob:; font-src 'self'; connect-src 'self' ws://127.0.0.1:${roomPort} http://127.0.0.1:${roomPort}; media-src 'self' blob:; worker-src 'self'; ` +
    `manifest-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'; object-src 'none'`;
  const pageHeaders = { 'content-security-policy': csp, 'x-content-type-options': 'nosniff', 'referrer-policy': 'no-referrer', 'x-frame-options': 'DENY', 'permissions-policy': 'microphone=(self), camera=(self), geolocation=(), payment=(), usb=(), interest-cohort=()' };
  const json = (res, status, body, headers = {}) => res.writeHead(status, { 'content-type': 'application/json', 'cache-control': 'no-store', ...headers }).end(JSON.stringify(body));

  const server = createServer((req, res) => {
    const st = state.station;
    const url = new URL(req.url, 'http://x');
    const path = url.pathname;
    const bearer = /^Bearer (.*)$/.exec(req.headers.authorization ?? '')?.[1] ?? null;
    if (path === '/api/v1/player') {
      const url = st.noStream ? '' : '/stream/programme.wav';
      return json(res, 200, { station: st.name, show: st.show_name, relay: { turn_tls_port: 5349 }, stream_url: url, streams: url ? [{ codec: 'opus', kbps: 96, url }] : [] });
    }
    if (path === '/stream/programme.wav') {
      // The listener mount, as a short tone: what the page plays while the desk's mix is silent.
      st.listenerStream.hits++;
      st.listenerStream.open++;
      res.on('close', () => st.listenerStream.open--);
      // Paced as a live stream is, so it is open for as long as the page listens.
      res.writeHead(200, { 'content-type': 'audio/wav', 'content-length': TONE.length, 'accept-ranges': 'none', 'cache-control': 'no-store' });
      // A few seconds at once, as a live stream's own buffer gives, then in real time.
      res.write(TONE.subarray(0, 44 + 16000 * 5));
      let at = 44 + 16000 * 5;
      const pace = setInterval(() => {
        if (at >= TONE.length) return;
        res.write(TONE.subarray(at, at + 3200));
        at += 3200;
      }, 200);
      res.on('close', () => clearInterval(pace));
      return;
    }
    if (path === '/api/v1/branding') return json(res, 200, { accent: st.accent, icon: null, banner: null });
    if (path === '/api/v1/monitor/events') {
      st.calls.push({ method: 'GET', path, bearer, body: undefined });
      if (st.down) return json(res, 503, { error: "the monitor link needs the station's database" });
      if (bearer !== st.secret) return json(res, 401, { error: ENDED });
      if (st.ended) return json(res, 410, { error: ENDED });
      if (st.tooMany) return json(res, 429, { error: st.tooMany }, { 'retry-after': '1' });
      res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', 'x-accel-buffering': 'no' });
      st.streams.add(res);
      const send = () => !st.silent && res.write(`event: view\ndata: ${JSON.stringify(st.view())}\n\n`);
      send();
      const every = setInterval(send, 1000);
      res.on('close', () => {
        clearInterval(every);
        st.streams.delete(res);
      });
      return;
    }
    if (path.startsWith('/api/v1/monitor')) {
      let raw = '';
      req.on('data', (d) => (raw += d));
      req.on('end', () => {
        if (st.holdPresses && req.method === 'POST' && !path.endsWith('/token')) {
          st.calls.push({ method: req.method, path, bearer, body: raw ? JSON.parse(raw) : undefined, held: true });
          return;
        }
        if (st.down || (st.dropPresses && req.method === 'POST' && !path.endsWith('/token'))) {
          st.calls.push({ method: req.method, path, bearer, body: raw ? JSON.parse(raw) : undefined, dropped: true });
          return res.destroy();
        }
        const [status, body, headers] = st.handle(req.method, path, bearer, raw ? JSON.parse(raw) : undefined);
        json(res, status, body, headers);
      });
      return;
    }
    if (path.startsWith('/api/')) return json(res, 404, { error: 'no such route' });
    const file = join(dist, path.endsWith('/') ? `${path}index.html` : path);
    if (!file.startsWith(dist) || !existsSync(file)) return res.writeHead(404, pageHeaders).end();
    res.writeHead(200, { ...pageHeaders, 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
  });
  await new Promise((r) => server.listen(port, '127.0.0.1', r));
  return Object.assign(state, {
    close: () => new Promise((r) => (server.closeAllConnections(), server.close(r))),
    /** A new station for the next scenario, on this server's port. */
    use(station) {
      state.station.cut();
      station.port = port;
      state.station = station;
      return station;
    },
  });
}
