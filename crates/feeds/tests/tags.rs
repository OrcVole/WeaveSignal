// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! An episode's tags and art, written last with lofty and read back, in each published format,
//! with the audio checked unchanged: Ogg Opus (a demo cart, CC0), FLAC (made here), MP3 and
//! AAC (`tests/fixtures/`, one second of a 440 Hz tone made with ffmpeg for these tests).

use std::path::{Path, PathBuf};

use weavesignal_cards::{
    CardData, EpisodeCard, Layout, Output, SeriesCard, ShowCard, StationCard, render,
};
use weavesignal_feeds::tags::{self, EpisodeTags, TagError};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("weavesignal-feeds-tags");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// The episode's own card square, as the archive art is (ADR-0054).
fn art() -> Vec<u8> {
    let d = CardData {
        layout: Layout::Slate,
        colour_slot: 3,
        station: StationCard {
            name: "Kettle FM".into(),
            place: "Dunfermline".into(),
            address: "kettle.example".into(),
        },
        show: ShowCard {
            name: "Saturday Night Kettle".into(),
            slug: "saturday-night".into(),
            description: String::new(),
            slot: None,
        },
        series: Some(SeriesCard {
            number: 3,
            title: None,
            first: None,
            last: None,
            episodes: None,
        }),
        episode: Some(EpisodeCard {
            number: 7,
            title: "Ghost Train night".into(),
            air: Some("2026-10-24T20:00[Europe/London]".parse().unwrap()),
        }),
        hosts: vec![
            "Shona Campbell".into(),
            "Catriona MacKenzie-Buchanan".into(),
        ],
        guests: vec!["Firth Lights".into(), "Dr Una Robertson".into()],
    };
    render(&d, Output::Square).unwrap().bytes
}

fn episode_tags() -> EpisodeTags {
    EpisodeTags {
        title: "Ghost Train night".into(),
        show: "Saturday Night Kettle".into(),
        hosts: vec![
            "Shona Campbell".into(),
            "Catriona MacKenzie-Buchanan".into(),
        ],
        station: "Kettle FM".into(),
        series: Some(3),
        episode: Some(7),
        date: Some("2026-10-24".parse().unwrap()),
        description: "Live music, requests and the Forth after dark.".into(),
        copyright: Some("2026 Kettle FM".into()),
        url: Some("https://kettle.example/saturday-night/s3/e7".into()),
    }
}

fn check_read_back(path: &Path, art: &[u8]) {
    let r = tags::read(path).unwrap();
    assert_eq!(r.title.as_deref(), Some("Ghost Train night"), "{path:?}");
    assert_eq!(r.album.as_deref(), Some("Saturday Night Kettle"));
    assert_eq!(
        r.artist.as_deref(),
        Some("Shona Campbell, Catriona MacKenzie-Buchanan")
    );
    assert_eq!(r.track, Some(7));
    assert_eq!(r.disk, Some(3));
    assert_eq!(r.genre.as_deref(), Some("Podcast"));
    assert!(
        r.cover.as_deref() == Some(art),
        "{path:?}: the card, byte for byte"
    );
    assert!(
        r.duration_ms > 500,
        "{path:?}: still plays ({} ms)",
        r.duration_ms
    );
}

/// Every packet after the two Opus headers, in order.
fn ogg_audio_packets(path: &Path) -> Vec<Vec<u8>> {
    let mut r = ogg::PacketReader::new(std::fs::File::open(path).unwrap());
    let mut out = Vec::new();
    while let Some(p) = r.read_packet().unwrap() {
        out.push(p.data);
    }
    assert!(out.len() > 2, "an Opus stream");
    assert!(out[0].starts_with(b"OpusHead"));
    assert!(out[1].starts_with(b"OpusTags"));
    out.split_off(2)
}

#[test]
fn ogg_opus_takes_the_card_as_metadata_block_picture_and_keeps_its_audio() {
    let path = scratch("episode.opus");
    std::fs::copy(root().join("../../assets/carts-demo/ding-1.opus"), &path).unwrap();
    let before = ogg_audio_packets(&path);
    let art = art();
    tags::write(&path, &episode_tags(), &art).unwrap();
    assert_eq!(
        ogg_audio_packets(&path),
        before,
        "the audio packets are untouched"
    );
    check_read_back(&path, &art);
    // The picture is where players look for it: a METADATA_BLOCK_PICTURE comment.
    let mut r = ogg::PacketReader::new(std::fs::File::open(&path).unwrap());
    r.read_packet().unwrap();
    let comments = r.read_packet().unwrap().unwrap().data;
    let needle = b"METADATA_BLOCK_PICTURE=";
    assert!(
        comments
            .windows(needle.len())
            .any(|w| w.eq_ignore_ascii_case(needle)),
        "OpusTags carries the picture"
    );
    // Written again (a re-render after air): one picture, not two.
    tags::write(&path, &episode_tags(), &art).unwrap();
    assert_eq!(ogg_audio_packets(&path), before);
    let file = lofty::read_from_path(&path).unwrap();
    use lofty::prelude::*;
    assert_eq!(file.primary_tag().unwrap().pictures().len(), 1);
}

/// FLAC's audio frames: everything after the last metadata block.
fn flac_frames(bytes: &[u8]) -> &[u8] {
    assert_eq!(&bytes[..4], b"fLaC");
    let mut i = 4;
    loop {
        let last = bytes[i] & 0x80 != 0;
        let len = usize::from(bytes[i + 1]) << 16
            | usize::from(bytes[i + 2]) << 8
            | usize::from(bytes[i + 3]);
        i += 4 + len;
        if last {
            return &bytes[i..];
        }
    }
}

