// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Pictures and documents posted in the chat, in the studio (ADR-0262): the switch, the queue's
// entries, the statement's words, the full-size view's registration.
import { render } from 'svelte/server';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { panel } from '../../../packages/ui/panels.ts';
import type { Attachment, HeldAvatar } from '../../../packages/ui/chat.ts';
import { attachmentUrl, sizeWords } from '../../../packages/ui/chat.ts';
import ChatPictures from './settings/ChatPictures.svelte';
import Needs from './screener/Needs.svelte';
import { chat } from './lib/chat.svelte.ts';
import { EMPTY_MOD, applyMod, needsList, statementPreview } from './lib/chatModel.ts';
import { attachmentsView, attachmentsWords, keptKind, setAttachments } from './lib/chatPictures.ts';

const pic: Attachment = { id: 'p-1', kind: 'picture', name: 'running order.png', mime: 'image/png', label: 'PNG', bytes: 210_000, width: 1600, height: 900, thumb_width: 480, thumb_height: 270 };
const doc: Attachment = { id: 'd-1', kind: 'document', name: 'rota.ods', mime: 'application/vnd.oasis.opendocument.spreadsheet', label: 'OpenDocument spreadsheet', bytes: 18_432 };
const account = { id: 'a-1', name: 'Ailsa Grant', since_ms: 0, standing: 'trusted' as const, guest: false, restricted: null, avatar: null };
const waiting = (a: Attachment, body: string, at: number): HeldAvatar => ({ id: a.id, account, uploaded_ms: at, bytes: a.bytes, kind: a.kind, attachment: a, message: { id: 7, body } });

describe('the switch for pictures and documents', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('is off as installed, follows the switch once used, and says so', () => {
    expect(attachmentsView({}, false)).toEqual({ on: false, chosen: false });
    expect(attachmentsView({ attachments: true }, false)).toEqual({ on: true, chosen: true });
    expect(attachmentsWords({ on: false, chosen: false })).toContain('words only');
    expect(attachmentsWords({ on: true, chosen: true })).toContain('waits for staff or a moderator');
  });

  it('changes one field of the settings and keeps the rest', async () => {
    const doc = { terms: 'Be kind.', avatar_uploads: true, attachments: null, evidence_viewer: null };
    const sent: unknown[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, init?: RequestInit) => {
        const body = init?.body ? JSON.parse(String(init.body)) : undefined;
        if (body) sent.push(body);
        return new Response(JSON.stringify(body ?? doc), { status: 200 });
      }),
    );
    expect(await setAttachments(true)).toEqual({ on: true, chosen: true });
    expect(sent).toEqual([{ ...doc, attachments: true }]);
  });

  it('is in the panel, with what it means for staff', () => {
    const { body } = render(ChatPictures, { props: { initial: { on: true, chosen: false }, initialAttachments: { on: false, chosen: false } } });
    expect(body).toMatch(/data-chat="attachments-switch"/);
    expect(body).not.toMatch(/data-chat="attachments-switch"[^>]*checked/);
    expect(body).toContain('will see every picture and document');
    expect(body).toContain('risk assessment');
    expect(body).toContain('CHAT_ATTACHMENTS');
    expect(panel('settings.chat-pictures').what).toContain('documents');
  });

  it('names a kept one by what it is', () => {
    expect(keptKind({ kind: 'document', file_name: 'rota.ods' })).toBe('Document “rota.ods”');
    expect(keptKind({ kind: 'picture', file_name: null })).toBe('Picture posted in the chat');
    expect(keptKind({})).toBe('Avatar picture');
  });
});

describe('the queue', () => {
  it('holds pictures and documents beside avatars, oldest first, and lets them go', () => {
    let s = applyMod(EMPTY_MOD, { t: 'mod_avatar', a: waiting(doc, '', 5_000) });
    s = applyMod(s, { t: 'mod_avatar', a: waiting(pic, "Tonight's running order", 2_000) });
    expect(needsList(s).map((e) => e.key)).toEqual(['avatar:p-1', 'avatar:d-1']);
    s = applyMod(s, { t: 'mod_avatar_done', id: 'p-1' });
    expect(needsList(s).map((e) => e.key)).toEqual(['avatar:d-1']);
  });

  it('shows a document by its name, type and size, never inline, with what it can carry', () => {
    chat.link = 'live';
    chat.canModerate = true;
    chat.mod = { ...EMPTY_MOD, avatars: [waiting(doc, 'The rota for December', 1_000), waiting(pic, '', 2_000)] };
    const { body } = render(Needs);
    expect(body).toContain('posted a document');
    expect(body).toContain('rota.ods');
    expect(body).toContain('OpenDocument spreadsheet · 18 KB');
    expect(body).toContain('data-chat="attachment-save"');
    expect(body).toContain('macros or scripts');
    expect(body).toContain('The rota for December');
    expect(body).toContain('posted a picture');
    expect(body).toContain('No words with it: the picture is the message.');
    expect(body).not.toContain('wants this as their avatar');
    chat.mod = EMPTY_MOD;
  });
});

describe('the statement and the view', () => {
  it('names a posted picture or document, not an avatar', () => {
    const facts = { station: 'Kettle FM', show: 'The Late Line', at: null, reason: 'spam' as const, then: 'none' as const, automated: false, info: null };
    const refused = statementPreview({ ...facts, subject: 'avatar-refuse', posted: 'document' });
    expect(refused).toContain('Kettle FM did not show the document you posted in The Late Line');
    expect(refused).not.toContain('avatar');
    const removed = statementPreview({ ...facts, subject: 'avatar-remove', posted: 'picture', reason: 'suspected_csam' });
    expect(removed).toContain("removed the picture you posted for breaking the station's rules on illegal content");
    expect(removed).not.toContain('at least a year');
    expect(statementPreview({ ...facts, subject: 'avatar-remove', posted: 'picture' })).toContain('removed the picture you posted in The Late Line, and the message with it');
  });

  it('serves each from its own route, and says sizes in words', () => {
    expect(attachmentUrl(pic, 'thumb')).toBe('/api/v1/chat/pictures/p-1/thumb');
    expect(attachmentUrl(doc)).toBe('/api/v1/chat/files/d-1');
    expect(sizeWords(840)).toBe('840 bytes');
    expect(sizeWords(2.4 * 1024 * 1024)).toBe('2.4 MB');
    expect(panel('screener.attachment').what).toContain('full size');
  });
});
