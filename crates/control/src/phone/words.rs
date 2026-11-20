// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The phone words: what a phone caller hears before they join the queue (Phase 17f; the
//! operator's picks of 2026-11-20).
//!
//! **Three cases.** The station's own recording, made in the studio app (the default the
//! operator chose, phone-words a); until there is one, **a short recording that ships with
//! WeaveSignal** ([`DEFAULT_WORDS`], read in a synthetic voice); or no words at all, when the
//! station turns `phone.consent_words` off (an internet radio station may not need them).
//!
//! **The default words are generic on purpose.** A file made once cannot say the station's name,
//! how long calls are kept or the takedown contact, so they say only who you have reached, that
//! the call may go on air and be recorded, and to press a key. Their version is the hash of that
//! fixed text, so it never changes under the station. The station's own recording binds the
//! words it was made from: those are the browser's consent words (`join.rs`) with the key
//! sentence after them, so changing the template, the station's name, the retention or the
//! contact closes the line until the words are recorded again (design 2.7).
//!
//! **One key rule everywhere: any key carries on.** The default recording says so; the studio
//! app asks the station to say so in theirs. A robocall rarely presses a key; a person does.

use crate::join::consent_version;

/// The words of the recording that ships with WeaveSignal: who you have reached, that the call
/// may go on air and be recorded, press a key.
pub const DEFAULT_WORDS: &str = "You're through to the studio, and your call may go on air and be recorded. Press any key to carry on.";
/// The shipped recordings' file names, in the image's folder of them (`phone.defaults_dir`) and,
/// copied there at start, in the station's phone-words directory, where the desk plays them.
/// Ogg Opus, about 55 KB each (`assets/phone/`, CC BY 4.0: NOTICES.md).
pub const ALBA_FILE: &str = "phone-words-alba.opus";
pub const EDINBURGH_FILE: &str = "phone-words-edinburgh.opus";
/// Every shipped recording.
pub const SHIPPED: &[&str] = &[ALBA_FILE, EDINBURGH_FILE];

/// The description of `phone.default_voice`, for the studio beside its choice and for the docs.
pub const DEFAULT_VOICE_HELP: &str = "Which recording of the phone words plays when the station has not recorded its own: alba, a Scottish woman's voice (the default), or edinburgh, a man's voice from Edinburgh; both say \"You're through to the studio, and your call may go on air and be recorded. Press any key to carry on.\" Choose own to play only the station's own recording: until there is one, the line takes no calls while the words are on. The station's own recording, once made, always plays instead of either.";

/// Which recording plays when the station has none of its own (`phone.default_voice`; the
/// operator's pick of 2026-11-20: Alba by default, Edinburgh the alternative).
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    serde::Deserialize,
    serde::Serialize,
    utoipa::ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum PhoneVoice {
    /// Alba, a Scottish woman (Piper's `en_GB-alba-medium`, from the Alba corpus).
    #[default]
    Alba,
    /// A man from Edinburgh (Piper's `en_GB-vctk-medium`, VCTK speaker p285).
    Edinburgh,
    /// No shipped recording: the station's own only.
    Own,
}

impl PhoneVoice {
    /// The shipped recording's file; none for `own`.
    pub fn file(self) -> Option<&'static str> {
        match self {
            PhoneVoice::Alba => Some(ALBA_FILE),
            PhoneVoice::Edinburgh => Some(EDINBURGH_FILE),
            PhoneVoice::Own => None,
        }
    }
}
/// The sentence a station's own recording ends with.
pub const KEY_SENTENCE: &str = "Press any key to carry on.";
/// The text a consent record keeps when the words were off: nothing was played or agreed.
pub const NOT_ASKED: &str =
    "(No words were played: the station has the phone line's consent words turned off.)";

/// Copy the shipped recordings from the image's folder into the station's phone-words folder,
/// where the desk plays them, when a copy is missing or differs (a new image's). A shipped file
/// the image lacks is left out, and the line says `default_missing` if it is the one chosen.
/// How many were copied.
pub fn install_shipped(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<usize> {
    let mut copied = 0;
    for name in SHIPPED {
        let src = from.join(name);
        let Ok(bytes) = std::fs::read(&src) else {
            continue;
        };
        let dst = to.join(name);
        if std::fs::read(&dst).is_ok_and(|have| have == bytes) {
            continue;
        }
        std::fs::create_dir_all(to)?;
        // Written beside, then renamed, so the desk never reads half a file.
        let tmp = to.join(format!(".{name}.part"));
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &dst)?;
        copied += 1;
    }
    Ok(copied)
}