/// The FLAC metadata block types, in order.
fn flac_blocks(bytes: &[u8]) -> Vec<u8> {
    let mut i = 4;
    let mut out = Vec::new();
    loop {
        out.push(bytes[i] & 0x7f);
        let last = bytes[i] & 0x80 != 0;
        let len = usize::from(bytes[i + 1]) << 16
            | usize::from(bytes[i + 2]) << 8
            | usize::from(bytes[i + 3]);
        i += 4 + len;
        if last {
            return out;
        }
    }
}

#[test]
fn flac_takes_the_card_as_a_picture_block_and_keeps_its_frames() {
    let pcm = weavesignal_media::Pcm {
        rate: 48_000,
        channels: 2,
        samples: (0..96_000)
            .map(|i| ((i / 2) as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.25)
            .collect(),
    };
    let path = scratch("master.flac");
    std::fs::write(&path, weavesignal_media::encode_flac(&pcm).unwrap()).unwrap();
    let before = std::fs::read(&path).unwrap();
    let art = art();
    tags::write(&path, &episode_tags(), &art).unwrap();
    let after = std::fs::read(&path).unwrap();
    assert_eq!(
        flac_frames(&after),
        flac_frames(&before),
        "the frames are untouched"
    );
    assert!(flac_blocks(&after).contains(&6), "a PICTURE block (type 6)");
    check_read_back(&path, &art);
}

/// MP3's frames: everything after the ID3v2 tag.
fn mp3_frames(bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(b"ID3") {
        let size = bytes[6..10]
            .iter()
            .fold(0usize, |acc, b| (acc << 7) | usize::from(b & 0x7f));
        let footer = if bytes[5] & 0x10 != 0 { 10 } else { 0 };
        &bytes[10 + size + footer..]
    } else {
        bytes
    }
}

#[test]
fn mp3_takes_the_card_as_an_apic_frame_and_keeps_its_frames() {
    let path = scratch("episode.mp3");
    std::fs::copy(root().join("tests/fixtures/tone.mp3"), &path).unwrap();
    let before = std::fs::read(&path).unwrap();
    let art = art();
    tags::write(&path, &episode_tags(), &art).unwrap();
    let after = std::fs::read(&path).unwrap();
    assert_eq!(mp3_frames(&after), mp3_frames(&before));
    assert!(after.windows(4).any(|w| w == b"APIC"));
    check_read_back(&path, &art);
}

/// An MP4 file's `mdat` payload.
fn mdat(bytes: &[u8]) -> &[u8] {
    let mut i = 0;
    while i + 8 <= bytes.len() {
        let size = u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
        if &bytes[i + 4..i + 8] == b"mdat" {
            return &bytes[i + 8..i + size];
        }
        i += size.max(8);
    }
    panic!("no mdat");
}

#[test]
fn aac_takes_the_card_as_covr_and_keeps_its_samples() {
    let path = scratch("episode.m4a");
    std::fs::copy(root().join("tests/fixtures/tone.m4a"), &path).unwrap();
    let before = std::fs::read(&path).unwrap();
    let art = art();
    tags::write(&path, &episode_tags(), &art).unwrap();
    let after = std::fs::read(&path).unwrap();
    assert_eq!(mdat(&after), mdat(&before));
    assert!(after.windows(4).any(|w| w == b"covr"));
    check_read_back(&path, &art);
}

#[test]
fn webm_and_matroska_are_refused_and_art_must_be_a_png() {
    assert!(matches!(
        tags::write(Path::new("show.webm"), &episode_tags(), &art()),
        Err(TagError::NoArt(_))
    ));
    assert!(matches!(
        tags::write(Path::new("show.mka"), &episode_tags(), &art()),
        Err(TagError::NoArt(_))
    ));
    let path = scratch("not-png.opus");
    std::fs::copy(root().join("../../assets/carts-demo/ding-1.opus"), &path).unwrap();
    assert!(matches!(
        tags::write(&path, &episode_tags(), b"\xff\xd8\xff not a png"),
        Err(TagError::NotPng)
    ));
}

/// ffprobe and metaflac, where installed, read the same art (report 08's fixture idea: a
/// library upgrade must not lose art silently). Skipped where the tools are absent.
#[test]
fn other_readers_see_the_art_too() {
    let art = art();
    let opus = scratch("probe.opus");
    std::fs::copy(root().join("../../assets/carts-demo/ding-1.opus"), &opus).unwrap();
    tags::write(&opus, &episode_tags(), &art).unwrap();
    if let Ok(out) = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(&opus)
        .output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{text}");
        assert!(
            text.contains("Ghost Train night"),
            "ffprobe reads the title: {text}"
        );
        assert!(
            text.contains("attached_pic") || text.contains("png"),
            "ffprobe sees the art: {text}"
        );
    }
    let pcm = weavesignal_media::Pcm {
        rate: 48_000,
        channels: 1,
        samples: vec![0.0; 48_000],
    };
    let flac = scratch("probe.flac");
    std::fs::write(&flac, weavesignal_media::encode_flac(&pcm).unwrap()).unwrap();
    tags::write(&flac, &episode_tags(), &art).unwrap();
    if let Ok(out) = std::process::Command::new("metaflac")
        .args(["--list", "--block-type=PICTURE"])
        .arg(&flac)
        .output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("type: 3 (Cover (front))"), "{text}");
        assert!(text.contains("MIME type: image/png"), "{text}");
        assert!(text.contains("width: 3000"), "{text}");
    }
}
