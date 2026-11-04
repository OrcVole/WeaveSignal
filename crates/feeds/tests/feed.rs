// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A show's feed against the Podcasting 2.0 namespace.
//!
//! The namespace publishes an XML schema (`tests/podcast-namespace/`, from
//! Podcastindex-org/podcast-namespace at commit d4036dc, 2026-10-10, CC0-1.0) that declares
//! only some of its tags: transcript, locked, funding, chapters, soundbite, person, location,
//! season, episode, value, valueRecipient and the deprecated images. Every one of those in our
//! feed is validated against it with `xmllint --schema` (libxml2, needed on the host). The
//! tags the schema does not declare (`guid`, `image`, `alternateEnclosure`, `source`,
//! `medium`) are checked here against the rules in the namespace's tag pages, required
//! attributes first. The whole feed is checked well-formed with `xmllint --noout` too.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

use quick_xml::Reader;
use quick_xml::events::Event;
use weavesignal_feeds::rss::{PODCAST_NS, rss};
use weavesignal_feeds::{Channel, EpisodeType, Image, Item, Media, Person, podcast_guid};

fn at(s: &str) -> jiff::Zoned {
    s.parse().unwrap()
}

fn square(href: &str, alt: &str) -> Image {
    Image {
        href: href.into(),
        width: 3000,
        height: 3000,
        alt: alt.into(),
        purpose: "artwork",
    }
}

fn social(href: &str, alt: &str) -> Image {
    Image {
        href: href.into(),
        width: 1200,
        height: 630,
        alt: alt.into(),
        purpose: "social",
    }
}

fn channel() -> Channel {
    Channel {
        feed_url: "https://kettle.example/feeds/saturday-night.xml".into(),
        guid: None,
        title: "Saturday Night Kettle".into(),
        description: "Live music, requests and the Forth after dark, every Saturday from eight."
            .into(),
        link: "https://kettle.example/saturday-night".into(),
        language: "en-gb".into(),
        square: square(
            "https://kettle.example/cards/saturday-night-show-square-0a1b2c3d4e5f.png",
            "Saturday Night Kettle, Saturdays 20:00 on Kettle FM, with Shona Campbell and Catriona MacKenzie-Buchanan. kettle.example/saturday-night",
        ),
        social: Some(social(
            "https://kettle.example/cards/saturday-night-show-link-0a1b2c3d4e5f.png",
            "Saturday Night Kettle, Saturdays 20:00 on Kettle FM",
        )),
        author: "Kettle FM".into(),
        owner_name: "Kettle FM".into(),
        owner_email: Some("podcasts@kettle.example".into()),
        copyright: Some("2026 Kettle FM".into()),
        category: Some(("Music".into(), Some("Music Commentary".into()))),
        explicit: false,
        locked: true,
        location: Some("Dunfermline".into()),
        persons: vec![
            Person::host("Shona Campbell"),
            Person::host("Catriona MacKenzie-Buchanan"),
        ],
        serial: false,
        built: at("2026-11-08T09:00[Europe/London]"),
        generator: "WeaveSignal".into(),
    }
}

fn item(n: u32, title: &str, on: &str, guests: &[&str]) -> Item {
    let base = format!("https://kettle.example/media/saturday-night-s3e{n}");
    let mut persons = vec![
        Person::host("Shona Campbell"),
        Person::host("Catriona MacKenzie-Buchanan"),
    ];
    persons.extend(guests.iter().map(|g| Person::guest(*g)));
    Item {
        guid: format!("6c0e2f0a-4b8e-4f7a-9d1e-0000000000{n:02}"),
        title: title.into(),
        description: format!("{title}: Saturday Night Kettle, series 3, episode {n}."),
        link: format!("https://kettle.example/saturday-night/s3/e{n}"),
        published: at(on),
        duration_s: 3 * 3600,
        enclosure: Media {
            url: format!("{base}.mp3"),
            length: 172_800_000,
            mime: "audio/mpeg".into(),
            codecs: None,
            bitrate: Some(128_000),
            title: Some("MP3".into()),
        },
        alternates: vec![Media {
            url: format!("{base}.opus"),
            length: 103_680_000,
            mime: "audio/ogg".into(),
            codecs: Some("opus".into()),
            bitrate: Some(96_000),
            title: Some("Opus".into()),
        }],
        square: Some(square(
            &format!("https://kettle.example/cards/saturday-night-s3e{n}-square-0a1b2c3d4e5f.png"),
            "the episode card",
        )),
        social: Some(social(
            &format!("https://kettle.example/cards/saturday-night-s3e{n}-link-0a1b2c3d4e5f.png"),
            "the episode card",
        )),
        season: Some((3, "Series 3".into())),
        episode: Some(n),
        episode_type: EpisodeType::Full,
        explicit: false,
        persons,
        transcript: None,
        chapters: Some(format!("{base}.chapters.json")),
    }
}

