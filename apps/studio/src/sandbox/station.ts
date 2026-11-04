// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The sandbox's station (ADR-0212): a simulated control plane and desk that live in the page,
// for the website's "Try the desk". It answers the studio app's commands with the same view the
// real control plane sends, by the same rules (the strips, the caller queue, sending, declining,
// taking a caller; `crates/control/src/studio.rs`), and a director plays Kettle FM's
// pre-recorded people (designed voices from the demo video) when the desk lets them be heard.
// Pure apart from the clock it is given: the sound is `sound.ts`'s, which follows this state.
// Nothing here reaches a server.

import type { CartCommand, Caller, ChatCommand, DeskCommand, DeskPerson, EndPassCommand, Notice, RundownItem, ScreenerCommand, Source, View } from '../lib/api.ts';
import type { Bank, Cart, Catalogue, Sounding } from '../../../../packages/ui/carts.ts';

// ---- the cast and their lines ----

export type Who = 'eilidh' | 'gregor' | 'shona' | 'niall' | 'jessie' | 'calum';

/** Where a line is heard: on the desk like any voice (`air`), or in a caller's booth with the
 *  screener (`booth`), which never reaches the programme. */
export type Heard = 'air' | 'booth';

export interface Line {
  id: string;
  who: Who;
  /** Its length in ms (the stems' `check.tsv`). */
  ms: number;
  text: string;
  heard: Heard;
  /** Skipped, not waited for, when its speaker cannot be heard. */
  optional?: boolean;
}

const L = (id: string, who: Who, s: number, text: string, heard: Heard = 'air', optional = false): Line => ({ id, who, ms: Math.round(s * 1000), text, heard, optional });

/** A run of lines that starts when its condition holds, once. */
interface Beat {
  id: string;
  when: (s: Station) => boolean;
  lines: Line[];
  /** What happens when the beat's last line has played. */
  after?: (s: Station, now: number) => void;
}

