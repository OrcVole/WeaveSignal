// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! An episode file's tags and cover art, written with lofty (ADR-0019).
//!
//! This is the last thing done to a published file: report 08 found that ffmpeg cannot write
//! art into Ogg Opus and that an ffmpeg remux silently drops it, so nothing may touch the file
//! after this. Ogg Opus takes the art as a `METADATA_BLOCK_PICTURE` comment, FLAC as a
//! `PICTURE` block, MP3 as an ID3v2 `APIC` frame and AAC in MP4 as `covr`. WebM and Matroska
//! cannot carry it here (WebM cannot at all); a video archive's art is carried in the feed.
//!
//! lofty is pinned at 0.25.5, whose release fixes the Ogg corruption on files without
//! page-aligned packets (lofty issue 729) that report 08 warned of; the tests check that the
//! audio packets are unchanged after a write.

use std::path::Path;

use lofty::config::WriteOptions;
use lofty::file::FileType;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::Tag;

/// What an episode file says about itself.
#[derive(Clone, Debug, PartialEq)]
pub struct EpisodeTags {
    /// "Ghost Train night".
    pub title: String,
    /// The show, as the album.
    pub show: String,
    /// The hosts, as the artist.
    pub hosts: Vec<String>,
    /// The station, as the album artist and the publisher.
    pub station: String,
    /// The series, as the disc number.
    pub series: Option<u32>,
    /// The episode, as the track number.
    pub episode: Option<u32>,
    /// The air date, `2026-10-24`.
    pub date: Option<jiff::civil::Date>,
    pub description: String,
    pub copyright: Option<String>,
    /// The episode's page.
    pub url: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum TagError {
    #[error("{0} files cannot carry cover art here; put the art in the feed")]
    NoArt(String),
    #[error("the art must be a PNG")]
    NotPng,
    #[error("cannot read the file's tags: {0}")]
    Read(String),
    #[error("cannot write the file's tags: {0}")]
    Write(String),
}

/// The formats an episode is published in, and the tag each carries.
pub fn supported(path: &Path) -> Result<FileType, TagError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "opus" | "ogg" => Ok(FileType::Opus),
        "flac" => Ok(FileType::Flac),
        "mp3" => Ok(FileType::Mpeg),
        "m4a" | "mp4" | "aac" => Ok(FileType::Mp4),
        other => Err(TagError::NoArt(if other.is_empty() {
            "These".to_owned()
        } else {
            other.to_uppercase()
        })),
    }
}

/// Write the tags and the cover art (the card's square, a PNG) into an episode file, in place.
/// Any earlier tags of the same kind are replaced; the audio is not touched.
pub fn write(path: &Path, tags: &EpisodeTags, art_png: &[u8]) -> Result<(), TagError> {
    supported(path)?;
    if !art_png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(TagError::NotPng);
    }
    let mut file = lofty::read_from_path(path).map_err(|e| TagError::Read(e.to_string()))?;
    let kind = file.primary_tag_type();
    let mut tag = Tag::new(kind);
    tag.set_title(tags.title.clone());
    tag.set_album(tags.show.clone());
    if !tags.hosts.is_empty() {
        tag.set_artist(tags.hosts.join(", "));
    }
    tag.insert_text(ItemKey::AlbumArtist, tags.station.clone());
    tag.insert_text(ItemKey::Publisher, tags.station.clone());
    if let Some(n) = tags.episode {
        tag.set_track(n);
    }
    if let Some(n) = tags.series {
        tag.set_disk(n);
    }
    if let Some(d) = tags.date {
        tag.insert_text(ItemKey::RecordingDate, d.to_string());
    }
    if !tags.description.is_empty() {
        tag.set_comment(tags.description.clone());
    }
    if let Some(c) = &tags.copyright {
        tag.insert_text(ItemKey::CopyrightMessage, c.clone());
    }
    if let Some(u) = &tags.url {
        tag.insert_text(ItemKey::PodcastUrl, u.clone());
    }
    tag.set_genre("Podcast".to_owned());
    tag.push_picture(
        Picture::unchecked(art_png.to_vec())
            .pic_type(PictureType::CoverFront)
            .mime_type(MimeType::Png)
            .description("Episode card")
            .build(),
    );
    file.insert_tag(tag);
    file.save_to_path(path, WriteOptions::default())
        .map_err(|e| TagError::Write(e.to_string()))
}

/// What an episode file carries, read back: the title, album, artist, track, disc, and the
/// front cover's bytes.
#[derive(Debug, PartialEq)]
pub struct ReadBack {
    pub title: Option<String>,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub track: Option<u32>,
    pub disk: Option<u32>,
    pub genre: Option<String>,
    pub cover: Option<Vec<u8>>,
    pub duration_ms: u64,
}

pub fn read(path: &Path) -> Result<ReadBack, TagError> {
    let file = lofty::read_from_path(path).map_err(|e| TagError::Read(e.to_string()))?;
    let duration_ms = u64::try_from(file.properties().duration().as_millis()).unwrap_or(u64::MAX);
    let Some(tag) = file.primary_tag() else {
        return Ok(ReadBack {
            title: None,
            album: None,
            artist: None,
            track: None,
            disk: None,
            genre: None,
            cover: None,
            duration_ms,
        });
    };
    Ok(ReadBack {
        title: tag.title().map(|s| s.into_owned()),
        album: tag.album().map(|s| s.into_owned()),
        artist: tag.artist().map(|s| s.into_owned()),
        track: tag.track(),
        disk: tag.disk(),
        genre: tag.genre().map(|s| s.into_owned()),
        // MP4's `covr` keeps no picture type: there the first picture is the cover.
        cover: tag
            .pictures()
            .iter()
            .find(|p| p.pic_type() == PictureType::CoverFront)
            .or_else(|| tag.pictures().first())
            .map(|p| p.data().to_vec()),
        duration_ms,
    })
}
