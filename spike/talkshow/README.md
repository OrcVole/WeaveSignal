# The Blether: a synthetic talk show for WeaveSignal tests

A test fixture, not product code (ADR-0078 in the project folder). A fictional show on the fictional Kettle FM
that exercises the decided features: a browser guest, cross-talk, carts, the screener talking to callers off
air, a profanity dump, the news on a hard post, playout down (OPEN), and the overlap handover to a remote DJ.

- `show.json`: cast, music, `say_as` respellings for the text-to-speech, per-cue seeds, and the cues (each with
  the feature it tests). In the live run the operator takes the host's lines; the others wait for the host.
- `synth.sh`: every speech cue through VoiceLab's tts-serve on 127.0.0.1:7500 into `out/cues/`.
- `build.py`: the timeline (`out/timeline.json`), one stem per participant (`out/stems/`), and two references:
  `out/room.wav` (what the room hears) and `out/listeners.wav` (dump cut, fallback while playout is down).

Ignored by git: `voices/` (consented personal-use clones and VCTK references, see `voices/ATTRIBUTION.txt`),
`music/` (copyrighted, trimmed from the operator's files) and `out/`. Never stream any of it publicly.

Voices: host, guest and caller copy the VoiceLab persona references (N, Trevor, Drukhi). Screener, newsreader,
farmer and DJ are VCTK speakers p249 (Aberdeen), p262 (Edinburgh), p292 (Belfast) and p286 (Newcastle): their
first two available utterances, silence trimmed, joined with 0.35 s gaps, 24 kHz mono, about 12 s, with the
corpus transcripts as `ref.txt`. OmniVoice's voice design has no Scottish, Northern Irish or Geordie accent; a
clone carries its reference's accent. Whisper (VoiceLab STT) checked every line; three were re-seeded.
