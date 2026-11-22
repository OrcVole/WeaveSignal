// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Adding and changing carts from the wall (1.2.1): the rules a file and a cart's words are held
// to before anything is sent, whose bank a change goes to and who may make it, the routes each
// change asks, and the wall's own affordances: an empty slot invites a cart, a cart that is not
// sounding has its Edit, and a preparing window, which fires nothing, still adds and edits.
// Every catalogue here is made for the test: the demo pack's carts are another branch's to change.
import { render } from 'svelte/server';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from './lib/api.ts';
import CartWall from './carts/CartWall.svelte';
import { account } from './lib/account.svelte.ts';
import {
  ACCEPT,
  NOT_YOURS,
  addedWords,
  bankNameProblem,
  bankRoute,
  bankWords,
  cartProblem,
  changeOf,
  emptySlots,
  fileProblem,
  labelFromFile,
  labelProblem,
  lengthWords,
  limitsOf,
  loadPack,
  loadWords,
  mayChange,
  ownerOfTab,
  packPlan,
  packView,
  patchCart,
  planAsks,
  planWords,
  putBank,
  putCart,
  rampMs,
  rampText,
  removeCart,
  removeShowBank,
  slotRoute,
  soundLine,
  takesLine,
  uploadQuery,
  type PackView,
} from './lib/cartEdit.ts';
import { addInstead, askSlot, cartEditor, keyAdds, type SlotAsk } from './lib/cartEdit.svelte.ts';
import { baseName } from './lib/cartDrop.ts';
import { studio } from './lib/studio.svelte.ts';
import { setup } from './lib/wizard.svelte.ts';
import type { Bank, Cart, Catalogue } from '../../../packages/ui/carts.ts';
import type { Me, StepView, View } from './lib/api.ts';
import type { Show } from './lib/schedule.ts';

vi.mock('./lib/api.ts', async (orig) => ({ ...(await orig<typeof api>()), netFetch: vi.fn(), catalogue: vi.fn() }));

const NOW = Date.UTC(2026, 10, 21, 10, 0, 0);
const staff = (id: string, name: string, role: Me['role'], preparing = false): Me => ({ kind: 'staff', role, account: { id, username: id, name, role, disabled_ms: null, created_ms: 0 }, preparing });
const FINLAY = staff('acc-finlay', 'Finlay Grant', 'presenter');
const EILIDH = staff('acc-eilidh', 'Eilidh Fraser-MacLeod', 'presenter');
const MOIRA = staff('acc-moira', 'Moira Campbell', 'producer');
const SORCHA = staff('acc-sorcha', 'Sorcha Kerr', 'screener');
const KEY: Me = { kind: 'station', role: 'admin', preparing: false };

const bank = (number: number, name: string, show: string | null, colour_slot = number): Bank => ({ number, name, colour_slot, show });
const cart = (bankN: number, slot: number, label: string, show: string | null, over: Partial<Cart> = {}): Cart => ({
  id: `${show ?? 'station'}-${bankN}-${slot}`,
  bank: bankN,
  slot,
  label,
  detail: '',
  mode: 'one_shot',
  file: `${bankN}-${slot}.flac`,
  duration_ms: 4_200,
  gain_db: -7.2,
  loudness_lufs: -15.8,
  true_peak_dbtp: -1.4,
  ramp_ms: null,
  origin: 'applause.wav',
  licence: '',
  missing: false,
  show,
  ...over,
});
/** Steeped's window: the station's bank 1, and Steeped's own bank 2; tabs 3 to 5 not made. */
const CAT: Catalogue = {
  version: 7,
  show: 'steeped',
  banks: [bank(1, 'Reactions', null), bank(2, 'Dog woofs', 'steeped', 5)],
  carts: [cart(1, 1, 'Applause', null), cart(1, 2, 'Rimshot', null), cart(1, 7, 'Rain bed', null, { mode: 'bed' }), cart(2, 1, 'Woof', 'steeped')],
  loudness_lufs: -23,
  max_upload_mb: 64,
  max_seconds: 600,
};
const L = limitsOf(CAT);
const step = (slug: string | null, may_change: boolean): StepView => ({ step: 'full', show: slug ? { slug, name: slug === 'steeped' ? 'Steeped' : slug } : null, on_air: !!slug, next: null, may_change });
const view = (s: StepView, sounding: View['carts']['sounding'] = []): View =>
  ({ station: 'Kettle FM', now_ms: NOW, sources: [], step: s, carts: { version: 7, desk: true, desk_error: null, sounding, ducked: false, mic_open: false, missing: [], importing: null } }) as unknown as View;
