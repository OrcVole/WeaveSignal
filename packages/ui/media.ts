// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// How every page that sends a voice into the room captures the microphone: the presenter's
// browser fallback and the guest and caller join pages (M3) take these constraints, for
// `getUserMedia({ audio })` or LiveKit's `createLocalAudioTrack(...)`.
//
// The second live run (2026-10-25) heard a guest and a caller come back into the programme
// 0.3 s late: their voices leaked from the presenter's headphones into the microphone and the
// browser's automatic gain control lifted them while the presenter was quiet. So echo
// cancelling and noise suppression are on and automatic gain control is off for everyone;
// the desk levels voices itself (the input chain: expander, compressor, leveller, limiter).
// Only an explicit echo-test page turns echo cancelling off, to hear the raw microphone.

export interface VoiceCapture {
  echoCancellation: boolean;
  noiseSuppression: boolean;
  autoGainControl: boolean;
}

/** Every join page: presenter, guest, caller, remote DJ in a browser. */
export const VOICE_CAPTURE: Readonly<VoiceCapture> = Object.freeze({
  echoCancellation: true,
  noiseSuppression: true,
  autoGainControl: false,
});

/** The echo-test page only: the microphone as it is, so a person hears what the room would. */
export const ECHO_TEST_CAPTURE: Readonly<VoiceCapture> = Object.freeze({
  echoCancellation: false,
  noiseSuppression: false,
  autoGainControl: false,
});
