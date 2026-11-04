// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// SPDX-FileCopyrightText: 2020 Ad Aures (Castopod, the tag order and choices ported below)

//! A show's podcast feed: RSS 2.0 with the iTunes and Podcasting 2.0 namespaces, one feed
//! per show (report 08, report 13).
//!
//! Ported from Castopod's `app/Helpers/rss_helper.php` (`get_rss_feed` and
//! `add_category_tag`), branch `develop` at commit e439b31ac2471f39de11fb9db0c82a23c88c1bb5
//! (2026-06-05), Copyright 2020 Ad Aures, AGPL-3.0-or-later, used here under AGPL-3.0-only as
//! ADR-0001 and ADR-0014 allow; the origin is recorded in NOTICES.md. What came across: the
//! order of the channel's and the items' elements, the `podcast:guid` seeded once from the
//! feed address with the namespace's UUID, `podcast:locked` with its owner, one
//! `podcast:person` per role with the taxonomy's role and group, the Apple category with
//! its plain-RSS twin, and the `image` block. What changed: the guid is seeded from the address
//! without its scheme or trailing slash, as the namespace says (Castopod seeds it with the
//! whole address); `podcast:image` replaces the deprecated `podcast:images`; every item
//! carries `podcast:alternateEnclosure` for the Opus copy (ADR-0018), which Castopod lacks;
//! the hosts are restated on every item, because item persons replace the channel's; and the
//! premium-feed, plugin, fediverse and comment parts are not ported.

use std::io::Cursor;

use jiff::Zoned;
use quick_xml::Writer;
use quick_xml::events::{BytesCData, BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use uuid::Uuid;

use crate::FeedError;

pub const ITUNES_NS: &str = "http://www.itunes.com/dtds/podcast-1.0.dtd";
pub const PODCAST_NS: &str = "https://podcastindex.org/namespace/1.0";
pub const ATOM_NS: &str = "http://www.w3.org/2005/Atom";

/// The Podcasting 2.0 namespace's own UUID, the seed for every `podcast:guid`.
pub const PODCAST_GUID_NAMESPACE: Uuid = uuid::uuid!("ead4c236-bf58-58c6-a2c6-a6b28d128cb6");

/// A podcast's `podcast:guid`: a UUIDv5 of its feed's address with the scheme and any trailing
/// slashes taken off, assigned once and kept for the podcast's life (the namespace's `guid`).
pub fn podcast_guid(feed_url: &str) -> Uuid {
    let bare = feed_url
        .split_once("://")
        .map_or(feed_url, |(_, rest)| rest)
        .trim_end_matches('/');
    Uuid::new_v5(&PODCAST_GUID_NAMESPACE, bare.as_bytes())
}

/// A role from the Podcast Taxonomy Project (`taxonomy.json`, CC0); the feed writes it in
/// lower case, which the namespace treats as the same.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub name: String,
    pub role: String,
    pub group: String,
    /// A normalised photo's address (D48), never one someone typed.
    pub img: Option<String>,
    pub href: Option<String>,
}

impl Person {
    pub fn host(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            role: "host".into(),
            group: "cast".into(),
            img: None,
            href: None,
        }
    }

    pub fn guest(name: impl Into<String>) -> Self {
        Self {
            role: "guest".into(),
            ..Self::host(name)
        }
    }
}

/// An image the feed points at, with what it is for (`podcast:image`'s `purpose`).
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub href: String,
    pub width: u32,
    pub height: u32,
    pub alt: String,
    /// `artwork` for the square, `social` for the link card.
    pub purpose: &'static str,
}

