// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The sandbox's station (ADR-0212, ADR-0222): a simulated control plane and desk that live in
// the page, for the website's "Try the desk" and its training scenarios. It answers the studio
// app's commands with the same view the real control plane sends, by the same rules (the
// strips, the caller queue, sending, declining, taking a caller, a dropped link's held place,
// accounts banned before, the station's mode; `crates/control/src/studio.rs`; the show's step,
// which closes call-in, refuses hidden seats and switches Dump, the handover and the picture
// off, ADR-0271, `crates/control/src/steps.rs`), and a director
// plays the scenario's pre-recorded people when the desk lets them be heard. Pure apart from
// the clock it is given: the sound is `sound.ts`'s, which follows this state. Nothing here
// reaches a server.

import type { CartCommand, Caller, ChatCommand, DeskCommand, DeskPerson, EndPassCommand, Invite, Notice, ScreenerCommand, ShowStep, Source, StepChanged, StepView, View } from '../lib/api.ts';
import { STEPS, STEP_WORD, atLeast } from '../lib/steps.ts';
import type { Bank, Cart, Catalogue, Sounding } from '../../../../packages/ui/carts.ts';
import type { StationMode } from '../../../../packages/ui/states.ts';
import type { Beat, Line, Speaking } from './lines.ts';
import type { Scenario } from './scenarios/scenario.ts';
import { SHOW } from './scenarios/show.ts';

export type { Line, Speaking } from './lines.ts';

/** A gap between one line and the next, as people leave in conversation. */
const GAP_MS = 450;
/** How long an optional line waits for its speaker before it is skipped. */
const OPTIONAL_WAIT_MS = 2_500;

// ---- the station ----

const STATION = 'Kettle FM';
const SLOTS = 5;
/** How long a dropped caller's place in the queue is held (studio.rs `PLACE_HELD_MS`). */
export const PLACE_HELD_MS = 5 * 60 * 1000;
/** How long the desk keeps someone whose link dropped before they leave its list (the tile
 *  says "kept 30 s"). */
export const AWAY_KEPT_MS = 30_000;
const NOTICES = 12;
/** The profanity delay this sandbox runs, and how fast it builds back after a dump (ms of delay
 *  per ms of time). The real desk slows what it plays to build back (ADR-0166). */
export const DELAY_TARGET_MS = 8_000;
/** From an invite being sent to its person in the room (opening the link, the microphone). */
export const INVITE_TO_JOIN_MS = 4_000;
/** How long an invite lasts: the real station's default `join.invite_ttl_s`. */
export const INVITE_TTL_MS = 4 * 3600 * 1000;
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

export class Refusal extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

/** The seat a step hides from a role, in the station's words for the studio (steps.rs
 *  `hidden_seat` and `Hidden::when_making`); null when the seat is open. `own`: the show's
 *  presenter on their own link, allowed at every step. */
export function hiddenSeat(step: ShowStep, role: Role, own = false): string | null {
  const at = STEP_WORD[step];
  if (role === 'presenter' && (own || atLeast(step, 'guests'))) return null;
  if ((role === 'presenter' || role === 'guest') && !atLeast(step, 'guests')) return `This show runs at ${at}, so it has no seat for a guest or a second presenter. They come with Guests.`;
  if (role === 'caller' && !atLeast(step, 'callers')) return `This show runs at ${at}, so it takes no callers. Call-in comes with Callers.`;
  if (role === 'dj' && !atLeast(step, 'full')) return 'An invitation for the next show\'s DJ comes with Full.';
  if (role === 'screener' && !atLeast(step, 'full')) return `This show runs at ${at}, so the screener's seat is not open. It comes with Full.`;
  return null;
}

/** The show's path, as a real station makes it from the name. */
const slugOf = (name: string): string => name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');

const preferred = (role: Role): number[] =>
  ({ presenter: [1, 2, 3, 4, 5], guest: [2, 3, 1, 4, 5], caller: [4, 3, 2, 5, 1], dj: [5, 1, 2, 3, 4] })[role as string] ?? [];

function person(identity: string, name: string, role: Role, extra: Partial<DeskPerson> = {}): DeskPerson {
  return { identity, name, role, on_air: false, mute: false, cue: false, talk: false, talk_to: null, present: true, away_ms: null, linked: true, lufs: null, lufs_peak: null, ...extra };
}