const BEATS: Beat[] = [
  {
    id: 'open',
    when: (s) => s.open('eilidh') && !s.shotOnAir(),
    lines: [
      L('l-open', 'eilidh', 8.74, "Good morning, and welcome to The Morning Kettle on Kettle FM. I'm Eilidh Fraser-MacLeod. The kettle's on, the phones are open, and this morning it's all about Leith Walk."),
      L('l-intro-gregor', 'eilidh', 7.11, "My guest this morning is the local historian Gregor Stewart, who's joining us from home with nothing but a link. Gregor, are you there?"),
    ],
  },
  {
    id: 'gregor',
    when: (s) => s.done('open') && s.open('gregor'),
    lines: [
      L('l-gregor-here', 'gregor', 3.92, "I'm here, Eilidh. No account, no app. I just pressed the button in the email."),
      L('l-photo', 'eilidh', 2.44, "Now, Leith Walk. You've brought a photograph."),
      L('l-tram', 'gregor', 8.29, 'I have. Nineteen fifty-six, the last tram down Leith Walk, and a crowd of folk waving it goodbye. Fifty years later, we spent a fortune putting them back.'),
      L('l-phones', 'eilidh', 4.55, "And the phones have lit up already. Here's a wee bit of music while Shona answers them."),
    ],
    after: (s, now) => {
      s.ring('niall', now);
      s.ring('calum', now + 9_000);
    },
  },
  {
    id: 'niall-booth',
    when: (s) => s.callerState('niall') === 'screening',
    lines: [
      L('l-s1', 'shona', 2.8, "Kettle FM, you're through to Shona. Who's this?", 'booth'),
      L('l-n1', 'niall', 3.6, "Niall Anderson, from Leith. It's about the bus lanes on Leith Walk.", 'booth'),
      L('l-s2', 'shona', 1.98, "Have you rung us before, Niall?", 'booth'),
      L('l-n2', 'niall', 2.52, "Three weeks running. Nobody's let me on yet.", 'booth'),
      L('l-s3', 'shona', 3.28, "You're on next. Keep it short, and turn your radio down.", 'booth'),
      L('l-n3', 'niall', 1.76, 'Will do. Cheers, Shona.', 'booth'),
    ],
    after: (s, now) => s.ring('jessie', now + 4_000),
  },
  {
    id: 'niall-cue',
    when: (s) => s.person('niall')?.cue === true && !s.open('niall'),
    lines: [
      L('l-e-cue', 'eilidh', 2.12, "Niall, it's Eilidh. You're on in a moment.", 'booth'),
      L('l-n-cue', 'niall', 1.02, 'Right you are.', 'booth'),
    ],
  },
  {
    id: 'niall-air',
    when: (s) => s.open('niall'),
    lines: [
      L('l-e-n', 'eilidh', 4.13, "Niall Anderson in Leith, good morning. You've been trying to get through for a while."),
      L('l-n-air', 'niall', 8.27, 'Three weeks, Eilidh. The bus lanes on Leith Walk stop right where the buses need them most. At half past eight you can walk into town faster than the bus.'),
      L('l-g-n', 'gregor', 3.7, 'Funny. They said the very same about the trams in nineteen fifty-six.', 'air', true),
      L('l-e-bye', 'eilidh', 1.88, 'Thanks, Niall. Keep ringing.'),
    ],
  },
  {
    id: 'jessie-booth',
    when: (s) => s.callerState('jessie') === 'screening',
    lines: [L('i-jessie', 'jessie', 4.04, "I'm Jessie. I'm ringing in too, and I'm about to say something I shouldn't.", 'booth')],
  },
  {
    id: 'jessie-air',
    when: (s) => s.open('jessie') && !s.open('niall'),
    lines: [
      L('l-e-j', 'eilidh', 3.47, 'Jessie Brown is on the line from the Infirmary. Jessie, go ahead.'),
      L('l-j', 'jessie', 7.53, "Eilidh, I'm a nurse on nights, and there's no bus home after ten past eleven. It's a bloody disgrace, and whoever planned it is a total arse."),
    ],
    after: (s, now) => {
      s.swore = now;
    },
  },
  {
    id: 'sorry',
    // After a dump, or a few seconds after she swore when nobody dumped.
    when: (s) => s.swore !== null && (s.delay.dumps > 0 || s.clock - s.swore > 4_000),
    lines: [L('l-e-sorry', 'eilidh', 5.36, "Jessie, I'll stop you there. Sorry about the language, folks. You never heard it, and that's the point.")],
  },
  {
    id: 'down',
    when: (s) => !s.playout && s.done('open'),
    lines: [
      L('l-e-down', 'eilidh', 5.05, "Gregor, the desk says playout's down. Listeners can't hear us, but we can still hear each other."),
      L('l-g-down', 'gregor', 3.41, "Just like the trams. Gone, but we're still talking about them.", 'air', true),
    ],
  },
  {
    id: 'back',
    when: (s) => s.done('down') && s.playout,
    lines: [L('l-e-back', 'eilidh', 1.16, "And we're back.")],
  },
  {
    id: 'end',
    when: (s) => s.done('sorry') && s.ended,
    lines: [L('l-e-end', 'eilidh', 6.81, "That's The Morning Kettle. Thanks to Gregor, to Niall and Jessie, and to Shona on the phones. Finlay Grant is on next with Steeped.")],
  },
];

/** Every line, for the sound to fetch. */
export const LINES: Line[] = BEATS.flatMap((b) => b.lines);

/** A gap between one line and the next, as people leave in conversation. */
const GAP_MS = 450;
/** How long an optional line waits for its speaker before it is skipped. */
const OPTIONAL_WAIT_MS = 2_500;

// ---- the station ----

const SHOW = 'The Morning Kettle';
const STATION = 'Kettle FM';
const SLOTS = 5;
const PLACE_HELD_MS = 5 * 60 * 1000;
const NOTICES = 12;
/** The profanity delay this sandbox runs, and how fast it builds back after a dump (ms of delay
 *  per ms of time). The real desk slows what it plays to build back (ADR-0166). */
export const DELAY_TARGET_MS = 8_000;
const REBUILD_RATE = 0.25;
/** The path beyond the desk: the stream's own buffer and a player's (a setting until measured). */
const LISTENER_PATH_MS = 6_000;

