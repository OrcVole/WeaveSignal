// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Calendar files, the requests (ADR-0281): read a file into proposed shows, make the chosen
// ones, and fetch a calendar file to save. Every request goes the way the rest of the studio's
// do (`netFetch`: from Rust with the station's key when packaged), so the import's POST and
// its body take the same road as a show's save.

import { FieldError, base, headers, netFetch, packaged } from '../../lib/api.ts';
import { DATES_SHOWN, asImportMade, asImportPreview, importRequest, type Chosen, type ImportMade, type ImportPreview } from './import.ts';
import { calendarPath, fileName, isCalendar } from './links.ts';

async function send(path: string, body: unknown): Promise<unknown> {
  const res = await netFetch(`${base()}${path}`, { method: 'POST', headers: headers(true), body: JSON.stringify(body) });
  // A refusal may come without a sentence (a proxy's 413 is a page, not JSON): the status is
  // kept, and `refusalWords` says it.
  const data = (await res.json().catch(() => ({}))) as Record<string, unknown>;
  if (!res.ok) {
    const error = typeof data.error === 'string' && data.error ? data.error : `${res.status} ${res.statusText}`.trim();
    throw new FieldError(res.status, error, typeof data.field === 'string' ? data.field : null, data);
  }
  return data;
}

/** Read a calendar file into proposed shows (`POST /api/v1/schedule/import/preview`). It writes
 *  nothing. 413 over 1 MB or 200 events; 422 for a file that is not a calendar. */
export async function importPreview(ics: string, limit = DATES_SHOWN): Promise<ImportPreview> {
  return asImportPreview(await send('/api/v1/schedule/import/preview', { ics, limit }));
}

/** Make the chosen shows in one transaction (`POST /api/v1/schedule/import`): all, or none. */
export async function importShows(picked: readonly Chosen[]): Promise<ImportMade> {
  return asImportMade(await send('/api/v1/schedule/import', importRequest(picked)));
}

/** A calendar file on this computer, read by the packaged app's own side. */
export interface CalendarFile {
  name: string;
  text: string;
}

/** Read the calendar file at a path the person pasted or dropped (the packaged app only: a
 *  page cannot read a path). The path goes to Rust as it was given, quotes, `~` and all;
 *  `calendar_read` cleans it and reads only an `.ics` of at most 1 MB that begins as a
 *  calendar, so the page can name no other file. A refusal is Rust's own sentence. */
export async function readCalendarAt(path: string): Promise<CalendarFile> {
  const { invoke } = await import('@tauri-apps/api/core');
  try {
    const f = await invoke<CalendarFile>('calendar_read', { path });
    return { name: String(f.name), text: String(f.text) };
  } catch (e) {
    throw new Error(typeof e === 'string' && e ? e : e instanceof Error && e.message ? e.message : 'The app could not read that file.');
  }
}

/** Files dragged over the window and dropped on it, in the packaged app: the webview takes a
 *  file drop itself there, so the page's own `drop` event carries no file, and the paths come
 *  through Tauri's drag-and-drop event instead. Answers the function that stops listening. */
export async function onFilesDropped(handler: (e: { kind: 'over' } | { kind: 'leave' } | { kind: 'drop'; paths: string[] }) => void): Promise<() => void> {
  const { getCurrentWebview } = await import('@tauri-apps/api/webview');
  return getCurrentWebview().onDragDropEvent((ev) => {
    const p = ev.payload;
    if (p.type === 'drop') handler({ kind: 'drop', paths: [...p.paths] });
    else if (p.type === 'leave') handler({ kind: 'leave' });
    else handler({ kind: 'over' });
  });
}

/** The schedule's calendar file, or a show's, as text. Read through the studio's own address
 *  with its sign-in, so it works where the station has no public address; what is saved is the
 *  same file a listener's calendar gets. */
export async function calendarText(show?: string | null): Promise<string> {
  const res = await netFetch(`${base()}${calendarPath(show)}`, { headers: headers(false) });
  if (!res.ok) {
    const data = (await res.json().catch(() => ({}))) as { error?: unknown };
    throw new FieldError(res.status, typeof data.error === 'string' && data.error ? data.error : res.status === 404 ? 'This station has no calendar files yet: it is older than 1.0.5.' : 'The station did not give the calendar file. Try again.', null);
  }
  const text = await res.text();
  if (!isCalendar(text)) throw new Error('The station did not answer with a calendar file. Try again.');
  return text;
}

/** Save a calendar file on this computer. The packaged app writes it into the Downloads folder
 *  from Rust and says where it went (a webview has no download the page can count on); a
 *  browser downloads it as any file. Answers the file's path when it is known. */
export async function saveCalendar(show: string | null | undefined, text: string): Promise<string | null> {
  const name = fileName(show);
  if (packaged()) {
    const { invoke } = await import('@tauri-apps/api/core');
    try {
      return await invoke<string>('calendar_save', { name, text });
    } catch (e) {
      throw new Error(`The file was not saved: ${typeof e === 'string' ? e : e instanceof Error ? e.message : 'the app could not write it'}.`);
    }
  }
  const url = URL.createObjectURL(new Blob([text], { type: 'text/calendar;charset=utf-8' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  a.hidden = true;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
  return null;
}
