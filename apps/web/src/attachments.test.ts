// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Pictures and documents in the chat, on the player (ADR-0262): who sees the attach control, the
// caps said before sending, a message's thumbnail and a document's card, the waiting state, and
// Report this.
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import type { Attachment, Msg } from '../../../packages/ui/chat.ts';
import Message from './lib/chat/Message.svelte';
import ReportDialog from './lib/chat/ReportDialog.svelte';
import { attachTooLarge, canAttach } from './lib/chat/model.ts';

const pic: Attachment = { id: 'p-1', kind: 'picture', name: 'running order.png', mime: 'image/png', label: 'PNG', bytes: 210_000, width: 1600, height: 900, thumb_width: 480, thumb_height: 270 };
const doc: Attachment = { id: 'd-1', kind: 'document', name: 'rota.ods', mime: 'application/vnd.oasis.opendocument.spreadsheet', label: 'OpenDocument spreadsheet', bytes: 18_432 };
const msg = (over: Partial<Msg>): Msg => ({
  id: 4, kind: 'chat', body: "Tonight's running order", author: { id: 'a-ailsa', name: 'Ailsa Grant', standing: 'trusted', guest: false }, reply_to: null,
  server_ms: 0, playhead_ms: null, state: 'visible', pinned: false, featured: false, reactions: [], ...over,
});
const props = { myName: 'Hamish Murray', mine: false, canReact: true, canReply: true, onreact: () => {}, onreply: () => {}, onreport: () => {} };

describe('who may attach', () => {
  it('a trusted chatter signed in by email, where the station has switched it on', () => {
    expect(canAttach({ attachments: true }, { kind: 'email', standing: 'trusted' })).toBe(true);
    expect(canAttach({ attachments: true }, { kind: 'email', standing: 'moderator' })).toBe(true);
    expect(canAttach({ attachments: true }, { kind: 'email', standing: 'member' })).toBe(false);
    expect(canAttach({ attachments: true }, { kind: 'guest', standing: 'trusted' })).toBe(false);
    expect(canAttach({ attachments: false }, { kind: 'email', standing: 'trusted' })).toBe(false);
    expect(canAttach({}, { kind: 'email', standing: 'trusted' })).toBe(false);
  });

  it('says a file is over the cap before sending it', () => {
    const caps = { max_picture_kb: 10240, max_document_kb: 5120 };
    expect(attachTooLarge({ size: 6 * 1024 * 1024, type: 'application/pdf' }, caps)).toBe('That document is larger than this chat takes (5 MB at most).');
    expect(attachTooLarge({ size: 6 * 1024 * 1024, type: 'image/jpeg' }, caps)).toBeNull();
    expect(attachTooLarge({ size: 1, type: '' }, undefined)).toBeNull();
  });
});

describe('a message with a picture or document', () => {
  it('shows a picture as a thumbnail that opens it', () => {
    const { body } = render(Message, { props: { ...props, m: msg({ attachment: pic }) } });
    expect(body).toContain('data-chat="attachment-thumb"');
    expect(body).toContain('src="/api/v1/chat/pictures/p-1/thumb"');
    expect(body).toContain('Open the picture running order.png full size');
  });

  it('shows a document by its name, type and size, as a download only', () => {
    const { body } = render(Message, { props: { ...props, m: msg({ body: '', attachment: doc }) } });
    expect(body).toContain('rota.ods');
    expect(body).toContain('OpenDocument spreadsheet · 18 KB');
    expect(body).toMatch(/href="\/api\/v1\/chat\/files\/d-1"[^>]*download/);
    expect(body).not.toContain('<iframe');
    expect(body).not.toContain('<object');
    expect(body).not.toContain('class="body"');
  });

  it('says it waits for the station, with its own preview, to its author', () => {
    const { body } = render(Message, { props: { ...props, mine: true, preview: 'blob:local', m: msg({ state: 'held', attachment: pic }) } });
    expect(body).toContain('Waiting for the station');
    expect(body).toContain('src="blob:local"');
    expect(body).not.toContain('/api/v1/chat/pictures/p-1/thumb');
  });

  it('can be reported as what it carries', () => {
    const info = { station: 'Kettle FM', reasons: [{ reason: 'abuse', words: 'Abuse or harassment', hint: '' }] } as never;
    const withDoc = render(ReportDialog, { props: { info, session: 's', target: msg({ attachment: doc }), opensOn: 'attachment', onclose: () => {} } }).body;
    expect(withDoc).toContain('data-chat="report-attachment-choice"');
    expect(withDoc).toContain('The document in this message, “rota.ods”');
    const without = render(ReportDialog, { props: { info, session: 's', target: msg({}), onclose: () => {} } }).body;
    expect(without).not.toContain('report-attachment-choice');
  });
});
