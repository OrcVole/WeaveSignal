// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What a change of station clears (ADR-0285; docs/reference/design/station-switcher.md,
// section 8). The studio's state is in module-level stores that live as long as the page, so
// each module that holds anything of a station says here how to close its links to that
// station and how to go back to what it was when the page first loaded. A switch runs them in
// that order, links first, then tells the Rust side and loads the page again.
//
// A module that holds only the app's own state (the theme, the text size, the update screen)
// registers nothing, and `reset.test.ts` lists it by name with the reason: a new module-level
// variable must be put in one place or the other, or that test fails.

const closers = new Map<string, () => void>();
const resets = new Map<string, () => void>();

/** Register how a module closes its links to the station (an event stream, a socket, a loop
 *  that reconnects). Registering under the same name again replaces it; the answer unregisters. */
export function onLeave(name: string, close: () => void): () => void {
  closers.set(name, close);
  return () => {
    if (closers.get(name) === close) closers.delete(name);
  };
}

/** Register how a module forgets the station: back to what it was when the page first loaded. */
export function onStationChange(name: string, reset: () => void): void {
  resets.set(name, reset);
}

/** Close every link to the station. One that throws does not keep the others open. */
export function closeLinks(): void {
  for (const close of closers.values()) {
    try {
      close();
    } catch {
      // A link that was never open: there is nothing to close.
    }
  }
}

/** Empty every store. One that throws does not keep the others full. */
export function resetStores(): void {
  for (const reset of resets.values()) {
    try {
      reset();
    } catch {
      // The page loads again after this; a store that could not be emptied goes with it.
    }
  }
}

/** The names registered, for the test that every store is here. */
export function registered(): { closers: string[]; resets: string[] } {
  return { closers: [...closers.keys()], resets: [...resets.keys()] };
}
