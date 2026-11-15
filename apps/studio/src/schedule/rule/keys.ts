// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The guided dialogs' two keys (new-show.md section 4). Esc leaves a text field first, and a
// second Esc closes the dialog (which then asks before discarding anything typed). ⏎ presses
// the part's primary button, except in a text field of several lines and on a control that
// takes ⏎ itself (a button, a list, a day in the small months).

/** A field a person types in. */
export function isTextField(el: Element | null): boolean {
  if (!el) return false;
  const tag = el.tagName;
  if (tag === 'TEXTAREA') return true;
  if (tag !== 'INPUT') return false;
  const type = (el.getAttribute('type') ?? 'text').toLowerCase();
  return !['checkbox', 'radio', 'button', 'submit', 'range', 'color', 'file'].includes(type);
}

/** What Esc does with the focus where it is: leave the field, or go on to close the dialog. */
export function escStep(active: Element | null): 'leave-field' | 'close' {
  return isTextField(active) ? 'leave-field' : 'close';
}

/** Whether ⏎ on this element should press the part's primary button. A form's own fields
 *  submit it themselves; this is for a focused row or the dialog's own ground. */
export function enterIsPrimary(el: Element | null): boolean {
  if (!el) return false;
  if (['BUTTON', 'A', 'TEXTAREA', 'SELECT', 'INPUT', 'SUMMARY'].includes(el.tagName)) return false;
  const role = el.getAttribute('role');
  return role !== 'gridcell' && role !== 'radio';
}

/** The keydown every guided dialog listens with. `close` is asked on the second Esc. */
export function dialogKeys(e: KeyboardEvent, dialog: HTMLElement, close: () => void, primary: () => void): void {
  if (e.defaultPrevented) return;
  const active = dialog.ownerDocument.activeElement;
  if (e.key === 'Escape') {
    e.preventDefault();
    if (escStep(active) === 'leave-field') dialog.focus();
    else close();
  } else if (e.key === 'Enter' && !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey && enterIsPrimary(active)) {
    e.preventDefault();
    primary();
  }
}