/// The station's own recording of the words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StationRecording {
    /// The words it was made from.
    pub text: String,
    /// `<uuid>.flac` in the phone-words directory.
    pub file: String,
    /// The gain the desk plays it at, in millibels (hundredths of a dB), from the media worker.
    pub gain_mb: i32,
}

/// What a phone caller hears first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PhoneWords {
    /// `phone.consent_words` is off.
    Off,
    /// A recording that ships with WeaveSignal (made at the loudness the desk wants), in the
    /// voice chosen; with `own`, none: the line stays closed until the station records its own.
    Default(PhoneVoice),
    /// The station's own recording.
    Station(StationRecording),
}

/// The words a station's recording should say today: the browser's consent words, filled, and
/// the key sentence.
pub fn station_words_today(filled_consent_words: &str) -> String {
    format!("{} {KEY_SENTENCE}", filled_consent_words.trim())
}

/// Why the line is closed, if it is, as far as the words go.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WordsProblem {
    /// The station's recording was made from other words than today's (design 2.7).
    RecordingOutOfDate,
    /// The default recording is not in the image (a build without it).
    DefaultMissing,
    /// `phone.default_voice` is `own` and the station has not recorded its words yet.
    NoRecording,
}

impl PhoneWords {
    /// Which words play: none with the setting off; the station's own recording whenever it
    /// has one (it wins over both shipped voices); else the shipped voice chosen.
    pub fn choose(
        consent_words_on: bool,
        voice: PhoneVoice,
        station: Option<StationRecording>,
    ) -> Self {
        match (consent_words_on, station) {
            (false, _) => PhoneWords::Off,
            (true, Some(r)) => PhoneWords::Station(r),
            (true, None) => PhoneWords::Default(voice),
        }
    }

    /// The gain the desk plays the file at, in millibels.
    pub fn gain_mb(&self) -> i32 {
        match self {
            PhoneWords::Station(r) => r.gain_mb,
            _ => 0,
        }
    }

    /// The text the caller hears (and the consent record keeps).
    pub fn text(&self) -> &str {
        match self {
            PhoneWords::Off => NOT_ASKED,
            PhoneWords::Default(_) => DEFAULT_WORDS,
            PhoneWords::Station(r) => &r.text,
        }
    }

    /// The words' version, as the browser's: the first 16 hex digits of their SHA-256.
    pub fn version(&self) -> String {
        consent_version(self.text())
    }

    /// The file the desk plays; none with the words off, or with `own` and no recording yet
    /// (which [`Self::problem`] reports, so the line is closed).
    pub fn file(&self) -> Option<&str> {
        match self {
            PhoneWords::Off => None,
            PhoneWords::Default(v) => v.file(),
            PhoneWords::Station(r) => Some(&r.file),
        }
    }

