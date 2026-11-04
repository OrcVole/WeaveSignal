// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The microphone's and the camera's refusals, as sentences a nervous caller can act on.

/** Why the microphone could not be had, in a sentence. */
export function micProblem(e: unknown): string {
  const name = e instanceof DOMException ? e.name : '';
  if (name === 'NotAllowedError' || name === 'SecurityError')
    return "The browser did not allow the microphone. Allow it for this page (the lock or the site settings by the address). On a phone, the browser itself may need it too: the phone's Settings, Apps, your browser, Permissions, Microphone.";
  if (name === 'NotFoundError' || name === 'OverconstrainedError') return 'No microphone was found. Plug one in or use a phone, then try again.';
  if (name === 'NotReadableError') return 'Another app is using the microphone. Close it, then try again.';
  return 'The microphone could not be started. Try again, or try another browser.';
}

/** Why the camera could not be had, in a sentence. A camera that does not start never keeps a
 * person out: the studio hears them without their picture (ADR-0171). */
export function cameraProblem(e: unknown, inCall = false): string {
  const name = e instanceof DOMException ? e.name : (e as { name?: string } | null)?.name ?? '';
  const rest = inCall ? 'The studio hears you without your picture.' : 'You can still join: the studio will hear you without your picture.';
  if (name === 'NotAllowedError' || name === 'SecurityError') return `The browser did not allow the camera. ${rest}`;
  if (name === 'NotFoundError' || name === 'OverconstrainedError') return `No camera was found. ${rest}`;
  if (name === 'NotReadableError') return `Another app is using the camera. ${rest}`;
  return `Your camera didn't start. ${rest}`;
}