export class Station {
  readonly scenario: Scenario;
  clock: number;
  readonly started: number;
  people: DeskPerson[];
  callers: CallerRec[] = [];
  lobby = new Map<string, Lobby>();
  slots: (string | null)[] = Array(SLOTS).fill(null);
  notices: Notice[] = [];
  noticeId = 0;
  delay = { delay_ms: DELAY_TARGET_MS, target_ms: DELAY_TARGET_MS, rebuilding: false, dumps: 0, dumped_at_ms: null as number | null };
  playout = true;
  playoutSince: number;
  /** The station's mode (ADR-0045): live, or automation holding the air (ADR-0183). */
  mode: StationMode;
  /** The show's step (ADR-0271): what is switched on. The scenario's at the start; the frame's
   *  step dialog moves it, as on a real station. */
  step: ShowStep;
  modes = { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false };
  beforeInflux: typeof this.modes | null = null;
  influxSince: number | null = null;
  sounding: Sounding[] = [];
  cartSeq = 0;
  playNo = 0;
  /** Accounts banned before, by identity (the station's setting). */
  bans: Record<string, { date: string; reason: string }>;
  /** When each person whose link dropped left the room, by identity. */
  away = new Map<string, number>();
  /** The microphone of the browser tab the DJ speaks through (ADR-0222): on unless the scenario
   *  turns it off. Off, the DJ's lines reach no one and their meter stays flat. */
  mic = true;
  /** Things that happen later: callers ringing, a link going bad, the simulated presenter. */
  private timers: { at: number; fn: (s: Station, now: number) => void }[] = [];
  /** Who has been asked to ring, so nobody rings twice. */
  private rings = new Set<string>();
  /** The director: beats done, the beat and line playing, and what is being said. */
  beatsDone = new Set<string>();
  beat: { beat: Beat; next: number; waitingSince: number | null } | null = null;
  speaking: Speaking | null = null;
  swore: number | null = null;
  /** The visitor ended the show (the guide's button): the presenter says goodbye. */
  ended = false;
  /** The sound has started (the visitor's first press). */
  begun = false;
  /** The show on air, as the desk names it: the scenario's, until a handover moves it on (on a
   *  real station the schedule does, at the post). */
  show: string;
  /** What a scenario notes for its guide: a mark is set once by a beat or a timer. */
  marks = new Set<string>();
  /** What the visitor did, for the guide and the end card: carts fired (by id, in order),
   *  fades of everything, cues of a cart, callers sent back to the queue, callers dropped. */
  fired: string[] = [];
  fades = 0;
  /** Invites made from the desk's dialog. */
  invites = 0;
  cartCues = 0;
  backs = 0;
  drops: Record<string, number> = {};
  /** Callers let go because the step fell below Callers, in order. */
  released: string[] = [];
  /** Levels from the sound, by identity (LUFS-like, approximate). */
  levels: Record<string, number | null> = {};
  readonly catalogue: Catalogue;
  /** Told of every line that starts. */
  onLine: ((s: Speaking) => void) | null = null;
  version = 0;

  constructor(now: number, catalogue: Catalogue, scenario: Scenario = SHOW) {
    this.scenario = scenario;
    this.clock = now;
    this.started = now;
    this.playoutSince = now - 3_600_000;
    this.catalogue = catalogue;
    this.mode = scenario.mode ?? 'live';
    this.step = scenario.step;
    this.bans = { ...(scenario.bans ?? {}) };
    this.show = scenario.show;
    const later = new Set(scenario.later ?? []);
    this.people = scenario.cast.filter((c) => !later.has(c.identity)).map((c) => person(c.identity, c.name, c.role));
    this.seat();
  }

  /** Every line the scenario can say, for the sound to fetch. */
  get lines(): Line[] {
    return this.scenario.beats.flatMap((b) => b.lines);
  }

