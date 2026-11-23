// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { defineConfig } from 'vitest/config';
import base from './vite.config.ts';

// The join page alone, built for its layout driver (tests/m3/layout.mjs) with a stand-in in
// livekit-client's place, so every state of a call (the rehearsal studio, the move, standing by)
// can be laid out and shot without a room. Never the build a station serves: it goes to
// target/join-standin/dist, and `pnpm build` does not read this file.
const here = (p: string) => decodeURIComponent(new URL(p, import.meta.url).pathname);

export default defineConfig({
  ...base,
  resolve: { alias: { 'livekit-client': here('../../tests/m3/fake-room.js') } },
  build: {
    ...base.build,
    outDir: here('../../target/join-standin/dist'),
    emptyOutDir: true,
    rollupOptions: { input: { join: here('join/index.html') } },
  },
});
