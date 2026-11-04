// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Tauri serves the built files from dist/; in development it loads the dev server.
//
// For the interface in a browser on loopback (the M2 run, tests/m2/), the dev server passes
// /api to the control plane named by WEAVESIGNAL_CONTROL_URL and adds the admin bearer from
// WEAVESIGNAL_CONTROL_TOKEN itself, so the page never holds the token.
const env = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env ?? {};
const control = env.WEAVESIGNAL_CONTROL_URL;
const bearer = env.WEAVESIGNAL_CONTROL_TOKEN;
// The chat service (M6), its own process: /api/v1/chat goes there, socket included, with the
// same bearer (the chat takes the station's admin bearer for the studio's routes).
const chat = env.WEAVESIGNAL_CHAT_URL;
const auth = bearer ? { Authorization: `Bearer ${bearer}` } : {};

// `--mode sandbox` builds the website's "Try the desk" (ADR-0212): the same app with the station
// simulated in the page (src/sandbox/), served at the root of demo.weavesignal.top, its sounds
// from /media/sandbox/ beside it (VITE_SANDBOX_MEDIA to change it).
export default defineConfig(({ mode }) => ({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: Number(env.WEAVESIGNAL_STUDIO_PORT ?? 17420),
    strictPort: true,
    host: '127.0.0.1',
    proxy: {
      ...(chat ? { '/api/v1/chat': { target: chat, ws: true, headers: auth } } : {}),
      ...(control ? { '/api': { target: control, headers: auth } } : {}),
    },
  },
  build: { target: 'es2022', outDir: mode === 'sandbox' ? 'dist-sandbox' : 'dist' },
  test: { environment: 'node', include: ['src/**/*.test.ts'] },
}));