type Role = DeskPerson['role'];

interface CallerRec {
  identity: string;
  name: string;
  state: Caller['state'];
  since_ms: number;
  arrived_ms: number;
  line: string;
  note: string;
  name_on_air: string;
  declined: { at_ms: number; reason: string } | null;
  dropped_ms: number | null;
  taken: boolean;
}

interface Lobby {
  name: string;
  topic: string;
  consent: { at_ms: number; version: string; by: 'caller' };
}

/** A line being spoken: who, which, and until when. */
export interface Speaking {
  line: Line;
  from: number;
  until: number;
}

export class Refusal extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

const preferred = (role: Role): number[] =>
  ({ presenter: [1, 2, 3, 4, 5], guest: [2, 3, 1, 4, 5], caller: [4, 3, 2, 5, 1], dj: [5, 1, 2, 3, 4] })[role as string] ?? [];

function person(identity: string, name: string, role: Role, extra: Partial<DeskPerson> = {}): DeskPerson {
  return { identity, name, role, on_air: false, mute: false, cue: false, talk: false, talk_to: null, present: true, away_ms: null, linked: true, lufs: null, lufs_peak: null, ...extra };
}

/** The callers who ring, when the show says so: what they typed on the player page. */
const CALLERS: Record<string, { name: string; topic: string }> = {
  niall: { name: 'Niall Anderson', topic: 'The bus lanes on Leith Walk' },
  jessie: { name: 'Jessie Brown', topic: 'No night bus from the Infirmary' },
  calum: { name: 'Calum Robertson', topic: 'Parking at Easter Road on match days' },
};

export class Station {
  clock: number;
  readonly started: number;
  people: DeskPerson[];
  callers: CallerRec[] = [];
  lobby = new Map<string, Lobby>();
  slots: (string | null)[] = Array(SLOTS).fill(null);
  notices: Notice[] = [];
  noticeId = 0;
  delay = { delay_ms: DELAY_TARGET_MS, rebuilding: false, dumps: 0, dumped_at_ms: null as number | null };
  playout = true;
  playoutSince: number;
  modes = { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false };
  beforeInflux: typeof this.modes | null = null;
  influxSince: number | null = null;
  sounding: Sounding[] = [];
  cartSeq = 0;
  playNo = 0;
  /** Who rings when: identity → the time they join the queue. */
  rings = new Map<string, number>();
  /** The director: beats done, the beat and line playing, and what is being said. */
  beatsDone = new Set<string>();
  beat: { beat: Beat; next: number; waitingSince: number | null } | null = null;
  speaking: Speaking | null = null;
  swore: number | null = null;
  /** The visitor ended the show (the guide's button): Eilidh says goodbye. */
  ended = false;
  /** Levels from the sound, by identity (LUFS-like, approximate). */
  levels: Record<string, number | null> = {};
  readonly catalogue: Catalogue;
  /** Told of every line that starts and every cart that starts or stops. */
  onLine: ((s: Speaking) => void) | null = null;
  version = 0;

  constructor(now: number, catalogue: Catalogue) {
    this.clock = now;
    this.started = now;
    this.playoutSince = now - 3_600_000;
    this.catalogue = catalogue;
    this.people = [
      person('eilidh', 'Eilidh Fraser-MacLeod', 'presenter'),
      person('gregor', 'Gregor Stewart', 'guest'),
      person('shona', 'Shona Campbell', 'screener'),
    ];
    this.seat();
  }

  // ---- reading ----

  person(id: string): DeskPerson | undefined {
    return this.people.find((p) => p.identity === id);
  }

  /** Heard on the desk: on air (or open while playout is down) and not muted. */
  open(id: string): boolean {
    const p = this.person(id);
    return !!p && p.present && p.on_air && !p.mute;
  }

  callerState(id: string): Caller['state'] | null {
    return this.callers.find((c) => c.identity === id)?.state ?? null;
  }

  done(beat: string): boolean {
    return this.beatsDone.has(beat);
  }

