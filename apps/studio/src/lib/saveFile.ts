// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Save a text file the page made (keyed shows, 1.3.2): a list of keys as CSV, a key's QR code
// as SVG. In the packaged app the system's own save dialog asks where it goes (`save_file`,
// src-tauri/src/save.rs: the page gives a name and the text, never a path, and is told where
// the file went); a browser downloads it as any file. Closing the dialog is an answer, not a
// failure: nothing was written, and the screen says so.

import { packaged } from './api.ts';

/** How a save ended. */
export type Saved =
  | { how: 'saved'; path: string }
  | { how: 'downloaded'; name: string }
  | { how: 'cancelled' };

/** The names `save_file` takes: lower-case letters, digits and hyphens, then `.csv` or `.svg`
 *  (the same rule as `file_name` in src-tauri/src/save.rs). */
export const SAVE_NAME = /^[a-z0-9][a-z0-9-]{0,99}\.(csv|svg)$/;

const MIME = { csv: 'text/csv;charset=utf-8', svg: 'image/svg+xml' } as const;

/** Save `text` as a file named `name`. Throws a sentence when the app could not write it. */
export async function saveText(name: string, text: string): Promise<Saved> {
  if (packaged()) {
    const { invoke } = await import('@tauri-apps/api/core');
    try {
      const path = await invoke<string | null>('save_file', { name, text });
      return path ? { how: 'saved', path } : { how: 'cancelled' };
    } catch (e) {
      throw new Error(`The file was not saved: ${typeof e === 'string' ? e : e instanceof Error ? e.message : 'the app could not write it'}.`);
    }
  }
  const url = URL.createObjectURL(new Blob([text], { type: MIME[name.endsWith('.svg') ? 'svg' : 'csv'] }));
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  a.hidden = true;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
  return { how: 'downloaded', name };
}

/** What the screen says after a save. `what` begins the sentence (`The leak report`); `after`
 *  is said when the person closed the dialog and something happened all the same (the station
 *  noting that a family's keys went out). */
export function savedWords(what: string, r: Saved, after = ''): string {
  switch (r.how) {
    case 'saved':
      return `${what} is saved as ${r.path}.`;
    case 'downloaded':
      return `${what} is saved as ${r.name}, where your browser keeps downloads.`;
    default:
      return `Not saved: the save dialog was closed.${after ? ` ${after}` : ''}`;
  }
}

/** A family's export is noted by the station when it is read, before anyone is asked where the
 *  file goes: closing the dialog does not undo that, and the screen must not hide it. */
export const EXPORT_NOTED = 'The station has still noted on each key that its text went out.';

/** A button that saves: in the packaged app it opens a dialog, and says so with its ellipsis. */
export const saveLabel = (label: string): string => (packaged() ? `${label}…` : label);
