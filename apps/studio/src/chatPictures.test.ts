// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Settings > Chat pictures (ADR-0259): where the switch stands in words, that a change keeps the
// rest of the chat's settings, and the panel rendered from a view.
import { render } from 'svelte/server';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { panel } from '../../../packages/ui/panels.ts';
import ChatPictures from './settings/ChatPictures.svelte';
import { SOR_CSAM, SOR_REASONS, reasonLabel, sorReasonsFor, statementPreview } from './lib/chatModel.ts';
import { EVIDENCE_DELETE_WORDS, keptWords, setUploads, uploadsView, uploadsWords, viewerWords, type Kept, type KeptPicture } from './lib/chatPictures.ts';

describe('the switch', () => {
  it('follows the install setting until it is used', () => {
    expect(uploadsView({}, true)).toEqual({ on: true, chosen: false });
    expect(uploadsView({ avatar_uploads: null }, false)).toEqual({ on: false, chosen: false });
    expect(uploadsView({ avatar_uploads: false }, true)).toEqual({ on: false, chosen: true });
    expect(uploadsView({ avatar_uploads: true }, false)).toEqual({ on: true, chosen: true });
  });

  it('says where it stands, and that waiting pictures are not touched', () => {
    expect(uploadsWords({ on: true, chosen: false })).toContain('as the station was installed');
    expect(uploadsWords({ on: true, chosen: true })).toContain('waits for a moderator');
    expect(uploadsWords({ on: false, chosen: true })).toContain('already waiting or showing stay as they are');
    expect(uploadsWords({ on: false, chosen: false })).toContain('as the station was installed');
  });
});

describe('a change', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('reads the whole document and puts it back with one field changed', async () => {
    const doc = { terms: 'Be kind.', accountable_person: 'Rory Bruce', guests: true, blocked_words: ['x'], avatar_uploads: null };
    const calls: { method: string; url: string; body?: unknown }[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, init?: RequestInit) => {
        const body = init?.body ? JSON.parse(String(init.body)) : undefined;
        calls.push({ method: init?.method ?? 'GET', url, body });
        return new Response(JSON.stringify(body ?? doc), { status: 200 });
      }),
    );
    const v = await setUploads(false);
    expect(v).toEqual({ on: false, chosen: true });
    expect(calls.map((c) => c.method)).toEqual(['GET', 'PUT']);
    expect(calls[1]!.url).toContain('/api/v1/chat/admin/settings');
    expect(calls[1]!.body).toEqual({ ...doc, avatar_uploads: false });
  });
});

describe('the panel', () => {
  it('is registered under its name', () => {
    expect(panel('settings.chat-pictures').what).toContain('uploaded pictures');
  });

  it('shows the switch as it stands and lets an admin use it', () => {
    const { body } = render(ChatPictures, { props: { initial: { on: true, chosen: false } } });
    expect(body).toContain('data-panel="settings.chat-pictures"');
    expect(body).toMatch(/data-chat="uploads-switch"[^>]*checked/);
    expect(body).not.toMatch(/data-chat="uploads-switch"[^>]*disabled/);
    expect(body).toContain('AVATAR_UPLOADS');
  });

  it('shows it off, and tells anyone but an admin to ask', () => {
    const { body } = render(ChatPictures, { props: { initial: { on: false, chosen: true }, admin: false } });
    expect(body).not.toMatch(/data-chat="uploads-switch"[^>]*checked/);
    expect(body).toMatch(/data-chat="uploads-switch"[^>]*disabled/);
    expect(body).toContain('Ask your station’s admin');
  });
});

// ---- ADR-0257 ----

const csam: KeptPicture = {
  id: 'b4c1c2f0-0000-4000-8000-000000000001',
  account: { id: 'a1', name: 'Rory MacLeod' },
  reason: 'suspected_csam',
  reason_words: 'Suspected child sexual abuse material',
  uploaded_ms: Date.UTC(2026, 10, 9, 18, 0),
  decided_ms: Date.UTC(2026, 10, 10, 9, 30),
  decided_by: 'Moira Dunn',
  keep_until_ms: Date.UTC(2027, 10, 10, 9, 30),
  evidence: true,
  has_original: true,
};
const rubbish: KeptPicture = { ...csam, id: 'b4c1c2f0-0000-4000-8000-000000000002', account: { id: 'a2', name: 'Ailsa Kerr' }, reason: 'intimate_image', reason_words: 'An intimate image shared without consent', keep_until_ms: null, evidence: false, has_original: true };
const admins = [
  { id: 's1', username: 'elspeth', name: 'Elspeth Gordon', role: 'admin' as const, disabled_ms: null, created_ms: 0 },
  { id: 's2', username: 'calum', name: 'Calum Reid', role: 'producer' as const, disabled_ms: null, created_ms: 0 },
  { id: 's3', username: 'morven', name: 'Morven Reid', role: 'admin' as const, disabled_ms: 5, created_ms: 0 },
];
const list = (you: boolean): Kept => ({
  pictures: [csam, rubbish],
  evidence_viewer: { id: 's1', name: 'Elspeth Gordon' },
  you_are_the_evidence_viewer: you,
});