  /** A one-shot cart sounds on air now (the director lets a jingle finish before speaking). */
  shotOnAir(): boolean {
    return this.sounding.some((s) => s.pool === 'shot' && !s.cue && !s.stopping);
  }

  /** A bed ducks under a voice on air (the desk's ducker, approximately). */
  ducked(): boolean {
    return this.people.some((p) => p.on_air && !p.mute) && !!this.speaking && this.speaking.line.heard === 'air' && this.open(this.speaking.line.who);
  }

  // ---- time ----

  /** Move the clock on: callers ring, the delay builds back, carts end, the director speaks. */
  tick(now: number): void {
    const dt = Math.max(0, now - this.clock);
    this.clock = now;
    for (const [id, at] of [...this.rings]) {
      if (now >= at) {
        this.rings.delete(id);
        this.join(id, now);
      }
    }
    if (this.delay.rebuilding) {
      this.delay.delay_ms = Math.min(DELAY_TARGET_MS, this.delay.delay_ms + dt * REBUILD_RATE);
      if (this.delay.delay_ms >= DELAY_TARGET_MS) this.delay.rebuilding = false;
    }
    const before = this.sounding.length;
    this.sounding = this.sounding.filter((s) => s.ends_ms === null || s.ends_ms > now);
    if (this.sounding.length !== before) this.version++;
    this.callers = this.callers.filter((c) => c.dropped_ms === null || now - c.dropped_ms < PLACE_HELD_MS);
    this.direct(now);
  }

  private direct(now: number): void {
    if (this.speaking && now < this.speaking.until) return;
    if (this.speaking) this.speaking = null;
    if (!this.beat) {
      const next = BEATS.find((b) => !this.beatsDone.has(b.id) && b.when(this));
      if (!next) return;
      this.beat = { beat: next, next: 0, waitingSince: null };
    }
    const b = this.beat;
    const line = b.beat.lines[b.next];
    if (!line) {
      this.beatsDone.add(b.beat.id);
      this.beat = null;
      b.beat.after?.(this, now);
      this.version++;
      return;
    }
    if (!this.canHear(line)) {
      b.waitingSince ??= now;
      if (line.optional && now - b.waitingSince > OPTIONAL_WAIT_MS) {
        b.next++;
        b.waitingSince = null;
      }
      return;
    }
    b.waitingSince = null;
    b.next++;
    this.speaking = { line, from: now, until: now + line.ms + GAP_MS };
    this.onLine?.(this.speaking);
    this.version++;
  }

  /** A line can be said now: a booth line while its caller is screened or cued, an air line
   *  while its speaker is open. */
  private canHear(line: Line): boolean {
    if (line.heard === 'booth') {
      if (line.who === 'shona' || line.who === 'eilidh') return true;
      const c = this.callers.find((x) => x.identity === line.who);
      const p = this.person(line.who);
      return !!c && c.dropped_ms === null && (c.state === 'screening' || !!p?.cue);
    }
    return this.open(line.who);
  }

  // ---- callers ----

  /** A caller rings at `at` (they join the queue then). */
  ring(id: string, at: number): void {
    if (this.person(id) || this.rings.has(id)) return;
    this.rings.set(id, at);
  }

  private join(id: string, now: number): void {
    const who = CALLERS[id];
    if (!who) return;
    this.people.push(person(id, who.name, 'caller'));
    this.lobby.set(id, { name: who.name, topic: who.topic, consent: { at_ms: now - 20_000, version: '2026-11-01', by: 'caller' } });
    this.callers.push({ identity: id, name: who.name, state: 'waiting', since_ms: now, arrived_ms: now, line: '', note: '', name_on_air: '', declined: null, dropped_ms: null, taken: false });
    this.version++;
  }

  // ---- the strips (studio.rs `sync`) ----