/// A media file: the enclosure, or an alternate.
#[derive(Clone, Debug, PartialEq)]
pub struct Media {
    pub url: String,
    pub length: u64,
    pub mime: String,
    /// RFC 6381 codecs, `opus` for Ogg Opus.
    pub codecs: Option<String>,
    /// Bits per second, averaged.
    pub bitrate: Option<u64>,
    /// A short name for the app's menu: "MP3", "Opus".
    pub title: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpisodeType {
    Full,
    Trailer,
    Bonus,
}

impl EpisodeType {
    fn as_str(self) -> &'static str {
        match self {
            EpisodeType::Full => "full",
            EpisodeType::Trailer => "trailer",
            EpisodeType::Bonus => "bonus",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Stable for the episode's life: its identifier, not its address.
    pub guid: String,
    pub title: String,
    pub description: String,
    /// The episode's own page, which carries the player (ADR-0056).
    pub link: String,
    pub published: Zoned,
    pub duration_s: u64,
    /// MP3 or AAC (ADR-0018).
    pub enclosure: Media,
    /// The Opus copy, and a video copy where there is one (ADR-0018, ADR-0019).
    pub alternates: Vec<Media>,
    pub square: Option<Image>,
    pub social: Option<Image>,
    pub season: Option<(u32, String)>,
    pub episode: Option<u32>,
    pub episode_type: EpisodeType,
    pub explicit: bool,
    /// Hosts and guests; the hosts are restated here, since item persons replace the
    /// channel's.
    pub persons: Vec<Person>,
    pub transcript: Option<(String, String)>,
    pub chapters: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    pub feed_url: String,
    /// Kept once assigned; `None` derives it from `feed_url`.
    pub guid: Option<Uuid>,
    pub title: String,
    pub description: String,
    /// The show's page.
    pub link: String,
    /// BCP 47, `en-gb`.
    pub language: String,
    pub square: Image,
    pub social: Option<Image>,
    /// The station.
    pub author: String,
    pub owner_name: String,
    pub owner_email: Option<String>,
    pub copyright: Option<String>,
    /// Apple's category and subcategory: ("Music", Some("Music Commentary")).
    pub category: Option<(String, Option<String>)>,
    pub explicit: bool,
    /// Whether other platforms may import the feed (`podcast:locked`); written only with an
    /// owner's address, which the tag requires.
    pub locked: bool,
    pub location: Option<String>,
    pub persons: Vec<Person>,
    /// `episodic` (newest first) or `serial`.
    pub serial: bool,
    pub built: Zoned,
    pub generator: String,
}

/// Mime types an enclosure may have (ADR-0018: MP3 or AAC, for Apple and Spotify).
pub const ENCLOSURE_TYPES: [&str; 4] = ["audio/mpeg", "audio/mp4", "audio/x-m4a", "audio/aac"];

type W = Writer<Cursor<Vec<u8>>>;

fn io(e: std::io::Error) -> FeedError {
    FeedError::Write(e.to_string())
}

fn start(w: &mut W, name: &str, attrs: &[(&str, &str)]) -> Result<(), FeedError> {
    let mut el = BytesStart::new(name);
    for (k, v) in attrs {
        el.push_attribute((*k, *v));
    }
    w.write_event(Event::Start(el)).map_err(io)
}

fn end(w: &mut W, name: &str) -> Result<(), FeedError> {
    w.write_event(Event::End(BytesEnd::new(name))).map_err(io)
}

fn empty(w: &mut W, name: &str, attrs: &[(&str, &str)]) -> Result<(), FeedError> {
    let mut el = BytesStart::new(name);
    for (k, v) in attrs {
        el.push_attribute((*k, *v));
    }
    w.write_event(Event::Empty(el)).map_err(io)
}

fn text(w: &mut W, name: &str, attrs: &[(&str, &str)], value: &str) -> Result<(), FeedError> {
    start(w, name, attrs)?;
    w.write_event(Event::Text(BytesText::new(&clean(value))))
        .map_err(io)?;
    end(w, name)
}

/// Text with the control characters XML 1.0 cannot carry taken out.
fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}

fn rfc2822(t: &Zoned) -> Result<String, FeedError> {
    jiff::fmt::rfc2822::to_string(t).map_err(|e| FeedError::Write(e.to_string()))
}

fn person(w: &mut W, p: &Person) -> Result<(), FeedError> {
    let mut attrs = vec![("role", p.role.as_str()), ("group", p.group.as_str())];
    if let Some(img) = &p.img {
        attrs.push(("img", img));
    }
    if let Some(href) = &p.href {
        attrs.push(("href", href));
    }
    text(w, "podcast:person", &attrs, &p.name)
}

fn image(w: &mut W, i: &Image) -> Result<(), FeedError> {
    let (width, height) = (i.width.to_string(), i.height.to_string());
    let ratio = aspect(i.width, i.height);
    empty(
        w,
        "podcast:image",
        &[
            ("href", &i.href),
            ("alt", &clean(&i.alt)),
            ("aspect-ratio", &ratio),
            ("width", &width),
            ("height", &height),
            ("type", "image/png"),
            ("purpose", i.purpose),
        ],
    )
}

fn aspect(w: u32, h: u32) -> String {
    fn gcd(a: u32, b: u32) -> u32 {
        if b == 0 { a } else { gcd(b, a % b) }
    }
    let g = gcd(w, h).max(1);
    format!("{}/{}", w / g, h / g)
}

fn alternate(w: &mut W, m: &Media, default: bool) -> Result<(), FeedError> {
    let length = m.length.to_string();
    let bitrate = m.bitrate.map(|b| b.to_string());
    let mut attrs = vec![("type", m.mime.as_str()), ("length", length.as_str())];
    if let Some(b) = &bitrate {
        attrs.push(("bitrate", b));
    }
    if let Some(c) = &m.codecs {
        attrs.push(("codecs", c));
    }
    if let Some(t) = &m.title {
        attrs.push(("title", t));
    }
    if default {
        attrs.push(("default", "true"));
    }
    start(w, "podcast:alternateEnclosure", &attrs)?;
    empty(w, "podcast:source", &[("uri", &m.url)])?;
    end(w, "podcast:alternateEnclosure")
}

fn check(c: &Channel, items: &[Item]) -> Result<(), FeedError> {
    if c.title.trim().is_empty() {
        return Err(FeedError::Missing("the show's title"));
    }
    if c.square.width < 1400 || c.square.width > 3000 || c.square.width != c.square.height {
        return Err(FeedError::Invalid(
            "podcast art is a square of 1,400 to 3,000 pixels".into(),
        ));
    }
    for i in items {
        if !ENCLOSURE_TYPES.contains(&i.enclosure.mime.as_str()) {
            return Err(FeedError::Invalid(format!(
                "episode `{}`: the enclosure is MP3 or AAC (ADR-0018), not {}",
                i.title, i.enclosure.mime
            )));
        }
        for p in i.persons.iter().chain(&c.persons) {
            if !crate::taxonomy::is_role(&p.group, &p.role) {
                return Err(FeedError::Invalid(format!(
                    "`{}` in `{}` is not a role in the Podcast Taxonomy",
                    p.role, p.group
                )));
            }
        }
    }
    Ok(())
}

/// The feed's XML.
pub fn rss(c: &Channel, items: &[Item]) -> Result<String, FeedError> {
    check(c, items)?;
    let mut w = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(io)?;
    start(
        &mut w,
        "rss",
        &[
            ("version", "2.0"),
            ("xmlns:itunes", ITUNES_NS),
            ("xmlns:podcast", PODCAST_NS),
            ("xmlns:atom", ATOM_NS),
        ],
    )?;
    start(&mut w, "channel", &[])?;
    empty(
        &mut w,
        "atom:link",
        &[
            ("href", &c.feed_url),
            ("rel", "self"),
            ("type", "application/rss+xml"),
        ],
    )?;
    text(&mut w, "lastBuildDate", &[], &rfc2822(&c.built)?)?;
    text(&mut w, "generator", &[], &c.generator)?;
    text(
        &mut w,
        "docs",
        &[],
        "https://www.rssboard.org/rss-specification",
    )?;
    let guid = c.guid.unwrap_or_else(|| podcast_guid(&c.feed_url));
    text(&mut w, "podcast:guid", &[], &guid.hyphenated().to_string())?;
    text(&mut w, "title", &[], &c.title)?;
    text(&mut w, "description", &[], &c.description)?;
    empty(&mut w, "itunes:image", &[("href", &c.square.href)])?;
    image(&mut w, &c.square)?;
    if let Some(s) = &c.social {
        image(&mut w, s)?;
    }
    text(&mut w, "language", &[], &c.language)?;
    if let Some(l) = &c.location {
        text(&mut w, "podcast:location", &[], l)?;
    }
    if let Some(owner) = &c.owner_email {
        text(
            &mut w,
            "podcast:locked",
            &[("owner", owner)],
            if c.locked { "yes" } else { "no" },
        )?;
    }
    text(&mut w, "podcast:medium", &[], "podcast")?;
    for p in &c.persons {
        person(&mut w, p)?;
    }
    if let Some((cat, sub)) = &c.category {
        // Castopod's add_category_tag: Apple's nested category, then the plain RSS ones.
        match sub {
            Some(sub) => {
                start(&mut w, "itunes:category", &[("text", cat)])?;
                empty(&mut w, "itunes:category", &[("text", sub)])?;
                end(&mut w, "itunes:category")?;
                text(&mut w, "category", &[], cat)?;
                text(&mut w, "category", &[], sub)?;
            }
            None => {
                empty(&mut w, "itunes:category", &[("text", cat)])?;
                text(&mut w, "category", &[], cat)?;
            }
        }
    }
    text(
        &mut w,
        "itunes:explicit",
        &[],
        if c.explicit { "true" } else { "false" },
    )?;
    text(&mut w, "itunes:author", &[], &c.author)?;
    text(&mut w, "link", &[], &c.link)?;
    start(&mut w, "itunes:owner", &[])?;
    text(&mut w, "itunes:name", &[], &c.owner_name)?;
    if let Some(e) = &c.owner_email {
        text(&mut w, "itunes:email", &[], e)?;
    }
    end(&mut w, "itunes:owner")?;
    text(
        &mut w,
        "itunes:type",
        &[],
        if c.serial { "serial" } else { "episodic" },
    )?;
    if let Some(copy) = &c.copyright {
        text(&mut w, "copyright", &[], copy)?;
    }
    start(&mut w, "image", &[])?;
    text(&mut w, "url", &[], &c.square.href)?;
    text(&mut w, "title", &[], &c.title)?;
    text(&mut w, "link", &[], &c.link)?;
    end(&mut w, "image")?;

    for i in items {
        start(&mut w, "item", &[])?;
        item(&mut w, i)?;
        end(&mut w, "item")?;
    }
    end(&mut w, "channel")?;
    end(&mut w, "rss")?;
    String::from_utf8(w.into_inner().into_inner()).map_err(|e| FeedError::Write(e.to_string()))
}

fn item(w: &mut W, i: &Item) -> Result<(), FeedError> {
    text(w, "title", &[], &i.title)?;
    let length = i.enclosure.length.to_string();
    empty(
        w,
        "enclosure",
        &[
            ("url", &i.enclosure.url),
            ("length", &length),
            ("type", &i.enclosure.mime),
        ],
    )?;
    text(w, "guid", &[("isPermaLink", "false")], &i.guid)?;
    text(w, "pubDate", &[], &rfc2822(&i.published)?)?;
    start(w, "description", &[])?;
    // Plain text in a CDATA section, as Castopod writes it; "]]>" cannot end it early.
    let desc = clean(&i.description).replace("]]>", "]]]]><![CDATA[>");
    w.write_event(Event::CData(BytesCData::new(desc)))
        .map_err(io)?;
    end(w, "description")?;
    text(w, "itunes:duration", &[], &i.duration_s.to_string())?;
    text(w, "link", &[], &i.link)?;
    if let Some(sq) = &i.square {
        empty(w, "itunes:image", &[("href", &sq.href)])?;
        image(w, sq)?;
    }
    if let Some(s) = &i.social {
        image(w, s)?;
    }
    text(
        w,
        "itunes:explicit",
        &[],
        if i.explicit { "true" } else { "false" },
    )?;
    if let Some(n) = i.episode {
        text(w, "itunes:episode", &[], &n.to_string())?;
    }
    if let Some((n, _)) = &i.season {
        text(w, "itunes:season", &[], &n.to_string())?;
    }
    text(w, "itunes:episodeType", &[], i.episode_type.as_str())?;
    if let Some((n, name)) = &i.season {
        // The namespace's schema requires the name, its prose calls it optional: always
        // written, the series' title or "Series 3".
        text(w, "podcast:season", &[("name", name)], &n.to_string())?;
    }
    if let Some(n) = i.episode {
        text(w, "podcast:episode", &[], &n.to_string())?;
    }
    // The enclosure again, as the default, then the Opus and any video copy.
    alternate(w, &i.enclosure, true)?;
    for a in &i.alternates {
        alternate(w, a, false)?;
    }
    if let Some((url, mime)) = &i.transcript {
        empty(w, "podcast:transcript", &[("url", url), ("type", mime)])?;
    }
    if let Some(url) = &i.chapters {
        empty(
            w,
            "podcast:chapters",
            &[("url", url), ("type", "application/json+chapters")],
        )?;
    }
    for p in &i.persons {
        person(w, p)?;
    }
    Ok(())
}
