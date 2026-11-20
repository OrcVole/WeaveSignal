// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The phone line's state as this window last read it (1.2, PLAN 17f), for the screener's queue:
// whether the line takes calls, and why not. `null` until read, and on a station that has no
// phone line's routes (older than 1.2), where nothing is said of one.

import { onStationChange } from './reset.ts';
import { phoneStatus, type PhoneStatus } from './phone.ts';

export const phone = $state({ status: null as PhoneStatus | null, absent: false });

let timer: ReturnType<typeof setInterval> | undefined;

async function read() {
  if (phone.absent) return;
  try {
    phone.status = await phoneStatus();
  } catch (e) {
    const status = (e as { status?: number } | null)?.status;
    if (status === 404 || status === 405) phone.absent = true;
  }
}

/** Read the line's state now and every twenty seconds while someone watches; returns the stop. */
export function watchPhone(): () => void {
  void read();
  clearInterval(timer);
  timer = setInterval(read, 20_000);
  return () => clearInterval(timer);
}

onStationChange('phone', () => {
  phone.status = null;
  phone.absent = false;
});
