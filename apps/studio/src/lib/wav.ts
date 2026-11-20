// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A recording made in the window, as a WAV file (16-bit, one channel): the one format every
// browser engine can make without an encoder of its own, and one the station's media worker
// reads. A browser's own recorder gives WebM in some engines, which the station does not read.

/** Float samples in [-1, 1], in blocks, as a 16-bit mono WAV file. */
export function encodeWav(blocks: readonly Float32Array[], sampleRate: number): Uint8Array {
  const frames = blocks.reduce((n, b) => n + b.length, 0);
  const out = new DataView(new ArrayBuffer(44 + frames * 2));
  const text = (at: number, s: string) => [...s].forEach((c, i) => out.setUint8(at + i, c.charCodeAt(0)));
  text(0, 'RIFF');
  out.setUint32(4, 36 + frames * 2, true);
  text(8, 'WAVE');
  text(12, 'fmt ');
  out.setUint32(16, 16, true);
  out.setUint16(20, 1, true); // PCM
  out.setUint16(22, 1, true); // one channel
  out.setUint32(24, sampleRate, true);
  out.setUint32(28, sampleRate * 2, true);
  out.setUint16(32, 2, true);
  out.setUint16(34, 16, true);
  text(36, 'data');
  out.setUint32(40, frames * 2, true);
  let at = 44;
  for (const b of blocks) {
    for (let i = 0; i < b.length; i++, at += 2) {
      const x = Math.max(-1, Math.min(1, b[i]));
      out.setInt16(at, Math.round(x < 0 ? x * 0x8000 : x * 0x7fff), true);
    }
  }
  return new Uint8Array(out.buffer);
}

/** Whether this window can record from a microphone at all (the packaged app's Linux webview
 *  has long shipped without it). */
export function canRecord(g: typeof globalThis = globalThis): boolean {
  const ctx = (g as unknown as { AudioContext?: unknown; webkitAudioContext?: unknown }).AudioContext ?? (g as unknown as { webkitAudioContext?: unknown }).webkitAudioContext;
  return typeof g.navigator?.mediaDevices?.getUserMedia === 'function' && typeof ctx === 'function';
}

export interface Recorder {
  /** Stop, and give the recording. */
  stop(): Promise<{ file: Blob; ms: number }>;
  /** Stop and throw the sound away. */
  cancel(): void;
}

/** Start recording from the microphone, with the browser's own processing off (a recording of
 *  words should be the voice as it is). Rejects when the microphone is refused. */
export async function record(): Promise<Recorder> {
  const stream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false, channelCount: 1 } });
  const Ctx = (globalThis as unknown as { AudioContext?: typeof AudioContext; webkitAudioContext?: typeof AudioContext }).AudioContext ?? (globalThis as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
  const ctx = new Ctx();
  const from = ctx.createMediaStreamSource(stream);
  const tap = ctx.createScriptProcessor(4096, 1, 1);
  const blocks: Float32Array[] = [];
  tap.onaudioprocess = (e) => blocks.push(new Float32Array(e.inputBuffer.getChannelData(0)));
  // The tap must reach the output to be run; it writes nothing there, so nothing is heard.
  const silent = ctx.createGain();
  silent.gain.value = 0;
  from.connect(tap);
  tap.connect(silent);
  silent.connect(ctx.destination);
  const end = () => {
    tap.disconnect();
    from.disconnect();
    for (const t of stream.getTracks()) t.stop();
    void ctx.close();
  };
  return {
    async stop() {
      end();
      const bytes = encodeWav(blocks, ctx.sampleRate);
      const frames = blocks.reduce((n, b) => n + b.length, 0);
      return { file: new Blob([bytes.buffer as ArrayBuffer], { type: 'audio/wav' }), ms: Math.round((frames / ctx.sampleRate) * 1000) };
    },
    cancel: end,
  };
}