const SHOWS = [{ id: 'steeped', slug: 'steeped', name: 'Steeped', presenter_account: 'acc-finlay', colour_slot: 5 }] as unknown as Show[];

function window_(me: Me | null, v: View, cat: Catalogue | null = CAT, bankN = 1): void {
  account.me = me;
  account.loaded = true;
  setup.shows = SHOWS;
  studio.view = v;
  studio.catalogue = cat;
  studio.cartShow = cat?.show ?? null;
  studio.bank = bankN;
  studio.tab = 'carts';
  studio.now = NOW;
  studio.skew = 0;
  studio.cartFocus = null;
  studio.note = null;
  cartEditor.slot = null;
  cartEditor.bank = null;
}

const buttons = (body: string): { tag: string; text: string }[] =>
  [...body.replace(/<dialog[\s\S]*?<\/dialog>/g, '').matchAll(/<button\b([^>]*)>([\s\S]*?)<\/button>/g)].map((m) => ({ tag: m[1], text: m[2].replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim() }));
const off = (b: { tag: string }): boolean => /\sdisabled(=""|\s|$)/.test(b.tag);
const pad = (all: { tag: string; text: string }[], slot: number) => all.find((b) => b.tag.includes(`data-cart-slot="${slot}"`))!;
const edit = (all: { tag: string; text: string }[], slot: number) => all.find((b) => b.tag.includes(`data-cart-edit="${slot}"`));

beforeEach(() => {
  vi.mocked(api.netFetch).mockReset();
  vi.mocked(api.catalogue).mockReset();
  window_(KEY, view(step('steeped', true)));
});

describe('a file before it is sent', () => {
  it('is held to the station’s limits, which the catalogue carries', () => {
    expect(L).toEqual({ maxMb: 64, maxSeconds: 600, loudnessLufs: -23 });
    // A station older than 1.2.1 says neither: its defaults.
    expect(limitsOf({ ...CAT, max_upload_mb: undefined, max_seconds: undefined })).toEqual({ maxMb: 64, maxSeconds: 600, loudnessLufs: -23 });
    expect(limitsOf(null)).toEqual({ maxMb: 64, maxSeconds: 600, loudnessLufs: null });
    expect(takesLine(L)).toBe('WAV, FLAC, MP3, Ogg (Vorbis or Opus) or M4A · up to 64 MB and 10 minutes');
    expect([lengthWords(600), lengthWords(60), lengthWords(90), lengthWords(3600)]).toEqual(['10 minutes', '1 minute', '90 seconds', '60 minutes']);
  });

  it('is refused here when it is empty, too large, or not a sound by its name and type', () => {
    expect(fileProblem({ name: 'ident.wav', size: 3_200_000, type: 'audio/wav' }, L)).toBe(null);
    // A drop has no filter: the ending is enough, whatever type the system gave it.
    expect(fileProblem({ name: 'IDENT.FLAC', size: 10, type: '' }, L)).toBe(null);
    expect(fileProblem({ name: 'voice-note', size: 10, type: 'audio/ogg' }, L)).toBe(null);
    expect(fileProblem({ name: 'ident.wav', size: 0, type: 'audio/wav' }, L)).toBe('“ident.wav” is empty.');
    expect(fileProblem({ name: 'set.wav', size: 65 * 1024 * 1024, type: 'audio/wav' }, L)).toBe('“set.wav” is 65.0 MB. This station takes sounds up to 64 MB.');
    expect(fileProblem({ name: 'set.wav', size: 65 * 1024 * 1024, type: 'audio/wav' }, { ...L, maxMb: 200 })).toBe(null);
    expect(fileProblem({ name: 'rota.pdf', size: 10, type: 'application/pdf' }, L)).toMatch(/does not look like a sound/);
    // The chooser offers what the worker reads.
    for (const e of ['.wav', '.flac', '.mp3', '.ogg', '.opus', '.m4a']) expect(ACCEPT).toContain(e);
  });

  it('offers its name as the cart’s', () => {
    expect(labelFromFile('01_Top-of-hour.wav')).toBe('01 Top of hour');
    expect(labelFromFile('C:\\Sounds\\Applause  long.FLAC')).toBe('Applause long');
    expect(labelFromFile('/home/finlay/woof.ogg')).toBe('woof');
    expect(labelFromFile('.wav')).toBe('New cart');
    expect([...labelFromFile(`${'x'.repeat(200)}.mp3`)]).toHaveLength(120);
    expect(baseName('C:\\Sounds\\woof.ogg')).toBe('woof.ogg');
    expect(baseName('/home/finlay/woof.ogg')).toBe('woof.ogg');
  });
});

