// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The vision mixer's rule: which picture is on air (ADR-0009's cut-only switcher). It follows
//! the audio: the person on air who is speaking, with a camera, is the shot; a cut waits until
//! the shot has been held a moment and the new speaker has been louder for a moment, so a cough
//! does not cut. With nobody on air on camera, the slate (ADR-0010). The studio can override:
//! the slate (desk B's F7), or one person's camera, until it hands back to the rule.
//!
//! Plain Rust, no GStreamer: [`Director::decide`] is called once a frame with what the mixing
//! pass knows, and answers with the shot.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Who chooses the picture.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum PictureMode {
    /// The rule: the on-air speaker, or the slate.
    #[default]
    Auto,
    /// The slate, whoever is speaking (F7).
    Slate,
    /// One person's camera, whoever is speaking; the slate while their camera has no picture.
    Person { identity: String },
}

/// What is on air.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "shot", rename_all = "snake_case")]
pub enum Shot {
    Slate,
    Person { identity: String },
}

/// One person, as the vision mixer sees them this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub identity: String,
    /// On air, present and unmuted, not in a private talk: their voice is in the programme.
    pub audible: bool,
    /// Their camera has a picture younger than the staleness limit.
    pub camera: bool,
    /// Their voice's momentary loudness (400 ms), LUFS; `None` for silence.
    pub loudness: Option<f32>,
}

/// The rule's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rule {
    /// Louder than this, a voice is speaking.
    pub speaking_lufs: f32,
    /// A new speaker must be this much louder than the shot's (or the shot silent).
    pub margin_db: f32,
    /// For this long before the cut.
    pub switch_after: Duration,
    /// The shortest shot.
    pub min_shot: Duration,
}

impl Default for Rule {
    fn default() -> Self {
        Rule {
            speaking_lufs: -45.0,
            margin_db: 3.0,
            switch_after: Duration::from_millis(600),
            min_shot: Duration::from_millis(2000),
        }
    }
}

/// The switcher's memory between frames.
#[derive(Debug)]
pub struct Director {
    rule: Rule,
    shot: Shot,
    since: Instant,
    /// Who is louder than the shot, and since when.
    challenger: Option<(String, Instant)>,
}

impl Director {
    pub fn new(rule: Rule, now: Instant) -> Self {
        Director {
            rule,
            shot: Shot::Slate,
            since: now,
            challenger: None,
        }
    }

    pub fn shot(&self) -> &Shot {
        &self.shot
    }