  private seat(): void {
    const ready = (id: string) => this.callers.some((c) => c.identity === id && c.state === 'ready' && !c.declined && c.dropped_ms === null);
    const taken: string[] = [];
    for (const c of this.callers) {
      const p = this.person(c.identity);
      if (c.state === 'ready' && p && (p.cue || p.on_air) && !c.taken) {
        c.taken = true;
        c.declined = null;
        taken.push(c.identity);
      }
    }
    this.slots = this.slots.map((id) => {
      const p = id ? this.person(id) : undefined;
      if (!p || !p.present) return null;
      if (p.role === 'caller') return ready(p.identity) || p.on_air ? id : null;
      return preferred(p.role).length ? id : null;
    });
    for (const p of this.people) {
      if (this.slots.includes(p.identity)) continue;
      const eligible = p.role === 'caller' ? p.on_air || ready(p.identity) : preferred(p.role).length > 0;
      if (!eligible || !p.present) continue;
      const n = preferred(p.role).find((n) => this.slots[n - 1] === null);
      if (n) this.slots[n - 1] = p.identity;
    }
    for (const id of taken) {
      const slot = this.slots.indexOf(id) + 1;
      this.notice('screener', 'good', 'caller.taken', id, `${this.short(id)} is with the desk`, slot ? `The presenter took them on strip ${slot}. Your note went with them.` : 'The presenter took them. Your note went with them.');
    }
  }

  private short(id: string): string {
    const c = this.callers.find((x) => x.identity === id);
    return (c?.name_on_air || c?.name || this.person(id)?.name || id).split(' ')[0];
  }

  private notice(surface: Notice['surface'], tone: Notice['tone'], kind: string, subject: string | null, title: string, message: string): void {
    this.notices.push({ id: ++this.noticeId, at_ms: this.clock, surface, tone, kind, subject, title, message });
    if (this.notices.length > NOTICES) this.notices.shift();
  }

  private caller(id: string): CallerRec {
    const c = this.callers.find((x) => x.identity === id);
    if (!c) throw new Refusal(404, `\`${id}\` is not in the queue`);
    return c;
  }

  // ---- commands ----

  desk(c: DeskCommand, now: number): void {
    this.clock = now;
    if (c.op === 'dump') {
      if (this.delay.delay_ms < 100 && !this.delay.rebuilding) throw new Refusal(409, 'The delay is off; there is nothing to dump.');
      this.delay = { delay_ms: 0, rebuilding: true, dumps: this.delay.dumps + 1, dumped_at_ms: now };
    } else if (c.op === 'picture') {
      throw new Refusal(409, 'Video is off on this station.');
    } else {
      const p = this.person(c.identity);
      if (!p) throw new Refusal(404, `\`${c.identity}\` is not in the room`);
      if (c.op === 'talk_to') {
        const to = this.person(c.to);
        if (!to) throw new Refusal(404, `\`${c.to}\` is not in the room`);
        p.talk_to = c.on ? c.to : null;
        to.talk = c.on;
      } else {
        if ((c.op === 'on_air' || c.op === 'cue') && c.on && p.role === 'caller' && !this.lobby.has(p.identity)) {
          throw new Refusal(409, `\`${p.identity}\` has no consent to record on file`);
        }
        p[c.op] = c.on;
        if (c.op === 'on_air' && c.on) p.cue = false;
      }
    }
    this.seat();
    this.version++;
  }