describe('a cart’s words', () => {
  it('needs a name of 1 to 120 characters, as the station does', () => {
    expect(labelProblem('Applause')).toBe(null);
    expect(labelProblem('   ')).toMatch(/Give the cart a name/);
    expect(labelProblem('x'.repeat(121))).toMatch(/at most 120/);
    expect(bankNameProblem('Jingles')).toBe(null);
    expect(bankNameProblem(' ')).toMatch(/Give the bank a name/);
    expect(bankNameProblem('x'.repeat(81))).toMatch(/at most 80/);
  });

  it('reads a ramp in seconds, and never longer than the cart', () => {
    expect(rampMs('', 4_200)).toBe(null);
    expect(rampMs('0', 4_200)).toBe(null);
    expect(rampMs('2.5', 4_200)).toBe(2_500);
    expect(rampMs('2,5', 4_200)).toBe(2_500);
    expect(rampMs('12', null)).toBe(12_000);
    expect(rampMs('12', 4_200)).toBe('long');
    expect(rampMs('soon', 4_200)).toBe('bad');
    expect(rampMs('-1', 4_200)).toBe('bad');
    expect([rampText(null), rampText(12_000), rampText(2_500)]).toEqual(['', '12', '2.5']);
  });

  it('sends only what changed, and nothing when nothing did', () => {
    const c = cart(1, 1, 'Applause', null, { ramp_ms: 1_000, licence: 'CC0 1.0' });
    const same = { label: 'Applause', detail: '', mode: 'one_shot' as const, ramp_ms: 1_000, licence: 'CC0 1.0' };
    expect(changeOf(c, same, 1)).toEqual({});
    expect(changeOf(c, { ...same, label: ' Applause, long ' }, 1)).toEqual({ label: 'Applause, long' });
    // A ramp taken away is null, not left out; a new key is a slot.
    expect(changeOf(c, { ...same, ramp_ms: null, mode: 'sweeper' }, 9)).toEqual({ mode: 'sweeper', ramp_ms: null, slot: 9 });
  });

  it('carries an upload’s words in its query, the empty ones left out', () => {
    expect(uploadQuery({ label: ' Woof ', detail: '', mode: 'one_shot', ramp_ms: null, licence: '', origin: 'woof.ogg' })).toBe('label=Woof&mode=one_shot&origin=woof.ogg');
    const q = new URLSearchParams(uploadQuery({ label: 'Intro: Ghost Train', detail: 'Firth Lights', mode: 'bed', ramp_ms: 12_000, licence: 'CC BY 4.0', origin: 'ghost train.wav' }));
    expect(Object.fromEntries(q)).toEqual({ label: 'Intro: Ghost Train', detail: 'Firth Lights', mode: 'bed', ramp_ms: '12000', licence: 'CC BY 4.0', origin: 'ghost train.wav' });
  });

  it('says what was measured and what the desk does about it', () => {
    expect(soundLine(cart(1, 1, 'Applause', null), -23)).toBe('0:05 · measured −15.8 LUFS, plays at −23.0 LUFS (gain −7.2 dB)');
    expect(soundLine(cart(1, 1, 'Applause', null, { loudness_lufs: null }), -23)).toBe('0:05');
    expect(addedWords(cart(1, 8, 'Woof', null), false, -23)).toEqual({ title: 'Cart added', message: '“Woof” is on W: 0:05 · measured −15.8 LUFS, plays at −23.0 LUFS (gain −7.2 dB).' });
    expect(addedWords(cart(1, 8, 'Woof', null), true, null).title).toBe('Sound replaced');
  });

  it('gives the station’s sentence for a refusal, and its own where the station has none', () => {
    expect(cartProblem(new api.ApiError(422, 'the file was refused: the file is longer than 600 s'), L)).toBe('the file was refused: the file is longer than 600 s');
    // A file over the limit comes back as a bare 413, from the station or a proxy before it.
    expect(cartProblem(new api.ApiError(413, '413 Payload Too Large'), L)).toBe('The file is too large: this station takes sounds up to 64 MB.');
    // A station older than 1.2.1 has no route to change a cart without its sound.
    expect(cartProblem(new api.ApiError(405, '405 Method Not Allowed'), L, 'change')).toMatch(/older than 1\.2\.1/);
    expect(cartProblem(new api.ApiError(404, 'bank 1 slot 9 is empty: there is no cart to change'), L, 'change')).toBe('bank 1 slot 9 is empty: there is no cart to change');
    expect(cartProblem(new api.ApiError(409, 'bank 1 slot 2 already holds “Rimshot”; move or remove that cart first'), L, 'change')).toMatch(/Rimshot/);
    expect(cartProblem(new TypeError('Failed to fetch'), L)).toBe('The file did not reach the station: Failed to fetch');
  });
});

