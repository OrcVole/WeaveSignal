// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The update screen's state (ADR-0015, ADR-0140): what the last check found, where a download
// is, and the three things a person can ask for (download and install, install what is
// downloaded, roll back). The commands are src-tauri/src/update.rs's; the rule for when they
// may start is lib/update.ts's `updateGate`, asked again at every step, so a strip opened
// during a download stops the install that follows it.
//
// Nothing here starts by itself except one check, a short while after the packaged app opens.
// A check only reads the feed; it installs nothing.

import { packaged } from './api.ts';
import { studio } from './studio.svelte.ts';
import { updateGate, type UpdateCheck, type UpdateGate } from './update.ts';

export type UpdatePhase = 'idle' | 'checking' | 'downloading' | 'installing' | 'rolling-back';

export const updates = $state({
  phase: 'idle' as UpdatePhase,
  /** The last answer; null before the first check (and always in a browser build). */
  check: null as UpdateCheck | null,
  checkedAt: null as number | null,
  /** The last failure, in the Rust side's words. */
  error: '',
  /** The download so far. */
  progress: null as { done: number; total: number | null } | null,
  /** The gate said no when the person asked: why. Cleared by the next ask that passes. */
  refused: '',
});

/** Whether the frame should point at an update: one is offered and not rolled back from. */
export function updateNag(): string | null {
  const a = updates.check?.available;
  return a && !a.skipped ? a.version : null;
}

/** The gate for this window, from the desk's view and the held talk keys. */
export function gate(): UpdateGate {
  return updateGate(studio.view, studio.talking);
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

const words = (e: unknown) => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

/** Ask the feed. Quietly nothing in a browser build, which has no updater. */
export async function checkNow(): Promise<void> {
  if (!packaged() || updates.phase !== 'idle') return;
  updates.phase = 'checking';
  updates.error = '';
  try {
    updates.check = await call<UpdateCheck>('update_check');
    updates.checkedAt = Date.now();
  } catch (e) {
    updates.error = `Could not check for updates: ${words(e)}`;
  } finally {
    updates.phase = 'idle';
  }
}

/** Download the update, then install it and restart, if nothing is on air at each step. */
export async function installNow(): Promise<void> {
  const a = updates.check?.available;
  if (!a || updates.phase !== 'idle') return;
  updates.error = '';
  const first = gate();
  updates.refused = first.blocked ?? '';
  if (first.blocked) return;
  try {
    if (!a.downloaded) {
      updates.phase = 'downloading';
      updates.progress = { done: 0, total: null };
      const { Channel } = await import('@tauri-apps/api/core');
      const progress = new Channel<{ done: number; total: number | null }>();
      progress.onmessage = (p) => (updates.progress = p);
      await call('update_download', { progress });
      a.downloaded = true;
    }
    // A show may have started while the file came down.
    const again = gate();
    updates.refused = again.blocked ?? '';
    if (again.blocked) {
      updates.phase = 'idle';
      return;
    }
    updates.phase = 'installing';
    await call('update_install'); // the app restarts; this returns only on a failure
  } catch (e) {
    updates.error = `The update was not installed: ${words(e)}`;
  } finally {
    updates.phase = 'idle';
    updates.progress = null;
  }
}

/** Go back to the version kept before the last update (an AppImage keeps one). */
export async function rollbackNow(): Promise<void> {
  if (!updates.check?.previous || updates.phase !== 'idle') return;
  updates.error = '';
  const g = gate();
  updates.refused = g.blocked ?? '';
  if (g.blocked) return;
  updates.phase = 'rolling-back';
  try {
    await call('update_rollback'); // the app restarts
  } catch (e) {
    updates.error = `Could not roll back: ${words(e)}`;
  } finally {
    updates.phase = 'idle';
  }
}

/** The one check after the packaged app opens, once the window has settled; returns its stop. */
export function installUpdateCheck(delayMs = 20_000): () => void {
  if (!packaged()) return () => {};
  const t = setTimeout(() => void checkNow(), delayMs);
  return () => clearTimeout(t);
}
