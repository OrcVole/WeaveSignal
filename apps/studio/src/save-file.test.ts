// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Saving a file from the Keys tab (1.3.2): the names the page gives the app's save command, what
// the screen says for each way a save ends, and that the page hands over no path.
import { describe, expect, it } from 'vitest';
import capability from '../src-tauri/capabilities/default.json';
import mainRs from '../src-tauri/src/main.rs?raw';
import rust from '../src-tauri/src/save.rs?raw';
import keysTab from './keys/Keys.svelte?raw';
import makeMany from './keys/MakeMany.svelte?raw';
import sendKey from './keys/SendKey.svelte?raw';
import helper from './lib/saveFile.ts?raw';
import { fileStem } from './lib/keys.ts';
import { EXPORT_NOTED, SAVE_NAME, saveLabel, saveText, savedWords } from './lib/saveFile.ts';

describe('the names the page gives', () => {
  it('are plain names the app’s save command takes: lower-case letters, digits and hyphens, .csv or .svg', () => {
    for (const label of ['Patrons 2026', 'Fèis Ìle / listeners', '  ', '../../etc', 'C:\\keys', 'Mòrag’s friends']) {
      expect(`keys-${fileStem(label)}.csv`).toMatch(SAVE_NAME);
      expect(`leak-report-${fileStem(label)}.csv`).toMatch(SAVE_NAME);
      expect(`key-${fileStem(label, 'qr')}.svg`).toMatch(SAVE_NAME);
    }
    expect('leak-report.csv').toMatch(SAVE_NAME);
  });

  it('is the rule the Rust side enforces, so a name refused there is refused here', () => {
    for (const bad of ['', '.csv', 'keys', 'keys.txt', '../keys.csv', 'a/b.csv', 'a\\b.csv', '/etc/x.csv', 'Keys.csv', 'a b.csv', 'a.b.csv', '-rf.csv', 'keys.CSV', `${'a'.repeat(101)}.csv`]) expect(bad).not.toMatch(SAVE_NAME);
    expect(`${'a'.repeat(100)}.csv`).toMatch(SAVE_NAME);
    // The same sentence in both files, so a change to one is seen against the other.
    expect(rust).toContain('b.is_ascii_lowercase() || b.is_ascii_digit() || b == b\'-\'');
    expect(rust).toContain('const MAX_STEM: usize = 100;');
  });
});

describe('the command', () => {
  it('is given a name and the text, and no path: the dialog’s answer is the only path written', () => {
    const sig = rust.slice(rust.indexOf('pub async fn save_file('), rust.indexOf(') -> Result<Option<String>, String>'));
    expect(sig).toContain('name: String');
    expect(sig).toContain('text: String');
    expect(sig).not.toMatch(/path|dir/i);
    expect(helper).toContain("invoke<string | null>('save_file', { name, text })");
  });

  it('is registered, and the window is granted none of the dialog plugin’s own commands', () => {
    expect(mainRs).toContain('save::save_file,');
    expect(capability.permissions).toEqual(['core:default']);
  });
});

describe('what the screen says', () => {
  it('names the place in the packaged app, and the downloads folder in a browser', () => {
    expect(savedWords('The leak report', { how: 'saved', path: '/home/isla/Documents/leak-report.csv' })).toBe('The leak report is saved as /home/isla/Documents/leak-report.csv.');
    expect(savedWords('The leak report', { how: 'downloaded', name: 'leak-report.csv' })).toBe('The leak report is saved as leak-report.csv, where your browser keeps downloads.');
  });

  it('says a closed dialog saved nothing, and what the station noted all the same', () => {
    expect(savedWords('The list', { how: 'cancelled' })).toBe('Not saved: the save dialog was closed.');
    expect(savedWords('Patrons: every key with its link', { how: 'cancelled' }, EXPORT_NOTED)).toBe('Not saved: the save dialog was closed. The station has still noted on each key that its text went out.');
    // Said only when nothing was saved: a saved file's sentence is whole without it.
    expect(savedWords('Patrons', { how: 'saved', path: '/tmp/keys-patrons.csv' }, EXPORT_NOTED)).not.toContain('noted');
  });

  it('marks a button that opens a dialog with an ellipsis, in the packaged app only', () => {
    // A test runs as the browser build does: no dialog, so no ellipsis.
    expect(saveLabel('Export CSV')).toBe('Export CSV');
    expect(typeof saveText).toBe('function');
  });
});

describe('the Keys tab’s three saves', () => {
  it('all go through the one helper, and none is hidden in the packaged app any more', () => {
    for (const [f, s] of Object.entries({ Keys: keysTab, MakeMany: makeMany, SendKey: sendKey })) {
      expect(s, f).toContain('saveText(');
      expect(s, f).not.toContain('createObjectURL');
      expect(s, f).not.toMatch(/#if !packaged\(\)/);
      expect(s, f).not.toContain('Copy as CSV\' : ');
    }
  });
});