describe('whose bank a change goes to, and who may make it', () => {
  it('follows the tab: the show’s own bank where it has one, else the station’s', () => {
    expect(ownerOfTab(CAT, 1)).toBe(null);
    expect(ownerOfTab(CAT, 2)).toBe('steeped');
    expect(ownerOfTab(CAT, 3)).toBe(null);
    expect(bankWords(CAT.banks[0], 1, 'Steeped')).toBe('the station’s bank 1, Reactions');
    expect(bankWords(CAT.banks[1], 2, 'Steeped')).toBe('Steeped’s own bank 2, Dog woofs');
    expect(emptySlots(CAT, 1)).toHaveLength(21);
    expect(emptySlots(CAT, 1).slice(0, 3)).toEqual([3, 4, 5]);
    expect(emptySlots(CAT, 2)).not.toContain(1);
  });

  it('lets any member of staff change the station’s banks, as the station does', () => {
    for (const me of [KEY, MOIRA, FINLAY, SORCHA, null]) expect(mayChange(null, me, step(null, false), false)).toBe(true);
  });

  it('keeps a show’s own banks to an admin, a producer, the key and the show’s presenter', () => {
    const live = step('steeped', false);
    expect(mayChange('steeped', KEY, live, false)).toBe(true);
    expect(mayChange('steeped', MOIRA, live, false)).toBe(true);
    // The step says whether this account may change the show on air.
    expect(mayChange('steeped', FINLAY, step('steeped', true), false)).toBe(true);
    expect(mayChange('steeped', EILIDH, live, false)).toBe(false);
    expect(mayChange('steeped', SORCHA, live, false)).toBe(false);
    // A preparing window works with its own show.
    expect(mayChange('steeped', FINLAY, step('the-morning-kettle', false), true)).toBe(true);
    expect(mayChange('steeped', SORCHA, live, true)).toBe(false);
    expect(NOT_YOURS('Steeped')).toBe('Only an admin, a producer or Steeped’s presenter changes its own bank.');
  });
});