  screener(c: ScreenerCommand, now: number): void {
    this.clock = now;
    const rec = this.caller(c.identity);
    switch (c.op) {
      case 'screen':
        for (const o of this.callers) if (o.state === 'screening' && o !== rec) Object.assign(o, { state: 'waiting', since_ms: now });
        if (rec.state === 'waiting') Object.assign(rec, { state: 'screening', since_ms: now });
        break;
      case 'send':
        if (rec.dropped_ms !== null) throw new Refusal(409, `\`${rec.identity}\`'s link is down; they cannot go to the desk`);
        Object.assign(rec, { state: 'ready', since_ms: now, declined: null, taken: false });
        this.notice('desk', 'good', 'caller.ready', rec.identity, `${this.short(rec.identity)} is ready for you`, rec.line || 'Sent by the screener.');
        break;
      case 'back':
        if (this.person(rec.identity)?.on_air) throw new Refusal(409, `\`${rec.identity}\` is on air; the presenter takes them off first`);
        Object.assign(rec, { state: 'waiting', since_ms: now, declined: null, taken: false });
        break;
      case 'decline':
        if (rec.state !== 'ready') throw new Refusal(409, `\`${rec.identity}\` was not sent to the desk`);
        rec.declined = { at_ms: now, reason: c.reason };
        rec.taken = false;
        this.notice('screener', 'warning', 'caller.declined', rec.identity, 'The desk is not taking callers', `The presenter declined ${this.short(rec.identity)}${c.reason ? `: “${c.reason}”` : ''}. ${this.short(rec.identity)} stays ready; send again when the desk is free.`);
        break;
      case 'notes':
        if (c.line !== undefined) rec.line = c.line.slice(0, 500);
        if (c.note !== undefined) rec.note = c.note.slice(0, 500);
        if (c.name_on_air !== undefined) rec.name_on_air = c.name_on_air.slice(0, 500);
        break;
      case 'drop': {
        this.callers = this.callers.filter((x) => x !== rec);
        this.people = this.people.filter((p) => p.identity !== rec.identity);
        break;
      }
      case 'release':
        if (rec.dropped_ms === null) throw new Refusal(409, `\`${rec.identity}\` is still on the line; drop them instead`);
        this.callers = this.callers.filter((x) => x !== rec);
        break;
    }
    this.seat();
    this.version++;
  }

  endPass(c: EndPassCommand, now: number): void {
    this.clock = now;
    const p = this.person(c.identity);
    if (!p) throw new Refusal(404, `\`${c.identity}\` is not in the room`);
    if (p.role === 'presenter') throw new Refusal(409, 'The presenter cannot be sent out of their own show.');
    this.people = this.people.filter((x) => x !== p);
    this.callers = this.callers.filter((x) => x.identity !== p.identity);
    this.seat();
    this.version++;
  }

  chat(c: ChatCommand, now: number): void {
    this.clock = now;
    if (c.op === 'mode') {
      this.modes[c.mode] = c.on;
    } else if (c.on && !this.beforeInflux) {
      this.beforeInflux = { ...this.modes };
      this.modes = { slow: true, hold_new: true, links_trusted: true, accounts_only: true, min_age_24h: true };
      this.influxSince = now;
      this.notice('screener', 'warning', 'chat.influx', null, 'Influx mode on', 'One press, five settings: accounts only, 24 h old, slow mode 10 s, every new chatter held, links for trusted chatters only. The undo is the same key.');
    } else if (!c.on && this.beforeInflux) {
      this.modes = this.beforeInflux;
      this.beforeInflux = null;
      this.influxSince = null;
      this.notice('screener', 'good', 'chat.influx-off', null, 'Influx mode off', "The chat's settings are back as they were before.");
    }
    this.version++;
  }

  carts(c: CartCommand, now: number): void {
    this.clock = now;
    const live = this.sounding.filter((s) => !s.stopping);
    switch (c.op) {
      case 'toggle': {
        const on = live.find((s) => s.cart === c.cart && !s.cue);
        if (on) this.stopCart(on, now);
        else this.fire(c.cart, false, now);
        break;
      }
      case 'fire':
        this.fire(c.cart, false, now);
        break;
      case 'cue': {
        const on = live.find((s) => s.cart === c.cart && s.cue);
        if (on) this.stopCart(on, now);
        else this.fire(c.cart, true, now);
        break;
      }
      case 'stop':
        for (const s of live.filter((x) => x.cart === c.cart)) this.stopCart(s, now);
        break;
      case 'stop_last': {
        const last = live.filter((s) => !s.cue).at(-1);
        if (last) this.stopCart(last, now);
        break;
      }
      case 'fade_all':
        for (const s of live) this.stopCart(s, now, 1_500);
        break;
      case 'clear_cue':
        for (const s of live.filter((x) => x.cue)) this.stopCart(s, now);
        break;
    }
    this.version++;
  }

