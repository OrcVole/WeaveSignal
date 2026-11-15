// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// A stand-in control plane for the New show driver (shoot.mjs): Kettle FM on Friday 13 November
// 2026, answering the routes of ADR-0278 in the shapes the studio app reads (apps/studio/src/
// lib/api.ts and the API reference). It works out a rule's dates,
// its sentence and its clashes, which in the product only the control plane does; this copy is
// limited to what the driver's shows use, and is test tooling, not a second implementation.
// Sample shows and people are the designs' (ADR-0071).

const ZONE = 'Europe/London';
const DAY = 86_400_000;
const WD = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];
const WD3 = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
const MON = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
const NTH = { 1: 'first', 2: 'second', 3: 'third', 4: 'fourth', '-1': 'last' };
const pad = (n) => String(n).padStart(2, '0');
const D = (iso) => Date.UTC(+iso.slice(0, 4), +iso.slice(5, 7) - 1, +iso.slice(8, 10));
const iso = (ms) => new Date(ms).toISOString().slice(0, 10);
const wd = (ms) => ((new Date(ms).getUTCDay() + 6) % 7) + 1;
const dim = (y, m) => new Date(Date.UTC(y, m + 1, 0)).getUTCDate();
const hm = (n) => `${pad(Math.floor((((n % 1440) + 1440) % 1440) / 60))}:${pad(((n % 1440) + 1440) % 60)}`;

