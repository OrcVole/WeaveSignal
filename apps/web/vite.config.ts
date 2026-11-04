// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// The station's public pages (M3): the player with call-in (`/`), the join page for invited
// guests, remote DJs and presenters (`/join/#<pass>`), and the echo test (`/echo-test/`).
// Plain pages, one entry each, so any static server or the station's proxy can serve the build.
//
// In development the dev server passes /api to the control plane named by
// WEAVESIGNAL_CONTROL_URL, *without* any bearer: these pages use only the public routes and the
// person's own pass, as they will through the proxy.
const env = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env ?? {};
const control = env.WEAVESIGNAL_CONTROL_URL;
// The chat service (M6), its own process: /api/v1/chat goes there, socket included.
const chat = env.WEAVESIGNAL_CHAT_URL;
const here = (p: string) => decodeURIComponent(new URL(p, import.meta.url).pathname);

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: Number(env.WEAVESIGNAL_WEB_PORT ?? 17430),
    strictPort: true,
    host: '127.0.0.1',
    // The shared tokens and state rules live in packages/ui, the glyph sprite in the studio app.
    fs: { allow: [here('../..')] },
    proxy: {
      ...(chat ? { '/api/v1/chat': { target: chat, ws: true } } : {}),
      ...(control ? { '/api': { target: control } } : {}),
    },
  },
  build: {
    target: 'es2022',
    outDir: 'dist',
    // livekit-client is one chunk of about 590 kB, loaded only when a call starts.
    chunkSizeWarningLimit: 700,
    rollupOptions: {
      input: {
        player: here('index.html'),
        join: here('join/index.html'),
        echo: here('echo-test/index.html'),
      },
    },
  },
  test: { environment: 'node', include: ['src/**/*.test.ts'] },
});
