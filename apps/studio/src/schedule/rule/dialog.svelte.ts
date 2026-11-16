// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The rule dialog, as the Schedule tab holds it: which show's or series' rule is being
// changed. The show editor, the series editor and the list of shows with nothing coming up
// each open it, and the tab draws it once, beside itself rather than inside the editor that
// asked: the editors' own field styles would otherwise reach into the dialog.

import { onStationChange } from '../../lib/reset.ts';
import type { IsoDate, Rule, RuleChanged } from '../../lib/api.ts';

export interface RuleDialogAsk {
  slug: string;
  name: string;
  /** The series whose own rule this is; null for the show's. */
  series?: number | null;
  rule?: Rule | null;
  follow?: Rule | null;
  /** Reset to show: the series follows its show again. */
  reset?: boolean;
  nextOn?: IsoDate | null;
  colour?: number | null;
  onSaved: (changed: RuleChanged | null) => void | Promise<void>;
}

/** `elsewhere`: the sentence of a change someone else made to this show's rule while the
 *  dialog was open (1.0.5), so a save does not quietly put this one in its place. */
export const ruleDialog = $state({ open: null as RuleDialogAsk | null, elsewhere: null as string | null });

// A change of station (ADR-0285): a rule being edited is that station's show's.
onStationChange('rule dialog', () => {
  ruleDialog.open = null;
  ruleDialog.elsewhere = null;
});

export function openRuleDialog(ask: RuleDialogAsk): void {
  ruleDialog.open = ask;
  ruleDialog.elsewhere = null;
}

export function closeRuleDialog(): void {
  ruleDialog.open = null;
  ruleDialog.elsewhere = null;
}
