// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The figures of tests/cc: one run's summary from the guest's lines, and the table of many.
//
//   node summarise.mjs run RECV_FILE key=value ...   one JSON line (called by run-cc.sh)
//   node summarise.mjs table MATRIX_JSONL            the table (the last line of a name wins)
import { readFileSync } from 'node:fs';

const [mode, file, ...pairs] = process.argv.slice(2);
const lines = readFileSync(file, 'utf8').split('\n').filter((l) => l.startsWith('{'));
const med = (xs) => (xs.length ? xs.slice().sort((a, b) => a - b)[Math.floor(xs.length / 2)] : null);
const round = (x, n = 0) => Number(x.toFixed(n));

function agg(rows, key) {
  const a = rows.map((r) => r[key]).filter(Boolean);
  if (!a.length) return null;
  const o = { intervals: a.length };
  if (key === 'audio') {
    Object.assign(o, {
      kbps_median: med(a.map((x) => x.kbps)),
      gap_pct_median: round(100 * med(a.map((x) => x.gap)), 1),
      gap_pct_max: round(100 * Math.max(...a.map((x) => x.gap)), 1),
      lost: a.reduce((s, x) => s + x.lost, 0),
      packets: a.reduce((s, x) => s + x.packets, 0),
      jitter_ms_max: Math.max(...a.map((x) => x.jitter_ms)),
      silent_intervals: a.filter((x) => x.energy <= 0).length,
    });
  } else {
    Object.assign(o, {
      kbps_median: med(a.map((x) => x.kbps)),
      kbps_min: Math.min(...a.map((x) => x.kbps)),
      kbps_max: Math.max(...a.map((x) => x.kbps)),
      fps_median: med(a.map((x) => x.fps)),
      fps_min: Math.min(...a.map((x) => x.fps)),
      sizes: [...new Set(a.map((x) => x.size))].sort(),
      freezes: a.reduce((s, x) => s + x.freezes, 0),
      nack: a.reduce((s, x) => s + x.nack, 0),
      pli: a.reduce((s, x) => s + x.pli, 0),
    });
  }
  return o;
}

if (mode === 'run') {
  const k = Object.fromEntries(pairs.map((p) => [p.slice(0, p.indexOf('=')), p.slice(p.indexOf('=') + 1)]));
  const n = (x) => Number(x);
  const rows = lines.map((l) => JSON.parse(l)).filter((r) => 't' in r);
  const dt = n(k.dt);
  // What the desk's uplink carried since the impairment went on: the bytes that left it, and the
  // packets netem or tbf dropped (at the packets' mean size), so the offered rate is the sum.
  const [db, dp, dd] = [n(k.desk_bytes), n(k.desk_packets), n(k.desk_dropped)];
  const [gp, gd] = [n(k.lk_packets), n(k.lk_dropped)];
  const mean = dp ? db / dp : 0;
  const out = {
    name: k.name, gcc: k.gcc, video: k.video === '1', extra_guests: n(k.guests), desk_binary: k.bin,
    desk_link: { netem: k.desk_netem, rate: k.desk_rate }, guest_link: { netem: k.guest_netem, rate: k.guest_rate },
    desk_uplink_kbps_sent: Math.round((db * 8) / dt / 1000),
    desk_uplink_kbps_offered: Math.round(((db + dd * mean) * 8) / dt / 1000),
    desk_uplink_dropped_pct: dp + dd ? round((100 * dd) / (dp + dd), 1) : 0,
    desk_cpu_pct_of_a_core: round(((n(k.cpu1) - n(k.cpu0)) / n(k.hz) / dt) * 100, 1),
    guest_downlink_dropped_pct: gp + gd ? round((100 * gd) / (gp + gd), 1) : 0,
    desk_cpu_quota_ms_per_100: k.desk_quota, livekit_cpu_quota_ms_per_100: k.lk_quota, busy_camera: !!k.busy,
    // Sampled mid-run (before) and after the guest has left (after: it races a return's teardown).
    desk_threads: [n(k.thr0), n(k.thr1)],
    warn_lines: n(k.warn_lines), session_dumps_with_rtpgccbwe: n(k.dumps),
    before: Object.fromEntries(['audio', 'video'].map((m) => [m, agg(rows.filter((r) => r.phase !== 'after'), m)])),
    after: Object.fromEntries(['audio', 'video'].map((m) => [m, agg(rows.filter((r) => r.phase === 'after'), m)])),
  };
  console.log(JSON.stringify(out));
} else if (mode === 'table') {
  const runs = new Map();
  for (const l of lines) {
    const r = JSON.parse(l);
    runs.set(r.name, r);
  }
  const f = (x, d = 0) => (x === null || x === undefined ? '-' : typeof x === 'number' ? x.toFixed(d) : String(x));
  console.log("uplink = everything the desk sent: every return in the room (the presenter's too) and the picture, plus signalling; audio and video columns are what the one guest's browser received; thr = threads sampled mid-run");
  console.log(`${'scenario'.padEnd(30)} ${'uplink kbps sent/offered'.padStart(25)} ${'drop%'.padStart(6)} | audio after: kbps  gap% med/max  lost/pkts | video after: kbps (min-max) fps  size  freeze nack pli | cpu% thr`);
  for (const [name, r] of runs) {
    const a = r.after?.audio ?? {};
    const v = r.after?.video;
    const audio = `${f(a.kbps_median).padStart(5)} ${f(a.gap_pct_median, 1).padStart(5)}/${f(a.gap_pct_max, 1).padEnd(5)} ${a.lost ?? '-'}/${a.packets ?? '-'}`;
    const video = v ? `${f(v.kbps_median).padStart(5)} (${f(v.kbps_min)}-${f(v.kbps_max)}) ${f(v.fps_median).padStart(3)} ${(v.sizes ?? []).join(',').padStart(9)} ${f(v.freezes).padStart(3)} ${f(v.nack).padStart(4)} ${f(v.pli).padStart(3)}` : '-';
    console.log(`${name.padEnd(30)} ${`${r.desk_uplink_kbps_sent}/${r.desk_uplink_kbps_offered}`.padStart(25)} ${String(r.desk_uplink_dropped_pct).padStart(6)} | ${audio.padEnd(34)} | ${video.padEnd(44)} | ${String(r.desk_cpu_pct_of_a_core).padStart(5)} ${r.desk_threads[0]}`);
  }
} else {
  console.error('run RECV key=value ... | table MATRIX_JSONL');
  process.exit(2);
}