describe('the station’s routes', () => {
  const answer = (status: number, body: unknown) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
  const last = () => {
    const [url, init] = vi.mocked(api.netFetch).mock.calls.at(-1)!;
    return { url: String(url), method: init?.method, body: init?.body, type: new Headers(init?.headers).get('content-type') };
  };

  it('name the station’s bank or the show’s own', () => {
    expect(bankRoute(null, 2)).toBe('/api/v1/carts/banks/2');
    expect(bankRoute('steeped', 2)).toBe('/api/v1/shows/steeped/carts/banks/2');
    expect(slotRoute(null, 2, 24)).toBe('/api/v1/carts/banks/2/slots/24');
    expect(slotRoute('the morning/kettle', 1, 1)).toBe('/api/v1/shows/the%20morning%2Fkettle/carts/banks/1/slots/1');
  });

  it('send a sound to its slot with the cart’s words, and answer with the cart', async () => {
    vi.mocked(api.netFetch).mockResolvedValue(answer(201, cart(2, 3, 'Woof', 'steeped')));
    const file = new Blob([new Uint8Array(16)], { type: 'audio/ogg' });
    const made = await putCart('steeped', 2, 3, { label: 'Woof', detail: '', mode: 'one_shot', ramp_ms: null, licence: '', origin: 'woof.ogg' }, file);
    expect(made.label).toBe('Woof');
    expect(last()).toMatchObject({ method: 'PUT', type: 'application/octet-stream', body: file });
    expect(last().url).toMatch(/\/api\/v1\/shows\/steeped\/carts\/banks\/2\/slots\/3\?label=Woof&mode=one_shot&origin=woof\.ogg$/);
  });

  it('stop waiting at once when asked, in either build', async () => {
    vi.mocked(api.netFetch).mockReturnValue(new Promise(() => {}));
    const stop = new AbortController();
    const sent = putCart(null, 1, 3, { label: 'Woof', detail: '', mode: 'one_shot', ramp_ms: null, licence: '', origin: '' }, new Blob(['x']), stop.signal);
    stop.abort();
    await expect(sent).rejects.toMatchObject({ name: 'AbortError' });
  });

  it('change a cart, empty a slot, make a bank and remove a show’s own', async () => {
    vi.mocked(api.netFetch).mockResolvedValue(answer(200, cart(1, 9, 'Applause, long', null)));
    await patchCart(null, 1, 1, { label: 'Applause, long', slot: 9 });
    expect(last()).toMatchObject({ method: 'PATCH', type: 'application/json', body: '{"label":"Applause, long","slot":9}' });
    expect(last().url).toMatch(/\/api\/v1\/carts\/banks\/1\/slots\/1$/);
    vi.mocked(api.netFetch).mockResolvedValue(answer(200, CAT));
    await removeCart('steeped', 2, 1);
    expect(last()).toMatchObject({ method: 'DELETE' });
    expect(last().url).toMatch(/\/api\/v1\/shows\/steeped\/carts\/banks\/2\/slots\/1$/);
    await putBank(null, 4, { name: ' Idents ', colour_slot: 11 });
    expect(last()).toMatchObject({ method: 'PUT', body: '{"name":"Idents","colour_slot":11}' });
    expect(last().url).toMatch(/\/api\/v1\/carts\/banks\/4$/);
    await removeShowBank('steeped', 2);
    expect(last()).toMatchObject({ method: 'DELETE' });
    expect(last().url).toMatch(/\/api\/v1\/shows\/steeped\/carts\/banks\/2$/);
  });

  it('throw the station’s sentence when it refuses', async () => {
    vi.mocked(api.netFetch).mockResolvedValue(answer(409, { error: 'bank 1 slot 2 already holds “Rimshot”; move or remove that cart first' }));
    await expect(patchCart(null, 1, 1, { slot: 2 })).rejects.toMatchObject({ status: 409, message: 'bank 1 slot 2 already holds “Rimshot”; move or remove that cart first' });
    vi.mocked(api.netFetch).mockResolvedValue(new Response('', { status: 413 }));
    await expect(putCart(null, 1, 1, { label: 'x', detail: '', mode: 'bed', ramp_ms: null, licence: '', origin: '' }, new Blob(['x']))).rejects.toMatchObject({ status: 413 });
  });
});