describe('the reason for suspected child sexual abuse material', () => {
  it('is offered for a picture and never for a message', () => {
    expect(sorReasonsFor(false)).toEqual(SOR_REASONS);
    expect(sorReasonsFor(true).at(-1)).toBe(SOR_CSAM);
    expect(SOR_REASONS.some((r) => r.reason === 'suspected_csam')).toBe(false);
  });

  it('is read as the reason it is', () => {
    expect(reasonLabel('suspected_csam', null)).toBe('suspected child sexual abuse');
  });

  it('tells the uploader less: the rules on illegal content and the review, nothing more', () => {
    const facts = { subject: 'avatar-refuse' as const, station: 'Kettle FM', show: '', at: null, then: 'none' as const, automated: false, info: null };
    const said = statementPreview({ ...facts, reason: 'suspected_csam' });
    expect(said).toBe("Kettle FM removed the picture you uploaded for breaking the station's rules on illegal content. A moderator decided. You can ask for a review within 14 days.");
    expect(said).not.toContain('child');
    const other = statementPreview({ ...facts, reason: 'abuse' });
    expect(other).toContain('it broke our terms');
    expect(other).not.toContain('at least a year');
  });
});

describe('the pictures kept', () => {
  it('says why each is kept and, for evidence, until when', () => {
    expect(keptWords(csam)).toBe('Suspected child sexual abuse material, by Moira Dunn on 10 Nov 2026. Kept until at least 10 Nov 2027.');
    expect(keptWords(rubbish)).toContain('Kept until its evidence hold is released.');
  });

  it('says who sees the evidence', () => {
    expect(viewerWords({ id: 's1', name: 'Elspeth Gordon' }, false)).toBe('Elspeth Gordon is the one person who can see pictures kept as evidence.');
    expect(viewerWords({ id: 's1', name: 'Elspeth Gordon' }, true)).toContain('You are the one person');
    expect(viewerWords(null, false)).toContain('no admin');
  });

  it('shows the list, the evidence marked, the picker, and View and Export only to the named person', () => {
    const them = render(ChatPictures, { props: { initial: { on: true, chosen: true }, initialKept: list(false), initialStaff: admins } }).body;
    expect(them).toContain('data-chat="kept"');
    expect((them.match(/data-chat="kept-delete"/g) ?? []).length).toBe(2);
    expect(them).toContain('Elspeth Gordon is the one person');
    expect(them).toContain('data-chat="evidence-viewer"');
    // The picker offers admins who are on (they have the screen that shows the evidence), not a producer or an account that is off.
    expect(them).toContain('Elspeth Gordon (elspeth, admin)');
    expect(them).not.toContain('Calum Reid');
    expect(them).not.toContain('Morven Reid');
    expect(them).not.toContain('data-chat="evidence-view"');
    expect(them).not.toContain('data-chat="evidence-export"');
    const you = render(ChatPictures, { props: { initial: { on: true, chosen: true }, initialKept: list(true), initialStaff: admins } }).body;
    const noStaff = render(ChatPictures, { props: { initial: { on: true, chosen: true }, initialKept: list(false), initialStaff: [] } }).body;
    expect(noStaff).toContain('data-chat="kept"');
    expect(noStaff).not.toContain('data-chat="evidence-viewer"');
    expect((you.match(/data-chat="evidence-view"/g) ?? []).length).toBe(1);
    expect((you.match(/data-chat="evidence-export"/g) ?? []).length).toBe(1);
    expect(you).toContain('You are the one person');
  });

  it('says what the law asks before a picture kept as evidence is deleted', () => {
    expect(EVIDENCE_DELETE_WORDS).toContain('report it and then keep it');
  });

  it('says when nothing is kept', () => {
    const { body } = render(ChatPictures, {
      props: { initial: { on: true, chosen: true }, initialKept: { pictures: [], evidence_viewer: null, you_are_the_evidence_viewer: false }, initialStaff: [] },
    });
    expect(body).toContain('data-chat="kept-empty"');
    expect(body).toContain('no admin');
  });
});