fn feed() -> String {
    rss(
        &channel(),
        &[
            item(
                9,
                "The big ceilidh",
                "2026-11-07T23:00[Europe/London]",
                &[
                    "Firth Lights",
                    "Dr Una Robertson",
                    "Rùnach",
                    "Lachlan Scott",
                    "نور الهدى حسن",
                ],
            ),
            item(
                7,
                "Ghost Train night & <friends>",
                "2026-10-24T23:00[Europe/London]",
                &["Firth Lights", "Dr Una Robertson"],
            ),
        ],
    )
    .unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("weavesignal-feeds-feed");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn xmllint(args: &[&str], file: &PathBuf) -> (bool, String) {
    let out = Command::new("xmllint")
        .args(args)
        .arg(file)
        .output()
        .expect("xmllint (libxml2) is needed for the feed tests");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// One element of the feed: its qualified name, attributes, text and children's names.
#[derive(Debug, Default, Clone)]
struct El {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<String>,
    /// The raw XML of the element, for the schema check.
    raw: String,
    parent: String,
}

impl El {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }
}

/// Every element, flattened, with its parent's name.
fn elements(xml: &str) -> Vec<El> {
    let mut r = Reader::from_str(xml);
    let mut stack: Vec<(El, usize)> = Vec::new();
    let mut out = Vec::new();
    loop {
        let pos = r.buffer_position() as usize;
        match r.read_event().unwrap() {
            Event::Start(e) => {
                let el = El {
                    name: e.name().as_ref().to_owned(),
                    attrs: e
                        .attributes()
                        .map(|a| {
                            let a = a.unwrap();
                            (
                                a.key.as_ref().to_owned(),
                                a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                    .unwrap()
                                    .into_owned(),
                            )
                        })
                        .collect(),
                    parent: stack
                        .last()
                        .map(|(p, _)| p.name.clone())
                        .unwrap_or_default(),
                    ..El::default()
                };
                if let Some((p, _)) = stack.last_mut() {
                    p.children.push(el.name.clone());
                }
                stack.push((el, pos));
            }
            Event::Empty(e) => {
                let el = El {
                    name: e.name().as_ref().to_owned(),
                    attrs: e
                        .attributes()
                        .map(|a| {
                            let a = a.unwrap();
                            (
                                a.key.as_ref().to_owned(),
                                a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                    .unwrap()
                                    .into_owned(),
                            )
                        })
                        .collect(),
                    raw: xml[pos..r.buffer_position() as usize].to_owned(),
                    parent: stack
                        .last()
                        .map(|(p, _)| p.name.clone())
                        .unwrap_or_default(),
                    ..El::default()
                };
                if let Some((p, _)) = stack.last_mut() {
                    p.children.push(el.name.clone());
                }
                out.push(el);
            }
            Event::Text(t) => {
                if let Some((p, _)) = stack.last_mut() {
                    p.text.push_str(&t.xml10_content());
                }
            }
            Event::GeneralRef(g) => {
                if let Some((p, _)) = stack.last_mut() {
                    let name = g.into_inner();
                    p.text.push_str(match name.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "?",
                    });
                }
            }
            Event::CData(t) => {
                if let Some((p, _)) = stack.last_mut() {
                    p.text.push_str(&t.into_inner());
                }
            }
            Event::End(_) => {
                let (mut el, start) = stack.pop().unwrap();
                el.raw = xml[start..r.buffer_position() as usize].to_owned();
                out.push(el);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

#[test]
fn the_guid_is_the_namespaces_uuid5_of_the_address_without_its_scheme() {
    // The namespace's own examples (docs/tags/guid.md).
    assert_eq!(
        podcast_guid("podnews.net/rss").to_string(),
        "9b024349-ccf0-5f69-a609-6b82873eab3c"
    );
    assert_eq!(
        podcast_guid("https://podnews.net/rss/").to_string(),
        "9b024349-ccf0-5f69-a609-6b82873eab3c"
    );
    assert_eq!(
        podcast_guid("https://mp3s.nashownotes.com/pc20rss.xml").to_string(),
        "917393e3-1b1e-5cef-ace4-edaa54e1f810"
    );
}

#[test]
fn the_feed_is_well_formed_and_its_declared_tags_pass_the_namespace_schema() {
    let xml = feed();
    let whole = scratch("saturday-night.xml");
    std::fs::write(&whole, &xml).unwrap();
    let (ok, err) = xmllint(&["--noout"], &whole);
    assert!(ok, "well-formed: {err}");

    // The schema's declared elements, wrapped in the namespace's own test wrapper.
    const DECLARED: [&str; 12] = [
        "transcript",
        "locked",
        "funding",
        "chapters",
        "soundbite",
        "person",
        "location",
        "season",
        "episode",
        "value",
        "valueRecipient",
        "images",
    ];
    let mut covered = BTreeSet::new();
    let mut body = String::new();
    for el in elements(&xml) {
        if let Some(local) = el.name.strip_prefix("podcast:")
            && DECLARED.contains(&local)
            && !el.parent.starts_with("podcast:")
        {
            covered.insert(local.to_owned());
            body.push_str(&el.raw);
            body.push('\n');
        }
    }
    for needed in [
        "locked", "person", "location", "season", "episode", "chapters",
    ] {
        assert!(
            covered.contains(needed),
            "the feed carries podcast:{needed}"
        );
    }
    let wrapped = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<podcast:test-wrapper xmlns:podcast=\"{PODCAST_NS}\">\n{body}</podcast:test-wrapper>\n"
    );
    let doc = scratch("declared.xml");
    std::fs::write(&doc, &wrapped).unwrap();
    let schema = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/podcast-namespace/podcast-wrapper.xsd");
    let (ok, err) = xmllint(&["--noout", "--schema", schema.to_str().unwrap()], &doc);
    assert!(ok, "the namespace's schema: {err}\n{wrapped}");

    // The schema refuses what it should: a check that the check works.
    let bad = scratch("bad.xml");
    std::fs::write(
        &bad,
        format!(
            "<podcast:test-wrapper xmlns:podcast=\"{PODCAST_NS}\"><podcast:locked>maybe</podcast:locked></podcast:test-wrapper>"
        ),
    )
    .unwrap();
    let (ok, _) = xmllint(&["--noout", "--schema", schema.to_str().unwrap()], &bad);
    assert!(!ok, "a locked without owner and with a bad value fails");
}

/// The tags of the namespace (its `docs/tags/` pages at commit d4036dc).
const NAMESPACE_TAGS: [&str; 30] = [
    "alternateEnclosure",
    "block",
    "chapters",
    "chat",
    "contentLink",
    "episode",
    "funding",
    "guid",
    "image",
    "integrity",
    "license",
    "liveItem",
    "location",
    "locked",
    "medium",
    "person",
    "podping",
    "podroll",
    "publisher",
    "remoteItem",
    "season",
    "socialInteract",
    "soundbite",
    "source",
    "trailer",
    "transcript",
    "txt",
    "updateFrequency",
    "valueRecipient",
    "value",
];

#[test]
fn the_tags_the_schema_does_not_declare_follow_the_namespaces_rules() {
    let xml = feed();
    let els = elements(&xml);
    for el in &els {
        if let Some(local) = el.name.strip_prefix("podcast:") {
            assert!(
                NAMESPACE_TAGS.contains(&local),
                "podcast:{local} is a namespace tag"
            );
            assert_ne!(
                local, "images",
                "podcast:images is deprecated; podcast:image replaces it"
            );
        }
    }
    let named = |n: &'static str| els.iter().filter(move |e| e.name == n);

    // guid: once, on the channel, the UUIDv5 of the feed's address.
    let guids: Vec<&El> = named("podcast:guid").collect();
    assert_eq!(guids.len(), 1);
    assert_eq!(guids[0].parent, "channel");
    assert_eq!(
        guids[0].text,
        podcast_guid("https://kettle.example/feeds/saturday-night.xml").to_string()
    );

    // image: href required; alt, aspect-ratio and width recommended; purpose tokens.
    let images: Vec<&El> = named("podcast:image").collect();
    assert_eq!(
        images.len(),
        2 + 2 * 2,
        "square and link card on the channel and each item"
    );
    for i in &images {
        assert!(i.attr("href").is_some_and(|h| h.starts_with("https://")));
        assert!(i.attr("alt").is_some_and(|a| !a.is_empty()));
        assert!(matches!(
            i.attr("aspect-ratio"),
            Some("1/1") | Some("40/21")
        ));
        assert!(i.attr("width").is_some());
        assert!(matches!(
            i.attr("purpose"),
            Some("artwork") | Some("social")
        ));
        assert!(i.attr("purpose").unwrap().len() <= 128);
    }

    // alternateEnclosure: in an item, type required, at least one source with a uri.
    let alts: Vec<&El> = named("podcast:alternateEnclosure").collect();
    assert_eq!(
        alts.len(),
        4,
        "the MP3 again as the default, and the Opus copy, per item"
    );
    for a in &alts {
        assert_eq!(a.parent, "item");
        assert!(a.attr("type").is_some());
        assert!(a.children.iter().any(|c| c == "podcast:source"));
        if let Some(t) = a.attr("title") {
            assert!(t.chars().count() <= 32);
        }
    }
    assert_eq!(
        alts.iter()
            .filter(|a| a.attr("default") == Some("true"))
            .count(),
        2
    );
    assert!(
        alts.iter()
            .any(|a| a.attr("type") == Some("audio/ogg") && a.attr("codecs") == Some("opus"))
    );
    for s in named("podcast:source") {
        assert_eq!(s.parent, "podcast:alternateEnclosure");
        assert!(s.attr("uri").is_some_and(|u| u.starts_with("https://")));
    }

    // medium: once, on the channel, one of the namespace's values.
    let media: Vec<&El> = named("podcast:medium").collect();
    assert_eq!(media.len(), 1);
    assert_eq!(media[0].parent, "channel");
    assert!(
        [
            "podcast",
            "music",
            "video",
            "film",
            "audiobook",
            "newsletter",
            "blog",
            "publisher",
            "course"
        ]
        .contains(&media[0].text.as_str())
    );

    // person: the hosts on the channel, and restated with the guests on each item.
    let channel_people: Vec<&El> = named("podcast:person")
        .filter(|p| p.parent == "channel")
        .collect();
    assert_eq!(channel_people.len(), 2);
    let item_people: Vec<&El> = named("podcast:person")
        .filter(|p| p.parent == "item")
        .collect();
    assert_eq!(item_people.len(), (2 + 5) + (2 + 2));
    assert!(
        item_people
            .iter()
            .any(|p| p.text == "نور الهدى حسن" && p.attr("role") == Some("guest"))
    );
    for p in item_people.iter().chain(&channel_people) {
        assert!(p.text.chars().count() <= 128 && !p.text.is_empty());
        assert!(weavesignal_feeds::taxonomy::is_role(
            p.attr("group").unwrap(),
            p.attr("role").unwrap()
        ));
    }
}

#[test]
fn the_rss_and_apple_parts_are_there() {
    let xml = feed();
    let els = elements(&xml);
    let one = |n: &str, parent: &str| {
        let found: Vec<&El> = els
            .iter()
            .filter(|e| e.name == n && e.parent == parent)
            .collect();
        assert_eq!(found.len(), 1, "one {n} in {parent}");
        found[0].clone()
    };
    assert_eq!(one("title", "channel").text, "Saturday Night Kettle");
    assert_eq!(one("itunes:explicit", "channel").text, "false");
    assert_eq!(one("itunes:type", "channel").text, "episodic");
    assert!(one("itunes:image", "channel").attr("href").is_some());
    assert_eq!(one("atom:link", "channel").attr("rel"), Some("self"));
    let owner = one("itunes:email", "itunes:owner");
    assert_eq!(owner.text, "podcasts@kettle.example");
    assert_eq!(
        one("podcast:locked", "channel").attr("owner"),
        Some("podcasts@kettle.example")
    );
    let cat = els
        .iter()
        .find(|e| e.name == "itunes:category" && e.parent == "channel")
        .unwrap();
    assert_eq!(cat.attr("text"), Some("Music"));

    let items: Vec<&El> = els.iter().filter(|e| e.name == "item").collect();
    assert_eq!(items.len(), 2);
    for enc in els.iter().filter(|e| e.name == "enclosure") {
        assert_eq!(enc.parent, "item");
        assert_eq!(
            enc.attr("type"),
            Some("audio/mpeg"),
            "MP3 for Apple and Spotify (ADR-0018)"
        );
        assert!(enc.attr("length").unwrap().parse::<u64>().unwrap() > 0);
        assert!(enc.attr("url").unwrap().ends_with(".mp3"));
    }
    for d in els.iter().filter(|e| e.name == "pubDate") {
        jiff::fmt::rfc2822::parse(&d.text).expect("RFC 2822 dates");
    }
    // Escaped, and read back as written.
    assert!(
        els.iter()
            .any(|e| e.name == "title" && e.text == "Ghost Train night & <friends>")
    );
    assert!(xml.contains("Ghost Train night &amp; &lt;friends&gt;"));
    let seasons: Vec<&El> = els.iter().filter(|e| e.name == "podcast:season").collect();
    assert!(
        seasons
            .iter()
            .all(|s| s.text == "3" && s.attr("name") == Some("Series 3"))
    );
}

#[test]
fn an_opus_enclosure_is_refused_since_apple_and_spotify_cannot_list_it() {
    let mut i = item(
        7,
        "Ghost Train night",
        "2026-10-24T23:00[Europe/London]",
        &[],
    );
    i.enclosure.mime = "audio/ogg".into();
    assert!(rss(&channel(), &[i]).is_err());
    let mut c = channel();
    c.square.width = 600;
    c.square.height = 600;
    assert!(
        rss(&c, &[]).is_err(),
        "podcast art is 1,400 to 3,000 square"
    );
}