describe('the wall’s affordances', () => {
  it('makes an empty slot the invitation to add, and gives an idle cart its Edit', () => {
    const all = buttons(render(CartWall).body);
    expect(pad(all, 3).text).toBe('3 empty Add a cart Press this pad, or drop a sound here.');
    expect(pad(all, 3).tag).toContain('aria-label="3, empty slot. Add a cart"');
    expect(pad(all, 3).tag).toContain('aria-haspopup="dialog"');
    // Slots 1, 2 and 7 hold carts: each has an Edit beside its pad, named for its key and cart.
    expect(edit(all, 1)!.tag).toContain('aria-label="Edit 1, Applause"');
    expect(edit(all, 7)!.tag).toContain('aria-label="Edit Q, Rain bed"');
    expect(edit(all, 3)).toBeUndefined();
    expect(all.filter((b) => b.tag.includes('data-cart-edit'))).toHaveLength(3);
    // The pad keeps its place for the keys and the tests that find it: 24 of them, no more.
    expect(all.filter((b) => b.tag.includes('data-cart-slot'))).toHaveLength(24);
    expect(all.some((b) => b.text === 'Edit bank…')).toBe(true);
  });

  it('keeps the Edit buttons out of the tab order but for the pad the focus is on', () => {
    studio.cartFocus = 2;
    const all = buttons(render(CartWall).body);
    expect(edit(all, 2)!.tag).toContain('tabindex="0"');
    expect(edit(all, 1)!.tag).toContain('tabindex="-1"');
    expect(edit(all, 7)!.tag).toContain('tabindex="-1"');
  });

  it('does not offer a sounding cart for change: the ring has its corner', () => {
    const sounding = [{ cart: 'station-1-7', label: 'Rain bed', pool: 'bed' as const, player: 1, mode: 'bed' as const, cue: false, play: 1, started_ms: NOW - 5_000, ends_ms: null, stopping: false, seq: 1 }];
    window_(KEY, view(step('steeped', true), sounding));
    const all = buttons(render(CartWall).body);
    expect(edit(all, 7)).toBeUndefined();
    expect(edit(all, 1)).toBeDefined();
  });

  it('says a missing cart is mended by editing it', () => {
    window_(KEY, view(step('steeped', true)), { ...CAT, carts: [cart(1, 1, 'Show close', null, { file: null, missing: true })] });
    const all = buttons(render(CartWall).body);
    expect(pad(all, 1).text).toContain('Its sound is gone. Edit this cart to put it back. The key does nothing.');
    expect(edit(all, 1)).toBeDefined();
  });

  it('offers nothing in another show’s own bank, and says whose it is', () => {
    // Eilidh, live while Steeped is on air: its own bank 2 is not hers; the station's bank 1 is anyone's.
    window_(EILIDH, view(step('steeped', false)), CAT, 2);
    let all = buttons(render(CartWall).body);
    expect(all.filter((b) => b.tag.includes('data-cart-edit'))).toHaveLength(0);
    expect(pad(all, 3).text).toBe('3 empty No cart here yet.');
    expect(pad(all, 3).tag).toContain(`title="${NOT_YOURS('Steeped')}"`);
    expect(all.some((b) => b.text === 'Edit bank…')).toBe(false);
    studio.bank = 1;
    all = buttons(render(CartWall).body);
    expect(edit(all, 1)).toBeDefined();
    expect(pad(all, 3).text).toContain('Add a cart');
  });

  it('offers to make a bank on a tab that has none', () => {
    studio.bank = 4;
    const all = buttons(render(CartWall).body);
    expect(all.some((b) => b.text === 'Make bank 4…')).toBe(true);
    expect(all.some((b) => b.text === 'Edit bank…')).toBe(false);
    expect(pad(all, 1).text).toContain('Add a cart');
  });

  it('in a preparing window fires nothing and still adds and edits', () => {
    // Finlay prepares Steeped while another show is on air: this is when a DJ fills their bank.
    window_({ ...FINLAY, preparing: true }, view(step('the-morning-kettle', false)), CAT, 2);
    const all = buttons(render(CartWall).body);
    // Woof, on key 1, does not fire from here; its Edit works.
    expect(off(pad(all, 1))).toBe(true);
    expect(off(edit(all, 1)!)).toBe(false);
    // Every empty slot adds.
    const empty = all.filter((b) => b.tag.includes('data-cart-slot') && b.text.includes('Add a cart'));
    expect(empty).toHaveLength(23);
    expect(empty.some(off)).toBe(false);
    expect(off(all.find((b) => b.text.startsWith('Fade all'))!)).toBe(true);
  });

  it('draws both dialogs under their registered names', () => {
    const body = render(CartWall).body;
    expect(body).toContain('data-panel="carts.edit"');
    expect(body).toContain('data-panel="carts.bank-edit"');
    // Neither takes a class the wall styles: `.bank` is a tab, and drew the closed dialog as one.
    for (const m of body.matchAll(/<dialog[^>]*class="([^"]*)"/g)) expect(m[1].split(' ')).not.toContain('bank');
  });
});