  /** The sound has started: the scenario's rings and events are timed from now, not from when
   *  the page opened. */
  begin(now: number): void {
    if (this.begun) return;
    this.begun = true;
    this.clock = now;
    this.scenario.start?.(this, now);
    this.version++;
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

  /** A caller's record as the screener sees it, or null. */
  callerRec(id: string): CallerRec | null {
    return this.callers.find((c) => c.identity === id) ?? null;
  }

  done(beat: string): boolean {
    return this.beatsDone.has(beat);
  }

  mark(what: string): void {
    this.marks.add(what);
    this.version++;
  }

  has(mark: string): boolean {
    return this.marks.has(mark);
  }

  /** A caller's link is degraded as the desk sees it: in the room, no audio reaching it. */
  degraded(id: string): boolean {
    const p = this.person(id);
    return !!p && p.present && !p.linked;
  }

  /** The visitor's own voice can be heard: their person is open and the tab's microphone is on. */
  heardFromYou(): boolean {
    return this.mic && this.open(this.scenario.you.identity);
  }

  /** A one-shot cart sounds on air now (the director lets a jingle finish before speaking). */
  shotOnAir(): boolean {
    return this.sounding.some((s) => s.pool === 'shot' && !s.cue && !s.stopping);
  }

  /** A bed ducks under a voice on air (the desk's ducker, approximately). */
  ducked(): boolean {
    if (!this.speaking || this.speaking.line.heard !== 'air') return false;
    const who = this.speaking.line.who;
    return this.open(who) && (who !== this.scenario.you.identity || this.mic);
  }

  /** Listeners hear the desk: playout is connected and automation does not hold the air
   *  (`listenersHear` in packages/ui/states.ts). */
  listenersHearDesk(): boolean {
    return this.playout && this.mode !== 'auto';
  }

  // ---- time ----

  /** Move the clock on: timers fire, the delay builds back, carts end, dropped links settle, the
   *  director speaks. */
  tick(now: number): void {
    const dt = Math.max(0, now - this.clock);
    this.clock = now;
    const due = this.timers.filter((t) => now >= t.at).sort((a, b) => a.at - b.at);
    if (due.length) {
      this.timers = this.timers.filter((t) => now < t.at);
      for (const t of due) t.fn(this, now);
    }
    if (this.delay.rebuilding) {
      this.delay.delay_ms = Math.min(DELAY_TARGET_MS, this.delay.delay_ms + dt * REBUILD_RATE);
      if (this.delay.delay_ms >= DELAY_TARGET_MS) this.delay.rebuilding = false;
    }
    const before = this.sounding.length;
    this.sounding = this.sounding.filter((s) => s.ends_ms === null || s.ends_ms > now);
    if (this.sounding.length !== before) this.version++;
    // Someone whose link dropped leaves the desk's list after its grace; their place in the
    // queue is held for longer.
    let left = false;
    for (const [id, at] of [...this.away]) {
      if (now - at >= AWAY_KEPT_MS) {
        this.away.delete(id);
        this.people = this.people.filter((p) => p.identity !== id);
        left = true;
      }
    }
    const held = this.callers.length;
    this.callers = this.callers.filter((c) => c.dropped_ms === null || now - c.dropped_ms < PLACE_HELD_MS);
    if (left || this.callers.length !== held) {
      this.seat();
      this.version++;
    }
    this.direct(now);
  }

  /** Something happens at `when`: a caller rings, a link goes bad, the presenter acts. */
  at(when: number, fn: (s: Station, now: number) => void): void {
    this.timers.push({ at: when, fn });
  }

  private direct(now: number): void {
    // A beat with no lines is a cue (the simulated presenter acting, a mark for the guide): it
    // fires as soon as its condition holds, whoever is speaking.
    for (const b of this.scenario.beats) {
      if (b.lines.length === 0 && !this.beatsDone.has(b.id) && b.when(this)) {
        this.beatsDone.add(b.id);
        b.after?.(this, now);
        this.version++;
      }
    }
    if (this.speaking && now < this.speaking.until) return;
    if (this.speaking) this.speaking = null;
    if (!this.beat) {
      const next = this.scenario.beats.find((b) => !this.beatsDone.has(b.id) && b.when(this));
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
      // A booth conversation ends when its caller has moved on (sent to the desk and taken, or
      // dropped): the rest of what they would have said is never said.
      if (line.heard === 'booth' && this.boothOver(line.who)) {
        this.beatsDone.add(b.beat.id);
        this.beat = null;
        b.beat.after?.(this, now);
        this.version++;
        return;
      }
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

  /** A caller's booth is no longer where they are: they are on air, or gone from the queue. */
  private boothOver(id: string): boolean {
    const c = this.callers.find((x) => x.identity === id);
    const p = this.person(id);
    if (!c) return this.scenario.callers[id] !== undefined;
    return !!p?.on_air || (c.state !== 'screening' && !p?.cue && c.state !== 'waiting');
  }

  /** A line can be said now: a booth line by staff always, a caller's booth line while they are
   *  screened or cued, an air line while its speaker is open or in the presenter's cue. */
  private canHear(line: Line): boolean {
    if (line.heard === 'booth') {
      const c = this.callers.find((x) => x.identity === line.who);
      if (!c) return true;
      const p = this.person(line.who);
      return c.dropped_ms === null && (c.state === 'screening' || !!p?.cue);
    }
    const p = this.person(line.who);
    return this.open(line.who) || (!!p && p.present && p.cue);
  }

  // ---- callers ----

  /** A caller rings at `at` (they join the queue then). */
  ring(id: string, at: number): void {
    if (this.person(id) || this.rings.has(id)) return;
    this.rings.add(id);
    this.at(at, (s, now) => {
      s.rings.delete(id);
      s.arrive(id, now);
    });
  }

  /** A caller joins the queue now, from the player page, with their consent to record. Below
   *  Callers the page says the lines are closed, so nobody can. */
  arrive(id: string, now: number): void {
    const who = this.scenario.callers[id];
    if (!who || this.person(id)) return;
    if (hiddenSeat(this.step, 'caller')) return;
    this.people.push(person(id, who.name, 'caller'));
    this.lobby.set(id, { name: who.name, topic: who.topic, consent: { at_ms: now - 20_000, version: '2026-11-01', by: 'caller' } });
    this.callers.push({ identity: id, name: who.name, state: 'waiting', since_ms: now, arrived_ms: now, line: '', note: '', name_on_air: '', declined: null, dropped_ms: null, taken: false });
    this.seat();
    this.version++;
  }

  /** The desk's invite dialog (`POST /api/v1/invites`): the answer a real station gives, with a
   *  link that opens nothing here. If the scenario is waiting for someone of that role (its
   *  `later`), it is their invite: they join a few seconds on, under their own name. */
  makeInvite(req: { role?: unknown; name?: unknown; for_me?: unknown }, now: number): Invite {
    const typed = typeof req.name === 'string' ? req.name.trim() : '';
    if ([...typed].length > 120) throw new Refusal(422, 'a name is at most 120 characters');
    if (req.for_me === true) {
      // The Solo DJ's own link (ADR-0274): a presenter's invitation for the chair you sit in,
      // allowed at every step. The identity is the account's own, so the same link is made again.
      if (req.role !== undefined && req.role !== 'presenter') throw new Refusal(422, 'an invitation for yourself is a presenter\'s');
      this.clock = now;
      const you = this.scenario.you;
      const pass = `sandbox-me-${now.toString(36)}`;
      return { pass, identity: `presenter-me-${you.identity}`, role: 'presenter', name: typed || you.name, expires_at_ms: now + INVITE_TTL_MS, link: `/join/#${pass}` };
    }
    const role = req.role;
    if (role === undefined) throw new Refusal(422, 'an invitation is for a guest, a remote DJ or a presenter: say which, or ask for one for yourself');
    if (role !== 'dj' && role !== 'guest' && role !== 'presenter') {
      throw new Refusal(422, `an invitation is for a guest, a remote DJ or a presenter; a ${String(role)} joins another way`);
    }
    // The show's step may have switched the seat off (ADR-0271).
    const hidden = hiddenSeat(this.step, role);
    if (hidden) throw new Refusal(422, hidden);
    const name = typed;
    this.clock = now;
    const id = (this.scenario.later ?? []).find(
      (x) => !this.person(x) && !this.has(`invited:${x}`) && this.scenario.cast.find((c) => c.identity === x)?.role === role,
    );
    if (id) {
      this.mark(`invited:${id}`);
      this.at(now + INVITE_TO_JOIN_MS, (s, t) => s.join(id, t));
    }
    const n = ++this.invites;
    const pass = `sandbox-${role}-${n}-${now.toString(36)}`;
    return { pass, identity: `${role}-sandbox${n}`, role, name, expires_at_ms: now + INVITE_TTL_MS, link: `/join/#${pass}` };
  }

  /** Someone from the cast who was not in the room joins now, from their invitation link (a
   *  guest, a remote DJ, a presenter): seated by their role, a guest or a presenter on air the
   *  moment they join, a DJ off air on strip 5. The pass is checked again when it is used: if the
   *  step has since hidden their seat they stay out, and their link is good for later. */
  join(id: string, now: number): void {
    const who = this.scenario.cast.find((c) => c.identity === id);
    if (!who || this.person(id)) return;
    if (hiddenSeat(this.step, who.role)) return;
    this.clock = now;
    this.people.push(person(id, who.name, who.role, { on_air: who.role === 'guest' || who.role === 'presenter' }));
    this.seat();
    const slot = this.slots.indexOf(id) + 1;
    const first = who.name.split(' ')[0];
    this.notice(
      'desk',
      'good',
      'person.joined',
      id,
      `${first} joined the room`,
      who.role === 'dj' ? `Off air under Not on air${slot ? `, on strip ${slot}` : ''}: cue them to hear them; ⇧⏎ brings them on air with you.` : `On air, on strip ${slot || '–'}.`,
    );
    this.version++;
  }

  // ---- links (the desk's view of each person's connection) ----

  /** A person's link is losing packets: the desk says LINK DEGRADED (they stay in the room). */
  degrade(id: string, on: boolean, now: number): void {
    const p = this.person(id);
    if (!p) return;
    this.clock = now;
    p.linked = !on;
    this.version++;
  }

  /** A person's link drops: the desk keeps them 30 s (the tile says so); a caller's place in
   *  the queue is held for 5 min, and the screener is told (studio.rs `sync`). */
  dropLink(id: string, now: number): void {
    const p = this.person(id);
    if (!p || !p.present) return;
    this.clock = now;
    p.present = false;
    p.linked = false;
    this.away.set(id, now);
    const c = this.callers.find((x) => x.identity === id);
    if (c && c.dropped_ms === null) {
      c.dropped_ms = now;
      this.notice('screener', 'warning', 'caller.dropped', id, `${this.short(id)}'s link dropped`, 'Their place in the queue is held for 5 min; if they call in again they come back where they were.');
    }
    this.seat();
    this.version++;
  }

  /** They are back: the link clears, a held place is taken up where it was (the caller's state
   *  is as it was: a ready caller comes back ready). */
  returnLink(id: string, now: number): void {
    this.clock = now;
    let p = this.person(id);
    const c = this.callers.find((x) => x.identity === id);
    if (!p) {
      const who = c ? { name: c.name, role: 'caller' as Role } : this.scenario.cast.find((x) => x.identity === id);
      if (!who) return;
      p = person(id, who.name, who.role);
      this.people.push(p);
    }
    p.present = true;
    p.linked = true;
    this.away.delete(id);
    if (c) c.dropped_ms = null;
    this.seat();
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
    // A slot is kept while its person is in the desk's list (a dropped link keeps them there
    // for a while) and, for a caller, while they are on air or ready and not dropped.
    this.slots = this.slots.map((id) => {
      const p = id ? this.person(id) : undefined;
      if (!p) return null;
      if (p.role === 'caller') return ready(p.identity) || p.on_air ? id : null;
      return preferred(p.role).length ? id : null;
    });
    for (const p of this.people) {
      if (this.slots.includes(p.identity)) continue;
      const eligible = p.role === 'caller' ? p.on_air || ready(p.identity) : preferred(p.role).length > 0;
      if (!eligible) continue;
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
    // What the show's step has switched off (steps.rs `command_refusal`), in the station's words.
    const at = STEP_WORD[this.step];
    if (c.op === 'dump' && !atLeast(this.step, 'callers')) throw new Refusal(409, `Dump is off at ${at}: there is nothing for it to do. It comes on at Callers.`);
    if (c.op === 'on_air' && c.on && this.person(c.identity)?.role === 'dj' && !atLeast(this.step, 'full')) throw new Refusal(409, `The handover is off at ${at}: it comes on at Full.`);
    if (c.op === 'picture' && !atLeast(this.step, 'full')) throw new Refusal(409, `The picture is held on the slate at ${at}: switching it comes with Full.`);
    if (c.op === 'dump') {
      if (this.delay.delay_ms < 100 && !this.delay.rebuilding) throw new Refusal(409, 'The delay is off; there is nothing to dump.');
      this.delay = { ...this.delay, delay_ms: 0, rebuilding: true, dumps: this.delay.dumps + 1, dumped_at_ms: now };
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
        this.backs++;
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
        this.away.delete(rec.identity);
        this.drops[rec.identity] = (this.drops[rec.identity] ?? 0) + 1;
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
    this.away.delete(p.identity);
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
        this.fades++;
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
    // A show's own cart sounds only in that show (a cart wall per show, 1.0.2).
    if (cart.show && cart.show !== slugOf(this.scenario.show)) throw new Refusal(409, OTHER_SHOW(cart.label));
    const pool = cart.mode === 'bed' ? 'bed' : 'shot';
    if (cue) this.cartCues++;
    else this.fired.push(id);
    // A bed replaces the bed that plays (one bed player); a shot plays beside the others.
    if (pool === 'bed') for (const s of this.sounding.filter((x) => x.pool === 'bed' && x.cue === cue && !x.stopping)) this.stopCart(s, now, 800);
    // A player of the pool that is free (the cart wall keys its list by player and pool).
    const used = new Set(this.sounding.filter((x) => x.pool === pool).map((x) => x.player));
    let player = 1;
    while (used.has(player)) player++;
    this.sounding.push({
      cart: id,
      label: cart.label,
      pool,
      player,
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

  /** The station's mode, a playout setting on a real station (ADR-0045): in auto mode
   *  automation holds the air and the lamp says AUTO (ADR-0183). */
  setMode(mode: StationMode, now: number): void {
    this.clock = now;
    if (this.mode === mode) return;
    this.mode = mode;
    if (mode === 'auto') this.notice('desk', 'warning', 'mode.auto', null, 'Automation holds the air', 'The station is in auto mode: listeners hear the log, and anything you open says OPEN, not ON AIR.');
    else this.notice('desk', 'good', 'mode.live', null, 'The station is live', 'The desk reaches listeners again: open faders say ON AIR.');
    this.version++;
  }

  /** The show's step as the view carries it: the scenario's show is the one on air, and the
   *  chair you sit in may change it. */
  stepView(): StepView {
    return { step: this.step, show: { slug: slugOf(this.scenario.show), name: this.scenario.show }, on_air: true, next: null, may_change: true };
  }

  /** Change the show's step (`PUT /api/v1/shows/{show}/step`). It takes effect at once: below
   *  Callers every caller who is only waiting is let go (told the lines have closed); a caller
   *  on the strip, in cue or on air stays until dropped, and nobody in the room is removed. */
  setStep(slug: string, step: unknown, now: number): StepChanged {
    if (slug !== slugOf(this.scenario.show)) throw new Refusal(404, `no show \`${slug}\``);
    if (!STEPS.includes(step as ShowStep)) throw new Refusal(422, 'step is `solo`, `guests`, `callers` or `full`');
    this.clock = now;
    const previous = this.step;
    this.step = step as ShowStep;
    if (previous !== this.step) {
      if (!atLeast(this.step, 'callers')) this.releaseWaiting();
      this.notice('desk', 'good', 'step.moved', null, `${this.scenario.you.name.split(' ')[0]} moved ${this.scenario.show} to ${STEP_WORD[this.step]}.`, '');
      this.seat();
      this.version++;
    }
    return { show: slug, name: this.scenario.show, step: this.step, previous, station: this.stepView() };
  }

  /** Let go every caller who is only waiting (studio.rs `releasable`): out of the queue and the
   *  room. Those who hold a strip, or are in cue or on air, stay. */
  private releaseWaiting(): void {
    const go = this.callers.filter((c) => {
      const p = this.person(c.identity);
      return !this.slots.includes(c.identity) && !p?.on_air && !p?.cue;
    });
    for (const c of go) {
      this.callers = this.callers.filter((x) => x !== c);
      this.people = this.people.filter((p) => p.identity !== c.identity);
      this.lobby.delete(c.identity);
      this.away.delete(c.identity);
      this.released.push(c.identity);
    }
  }

  /** The microphone of the browser tab the DJ speaks through. */
  setMic(on: boolean, now: number): void {
    this.clock = now;
    this.mic = on;
    this.version++;
  }

  endShow(now: number): void {
    this.ended = true;
    this.clock = now;
    this.version++;
  }

  /** The sandbox's delay, split as a real station splits it (ADR-0272): the desk's delay and a
   *  listener process holding half a second are read, the rest of the path is the setting. */
  private listenerDelay(): View['listener_delay'] {
    const desk_ms = Math.floor(this.delay.delay_ms / 100) * 100;
    const process_ms = 500;
    const rest_ms = LISTENER_PATH_MS - process_ms;
    return { ms: desk_ms + process_ms + rest_ms, measured: false, desk_ms, process_ms, rest_ms };
  }

  /** The sandbox's listeners: one morning audience, all on the Opus stream. */
  private audience(now: number): NonNullable<View['audience']> {
    const listeners = listenersAt(now, this.started);
    return { listeners, by_mount: [{ mount: '/programme.opus', listeners }], error: null, edges: false };
  }

  // ---- the view ----

  view(now: number): View {
    const cartsMicOpen = this.people.some((p) => p.on_air && !p.mute);
    const ducked = this.ducked();
    const withLevel = (p: DeskPerson): DeskPerson => ({
      ...p,
      away_ms: p.present ? null : now - (this.away.get(p.identity) ?? now),
      lufs: this.levels[p.identity] ?? null,
      lufs_peak: this.levels[p.identity] ?? null,
    });
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
        banned_before: this.bans[c.identity] ?? null,
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
    // The presenter's seat (ADR-0231, as studio.rs): the first presenter while any presenter is
    // on air; with none on air, the first DJ on air (after a handover); else the first presenter.
    const onAir = (role: Role) => this.slots.find((id) => { const p = id ? this.person(id) : undefined; return !!p && p.role === role && p.on_air && p.present; }) ?? null;
    const presenter = onAir('presenter') ? seat('presenter') : (onAir('dj') ?? seat('presenter'));
    const screener = seat('screener');
    return {
      station: STATION,
      now_ms: now,
      show: { title: this.show, rundown: this.scenario.rundown(this.started) },
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
      // The sandbox has no chat service, so nothing is counted.
      chat: { ...this.modes, influx: !!this.beforeInflux, influx_since_ms: this.influxSince, held: null, open_reports: null },
      notices: [...this.notices],
      carts: { version: this.catalogue.version, desk: true, desk_error: null, sounding: this.sounding.map((s) => ({ ...s })), ducked, mic_open: cartsMicOpen, missing: [], importing: null },
      listener_delay: this.listenerDelay(),
      default_fallback: false,
      audience: this.audience(now),
      mode: this.mode,
      step: this.stepView(),
    };
  }
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

/** The sandbox's first bank: The Morning Kettle's own (a cart wall per show, 1.0.2), its top row
 *  F1 to F6; then the pack's four, the station's. Every scenario's show is The Morning Kettle. */
const KETTLE_BANK = ['jingle-1', 'bed-1', 'applause-1', 'rimshot-1', 'news-sting-1', 'sad-trombone-1', 'jingle-2', 'bed-2', 'laugh-1', 'cheer-1', 'drumroll-1', 'pips-1', 'phone-ring-1', 'bagpipe-1', 'whoosh-1', 'ding-1'];

export function catalogueFrom(pack: { banks: Record<string, string>; carts: PackCart[] }, show = slugOf('The Morning Kettle')): Catalogue {
  const banks: Bank[] = [{ number: 1, name: 'The Morning Kettle', colour_slot: 1, show }];
  for (const [n, name] of Object.entries(pack.banks)) banks.push({ number: Number(n) + 1, name, colour_slot: Number(n) + 1, show: null });
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
    show: bank === 1 ? show : null,
  });
  const carts: Cart[] = [];
  KETTLE_BANK.forEach((id, i) => {
    const p = pack.carts.find((c) => c.id === id);
    if (p) carts.push(cart(p, 1, i + 1));
  });
  for (const p of pack.carts) carts.push(cart(p, p.suggested.bank + 1, p.suggested.slot, `${p.id}@${p.suggested.bank + 1}`));
  return { version: 1, show: null, banks, carts, loudness_lufs: -16 };
}

/** The catalogue as `GET /api/v1/carts?show=` answers: the station's banks and carts, and that
 *  show's own beside them. */
export function catalogueFor(cat: Catalogue, show: string | null): Catalogue {
  const shown = (x: { show?: string | null }) => (x.show ?? null) === null || x.show === show;
  return { ...cat, show, banks: cat.banks.filter(shown), carts: cat.carts.filter(shown) };
}

/** What the station says when a show's own cart is fired outside its show (`OtherShow`). */
export const OTHER_SHOW = (label: string): string => `“${label}” is in another show's own bank: it fires only while that show is the one on air`;

/** The pack's file for a cart id (a cart on two banks has two ids). */
export function fileOf(cat: Catalogue, id: string): string | null {
  return cat.carts.find((c) => c.id === id)?.file ?? null;
}
