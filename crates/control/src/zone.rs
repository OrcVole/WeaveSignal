// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The station's time zone, one setting (review 0025, S1). It lives in `playout_settings`,
//! where the studio's settings change it; a new station takes `publishing.zone` from the
//! configuration as its first value. Automation's clock hours, show slots, episode times and
//! the Schedule tab all read this one zone, so a show at 10:00 and automation's hour 10 are
//! the same hour.

use std::sync::RwLock;

use jiff::tz::TimeZone;

/// The zone's IANA name, shared by the routes that need it.
#[derive(Debug)]
pub struct StationZone(RwLock<String>);

impl StationZone {
    pub fn new(name: &str) -> Self {
        Self(RwLock::new(name.to_owned()))
    }

    /// The IANA name, such as `Europe/London`.
    pub fn name(&self) -> String {
        self.0.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// The zone itself; UTC if the name stopped resolving (both writers check it first).
    pub fn get(&self) -> TimeZone {
        TimeZone::get(&self.name()).unwrap_or(TimeZone::UTC)
    }

    pub fn set(&self, name: &str) {
        *self.0.write().unwrap_or_else(|p| p.into_inner()) = name.to_owned();
    }
}

impl Default for StationZone {
    fn default() -> Self {
        Self::new("UTC")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_is_seen_by_every_holder() {
        let z = std::sync::Arc::new(StationZone::new("Europe/London"));
        let other = z.clone();
        z.set("America/New_York");
        assert_eq!(other.name(), "America/New_York");
        assert_eq!(other.get(), TimeZone::get("America/New_York").unwrap());
    }
}