    /// How long the shot has been on air.
    pub fn held(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.since)
    }

    fn cut(&mut self, to: Shot, now: Instant) -> bool {
        self.challenger = None;
        if to == self.shot {
            return false;
        }
        self.shot = to;
        self.since = now;
        true
    }

    /// The shot for this frame; `true` with it when it is a cut.
    pub fn decide(&mut self, mode: &PictureMode, people: &[Candidate], now: Instant) -> bool {
        let speaking = |c: &Candidate| {
            c.loudness
                .filter(|l| *l >= self.rule.speaking_lufs)
                .unwrap_or(f32::NEG_INFINITY)
        };
        match mode {
            PictureMode::Slate => return self.cut(Shot::Slate, now),
            PictureMode::Person { identity } => {
                let on = people.iter().any(|c| &c.identity == identity && c.camera);
                let to = if on {
                    Shot::Person {
                        identity: identity.clone(),
                    }
                } else {
                    Shot::Slate
                };
                return self.cut(to, now);
            }
            PictureMode::Auto => {}
        }
        let eligible: Vec<&Candidate> = people.iter().filter(|c| c.audible && c.camera).collect();
        let loudest = eligible
            .iter()
            .copied()
            .max_by(|a, b| speaking(a).total_cmp(&speaking(b)));
        let Some(loudest) = loudest else {
            return self.cut(Shot::Slate, now);
        };
        let current = match &self.shot {
            Shot::Person { identity } => eligible.iter().copied().find(|c| &c.identity == identity),
            Shot::Slate => None,
        };
        let Some(current) = current else {
            // The shot is the slate, or its person lost their camera or left the air: the
            // loudest who can be seen, at once (a speaker if anyone speaks).
            let to = Shot::Person {
                identity: loudest.identity.clone(),
            };
            return self.cut(to, now);
        };
        if loudest.identity == current.identity
            || speaking(loudest) == f32::NEG_INFINITY
            || speaking(loudest) < speaking(current) + self.rule.margin_db
        {
            self.challenger = None;
            return false;
        }
        let since = match &self.challenger {
            Some((id, t)) if id == &loudest.identity => *t,
            _ => {
                self.challenger = Some((loudest.identity.clone(), now));
                now
            }
        };
        if now.saturating_duration_since(since) >= self.rule.switch_after
            && self.held(now) >= self.rule.min_shot
        {
            let to = Shot::Person {
                identity: loudest.identity.clone(),
            };
            return self.cut(to, now);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, audible: bool, camera: bool, lufs: Option<f32>) -> Candidate {
        Candidate {
            identity: id.into(),
            audible,
            camera,
            loudness: lufs,
        }
    }

    fn person(id: &str) -> Shot {
        Shot::Person {
            identity: id.into(),
        }
    }

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn nobody_on_camera_is_the_slate() {
        let t0 = Instant::now();
        let mut d = Director::new(Rule::default(), t0);
        d.decide(&PictureMode::Auto, &[], t0);
        assert_eq!(d.shot(), &Shot::Slate);
        // On air but no camera; on camera but off air: still the slate.
        let people = [
            c("a", true, false, Some(-20.0)),
            c("b", false, true, Some(-20.0)),
        ];
        d.decide(&PictureMode::Auto, &people, t0 + MS(40));
        assert_eq!(d.shot(), &Shot::Slate);
    }

    #[test]
    fn the_first_person_on_air_on_camera_is_cut_to_at_once() {
        let t0 = Instant::now();
        let mut d = Director::new(Rule::default(), t0);
        let cut = d.decide(&PictureMode::Auto, &[c("a", true, true, None)], t0);
        assert!(cut);
        assert_eq!(d.shot(), &person("a"));
    }

    #[test]
    fn a_new_speaker_is_cut_to_after_the_wait_and_the_shortest_shot() {
        let t0 = Instant::now();
        let mut d = Director::new(Rule::default(), t0);
        let a_speaks = [
            c("a", true, true, Some(-20.0)),
            c("b", true, true, Some(-60.0)),
        ];
        d.decide(&PictureMode::Auto, &a_speaks, t0);
        assert_eq!(d.shot(), &person("a"));
        let b_speaks = [c("a", true, true, None), c("b", true, true, Some(-22.0))];
        // B speaks from 1 s: the shot is held until 2 s, B has spoken 600 ms by 1.6 s.
        for ms in (1000..2000).step_by(50) {
            d.decide(&PictureMode::Auto, &b_speaks, t0 + MS(ms));
            assert_eq!(d.shot(), &person("a"), "at {ms} ms");
        }
        assert!(d.decide(&PictureMode::Auto, &b_speaks, t0 + MS(2000)));
        assert_eq!(d.shot(), &person("b"));
    }

    #[test]
    fn a_cough_does_not_cut() {
        let t0 = Instant::now();
        let mut d = Director::new(Rule::default(), t0);
        d.decide(&PictureMode::Auto, &[c("a", true, true, Some(-20.0))], t0);
        let cough = [
            c("a", true, true, Some(-25.0)),
            c("b", true, true, Some(-15.0)),
        ];
        let quiet = [
            c("a", true, true, Some(-25.0)),
            c("b", true, true, Some(-70.0)),
        ];
        let mut t = t0 + MS(3000);
        for _ in 0..10 {
            d.decide(&PictureMode::Auto, &cough, t);
            t += MS(50);
        }
        d.decide(&PictureMode::Auto, &quiet, t);
        t += MS(50);
        for _ in 0..10 {
            d.decide(&PictureMode::Auto, &cough, t);
            t += MS(50);
        }
        assert_eq!(d.shot(), &person("a"));
        // Not louder by the margin: no cut however long.
        let close = [
            c("a", true, true, Some(-20.0)),
            c("b", true, true, Some(-18.5)),
        ];
        for _ in 0..100 {
            d.decide(&PictureMode::Auto, &close, t);
            t += MS(50);
        }
        assert_eq!(d.shot(), &person("a"));
    }

    #[test]
    fn taking_the_shot_off_air_cuts_away_at_once() {
        let t0 = Instant::now();
        let mut d = Director::new(Rule::default(), t0);
        d.decide(
            &PictureMode::Auto,
            &[c("a", true, true, Some(-20.0)), c("b", true, true, None)],
            t0,
        );
        assert_eq!(d.shot(), &person("a"));
        d.decide(
            &PictureMode::Auto,
            &[c("a", false, true, Some(-20.0)), c("b", true, true, None)],
            t0 + MS(100),
        );
        assert_eq!(d.shot(), &person("b"));
        d.decide(
            &PictureMode::Auto,
            &[c("a", false, true, None), c("b", true, false, None)],
            t0 + MS(200),
        );
        assert_eq!(d.shot(), &Shot::Slate);
    }

    #[test]
    fn the_override_holds_until_handed_back() {
        let t0 = Instant::now();
        let mut d = Director::new(Rule::default(), t0);
        let people = [
            c("a", true, true, Some(-20.0)),
            c("b", true, true, Some(-60.0)),
        ];
        d.decide(&PictureMode::Slate, &people, t0);
        assert_eq!(d.shot(), &Shot::Slate);
        let b = PictureMode::Person {
            identity: "b".into(),
        };
        d.decide(&b, &people, t0 + MS(50));
        assert_eq!(d.shot(), &person("b"));
        // B's camera goes: the slate, not someone else.
        d.decide(
            &b,
            &[c("a", true, true, Some(-20.0)), c("b", true, false, None)],
            t0 + MS(100),
        );
        assert_eq!(d.shot(), &Shot::Slate);
        d.decide(&PictureMode::Auto, &people, t0 + MS(150));
        assert_eq!(d.shot(), &person("a"));
    }

    #[test]
    fn modes_read_and_write_as_the_socket_words() {
        let m: PictureMode = serde_json::from_str(r#"{"mode":"slate"}"#).unwrap();
        assert_eq!(m, PictureMode::Slate);
        let m: PictureMode =
            serde_json::from_str(r#"{"mode":"person","identity":"callum"}"#).unwrap();
        assert_eq!(
            m,
            PictureMode::Person {
                identity: "callum".into()
            }
        );
        assert_eq!(
            serde_json::to_string(&Shot::Slate).unwrap(),
            r#"{"shot":"slate"}"#
        );
    }
}
