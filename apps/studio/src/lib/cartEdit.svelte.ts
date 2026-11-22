// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What the cart wall's two dialogs are open on (1.2.1): a slot to add a cart to or change, and
// a bank to make or change. The wall's pads, their keys and a file dropped on the window all ask
// here; the dialogs (carts/CartEditor.svelte, carts/BankEditor.svelte) answer.

import type { Action } from '../../../../packages/ui/keymap.ts';
import { cartKey } from '../../../../packages/ui/keymap.ts';
import { bankAt, cartAt } from '../../../../packages/ui/carts.ts';
import { account } from './account.svelte.ts';
import { NOT_YOURS, mayChange, ownerOfTab, type OwnerSlug } from './cartEdit.ts';
import { preparing } from './prepare.svelte.ts';
import { onStationChange } from './reset.ts';
import { say, studio } from './studio.svelte.ts';
import { setup } from './wizard.svelte.ts';

/** A slot the cart dialog is open on. */
export interface SlotAsk {
  /** The tab's number, which is the bank's. */
  bank: number;
  slot: number;
  /** A file that came with the ask: dropped on the pad, or on the window. */
  file: File | null;
}

/** A bank the bank dialog is open on. */
export interface BankAsk {
  n: number;
  /** Whose bank it makes or changes: the station's (null) or a show's own. */
  owner: OwnerSlug;
  /** Once the bank is made, go on to add a cart to this slot of it. */
  thenSlot: number | null;
  /** The file that waits for that slot. */
  file: File | null;
}

const fresh = () => ({
  slot: null as SlotAsk | null,
  bank: null as BankAsk | null,
  /** The default pack's dialog is open, on the tab the window showed when it was asked for. */
  pack: null as number | null,
});

export const cartEditor = $state(fresh());

// A change of station (ADR-0285) closes both: they name the station being left.
onStationChange('cart editor', () => Object.assign(cartEditor, fresh()));

/** The name of the show whose own banks this window works with. */
export function cartShowName(): string | null {
  const slug = studio.cartShow;
  if (!slug) return null;
  const v = studio.view;
  if (v?.step?.show?.slug === slug) return v.step.show.name;
  return setup.shows.find((s) => s.slug === slug)?.name ?? slug;
}

/** Whether this window may change the bank on tab `n`, as far as it can tell. */
export function mayChangeTab(n: number): boolean {
  return mayChange(ownerOfTab(studio.catalogue, n), account.me, studio.view?.step, preparing());
}

/** Whether this window may make or change a bank of its show's own. */
export function mayChangeShow(): boolean {
  return studio.cartShow !== null && mayChange(studio.cartShow, account.me, studio.view?.step, preparing());
}

/** Open the cart dialog on a slot of the tab this window shows: to add a cart to an empty one,
 *  or to change the cart it holds. A bank nobody has made is made first. Says why not when the
 *  bank is another's to change. */
export function askSlot(slot: number, file: File | null = null): void {
  const cat = studio.catalogue;
  if (!cat) return;
  const n = studio.bank;
  const bank = bankAt(cat, n);
  if (!bank) {
    // The first cart of a bank nobody has made: its name and colour first, then the cart.
    cartEditor.bank = { n, owner: null, thenSlot: slot, file };
    return;
  }
  if (!mayChangeTab(n)) {
    say('good', 'Not yours to change', NOT_YOURS(cartShowName() ?? 'the show'));
    return;
  }
  cartEditor.slot = { bank: n, slot, file };
}

/** Open the bank dialog: the tab's own bank as it is, or a bank to make for `owner`. */
export function askBank(n: number, owner: OwnerSlug): void {
  cartEditor.bank = { n, owner, thenSlot: null, file: null };
}

/** Whether an empty slot's key adds a cart from this window now. Never while on air: a stray key
 *  would put a dialog over the wall a presenter is firing from, and keep the keyboard until Esc.
 *  A preparing window changes nothing on air, so its keys add; a live window's add only while no
 *  show is on air and nobody is on air at the desk. The pad's press and a dropped sound always
 *  add, as they are meant. */
export function keyAdds(): boolean {
  if (preparing()) return true;
  const v = studio.view;
  return !(v?.step?.on_air || v?.sources?.some((s) => s.on_air));
}

/** A cart's key or ⏎ on a slot that holds no cart opens the dialog to add one, while nothing is
 *  on air ([`keyAdds`]). True when the key was taken. Only on the Carts tab: F1 to F6 from
 *  another tab never put a dialog over the desk. */
export function addInstead(a: Action, focusOnCart: boolean): boolean {
  if (studio.tab !== 'carts' || !studio.catalogue || !keyAdds()) return false;
  const slot = a.type === 'cart-key' ? a.slot : a.type === 'cart-enter' && focusOnCart ? studio.cartFocus : null;
  if (slot === null || slot === undefined) return false;
  if (cartAt(studio.catalogue, studio.bank, slot)) return false;
  askSlot(slot);
  return true;
}

/** The pad's own name for an empty slot, read aloud. */
export function emptyAria(slot: number, may: boolean): string {
  return may ? `${cartKey(slot)}, empty slot. Add a cart` : `${cartKey(slot)}, empty slot`;
}