describe('an empty slot’s key', () => {
  /** Nothing on air: no show by the schedule, nobody on air at the desk. */
  const OFF_AIR = { ...step('steeped', true), on_air: false };
  const onDesk = (on_air: boolean) => ({ ...view(OFF_AIR), sources: [{ identity: 'eilidh', name: 'Eilidh Fraser-MacLeod', role: 'presenter', on_air }] }) as unknown as View;

  it('never opens the dialog while on air: a stray key must not cover the wall', () => {
    // A live window with a show on air (the test's default view).
    expect(keyAdds()).toBe(false);
    expect(addInstead({ type: 'cart-key', slot: 3 }, false)).toBe(false);
    studio.cartFocus = 3;
    expect(addInstead({ type: 'cart-enter' }, true)).toBe(false);
    expect(cartEditor.slot).toBe(null);
    // No show by the schedule, but someone is on air at the desk: the same.
    window_(KEY, onDesk(true));
    expect(keyAdds()).toBe(false);
    expect(addInstead({ type: 'cart-key', slot: 3 }, false)).toBe(false);
    // Off air it adds; and a preparing window's key adds whatever is on air.
    window_(KEY, onDesk(false));
    expect(keyAdds()).toBe(true);
    window_({ ...FINLAY, preparing: true }, view(step('the-morning-kettle', false)), CAT, 2);
    expect(keyAdds()).toBe(true);
    expect(addInstead({ type: 'cart-key', slot: 3 }, false)).toBe(true);
    expect(cartEditor.slot).toEqual({ bank: 2, slot: 3, file: null });
  });

  it('on air the pad says to press it, and its press still adds', () => {
    const all = buttons(render(CartWall).body);
    expect(pad(all, 3).text).toBe('3 empty Add a cart Press this pad, or drop a sound here.');
    window_(KEY, view(OFF_AIR));
    expect(pad(buttons(render(CartWall).body), 3).text).toBe('3 empty Add a cart Press 3 , or drop a sound here.');
  });

  it('opens the dialog to add a cart off air, on the Carts tab alone', () => {
    window_(KEY, view(OFF_AIR));
    expect(addInstead({ type: 'cart-key', slot: 3 }, false)).toBe(true);
    expect(cartEditor.slot).toEqual({ bank: 1, slot: 3, file: null });
    cartEditor.slot = null;
    // A slot that holds a cart fires it: the key is not taken.
    expect(addInstead({ type: 'cart-key', slot: 1 }, false)).toBe(false);
    expect(cartEditor.slot).toBe(null);
    // ⏎ adds only from the pad the arrows reached.
    studio.cartFocus = 4;
    expect(addInstead({ type: 'cart-enter' }, false)).toBe(false);
    expect(addInstead({ type: 'cart-enter' }, true)).toBe(true);
    expect((cartEditor.slot as SlotAsk | null)?.slot).toBe(4);
    cartEditor.slot = null;
    // F1 to F6 from the desk never put a dialog over it; nor does any key from another tab.
    expect(addInstead({ type: 'cart', n: 3 }, true)).toBe(false);
    studio.tab = 'desk';
    expect(addInstead({ type: 'cart-key', slot: 3 }, false)).toBe(false);
    expect(cartEditor.slot).toBe(null);
  });

  it('makes the bank first on a tab that has none, and keeps the dropped file for the cart', () => {
    studio.bank = 4;
    const file = new File(['x'], 'ident.wav');
    askSlot(6, file);
    expect(cartEditor.slot).toBe(null);
    expect(cartEditor.bank).toEqual({ n: 4, owner: null, thenSlot: 6, file });
  });

  it('says whose the bank is when it is not this window’s to change', () => {
    window_(EILIDH, view({ ...step('steeped', false), on_air: false }), CAT, 2);
    expect(addInstead({ type: 'cart-key', slot: 3 }, false)).toBe(true);
    expect(cartEditor.slot).toBe(null);
    expect(studio.note?.message).toBe(NOT_YOURS('Steeped'));
  });

  it('does nothing before the catalogue is read', () => {
    window_(KEY, view(step(null, false)), null);
    expect(addInstead({ type: 'cart-key', slot: 3 }, false)).toBe(false);
  });
});

