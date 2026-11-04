// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WeaveSignal's podcast feeds (M8).
//!
//! - [`rss`]: a show's feed, RSS 2.0 with the iTunes and Podcasting 2.0 namespaces, an MP3 or
//!   AAC enclosure and the Opus copy as `podcast:alternateEnclosure` (ADR-0018); the tag order
//!   and choices are ported from Castopod (ADR-0014).
//! - [`tags`]: an episode file's tags and cover art, written with lofty as the last step after
//!   encoding (ADR-0019, report 08).
//! - [`taxonomy`]: the Podcast Taxonomy Project's roles for `podcast:person`.

pub mod rss;
pub mod tags;
pub mod taxonomy;

pub use rss::{Channel, EpisodeType, Image, Item, Media, Person, podcast_guid};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum FeedError {
    #[error("the feed needs {0}")]
    Missing(&'static str),
    #[error("{0}")]
    Invalid(String),
    #[error("the feed could not be written: {0}")]
    Write(String),
}