    /// Whether the words let the line open: the station's recording must be of today's words,
    /// and the default recording must be there.
    pub fn problem(&self, today: &str, default_present: bool) -> Option<WordsProblem> {
        match self {
            PhoneWords::Off => None,
            PhoneWords::Default(PhoneVoice::Own) => Some(WordsProblem::NoRecording),
            PhoneWords::Default(_) => (!default_present).then_some(WordsProblem::DefaultMissing),
            PhoneWords::Station(r) => (consent_version(&r.text)
                != consent_version(&station_words_today(today)))
            .then_some(WordsProblem::RecordingOutOfDate),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_words_are_brief_and_fixed() {
        let w = PhoneWords::choose(true, PhoneVoice::default(), None);
        assert_eq!(w, PhoneWords::Default(PhoneVoice::Alba));
        assert_eq!(w.text(), DEFAULT_WORDS);
        assert_eq!(w.version().len(), 16);
        assert_eq!(w.file(), Some(ALBA_FILE));
        // Two sentences, and the key rule the gate keeps.
        assert_eq!(DEFAULT_WORDS.matches(". ").count() + 1, 2);
        assert!(DEFAULT_WORDS.ends_with(KEY_SENTENCE));
        assert!(DEFAULT_WORDS.split_whitespace().count() <= 25);
        // They never close the line when the station changes its own words.
        assert_eq!(w.problem("Anything at all.", true), None);
        assert_eq!(
            w.problem("Anything at all.", false),
            Some(WordsProblem::DefaultMissing)
        );
    }

    #[test]
    fn off_plays_nothing_and_says_so() {
        let w = PhoneWords::choose(
            false,
            PhoneVoice::Alba,
            Some(StationRecording {
                text: "x".into(),
                file: "y.flac".into(),
                gain_mb: 0,
            }),
        );
        assert_eq!(w, PhoneWords::Off);
        assert_eq!(w.file(), None);
        assert_eq!(w.text(), NOT_ASKED);
        assert_eq!(w.problem("x", false), None);
    }

    #[test]
    fn a_stations_recording_binds_its_words() {
        let today = "You'll be live on Kettle FM, and we may keep your call for two years.";
        let rec = station_words_today(today);
        let w = PhoneWords::choose(
            true,
            PhoneVoice::Edinburgh,
            Some(StationRecording {
                text: rec.clone(),
                file: "abc.flac".into(),
                gain_mb: -350,
            }),
        );
        assert_eq!(w.gain_mb(), -350);
        assert_eq!(w.file(), Some("abc.flac"));
        assert_eq!(w.problem(today, true), None);
        // Change any filled field and the recording is out of date.
        let tomorrow = today.replace("two years", "30 days");
        assert_eq!(
            w.problem(&tomorrow, true),
            Some(WordsProblem::RecordingOutOfDate)
        );
        assert_ne!(w.version(), PhoneWords::Default(PhoneVoice::Alba).version());
    }

    #[test]
    fn the_shipped_recordings_are_copied_once_and_again_when_they_change() {
        let base = std::env::temp_dir().join(format!("ws-phone-shipped-{}", std::process::id()));
        let (from, to) = (base.join("image"), base.join("words"));
        std::fs::create_dir_all(&from).unwrap();
        std::fs::write(from.join(ALBA_FILE), b"alba").unwrap();
        assert_eq!(
            install_shipped(&from, &to).unwrap(),
            1,
            "edinburgh is absent: left out"
        );
        assert_eq!(std::fs::read(to.join(ALBA_FILE)).unwrap(), b"alba");
        assert_eq!(install_shipped(&from, &to).unwrap(), 0, "already there");
        std::fs::write(from.join(ALBA_FILE), b"alba 2").unwrap();
        std::fs::write(from.join(EDINBURGH_FILE), b"edinburgh").unwrap();
        assert_eq!(install_shipped(&from, &to).unwrap(), 2);
        assert_eq!(std::fs::read(to.join(ALBA_FILE)).unwrap(), b"alba 2");
        // The repository's own recordings are the two the image ships.
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/phone");
        for name in SHIPPED {
            let len = std::fs::metadata(assets.join(name)).unwrap().len();
            assert!((10_000..200_000).contains(&len), "{name}: {len} bytes");
            assert!(
                assets.join(format!("{name}.license")).is_file(),
                "{name}.license"
            );
        }
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn the_voice_chooses_the_shipped_file_and_the_stations_own_wins() {
        let own = StationRecording {
            text: station_words_today("Words."),
            file: "abc.flac".into(),
            gain_mb: 0,
        };
        for (voice, file) in [
            (PhoneVoice::Alba, Some(ALBA_FILE)),
            (PhoneVoice::Edinburgh, Some(EDINBURGH_FILE)),
            (PhoneVoice::Own, None),
        ] {
            let w = PhoneWords::choose(true, voice, None);
            assert_eq!(w.file(), file, "{voice:?}");
            // The station's own recording plays instead of any shipped voice.
            let w = PhoneWords::choose(true, voice, Some(own.clone()));
            assert_eq!(w.file(), Some("abc.flac"), "{voice:?}");
            assert_eq!(w.problem("Words.", false), None);
        }
        // Both shipped voices say the same words, so a caller's consent is to the same version.
        assert_eq!(
            PhoneWords::Default(PhoneVoice::Alba).version(),
            PhoneWords::Default(PhoneVoice::Edinburgh).version()
        );
        // `own` with nothing recorded closes the line; a missing shipped file closes it too.
        assert_eq!(
            PhoneWords::choose(true, PhoneVoice::Own, None).problem("x", true),
            Some(WordsProblem::NoRecording)
        );
        assert_eq!(
            PhoneWords::choose(true, PhoneVoice::Edinburgh, None).problem("x", false),
            Some(WordsProblem::DefaultMissing)
        );
        // The setting's words in a settings file, and its description.
        #[derive(serde::Deserialize)]
        struct S {
            v: PhoneVoice,
        }
        for (t, v) in [
            ("alba", PhoneVoice::Alba),
            ("edinburgh", PhoneVoice::Edinburgh),
            ("own", PhoneVoice::Own),
        ] {
            assert_eq!(toml::from_str::<S>(&format!("v = \"{t}\"")).unwrap().v, v);
        }
        assert!(toml::from_str::<S>("v = \"aberdeen\"").is_err());
        assert!(DEFAULT_VOICE_HELP.contains("alba") && DEFAULT_VOICE_HELP.contains("own"));
        assert_eq!(SHIPPED.len(), 2);
    }
}