describe('the default pack, loaded again', () => {
  /** A pack made for the test: bank 1 has keys 1, 2 and 3; bank 2 has key 1. */
  const PACK: PackView = {
    available: true,
    loading: false,
    banks: [
      { number: 1, name: 'Reactions', carts: [{ slot: 1, label: 'Clap', mode: 'one_shot' }, { slot: 2, label: 'Swish', mode: 'sweeper' }, { slot: 3, label: 'Rain (loop)', mode: 'bed' }] },
      { number: 2, name: 'Idents', carts: [{ slot: 1, label: 'Ident', mode: 'one_shot' }] },
    ],
    // On the station: Applause (key 1) and Rain bed (key Q) are defaults nobody changed.
    untouched: ['station-1-1', 'station-1-7'],
  };

  it('loads an empty bank directly, and makes one that has not been made', () => {
    const p = packPlan(PACK, CAT, 1, 4)!;
    expect(p).toEqual({ bank: 1, into: 4, made: false, load: 3, replace: 0, remove: 0, keep: [], leftOut: 0 });
    expect(planAsks(p)).toBe(false);
    expect(planWords(p, null)).toEqual(['The station’s bank 4 has not been made: it is made under the pack’s name. 3 sounds are loaded into it.']);
    expect(loadWords([p])).toBe('Load 3 sounds');
    const made = packPlan(PACK, { ...CAT, banks: [...CAT.banks, bank(4, 'Spare', null)] }, 2, 4)!;
    expect(planWords(made, 'Spare')).toEqual(['The station’s bank 4, Spare, is empty. 1 sound is loaded into it.']);
    expect(loadWords([made])).toBe('Load 1 sound');
    expect(packPlan(PACK, CAT, 9, 1)).toBe(null);
  });

  it('in a bank that holds carts replaces only the defaults nobody changed, and says so first', () => {
    // The station's bank 1 holds Applause (1, a default), Rimshot (2, a person's) and Rain bed
    // (Q, a default the pack has no sound for).
    const p = packPlan(PACK, CAT, 1, 1)!;
    expect(p).toEqual({ bank: 1, into: 1, made: true, load: 2, replace: 1, remove: 1, keep: [2], leftOut: 1 });
    expect(planAsks(p)).toBe(true);
    expect(planWords(p, 'Reactions')).toEqual([
      'The station’s bank 1, Reactions, holds 3 carts.',
      '2 are default sounds nobody changed and are replaced (1 goes with nothing in its place: the pack has no sound for that key).',
      '1 is yours and stays, on 2; the pack’s sound for that key is left out.',
      '2 sounds are loaded.',
    ]);
    expect(loadWords([p])).toBe('Replace 2 and load 2');
    // A show's own carts are never the station's to count: Steeped's Woof is in its own bank 2.
    const two = packPlan(PACK, CAT, 2, 2)!;
    expect(two).toEqual({ bank: 2, into: 2, made: false, load: 1, replace: 0, remove: 0, keep: [], leftOut: 0 });
    expect(loadWords([p, two])).toBe('Replace 2 and load 3');
    // Nothing nobody changed: everything stays, and only the free keys are filled.
    const none = packPlan({ ...PACK, untouched: [] }, CAT, 1, 1)!;
    expect(none).toMatchObject({ load: 1, replace: 0, remove: 0, keep: [1, 2, 7], leftOut: 2 });
    expect(planWords(none, 'Reactions')[1]).toBe('None is a default sound nobody changed, so nothing is replaced.');
    expect(planWords(none, 'Reactions')[2]).toBe('3 are yours and stay, on 1, 2, Q; the pack’s 2 sounds for those keys are left out.');
  });

  it('asks the station for the pack, and to load with replace only where a bank holds carts', async () => {
    const answer = (status: number, body: unknown) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
    vi.mocked(api.netFetch).mockResolvedValue(answer(200, PACK));
    expect((await packView()).banks).toHaveLength(2);
    expect(String(vi.mocked(api.netFetch).mock.calls.at(-1)![0])).toMatch(/\/api\/v1\/carts\/pack$/);
    vi.mocked(api.netFetch).mockResolvedValue(answer(202, { plans: [], loading: 3 }));
    expect(await loadPack([{ bank: 1, into: 4 }], false)).toMatchObject({ loading: 3 });
    const [url, init] = vi.mocked(api.netFetch).mock.calls.at(-1)!;
    expect(String(url)).toMatch(/\/api\/v1\/carts\/pack$/);
    expect(init).toMatchObject({ method: 'POST', body: '{"loads":[{"bank":1,"into":4}],"replace":false}' });
    vi.mocked(api.netFetch).mockResolvedValue(answer(409, { error: 'a pack is loading now; wait until it has finished' }));
    await expect(loadPack([{ bank: 1, into: 1 }], true)).rejects.toMatchObject({ status: 409, message: 'a pack is loading now; wait until it has finished' });
  });

  it('is offered beside the tabs, and its dialog is drawn under its registered name', () => {
    const body = render(CartWall).body;
    expect(buttons(body).some((b) => b.text === 'Default pack…')).toBe(true);
    expect(body).toContain('data-panel="carts.pack"');
  });
});
