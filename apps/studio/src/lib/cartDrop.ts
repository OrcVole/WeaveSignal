// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A sound dragged onto the studio window (1.2.1). In a browser the page hears the drop and holds
// the file. In the packaged app the webview takes a file drop itself, so the page's own `drop`
// carries no file: Tauri tells the page the paths and where the pointer was, and the app's Rust
// side reads the one file (`sound_read`: a regular file with a sound's ending, no larger than
// the station takes). Either way the default is always stopped, or a file that missed its mark
// would be opened in place of the studio.

import { packaged } from './api.ts';

export type DropEvent =
  | { kind: 'over' }
  | { kind: 'leave' }
  /** `file` is the first file dropped; `problem` is why it could not be read; `at` is the
   *  element under the pointer. `more` counts the other files dropped with it, which are left. */
  | { kind: 'drop'; file: File | null; problem: string | null; at: Element | null; more: number };

/** A path's last part, on either kind of system. */
export function baseName(path: string): string {
  return path.replace(/^.*[\\/]/, '');
}

/** Read a dropped file in the packaged app: its bytes, under its own name. */
export async function readDropped(path: string, maxMb: number): Promise<File> {
  const { invoke } = await import('@tauri-apps/api/core');
  try {
    const bytes = await invoke<ArrayBuffer>('sound_read', { path, maxMb });
    return new File([bytes], baseName(path));
  } catch (e) {
    throw new Error(typeof e === 'string' && e ? e : e instanceof Error && e.message ? e.message : 'The app could not read that file.');
  }
}

/** Listen for files dragged over the window and dropped on it, until the answer is called.
 *  `maxMb` is asked for at the drop, so it is the station's limit as it is then. */
export function listenForDrops(handler: (e: DropEvent) => void, maxMb: () => number): () => void {
  const app = packaged();
  const hasFiles = (e: DragEvent) => [...(e.dataTransfer?.types ?? [])].includes('Files');
  const onOver = (e: DragEvent) => {
    e.preventDefault();
    if (!app && hasFiles(e)) handler({ kind: 'over' });
  };
  const onLeave = (e: DragEvent) => {
    if (!e.relatedTarget) handler({ kind: 'leave' });
  };
  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    // The packaged app is told the paths by Tauri (below); the page's own event has no file.
    if (app) return;
    const files = [...(e.dataTransfer?.files ?? [])];
    if (!files.length) return handler({ kind: 'leave' });
    handler({ kind: 'drop', file: files[0], problem: null, at: e.target instanceof Element ? e.target : null, more: files.length - 1 });
  };
  window.addEventListener('dragover', onOver);
  window.addEventListener('dragleave', onLeave);
  window.addEventListener('drop', onDrop);
  let stopTauri: (() => void) | null = null;
  let gone = false;
  if (app) {
    void import('@tauri-apps/api/webview')
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent((ev) => {
          const p = ev.payload;
          if (p.type === 'leave') return handler({ kind: 'leave' });
          if (p.type !== 'drop') return handler({ kind: 'over' });
          if (!p.paths.length) return handler({ kind: 'leave' });
          // Tauri's position is in the screen's own pixels; the page's are its CSS pixels.
          const ratio = globalThis.devicePixelRatio || 1;
          const at = document.elementFromPoint(p.position.x / ratio, p.position.y / ratio);
          const more = p.paths.length - 1;
          readDropped(p.paths[0], maxMb())
            .then((file) => handler({ kind: 'drop', file, problem: null, at, more }))
            .catch((err: unknown) => handler({ kind: 'drop', file: null, problem: err instanceof Error ? err.message : String(err), at, more }));
        }),
      )
      .then((stop) => {
        if (gone) stop();
        else stopTauri = stop;
      })
      .catch(() => {
        // No webview to listen on (a test, an older shell): the file chooser still works.
      });
  }
  return () => {
    gone = true;
    window.removeEventListener('dragover', onOver);
    window.removeEventListener('dragleave', onLeave);
    window.removeEventListener('drop', onDrop);
    stopTauri?.();
  };
}
