// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Import a calendar… in the packaged app (the 1.0.5 test run): a pasted path and a dropped
// file are read by the app's own side, with Tauri's core and its webview replaced by fakes
// that answer as `calendar_read` and the drag-and-drop event do. What the command itself
// reads and refuses is tested in Rust (src-tauri/src/calendar.rs).
import { render } from 'svelte/server';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const calls: { cmd: string; args: Record<string, unknown> }[] = [];
let answer: (cmd: string, args: Record<string, unknown>) => unknown = () => null;
let heard: ((e: { payload: Record<string, unknown> }) => void) | null = null;
let stopped = 0;

vi.mock('@tauri-apps/api/core', () => ({
  invoke: async (cmd: string, args: Record<string, unknown>) => {
    calls.push({ cmd, args });
    return answer(cmd, args);
  },
}));
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async (handler: (e: { payload: Record<string, unknown> }) => void) => {
      heard = handler;
      return () => {
        stopped += 1;
      };
    },
  }),
}));

import ImportCalendar from './schedule/files/ImportCalendar.svelte';
import { onFilesDropped, readCalendarAt } from './schedule/files/station.ts';

const ICS = 'BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n';
const clean = (body: string) => body.replace(/<!--.*?-->/g, '');

beforeEach(() => {
  calls.length = 0;
  heard = null;
  stopped = 0;
  answer = () => null;
  vi.stubGlobal('location', { protocol: 'tauri:', hostname: 'localhost', search: '' });
});
afterEach(() => vi.unstubAllGlobals());

describe('a pasted path, in the packaged app', () => {
  it('the first part has a field for the path, beside the chooser and the drop', () => {
    const body = clean(render(ImportCalendar, { props: { today: '2026-11-15', onMade: () => {}, onOpen: () => {}, onClose: () => {} } }).body);
    expect(body).toContain('>Choose a file…</button>');
    expect(body).toContain('Or drop the file anywhere on this dialog.');
    expect(body).toMatch(/<label for="ic-path">Or paste the file’s path<\/label>/);
    expect(body).toMatch(/<input id="ic-path" type="text"[^>]*placeholder="~\/Downloads\/schedule\.ics"[^>]*spellcheck="false"/);
    // Nothing to read yet: the button waits for a path.
    expect(body).toMatch(/<button type="submit" class="btn" disabled[^>]*>Read the file<\/button>/);
    expect(body).toContain('Quotes around it and');
    expect(body).toContain('Only a file ending');
  });

  it('goes to the app’s own side as it was pasted, quotes and ~ and all, and comes back as the file', async () => {
    answer = () => ({ name: 'schedule (2).ics', text: ICS });
    const raw = " '~/Downloads/schedule (2).ics' ";
    expect(await readCalendarAt(raw)).toEqual({ name: 'schedule (2).ics', text: ICS });
    // The page cleans nothing and names no other command: what may be read is Rust's to say.
    expect(calls).toEqual([{ cmd: 'calendar_read', args: { path: raw } }]);
  });

  it('a refusal is the app’s own sentence', async () => {
    answer = () => {
      throw 'Only a calendar file is read: its name must end in .ics.';
    };
    await expect(readCalendarAt('/etc/passwd')).rejects.toThrow('Only a calendar file is read: its name must end in .ics.');
    answer = () => {
      throw undefined;
    };
    await expect(readCalendarAt('/home/iona/x.ics')).rejects.toThrow('The app could not read that file.');
  });
});

describe('a file dropped on the window, in the packaged app', () => {
  it('comes through Tauri’s event as paths, with the drag over and away from the window', async () => {
    const seen: unknown[] = [];
    const stop = await onFilesDropped((e) => seen.push(e));
    expect(heard).not.toBeNull();
    heard!({ payload: { type: 'enter', paths: ['/home/iona/schedule.ics'], position: { x: 1, y: 1 } } });
    heard!({ payload: { type: 'over', position: { x: 2, y: 2 } } });
    heard!({ payload: { type: 'leave' } });
    heard!({ payload: { type: 'drop', paths: ['/home/iona/schedule.ics'], position: { x: 2, y: 2 } } });
    expect(seen).toEqual([{ kind: 'over' }, { kind: 'over' }, { kind: 'leave' }, { kind: 'drop', paths: ['/home/iona/schedule.ics'] }]);
    stop();
    expect(stopped).toBe(1);
  });
});
