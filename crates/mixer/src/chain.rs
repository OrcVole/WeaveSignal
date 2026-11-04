// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! An input chain: the telephone band (a caller's, when it is on), expander, compressor, make-up
//! gain (steered by the leveller when it is on), limiter.

use serde::{Deserialize, Serialize};

use crate::{
    Compressor, CompressorSettings, Expander, ExpanderSettings, Leveller, LevellerSettings,
    Limiter, LimiterSettings, SettingsError, Telephone, TelephoneSettings, db_to_lin,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ChainSettings {
    /// Before all else: the phone line's band, so a caller sounds like a call (on by default for
    /// callers while we run diagnostics, operator 2026-11-01).
    pub telephone: Option<TelephoneSettings>,
    /// Turns the microphone down while its owner is quiet (headphone bleed).
    pub expander: Option<ExpanderSettings>,
    pub compressor: Option<CompressorSettings>,
    /// The gain after the compressor, in dB; the leveller's starting point when it is on.
    pub makeup_db: f32,
    pub leveller: Option<LevellerSettings>,
    pub limiter: Option<LimiterSettings>,
}

impl ChainSettings {
    /// The spike's presenter chain from the live runs: a downward expander under -34 dBFS,
    /// compressor, +5.6 dB steered toward -25 dB of speech, and a limiter at -1 dBFS.
    pub fn presenter() -> Self {
        Self {
            telephone: None,
            expander: Some(ExpanderSettings::default()),
            compressor: Some(CompressorSettings::default()),
            makeup_db: 5.6,
            leveller: Some(LevellerSettings::default()),
            limiter: Some(LimiterSettings::default()),
        }
    }

    /// A caller's chain while we run diagnostics: the phone line's band and a light compressor,
    /// so a caller sounds like a call and sits level beside the studio.
    pub fn caller() -> Self {
        Self {
            telephone: Some(TelephoneSettings::default()),
            compressor: Some(CompressorSettings::default()),
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.telephone.is_none()
            && self.expander.is_none()
            && self.compressor.is_none()
            && self.leveller.is_none()
            && self.limiter.is_none()
            && self.makeup_db == 0.0
    }

    pub fn validate(&self) -> Result<(), SettingsError> {
        if !(-24.0..=30.0).contains(&self.makeup_db) {
            return Err(SettingsError(format!(
                "makeup_db {} must be -24 to 30",
                self.makeup_db
            )));
        }
        if let Some(t) = &self.telephone {
            t.validate()?;
        }
        if let Some(e) = &self.expander {
            e.validate()?;
        }
        if let Some(c) = &self.compressor {
            c.validate()?;
        }
        if let Some(l) = &self.leveller {
            l.validate()?;
        }
        if let Some(l) = &self.limiter {
            l.validate()?;
        }
        Ok(())
    }
}

pub struct Chain {
    telephone: Option<Telephone>,
    expander: Option<Expander>,
    compressor: Option<Compressor>,
    makeup: f32,
    leveller: Option<Leveller>,
    limiter: Option<Limiter>,
}

impl Chain {
    pub fn new(settings: &ChainSettings, channels: usize) -> Self {
        Self {
            telephone: settings
                .telephone
                .as_ref()
                .map(|t| Telephone::new(t, channels)),
            expander: settings
                .expander
                .as_ref()
                .map(|e| Expander::new(e, channels)),
            compressor: settings
                .compressor
                .as_ref()
                .map(|c| Compressor::new(c, channels)),
            makeup: db_to_lin(settings.makeup_db),
            leveller: settings
                .leveller
                .as_ref()
                .map(|l| Leveller::new(l, channels, settings.makeup_db)),
            limiter: settings.limiter.as_ref().map(|l| Limiter::new(l, channels)),
        }
    }

    /// The gain the leveller has reached, if it is on.
    pub fn leveller_gain_db(&self) -> Option<f32> {
        self.leveller.as_ref().map(Leveller::gain_db)
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        if let Some(t) = &mut self.telephone {
            t.process(samples);
        }
        if let Some(e) = &mut self.expander {
            e.process(samples);
        }
        if let Some(c) = &mut self.compressor {
            c.process(samples);
        }
        match &mut self.leveller {
            Some(l) => l.process(samples),
            None if self.makeup != 1.0 => samples.iter_mut().for_each(|s| *s *= self.makeup),
            None => {}
        }
        if let Some(l) = &mut self.limiter {
            l.process(samples);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_signal::{peak, sine};

    #[test]
    fn the_presenter_chain_never_exceeds_its_ceiling() {
        let s = ChainSettings::presenter();
        s.validate().unwrap();
        let mut c = Chain::new(&s, 2);
        let mut x = sine(200.0, 0.95, 3.0);
        for b in x.chunks_mut(960) {
            c.process(b);
        }
        assert!(peak(&x) <= db_to_lin(-1.0) + 1e-6);
    }

    #[test]
    fn the_chain_reads_from_settings() {
        let text = r#"
makeup_db = 3.0
[compressor]
threshold_db = -18.0
ratio = 3.0
[limiter]
ceiling_db = -2.0
"#;
        let s: ChainSettings = toml::from_str(text).unwrap();
        assert_eq!(s.makeup_db, 3.0);
        assert_eq!(s.compressor.unwrap().ratio, 3.0);
        assert_eq!(s.compressor.unwrap().attack_ms, 5.0, "unset fields default");
        assert!(s.leveller.is_none());
        assert_eq!(s.limiter.unwrap().ceiling_db, -2.0);
        assert!(ChainSettings::default().is_empty());
    }

    #[test]
    fn the_presenter_chain_starts_with_the_expander_and_keeps_bleed_down() {
        // Bleed at -50 dBFS alone: the expander takes it 18 dB down before the make-up gain,
        // so the chain's output stays well under what the raw bleed plus make-up would be.
        let s = ChainSettings::presenter();
        assert_eq!(s.expander, Some(ExpanderSettings::default()));
        let mut c = Chain::new(&s, 2);
        let mut bleed = sine(440.0, db_to_lin(-50.0), 2.0);
        for b in bleed.chunks_mut(960) {
            c.process(b);
        }
        let out = crate::test_signal::rms_db(&bleed[bleed.len() / 2..]);
        assert!(out < -50.0 - 3.01 - 6.0, "bleed came out at {out:.1} dBFS");
        let text = "[expander]\nthreshold_db = -40.0\n";
        let s: ChainSettings = toml::from_str(text).unwrap();
        assert_eq!(s.expander.unwrap().threshold_db, -40.0);
        assert_eq!(s.expander.unwrap().ratio, 3.0, "unset fields default");
    }

    #[test]
    fn an_empty_chain_changes_nothing() {
        let mut c = Chain::new(&ChainSettings::default(), 2);
        let x = sine(440.0, 0.5, 0.1);
        let mut y = x.clone();
        c.process(&mut y);
        assert_eq!(x, y);
    }
}
