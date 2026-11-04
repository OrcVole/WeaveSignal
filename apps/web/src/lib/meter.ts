// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The microphone check's level: RMS of a block as dBFS, and its place on a −60 to 0 dB bar.

/** The block's RMS level in dBFS; silence is −Infinity. */
export function dbfs(block: Float32Array): number {
  let sum = 0;
  for (const x of block) sum += x * x;
  const rms = Math.sqrt(sum / Math.max(1, block.length));
  return rms > 0 ? 20 * Math.log10(rms) : -Infinity;
}

/** A level's fill on a −60 to 0 dBFS bar, 0 to 100. */
export function barPct(db: number): number {
  if (!Number.isFinite(db)) return 0;
  return Math.round(Math.min(100, Math.max(0, ((db + 60) / 60) * 100)));
}

/** Whether the microphone is heard at all: above −50 dBFS at its loudest recently. */
export function heard(peakDb: number): boolean {
  return peakDb > -50;
}