const fmt = new Intl.DateTimeFormat('en-GB', { timeZone: ZONE, hourCycle: 'h23', year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
function wall(ms) {
  const p = Object.fromEntries(fmt.formatToParts(new Date(ms)).map((x) => [x.type, x.value]));
  return Date.UTC(+p.year, +p.month - 1, +p.day, +p.hour % 24, +p.minute);
}
/** The moment the station's wall clock reads a date and minutes after midnight, in epoch seconds. */
export function instant(date, minutes) {
  const want = D(date) + minutes * 60_000;
  let guess = want;
  for (let i = 0; i < 3; i++) guess += want - wall(guess);
  return Math.round(guess / 1000);
}

function nthWeekday(y, m, nth, weekday) {
  if (nth === -1) {
    const last = Date.UTC(y, m, dim(y, m));
    return last - ((wd(last) - weekday + 7) % 7) * DAY;
  }
  const first = Date.UTC(y, m, 1);
  const at = first + ((weekday - wd(first) + 7) % 7) * DAY + (nth - 1) * 7 * DAY;
  return new Date(at).getUTCMonth() === m ? at : null;
}

/** The rule's own dates from its start, in order, before skips and moves; at most `limit`. */
function pattern(rule, limit = 600) {
  const out = [];
  const start = D(rule.starts_on);
  const r = rule.repeat;
  const until = rule.end.kind === 'until' ? D(rule.end.until) : Infinity;
  const push = (ms) => {
    if (ms >= start && ms <= until && out.length < limit) out.push(ms);
  };
  if (r.kind === 'once') push(start);
  else if (r.kind === 'dates') for (const d of [...r.dates].sort()) push(D(d));
  else if (r.kind === 'weekly') {
    const week0 = start - (wd(start) - 1) * DAY;
    for (let w = 0; out.length < limit && w < 2000; w += r.every) {
      for (const d of r.weekdays) push(week0 + w * 7 * DAY + (d - 1) * DAY);
      if (week0 + w * 7 * DAY > until) break;
    }
  } else if (r.kind === 'monthly_nth' || r.kind === 'monthly_day') {
    const s = new Date(start);
    for (let i = 0; out.length < limit && i < 1200; i += r.every) {
      const y = s.getUTCFullYear() + Math.floor((s.getUTCMonth() + i) / 12);
      const m = (s.getUTCMonth() + i) % 12;
      const at = r.kind === 'monthly_nth' ? nthWeekday(y, m, r.nth, r.weekday) : r.day === -1 ? Date.UTC(y, m, dim(y, m)) : r.day <= dim(y, m) ? Date.UTC(y, m, r.day) : null;
      if (at !== null) push(at);
      if (Date.UTC(y, m, 1) > until) break;
    }
  } else if (r.kind === 'yearly') {
    for (let y = new Date(start).getUTCFullYear(); out.length < limit && y < 2400; y++) {
      if (r.day <= dim(y, r.month - 1)) push(Date.UTC(y, r.month - 1, r.day));
      if (Date.UTC(y, 0, 1) > until) break;
    }
  }
  return out;
}

/** The dates that air and the dates skipped, in order, each with its real start. `count`
 *  counts episodes, so a skipped date does not use one up (ADR-0278). */
export function occurrences(rule, limit = 600) {
  const out = [];
  let made = 0;
  const cap = rule.end.kind === 'count' ? rule.end.count : Infinity;
  for (const ms of pattern(rule, limit)) {
    if (made >= cap) break;
    const on = iso(ms);
    if (rule.skip.includes(on)) {
      out.push({ on, skipped: true, start: instant(on, rule.start), minutes: rule.minutes ?? 60, moved_from: null });
      continue;
    }
    const move = rule.moves.find((m) => m.from === on);
    const start = move ? instant(move.to.slice(0, 10), +move.to.slice(11, 13) * 60 + +move.to.slice(14, 16)) : instant(on, rule.start);
    out.push({ on, skipped: false, start, minutes: move?.minutes ?? rule.minutes ?? 60, moved_from: move ? `${on}T${hm(rule.start)}:00` : null });
    made++;
  }
  return out;
}

const dayMonth = (ms, year, today) => `${new Date(ms).getUTCDate()} ${MON[new Date(ms).getUTCMonth()]}${year && new Date(ms).getUTCFullYear() !== new Date(D(today)).getUTCFullYear() ? ` ${new Date(ms).getUTCFullYear()}` : ''}`;
function days(ws) {
  const s = [...ws].sort((a, b) => a - b);
  if (s.length === 7) return null;
  if (s.length >= 3 && s.every((d, i) => i === 0 || d === s[i - 1] + 1)) return `${WD[s[0] - 1]} to ${WD[s.at(-1) - 1]}`;
  if (s.length === 1) return WD[s[0] - 1];
  return `${s.slice(0, -1).map((d) => WD[d - 1]).join(', ')} and ${WD[s.at(-1) - 1]}`;
}

/** The sentence and the short form, in the grammar of new-show.md section 2. */
export function words(rule, today) {
  const r = rule.repeat;
  const end = rule.start + (rule.minutes ?? 60);
  const times = `${hm(rule.start)} to ${hm(end)}${end >= 1440 ? ' the next day' : ''}`;
  const shortTimes = `${hm(rule.start)}–${hm(end)}`;
  const start = D(rule.starts_on);
  let pat;
  let short;
  if (r.kind === 'once') [pat, short] = [`Once, on ${WD[wd(start) - 1]} ${dayMonth(start, true, today)}`, `Once, ${WD3[wd(start) - 1]} ${new Date(start).getUTCDate()} ${MON[new Date(start).getUTCMonth()].slice(0, 3)}`];
  else if (r.kind === 'weekly') {
    const d = days(r.weekdays);
    pat = d === null ? 'Every day' : `Every ${r.every === 1 ? 'week' : `${r.every} weeks`} on ${d}`;
    const s3 = [...r.weekdays].sort((a, b) => a - b).map((x) => WD3[x - 1]);
    short = `${r.every === 1 ? '' : `Every ${r.every} weeks, `}${s3.length > 2 && d?.includes(' to ') ? `${s3[0]}–${s3.at(-1)}` : s3.join(' and ')}`;
  } else if (r.kind === 'monthly_nth') [pat, short] = [`Every ${r.every === 1 ? 'month' : `${r.every} months`} on the ${NTH[r.nth]} ${WD[r.weekday - 1]}`, `${NTH[r.nth][0].toUpperCase()}${NTH[r.nth].slice(1)} ${WD3[r.weekday - 1]} monthly`];
  else if (r.kind === 'monthly_day') [pat, short] = [r.day === -1 ? 'Every month on the last day' : `Every month on the ${r.day}${['th', 'st', 'nd', 'rd'][r.day % 10 > 3 || (r.day > 10 && r.day < 14) ? 0 : r.day % 10]}`, `Day ${r.day === -1 ? 'last' : r.day} monthly`];
  else if (r.kind === 'yearly') [pat, short] = [`Every year on ${r.day} ${MON[r.month - 1]}`, `Yearly, ${r.day} ${MON[r.month - 1].slice(0, 3)}`];
  else [pat, short] = [`On ${r.dates.length} date${r.dates.length === 1 ? '' : 's'} you chose`, `${r.dates.length} chosen dates`];
  let sentence = `${pat}, ${times}`;
  if (r.kind !== 'once' && start >= D(today)) sentence += r.kind === 'dates' ? `, from ${WD[wd(start) - 1]} ${dayMonth(start, true, today)}` : r.kind === 'yearly' ? '' : `, from ${dayMonth(start, true, today)}`;
  if (rule.end.kind === 'until') sentence += `, until ${dayMonth(D(rule.end.until), true, today)}`;
  if (rule.end.kind === 'count') sentence += `, for ${rule.end.count} episodes`;
  sentence += '.';
  if (rule.skip.length && rule.skip.length <= 2) sentence += ` Not on ${rule.skip.map((d) => dayMonth(D(d), true, today)).join(' or ')}.`;
  else if (rule.skip.length) sentence += ` ${rule.skip.length} dates skipped.`;
  const occ = rule.end.kind === 'never' ? [] : occurrences(rule).filter((o) => !o.skipped);
  const lastOn = occ.at(-1)?.start;
  const last = lastOn && r.kind !== 'once' ? `The last is ${new Intl.DateTimeFormat('en-GB', { timeZone: ZONE, weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' }).format(lastOn * 1000).replace(',', '')}.` : null;
  return { sentence, short: `${short}, ${shortTimes}`, last };
}

/** Kettle FM, as the week holds it on Friday 13 November 2026 at 10:00. */
export class Station {
  constructor({ today = '2026-11-13', nowMinutes = 600, version = '1.0.4', role = 'producer' } = {}) {
    this.today = today;
    this.now = instant(today, nowMinutes);
    this.version = version;
    this.role = role;
    this.shows = [];
    this.episodes = [];
    this.calls = [];
    // What the station has told its windows (the studio's view carries the last of them).
    this.notices = [];
    this.noticeId = 0;
    const weekly = (weekdays, start, minutes) => ({ starts_on: '2026-09-07', start, minutes, repeat: { kind: 'weekly', every: 1, weekdays }, end: { kind: 'never' }, skip: [], moves: [] });
    this.add({ slug: 'kettle-breakfast', name: 'Kettle Breakfast', colour_slot: 1, presenter: 'Eilidh Fraser', rule: weekly([1, 2, 3, 4, 5], 420, 180) });
    this.add({ slug: 'firth-sessions', name: 'Firth Sessions', colour_slot: 6, presenter: 'Callum MacLeod', rule: weekly([2], 1260, 60) });
    this.add({ slug: 'saturday-night', name: 'Saturday Night Kettle', colour_slot: 3, presenter: 'Shona Campbell', hosts: ['Shona Campbell', 'Catriona MacKenzie-Buchanan'], rule: weekly([6], 1200, 180) });
    this.add({ slug: 'six-nations-preview', name: 'Six Nations Preview', colour_slot: 9, presenter: 'Hamish Stewart', rule: { starts_on: '2027-03-02', start: 1170, minutes: 60, repeat: { kind: 'once' }, end: { kind: 'never' }, skip: [], moves: [] } });
    this.add({ slug: 'northern-lights', name: 'Northern Lights', colour_slot: 7, presenter: 'Tara Buchanan', rule: null });
    this.add({ slug: 'st-andrews-night-ceilidh', name: 'Harvest Home Ceilidh', colour_slot: 4, presenter: '', rule: { starts_on: '2026-10-31', start: 1140, minutes: 120, repeat: { kind: 'once' }, end: { kind: 'never' }, skip: [], moves: [] } });
    // Kettle Breakfast and Saturday Night Kettle have this week's episodes; the rest wait as dates.
    for (const d of ['2026-11-09', '2026-11-10', '2026-11-11', '2026-11-12', '2026-11-13']) this.draft('kettle-breakfast', d, { aired: d < today });
    this.draft('saturday-night', '2026-11-14');
  }

  /** Who the station says made a change: the first name of whoever is signed in. */
  mover() {
    return this.role === 'presenter' ? 'Ailsa' : 'Shona';
  }

  /** Every window is told a show's rule changed: *Moira changed when The Folk Club airs.* */
  ruleChanged(who, show) {
    this.notices.push({ id: ++this.noticeId, at_ms: this.now * 1000, surface: 'desk', tone: 'good', kind: 'rule.changed', subject: null, title: `${who} changed when ${show} airs.`, message: '' });
    if (this.notices.length > 32) this.notices.shift();
  }

  /** Somebody in another window changes when a show airs. */
  changeElsewhere(who, slug, rule) {
    const s = this.shows.find((x) => x.slug === slug);
    s.rule = rule;
    this.ruleChanged(who, s.name);
  }

  /** The studio's view (apps/studio/src/lib/api.ts `View`): a station with nobody at the desk,
   *  which is all the Schedule tab needs of it, and the notices. */
  studioView() {
    return {
      station: 'Kettle FM',
      now_ms: this.now * 1000,
      show: { title: '', rundown: [] },
      desk: { reachable: false, error: null, playout: { connected: false, for_ms: null }, delay: null, picture: null },
      seats: { presenter: null, screener: null, presenter_name: '', screener_name: '' },
      sources: [],
      callers: [],
      desk_taking_callers: true,
      chat: { slow: false, hold_new: false, links_trusted: false, accounts_only: false, min_age_24h: false, influx: false, influx_since_ms: null, held: null, open_reports: null },
      notices: [...this.notices],
      carts: { version: 1, desk: false, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
      listener_delay: { ms: 0, measured: false },
      default_fallback: false,
      mode: null,
      step: { step: 'full', show: null, on_air: false, next: null, may_change: true },
    };
  }

  add(s) {
    this.shows.push({ id: `id-${s.slug}`, description: '', layout: 'poster', hosts: [], tags: [], explicit: false, feed_guid: null, cards: null, contact: '', links: [], commercial_music: false, presenter_account: null, step: 'full', series: [], ...s });
  }

  /** Draft the episode that fills a rule date. */
  draft(slug, on, o = {}) {
    const s = this.shows.find((x) => x.slug === slug);
    if (!s.series.length) s.series.push({ id: `${slug}-s1`, show_id: s.id, number: 1, title: null, description: null, hosts: null, slot: null, rule: null, starts_on: null, planned_episodes: null, tags: null, cards: null });
    const occ = occurrences(s.rule).find((x) => x.on === on);
    const number = this.episodes.filter((e) => e.slug === slug).length + 1;
    const ep = { id: `${slug}-${number}`, series_id: `${slug}-s1`, slug, number, title: '', description: '', kind: 'live', air_at: occ.start, minutes: null, hosts: null, guests: [], tags: null, aired_at: o.aired ? occ.start + occ.minutes * 60 : null, occurrence: on, frozen: null, files: null, cards: null };
    this.episodes.push(ep);
    return ep;
  }

  slotOf(rule) {
    const r = rule?.repeat;
    return r?.kind === 'weekly' && r.every === 1 && r.weekdays.length === 1 && rule.end.kind === 'never' && !rule.skip.length && !rule.moves.length ? { weekday: r.weekdays[0], start: rule.start, minutes: rule.minutes } : null;
  }

  /** The dates of a show's rule from now on that no episode fills and that are not skipped. */
  unfilled(s) {
    if (!s.rule) return [];
    const filled = new Set(this.episodes.filter((e) => e.slug === s.slug).map((e) => e.occurrence));
    return occurrences(s.rule).filter((o) => !o.skipped && !filled.has(o.on) && o.start + o.minutes * 60 > this.now);
  }

  nextOn(s) {
    const drafted = this.episodes.filter((e) => e.slug === s.slug && e.aired_at === null && e.air_at + 3600 > this.now).sort((a, b) => a.air_at - b.air_at)[0];
    const date = this.unfilled(s)[0];
    const next = [drafted && { on: drafted.occurrence, start: drafted.air_at, minutes: 60 }, date && { on: date.on, start: date.start, minutes: date.minutes }].filter(Boolean).sort((a, b) => a.start - b.start)[0];
    if (next) return { ...next, reason: null, last_on: null };
    if (!s.rule) return { on: null, start: null, minutes: null, reason: 'none', last_on: null };
    const all = occurrences(s.rule).filter((o) => !o.skipped);
    const reason = s.rule.repeat.kind === 'once' ? 'once_passed' : s.rule.end.kind === 'count' ? 'complete' : 'ended';
    return { on: null, start: null, minutes: null, reason, last_on: all.at(-1)?.on ?? null };
  }

  show(s) {
    const { series: _series, ...rest } = s;
    return { ...rest, slot: this.slotOf(s.rule), rule_words: s.rule ? words(s.rule, this.today) : null, next_on: this.nextOn(s) };
  }

  resolved(s, ep = null) {
    return { show: s.name, slug: s.slug, description: s.description, colour_slot: s.colour_slot, layout: s.layout, slot: this.slotOf(s.rule), rule: s.rule, rule_words: s.rule ? words(s.rule, this.today) : null, hosts: s.hosts, guests: ep?.guests ?? [], tags: s.tags, explicit: false, series: { number: 1, title: null, first: null, last: null, episodes: null }, episode: ep ? { id: ep.id, number: ep.number, title: ep.title, description: '', kind: 'live', air_at: ep.air_at, minutes: ep.minutes } : null, inherited: ['hosts', 'tags', 'minutes'], frozen: ep?.aired_at != null };
  }

  view(s) {
    return { show: this.show(s), resolved: this.resolved(s), series: s.series };
  }

  /** Every other show's times that overlap a start and a length by a minute or more. */
  clashes(slug, start, minutes) {
    const out = [];
    for (const o of this.shows) {
      if (o.slug === slug || !o.rule) continue;
      for (const occ of occurrences(o.rule, 400)) {
        if (occ.skipped || occ.start >= start + minutes * 60 || start >= occ.start + occ.minutes * 60) continue;
        const overlap = Math.round((Math.min(start + minutes * 60, occ.start + occ.minutes * 60) - Math.max(start, occ.start)) / 60);
        if (overlap < 1) continue;
        const d = days(o.rule.repeat.weekdays ?? []);
        out.push({ show: o.name, slug: o.slug, start: occ.start, minutes: occ.minutes, overlap_minutes: overlap, when: o.rule.repeat.kind === 'weekly' && d ? `${d.includes(' ') ? d : `${d}s`}, ${hm(o.rule.start)} to ${hm(o.rule.start + o.rule.minutes)}` : null, with: o.hosts.length ? o.hosts : o.presenter ? [o.presenter] : [], refused: this.role === 'presenter' });
        break;
      }
    }
    return out;
  }

  preview({ rule, show = null, count = 10 }) {
    const problems = [];
    const hints = [];
    const kept = { ...rule, skip: [], moves: [] };
    const given = new Set(pattern(kept).map(iso));
    const dropped = { skip: rule.skip.filter((d) => !given.has(d)), moves: rule.moves.filter((m) => !given.has(m.from)).map((m) => m.from) };
    const clean = { ...rule, skip: rule.skip.filter((d) => given.has(d)), moves: rule.moves.filter((m) => given.has(m.from)) };
    if (rule.end.kind === 'until' && rule.end.until < rule.starts_on) problems.push({ field: 'end', message: `It ends on ${dayMonth(D(rule.end.until), true, this.today)}, before its first date, ${dayMonth(D(rule.starts_on), true, this.today)}.` });
    const filled = new Map(this.episodes.filter((e) => e.slug === show).map((e) => [e.occurrence, e]));
    const all = occurrences(clean);
    const live = all.filter((o) => o.skipped || o.start + o.minutes * 60 > this.now);
    if (rule.repeat.kind === 'once' && !live.length) problems.push({ field: 'starts_on', message: 'That time has passed. Pick a time still to come.' });
    else if (all.length && !all[0].skipped && all[0].start + all[0].minutes * 60 <= this.now && live.find((o) => !o.skipped)) {
      hints.push({ field: 'starts_on', message: `The first date has passed. The first episode will be ${WD[wd(D(live.find((o) => !o.skipped).on)) - 1]} ${dayMonth(D(live.find((o) => !o.skipped).on), true, this.today)}.` });
    }
    if (rule.repeat.kind === 'yearly' && rule.repeat.month === 2 && rule.repeat.day === 29) hints.push({ field: 'repeat', message: 'Only in leap years. The next is 29 February 2028.' });
    let n = this.episodes.filter((e) => e.slug === show && !live.some((o) => o.on === e.occurrence)).length;
    const dates = [];
    let airing = 0;
    for (const o of live) {
      if (airing >= count) break;
      const ep = filled.get(o.on);
      if (o.skipped) dates.push({ on: o.on, number: null, start: o.start, minutes: o.minutes, state: 'skipped', now: false, moved_from: null, clock: null, episode: null, clashes: [] });
      else {
        n++;
        airing++;
        const nominal = instant(o.on, rule.start);
        const civil = wall(o.start * 1000) - D(o.on) - rule.start * 60_000;
        dates.push({ on: o.on, number: ep?.number ?? n, start: o.start, minutes: o.minutes, state: ep ? 'filled' : 'airs', now: o.start <= this.now, moved_from: o.moved_from, clock: !o.moved_from && civil !== 0 ? (civil > 0 ? 'forward' : 'back') : !o.moved_from && instant(o.on, rule.start + 60) - nominal === 7200 ? 'back' : null, episode: ep ? { series: 1, number: ep.number, aired: ep.aired_at !== null } : null, clashes: ep ? [] : this.clashes(show, o.start, o.minutes) });
      }
    }
    const total = rule.end.kind === 'never' && !['once', 'dates'].includes(rule.repeat.kind) ? null : live.filter((o) => !o.skipped).length;
    const airs = dates.filter((d) => d.state === 'airs');
    let clear = null;
    if (airs.some((d) => d.clashes.length)) {
      for (let step = 30; step <= 180 && !clear; step += 30) {
        for (const start of [rule.start + step, rule.start - step]) {
          if (start < 0 || start >= 1440) continue;
          if (airs.every((d) => !this.clashes(show, instant(d.on, start), d.minutes).length)) {
            clear = { start, minutes: rule.minutes ?? 60 };
            break;
          }
        }
      }
    }
    const s = this.shows.find((x) => x.slug === show);
    const eps = this.episodes.filter((e) => e.slug === show);
    const change = s
      ? {
          aired: eps.filter((e) => e.aired_at !== null).length,
          since: eps.filter((e) => e.aired_at !== null).map((e) => e.occurrence).sort()[0] ?? null,
          drafted: eps.filter((e) => e.aired_at === null && e.occurrence && !e.by_hand).map((e, i) => {
            const to = airs.filter((d) => !filled.has(d.on) || true)[i] ?? null;
            return { series: 1, number: e.number, from: e.air_at, to: to ? to.start : null, clash: to ? (this.clashes(show, to.start, to.minutes)[0] ?? null) : null };
          }),
          by_hand: eps.filter((e) => e.aired_at === null && e.by_hand).map((e) => ({ series: 1, number: e.number, at: e.air_at })),
        }
      : null;
    return { ...words(clean, this.today), dates, total, clear, problems, hints, dropped, change };
  }

  /** The week's reply: its episodes and every rule's unfilled dates in the window. */
  week(fromMs, toMs) {
    const inside = (start) => start * 1000 >= fromMs && start * 1000 < toMs;
    const episodes = this.episodes.filter((e) => inside(e.air_at)).map((e) => {
      const s = this.shows.find((x) => x.slug === e.slug);
      const { slug: _slug, by_hand: _hand, ...episode } = e;
      return { show: this.show(s), series: s.series[0], episode, resolved: this.resolved(s, e) };
    });
    const dates = this.shows.flatMap((s) => {
      let n = this.episodes.filter((e) => e.slug === s.slug).length;
      return this.unfilled(s).map((o) => ({ show: s.slug, series: s.series[0]?.number ?? null, on: o.on, number: ++n, start: o.start, minutes: o.minutes, moved_from: o.moved_from, clock: null })).filter((d) => inside(d.start));
    });
    return this.version >= '1.0.4' ? { zone: ZONE, episodes, dates } : { zone: ZONE, episodes };
  }

  /** One request: `[status, body]`. */
  handle(method, path, body) {
    this.calls.push({ method, path, body });
    const url = new URL(path, 'http://station');
    const p = url.pathname;
    if (p === '/health') return [200, { status: 'ok', version: this.version, database: 'ok' }];
    if (p === '/api/v1/sessions/me') return [200, this.role === 'presenter' ? { kind: 'staff', role: 'presenter', account: { id: 'a-ailsa', username: 'ailsa', name: 'Ailsa Drummond', role: 'presenter', disabled_ms: null, created_ms: 1 } } : { kind: 'staff', role: 'producer', account: { id: 'a-shona', username: 'shona', name: 'Shona Campbell', role: 'producer', disabled_ms: null, created_ms: 1 } }];
    if (p === '/api/v1/shows' && method === 'GET') return [200, this.shows.map((s) => this.show(s))];
    if (p === '/api/v1/schedule/episodes') return [200, this.week(+url.searchParams.get('from_ms'), +url.searchParams.get('to_ms'))];
    if (p === '/api/v1/schedule/rule/preview') return [200, this.preview(body)];
    if (p === '/api/v1/schedule/clocks') return [200, { clocks: [{ id: 'day', name: 'Daytime mix', slots: [] }] }];
    if (p === '/api/v1/schedule/week') return [200, { hours: [] }];
    if (p === '/api/v1/playout/settings') return [200, { time_zone: ZONE, artist_separation_min: 60, title_separation_min: 120, album_separation_min: 30, mode: 'auto', default_clock: 'day' }];
    if (p === '/api/v1/playout/zone') return [200, { zone: ZONE, set: true }];
    let m;
    if ((m = /^\/api\/v1\/shows\/([^/]+)$/.exec(p))) {
      let s = this.shows.find((x) => x.slug === m[1]);
      if (method === 'GET') return s ? [200, this.view(s)] : [404, { error: 'no such show' }];
      // Delete show (ADR-0279): an admin or a producer, a show none of whose episodes has aired.
      if (method === 'DELETE') {
        if (this.role === 'presenter') return [403, { error: 'Only an admin or a producer deletes a show.' }];
        if (!s) return [404, { error: `show \`${m[1]}\` does not exist` }];
        const mine = this.episodes.filter((e) => e.slug === s.slug);
        const aired = mine.filter((e) => e.aired_at !== null).length;
        if (aired) return [409, { error: `${s.name} has aired ${aired} episode${aired === 1 ? '' : 's'}, so it stays as the station's record: only a show that has never aired can be deleted.` }];
        this.shows = this.shows.filter((x) => x !== s);
        this.episodes = this.episodes.filter((e) => e.slug !== s.slug);
        return [200, { slug: s.slug, name: s.name, series: s.series.length, episodes: mine.length, cart_banks: 0, carts: 0, files_deleted: 0 }];
      }
      const { rule, draft_first, slot: _slot, ...fields } = body;
      if (/^the the/i.test(fields.name ?? '')) return [422, { error: 'The name does not fit the card at its smallest size: shorten it.', field: 'name' }];
      if (draft_first && rule) {
        const first = occurrences(rule).find((o) => !o.skipped && o.start + o.minutes * 60 > this.now);
        const clash = first && this.clashes(m[1], first.start, first.minutes)[0];
        if (clash && this.role === 'presenter') return [409, { error: `${clash.show} has that time.`, field: 'rule', clash }];
      }
      if (!s) {
        this.add({ slug: m[1], ...fields, rule: rule ?? null });
        s = this.shows.at(-1);
      } else Object.assign(s, fields, rule !== undefined ? { rule } : {});
      let drafted = null;
      if (draft_first && s.rule) {
        const first = this.unfilled(s)[0];
        if (first) {
          const ep = this.draft(s.slug, first.on);
          const { slug: _slug, ...episode } = ep;
          drafted = { episode, resolved: this.resolved(s, ep) };
        }
      }
      return [200, { ...this.view(s), drafted }];
    }
    if ((m = /^\/api\/v1\/shows\/([^/]+)\/rule$/.exec(p))) {
      const s = this.shows.find((x) => x.slug === m[1]);
      const moved = [];
      if (body.rule && body.drafted === 'move') {
        const dates = occurrences(body.rule).filter((o) => !o.skipped && o.start + o.minutes * 60 > this.now);
        this.episodes.filter((e) => e.slug === s.slug && e.aired_at === null).forEach((e, i) => {
          if (!dates[i] || dates[i].start === e.air_at) return;
          moved.push({ series: 1, number: e.number, from: e.air_at, to: dates[i].start });
          Object.assign(e, { air_at: dates[i].start, occurrence: dates[i].on });
        });
      }
      s.rule = body.rule;
      // As the control plane does (crates/control/src/rules.rs): the window that saved is told too.
      this.ruleChanged(this.mover(), s.name);
      return [200, { rule: s.rule, rule_words: s.rule ? words(s.rule, this.today) : null, moved }];
    }
    if ((m = /^\/api\/v1\/shows\/([^/]+)\/series\/(\d+)$/.exec(p))) {
      const s = this.shows.find((x) => x.slug === m[1]);
      if (method === 'PUT' && !s.series.length) s.series.push({ id: `${s.slug}-s1`, show_id: s.id, number: 1, title: null, description: null, hosts: null, slot: null, rule: null, starts_on: null, planned_episodes: null, tags: null, cards: null });
      return [200, { series: s.series[0], resolved: this.resolved(s), episodes: this.episodes.filter((e) => e.slug === s.slug).map((e) => ({ episode: e, resolved: this.resolved(s, e) })) }];
    }
    if ((m = /^\/api\/v1\/shows\/([^/]+)\/series\/(\d+)\/episodes$/.exec(p))) {
      const s = this.shows.find((x) => x.slug === m[1]);
      const made = [];
      for (let i = 0; i < (body.count ?? 1); i++) {
        const next = this.unfilled(s)[0];
        if (!next) break;
        const ep = this.draft(s.slug, next.on);
        made.push({ episode: ep, resolved: this.resolved(s, ep) });
      }
      return [200, made];
    }
    if (p === '/api/v1/retention') return [200, { setting: { us_statutory_licence: false } }];
    return [404, { error: `the stand-in station has no ${method} ${p}` }];
  }
}
