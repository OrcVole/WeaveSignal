// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { defineConfig } from 'vitest/config';
import base from './vite.config.ts';

// The monitor page alone, built for its driver (tests/monitor-page/shoot.mjs) with a stand-in in
// livekit-client's place, so the page's own behaviour round the room (the press, the level bar,
// the volume, a refused seat, a dropped room) can be driven without a room. Never the build a
// station serves: it goes to target/monitor-page/dist, and `pnpm build` does not read this file.
const here = (p: string) => decodeURIComponent(new URL(p, import.meta.url).pathname);

export default defineConfig({
  ...base,
  resolve: { alias: { 'livekit-client': here('../../tests/monitor-page/fake-livekit.js') } },
  build: {
    ...base.build,
    outDir: here('../../target/monitor-page/dist'),
    emptyOutDir: true,
    rollupOptions: { input: { monitor: here('monitor/index.html') } },
  },
});