  private fire(id: string, cue: boolean, now: number): void {
    const cart = this.catalogue.carts.find((c) => c.id === id);
    if (!cart || !cart.file) throw new Refusal(404, `No cart \`${id}\``);
    const pool = cart.mode === 'bed' ? 'bed' : 'shot';
    // A bed replaces the bed that plays (one bed player); a shot plays beside the others.
    if (pool === 'bed') for (const s of this.sounding.filter((x) => x.pool === 'bed' && x.cue === cue && !x.stopping)) this.stopCart(s, now, 800);
    this.sounding.push({
      cart: id,
      pool,
      player: 1,
      mode: cart.mode,
      cue,
      play: ++this.playNo,
      started_ms: now,
      ends_ms: cart.mode === 'bed' ? null : now + cart.duration_ms,
      stopping: false,
      seq: ++this.cartSeq,
    });
  }

  private stopCart(s: Sounding, now: number, fadeMs = 300): void {
    s.stopping = true;
    s.ends_ms = now + fadeMs;
    s.seq = ++this.cartSeq;
  }

  // ---- the guide's own levers ----

  /** Pull playout's plug for a while, as the demo video does: the lamp says PLAYOUT DOWN. */
  setPlayout(on: boolean, now: number): void {
    this.playout = on;
    this.playoutSince = now;
    if (!on) this.notice('desk', 'serious', 'playout.down', null, 'Playout is down', 'Listeners hear the fallback; the room still hears itself. Everything you open says OPEN, not ON AIR, until playout is back.');
    else this.notice('desk', 'good', 'playout.back', null, 'Playout is back', 'Listeners hear the desk again.');
    this.version++;
  }

  endShow(now: number): void {
    this.ended = true;
    this.clock = now;
    this.version++;
  }

  // ---- the view ----

  view(now: number): View {
    const cartsMicOpen = this.people.some((p) => p.on_air && !p.mute);
    const ducked = this.ducked();
    const withLevel = (p: DeskPerson): DeskPerson => ({ ...p, lufs: this.levels[p.identity] ?? null, lufs_peak: this.levels[p.identity] ?? null });
    const sources: Source[] = this.slots.flatMap((id, i) => {
      const p = id ? this.person(id) : undefined;
      if (!p) return [];
      const c = this.callers.find((x) => x.identity === p.identity);
      return [{ ...withLevel(p), slot: i + 1, caller: c ? { line: c.line, note: c.note, name_on_air: c.name_on_air, sent: c.state === 'ready', taken: c.taken } : null }];
    });
    const callers: Caller[] = this.callers.map((c, i) => {
      const p = this.person(c.identity);
      const lobby = this.lobby.get(c.identity);
      return {
        identity: c.identity,
        name: c.name,
        ordinal: i + 1,
        state: c.state,
        since_ms: c.since_ms,
        arrived_ms: c.arrived_ms,
        line: c.line,
        note: c.note,
        name_on_air: c.name_on_air,
        declined: c.declined,
        banned_before: null,
        topic: lobby?.topic ?? '',
        consent: lobby?.consent ?? null,
        dropped_ms: c.dropped_ms,
        slot: this.slots.indexOf(c.identity) + 1 || null,
        on_air: !!p?.on_air,
        cue: !!p?.cue,
        linked: !!p?.linked,
        present: !!p?.present,
        lufs: this.levels[c.identity] ?? null,
      };
    });
    const seat = (role: Role) => this.slots.find((id) => id && this.person(id)?.role === role) ?? this.people.find((p) => p.role === role && p.present)?.identity ?? null;
    const presenter = seat('presenter');
    const screener = seat('screener');
    const rundown: RundownItem[] = rundownFrom(this.started);
    return {
      station: STATION,
      now_ms: now,
      show: { title: SHOW, rundown },
      desk: {
        reachable: true,
        error: null,
        playout: { connected: this.playout, for_ms: now - this.playoutSince },
        delay: { delay_ms: Math.round(this.delay.delay_ms), target_ms: DELAY_TARGET_MS, rebuilding: this.delay.rebuilding, dumps: this.delay.dumps, dumped_at_ms: this.delay.dumped_at_ms },
        picture: null,
      },
      seats: { presenter, screener, presenter_name: presenter ? (this.person(presenter)?.name ?? '') : '', screener_name: screener ? (this.person(screener)?.name ?? '') : '' },
      sources,
      callers,
      desk_taking_callers: !this.callers.some((c) => c.declined),
      chat: { ...this.modes, influx: !!this.beforeInflux, influx_since_ms: this.influxSince },
      notices: [...this.notices],
      carts: { version: this.catalogue.version, desk: true, desk_error: null, sounding: this.sounding.map((s) => ({ ...s })), ducked, mic_open: cartsMicOpen, missing: [], importing: null },
      listener_delay: { ms: LISTENER_PATH_MS + Math.floor(this.delay.delay_ms / 100) * 100, measured: false },
      default_fallback: false,
      audience: { listeners: listenersAt(now, this.started), edges: false },
      mode: 'live',
    };
  }
}

