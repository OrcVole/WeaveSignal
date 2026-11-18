<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The master lamp (desk.lamp): ON AIR means only "listeners can hear this" (ADR-0062). When
  // playout is down it is the hollow PLAYOUT DOWN square, never OPEN: OPEN is a source's word.
  // In auto mode it is the hollow AUTO lamp in the mode colour (ADR-0183), at the lamp's full
  // size. Its word carries the tooltip every state word carries; AUTO's adds what automation is
  // and plays, in the words the button under it and the air dialog use (`AUTOMATION_PLAYS`).
  import { LAMP, type LampState } from '../../../../packages/ui/states.ts';
  import StateChip from '../../../../packages/ui/StateChip.svelte';
  import { AUTOMATION_PLAYS } from '../lib/air.ts';

  /** `clear`: the lamp with the button under it (AirControl): the tooltip opens under both, so
   *  it never lies over the button the pointer is on its way to. */
  let { state, clear = null }: { state: LampState; clear?: HTMLElement | null } = $props();
  const look = $derived(LAMP[state]);
</script>

<StateChip
  lamp
  chip={look.chip}
  more={state === 'auto' ? AUTOMATION_PLAYS : undefined}
  {clear}
  class="{state === 'on-air' ? 'onair' : state === 'auto' ? 'auto' : 'serious'} lamp-{state}"
  data-panel="desk.lamp"
  data-state={state}
  role="status"
  aria-live="polite"
/>