/** The show's rundown, from the moment the sandbox opened (station time, HH:MM). */
function rundownFrom(start: number): RundownItem[] {
  const at = (min: number) => {
    const d = new Date(start + min * 60_000);
    return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  };
  return [
    { at: at(0), title: 'Open and jingle', detail: 'Eilidh on air, the steel jingle', hard_post: false, key: null },
    { at: at(1), title: 'Gregor Stewart', detail: 'Local historian: the last tram down Leith Walk, 1956', hard_post: false, key: null },
    { at: at(3), title: 'Phones', detail: 'Shona screens; Leith Walk callers', hard_post: false, key: null },
    { at: at(8), title: 'News', detail: 'Top of the hour', hard_post: true, key: null },
    { at: at(10), title: 'Steeped with Finlay Grant', detail: 'Next show', hard_post: false, key: null },
  ];
}

/** A listener count that drifts like a morning audience: about 140, never the same twice. */
function listenersAt(now: number, start: number): number {
  const t = (now - start) / 1000;
  return Math.round(138 + t / 20 + 6 * Math.sin(t / 37) + 3 * Math.sin(t / 11));
}

// ---- the cart wall ----

/** The demo cart pack's entry (assets/carts-demo/carts.json). */
export interface PackCart {
  id: string;
  label: string;
  file: string;
  duration_s: number;
  category: string;
  suggested: { bank: number; slot: number };
  loop: boolean;
  licence: string;
}

/** The sandbox's first bank: The Morning Kettle's own top row (F1 to F6), then the pack's four. */
const KETTLE_BANK = ['jingle-1', 'bed-1', 'applause-1', 'rimshot-1', 'news-sting-1', 'sad-trombone-1', 'jingle-2', 'bed-2', 'laugh-1', 'cheer-1', 'drumroll-1', 'pips-1', 'phone-ring-1', 'bagpipe-1', 'whoosh-1', 'ding-1'];

export function catalogueFrom(pack: { banks: Record<string, string>; carts: PackCart[] }): Catalogue {
  const banks: Bank[] = [{ number: 1, name: 'The Morning Kettle', colour_slot: 1 }];
  for (const [n, name] of Object.entries(pack.banks)) banks.push({ number: Number(n) + 1, name, colour_slot: Number(n) + 1 });
  const cart = (p: PackCart, bank: number, slot: number, id = p.id): Cart => ({
    id,
    bank,
    slot,
    label: p.label,
    detail: p.category,
    mode: p.loop ? 'bed' : 'one_shot',
    file: p.file,
    duration_ms: Math.round(p.duration_s * 1000),
    gain_db: 0,
    loudness_lufs: -16,
    true_peak_dbtp: -1,
    ramp_ms: null,
    origin: 'WeaveSignal demo cart pack',
    licence: p.licence,
    missing: false,
  });
  const carts: Cart[] = [];
  KETTLE_BANK.forEach((id, i) => {
    const p = pack.carts.find((c) => c.id === id);
    if (p) carts.push(cart(p, 1, i + 1));
  });
  for (const p of pack.carts) carts.push(cart(p, p.suggested.bank + 1, p.suggested.slot, `${p.id}@${p.suggested.bank + 1}`));
  return { version: 1, banks, carts, loudness_lufs: -16 };
}

/** The pack's file for a cart id (a cart on two banks has two ids). */
export function fileOf(cat: Catalogue, id: string): string | null {
  return cat.carts.find((c) => c.id === id)?.file ?? null;
}
