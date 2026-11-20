// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! An HLS stream pulled (ADR-0280, PLAN 17b): a pulled source may be a live `.m3u8` stream.
//!
//! - The text: [`parse`] reads a media playlist, [`next_segments`] decides which segments to
//!   fetch next, [`refresh_after`] when to read it again, and [`first_variant`] picks the first
//!   stream of a master playlist.
//! - The puller ([`open`]): the playlist read again and again, each new segment's address
//!   resolved against the playlist's own (after any redirect), fetched through the guarded
//!   [`Fetcher`] (so a segment on another host passes the same check), and handed to the
//!   decoding worker **at the pace it plays** ([`Pacer`]): the first read returns the last three
//!   segments at once, and the desk is fed in real time, not in bursts.
//! - The segments are MPEG-TS, or packed AAC (ADTS) or MP3, told by their first bytes
//!   ([`segment_format`]); a packed segment's leading ID3 tag (HLS puts its timestamp there) is
//!   taken off ([`strip_id3`]). fMP4 segments are refused with words that say so.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use tokio::sync::mpsc;

use crate::pull::{Fetcher, Opened, PullError, address_of};
use weavesignal_proto::pull::PullTarget;

/// One media segment: its sequence number, how long it plays, and the URI to fetch.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// The media-sequence position of this segment within the playlist.
    pub sequence: u64,
    /// How long, in seconds, this segment plays.
    pub duration_s: f64,
    /// The segment's URI, relative or absolute, kept verbatim for the fetcher to resolve.
    pub uri: String,
}

/// A parsed media playlist: its timing, its starting sequence, whether it ends, and its segments.
#[derive(Debug, Clone, PartialEq)]
pub struct Playlist {
    /// The `#EXT-X-TARGETDURATION`, in seconds.
    pub target_duration_s: u64,
    /// The `#EXT-X-MEDIA-SEQUENCE`, the sequence number of the first segment.
    pub media_sequence: u64,
    /// True when a `#EXT-X-ENDLIST` line is present, i.e. a VOD or finished live stream.
    pub ended: bool,
    /// The segments in playlist order; `sequence` is `media_sequence + the segment's index`.
    pub segments: Vec<Segment>,
}

/// Why a media playlist could not be parsed.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PlaylistError {
    /// The first non-empty line was not `#EXTM3U`.
    #[error("not an HLS playlist (no #EXTM3U first line)")]
    NotPlaylist,
    /// The playlist carries `#EXT-X-STREAM-INF`, i.e. it is a master, not a media playlist.
    #[error("a master playlist, not a media playlist")]
    Master,
    /// A `#EXT-X-KEY` line uses a METHOD other than `NONE`, i.e. encrypted segments.
    #[error("encrypted segments are not supported")]
    Encrypted,
    /// A line could not be understood, with the offending line text.
    #[error("bad line: {0}")]
    Bad(String),
}

/// Parse a media playlist's text into a [`Playlist`], or why it is not one.
pub fn parse(text: &str) -> Result<Playlist, PlaylistError> {
    let mut lines = text.lines();

    // The first non-empty line must be the `#EXTM3U` magic, or this is not an HLS playlist.
    let first = loop {
        match lines.next() {
            None => return Err(PlaylistError::NotPlaylist),
            Some(raw) => {
                let line = raw.trim();
                if !line.is_empty() {
                    break line;
                }
            }
        }
    };
    if first != "#EXTM3U" {
        return Err(PlaylistError::NotPlaylist);
    }

    let mut target_duration_s: u64 = 0;
    let mut media_sequence: u64 = 0;
    let mut ended = false;
    let mut segments: Vec<Segment> = Vec::new();
    let mut extinf: Option<f64> = None;

    for raw in lines {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }

        // A master playlist carries one of these; report it as soon as it is seen.
        if line.starts_with("#EXT-X-STREAM-INF") {
            return Err(PlaylistError::Master);
        }

        // A `#EXT-X-KEY:` line means encrypted segments unless it says METHOD=NONE.
        if line.starts_with("#EXT-X-KEY:") && !line.contains("METHOD=NONE") {
            return Err(PlaylistError::Encrypted);
        }

        // `#EXTINF:<duration>[,title]` sets the duration of the next URI line.
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            let duration: f64 = rest
                .split(',')
                .next()
                .unwrap_or("")
                .trim()
                .parse()
                .map_err(|_| PlaylistError::Bad(line.to_owned()))?;
            // `nan`, `inf` and a negative length parse as numbers, but are none.
            if !duration.is_finite() || duration < 0.0 {
                return Err(PlaylistError::Bad(line.to_owned()));
            }
            extinf = Some(duration);
            continue;
        }

        if let Some(rest) = line.strip_prefix("#EXT-X-TARGETDURATION:") {
            target_duration_s = rest
                .trim()
                .parse()
                .map_err(|_| PlaylistError::Bad(line.to_owned()))?;
            continue;
        }

        if let Some(rest) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            media_sequence = rest
                .trim()
                .parse()
                .map_err(|_| PlaylistError::Bad(line.to_owned()))?;
            continue;
        }

        if line == "#EXT-X-ENDLIST" {
            ended = true;
            continue;
        }

        // Any other `#EXT-X-...` or comment line is ignored.
        if line.starts_with('#') {
            continue;
        }

        // A bare line ends a segment: it needs the `#EXTINF` duration from the line above.
        let duration = extinf
            .take()
            .ok_or_else(|| PlaylistError::Bad(line.to_owned()))?;
        segments.push(Segment {
            sequence: media_sequence + segments.len() as u64,
            duration_s: duration,
            uri: line.to_owned(),
        });
    }

    Ok(Playlist {
        target_duration_s,
        media_sequence,
        ended,
        segments,
    })
}

/// The segments to fetch now.
///
/// `last_seen` is the sequence of the last segment already fetched (`None` on the first read).
/// On the first read, start at the last three segments (a live edge, not the whole window). On a
/// later read, every segment with `sequence > last_seen`, unless the window has moved past
/// `last_seen` — the first segment's sequence greater than `last_seen + 1` — in which case the
/// whole window is returned, because some segments were missed.
pub fn next_segments(p: &Playlist, last_seen: Option<u64>) -> Vec<&Segment> {
    match last_seen {
        None => {
            let start = p.segments.len().saturating_sub(3);
            p.segments[start..].iter().collect()
        }
        Some(last) => {
            if p.segments
                .first()
                .is_some_and(|first| first.sequence > last + 1)
            {
                return p.segments.iter().collect();
            }
            p.segments.iter().filter(|s| s.sequence > last).collect()
        }
    }
}

/// How long to wait before reading the playlist again: half the target duration, clamped to
/// between 1 and 10 seconds.
pub fn refresh_after(p: &Playlist) -> Duration {
    Duration::from_secs((p.target_duration_s / 2).clamp(1, 10))
}

/// The first stream's address in a master playlist (the line after the first
/// `#EXT-X-STREAM-INF`), as written.
pub fn first_variant(text: &str) -> Option<String> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    lines.find(|l| l.starts_with("#EXT-X-STREAM-INF"))?;
    lines.find(|l| !l.starts_with('#')).map(str::to_owned)
}

/// A packed segment without its leading ID3v2 tag (HLS's timestamp), if it has one.
pub fn strip_id3(b: &[u8]) -> &[u8] {
    if b.len() < 10 || &b[..3] != b"ID3" {
        return b;
    }
    // The size is four 7-bit bytes; a footer (flag 0x10) adds ten more.
    let size = b[6..10]
        .iter()
        .fold(0usize, |acc, &x| (acc << 7) | usize::from(x & 0x7f));
    let footer = if b[5] & 0x10 != 0 { 10 } else { 0 };
    b.get(10 + size + footer..).unwrap_or_default()
}

/// The worker's format for a segment, by its first bytes (after any ID3 tag): `mpegts`, `aac`
/// (ADTS) or `mp3`.
pub fn segment_format(segment: &[u8]) -> Result<&'static str, PullError> {
    let b = strip_id3(segment);
    if b.first() == Some(&0x47) && b.get(188).is_none_or(|&x| x == 0x47) {
        return Ok("mpegts");
    }
    if b.len() >= 2 && b[0] == 0xff && b[1] & 0xf0 == 0xf0 && b[1] & 0x06 == 0 {
        return Ok("aac");
    }
    if b.len() >= 2 && b[0] == 0xff && b[1] & 0xe0 == 0xe0 && b[1] & 0x06 != 0 {
        return Ok("mp3");
    }
    Err(PullError::Segment(
        "the playlist's segments are not MPEG-TS, AAC or MP3 (fMP4 segments cannot be read yet)"
            .into(),
    ))
}

/// A segment's address, `uri` read against the playlist's address `base` (RFC 3986).
pub fn resolve(base: &str, uri: &str) -> Result<String, PullError> {
    let base = url::Url::parse(base).map_err(|e| PullError::Playlist(e.to_string()))?;
    base.join(uri)
        .map(String::from)
        .map_err(|e| PullError::Playlist(format!("a segment's address ({e})")))
}

/// How far ahead of the clock a pull may hand the worker its sound.
const LEAD: Duration = Duration::from_secs(1);
/// A segment is handed over in slices this long.
const SLICE: Duration = Duration::from_millis(200);
/// Behind the clock by more than this (a slow fetch), the clock starts again, not a burst.
const BEHIND: Duration = Duration::from_secs(5);
/// The largest playlist and segment taken.
const PLAYLIST_MAX: u64 = 1 << 20;
const SEGMENT_MAX: u64 = 16 << 20;
/// Failed reads of the playlist in a row before the pull ends (and the supervisor tries again).
const TRIES: u32 = 3;

/// Hands segments on at the pace they play: each in slices, each slice when the clock reaches
/// its place, up to [`LEAD`] early.
#[derive(Debug, Default)]
pub struct Pacer {
    start: Option<Instant>,
    media: Duration,
}

impl Pacer {
    /// Send `data`, which plays for `dur`, in paced slices; false once nobody listens.
    pub async fn send(&mut self, tx: &mpsc::Sender<Bytes>, data: Bytes, dur: Duration) -> bool {
        let now = Instant::now();
        let start = *self.start.get_or_insert(now);
        if now > start + self.media + BEHIND {
            self.start = Some(now - self.media);
        }
        let n = (dur.as_secs_f64() / SLICE.as_secs_f64()).ceil().max(1.0) as usize;
        let step = data.len().div_ceil(n).max(1);
        let per = dur / n as u32;
        let mut at = 0;
        while at < data.len() {
            let due = self.start.unwrap_or(now) + self.media;
            tokio::time::sleep_until(due.checked_sub(LEAD).unwrap_or(due).into()).await;
            let end = (at + step).min(data.len());
            if tx.send(data.slice(at..end)).await.is_err() {
                return false;
            }
            self.media += per;
            at = end;
        }
        true
    }
}

async fn fetch(
    fetcher: &Arc<Fetcher>,
    url: &str,
    limit: u64,
) -> Result<(String, Vec<u8>), PullError> {
    let (f, u) = (fetcher.clone(), url.to_owned());
    tokio::task::spawn_blocking(move || f.fetch(&u, limit))
        .await
        .map_err(|e| PullError::Http(e.to_string()))?
}

/// The media playlist at `url`: a master playlist's first stream is followed once. Its address
/// after any redirect (segments are read against it), and the playlist.
async fn read_playlist(fetcher: &Arc<Fetcher>, url: &str) -> Result<(String, Playlist), PullError> {
    let (at, body) = fetch(fetcher, url, PLAYLIST_MAX).await?;
    let text = String::from_utf8_lossy(&body);
    match parse(&text) {
        Ok(p) => Ok((at, p)),
        Err(PlaylistError::Master) => {
            let variant = first_variant(&text)
                .ok_or_else(|| PullError::Playlist("a master playlist with no stream".into()))?;
            let next = resolve(&at, &variant)?;
            let (at, body) = fetch(fetcher, &next, PLAYLIST_MAX).await?;
            let p = parse(&String::from_utf8_lossy(&body))
                .map_err(|e| PullError::Playlist(e.to_string()))?;
            Ok((at, p))
        }
        Err(e) => Err(PullError::Playlist(e.to_string())),
    }
}

/// A segment fetched, ready for the worker (a packed one without its ID3 tag).
async fn segment(
    fetcher: &Arc<Fetcher>,
    base: &str,
    s: &Segment,
) -> Result<(Bytes, Duration), PullError> {
    let (_, body) = fetch(fetcher, &resolve(base, &s.uri)?, SEGMENT_MAX).await?;
    let start = body.len() - strip_id3(&body).len();
    let dur = Duration::from_secs_f64(s.duration_s.clamp(0.0, 60.0));
    Ok((Bytes::from(body).slice(start..), dur))
}

/// Open an HLS pull: the playlist read, the first new segment fetched to learn the format, then
/// a task that follows the playlist and paces the segments into the channel. It ends when the
/// playlist ends, when [`TRIES`] reads fail in a row, or when the session drops the channel.
pub async fn open(fetcher: Arc<Fetcher>, t: PullTarget) -> Result<Opened, PullError> {
    let (base, p) = read_playlist(&fetcher, &t.url).await?;
    // An ended playlist is a recording, or a stream that has finished: opened again and again
    // its last segments would play again each time.
    if p.ended {
        return Err(PullError::Playlist(
            "it has ended (#EXT-X-ENDLIST): the station pulls live streams only".into(),
        ));
    }
    let first = next_segments(&p, None)
        .first()
        .map(|s| (*s).clone())
        .ok_or_else(|| PullError::Playlist("it has no segments yet".into()))?;
    let (bytes, dur) = segment(&fetcher, &base, &first).await?;
    let format = segment_format(&bytes)?;
    let (tx, rx) = mpsc::channel::<Bytes>(64);
    let playlist_url = t.url.clone();
    let task = tokio::spawn(async move {
        let mut pacer = Pacer::default();
        if !pacer.send(&tx, bytes, dur).await {
            return;
        }
        let (mut base, mut p, mut last) = (base, p, first.sequence);
        let mut failed = 0;
        loop {
            let mut sent = false;
            for s in next_segments(&p, Some(last))
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
            {
                last = s.sequence;
                match segment(&fetcher, &base, &s).await {
                    Ok((b, d)) => {
                        sent = true;
                        if !pacer.send(&tx, b, d).await {
                            return;
                        }
                    }
                    Err(e) => {
                        tracing::info!(error = %e, "an HLS segment could not be fetched; skipped")
                    }
                }
            }
            if p.ended {
                return;
            }
            if !sent {
                tokio::time::sleep(refresh_after(&p)).await;
            }
            match read_playlist(&fetcher, &playlist_url).await {
                Ok((b, np)) => {
                    (base, p, failed) = (b, np, 0);
                }
                Err(e) => {
                    failed += 1;
                    tracing::info!(error = %e, failed, "an HLS playlist could not be read");
                    if failed >= TRIES {
                        return;
                    }
                }
            }
        }
    });
    Ok(Opened {
        format,
        input: rx,
        address: address_of(&t),
        task: Some(task),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live_playlist() -> Playlist {
        let text = "\
#EXTM3U
#EXT-X-TARGETDURATION:6
#EXT-X-MEDIA-SEQUENCE:100
#EXTINF:6.0,,seg0
seg0.ts
#EXTINF:6.0,,seg1
seg1.ts
#EXTINF:6.0,,seg2
seg2.ts
#EXTINF:6.0,,seg3
seg3.ts
";
        parse(text).expect("a live four-segment playlist parses")
    }

    #[test]
    fn parses_live_four_segment_playlist() {
        let p = live_playlist();
        assert!(!p.ended);
        assert_eq!(p.media_sequence, 100);
        assert_eq!(p.segments.len(), 4);
        assert_eq!(p.segments[0].sequence, 100);
        assert_eq!(p.segments[3].sequence, 103);
        assert_eq!(p.segments[0].duration_s, 6.0);
        assert_eq!(p.segments[0].uri, "seg0.ts");
        assert_eq!(p.segments[3].uri, "seg3.ts");
    }

    #[test]
    fn next_segments_follows_the_live_edge() {
        let p = live_playlist();

        assert_eq!(next_segments(&p, None).len(), 3);
        let seqs: Vec<u64> = next_segments(&p, None).iter().map(|s| s.sequence).collect();
        assert_eq!(seqs, vec![101, 102, 103]);

        let seqs: Vec<u64> = next_segments(&p, Some(101))
            .iter()
            .map(|s| s.sequence)
            .collect();
        assert_eq!(seqs, vec![102, 103]);

        assert!(next_segments(&p, Some(103)).is_empty());

        let seqs: Vec<u64> = next_segments(&p, Some(50))
            .iter()
            .map(|s| s.sequence)
            .collect();
        assert_eq!(seqs, vec![100, 101, 102, 103]);
    }

    #[test]
    fn ended_playlist_is_marked() {
        let text = "\
#EXTM3U
#EXT-X-TARGETDURATION:6
#EXT-X-ENDLIST
#EXTINF:6.0,,a
a.ts
";
        let p = parse(text).expect("an ended playlist parses");
        assert!(p.ended);
        assert_eq!(p.segments.len(), 1);
    }

    #[test]
    fn rejects_the_wrong_playlists() {
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=128000\nhttp://x/stream.m3u8\n";
        assert_eq!(parse(master), Err(PlaylistError::Master));

        assert_eq!(parse("hello world\n"), Err(PlaylistError::NotPlaylist));

        let enc = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXTINF:6.0,,a\na.ts\n";
        assert_eq!(parse(enc), Err(PlaylistError::Encrypted));

        // A `#EXT-X-KEY:METHOD=NONE` line is ignored, so only the missing `#EXTINF` fails.
        let none_key = "#EXTM3U\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:6.0,,a\na.ts\n";
        assert!(parse(none_key).is_ok());

        let bad = "#EXTM3U\n#EXTINF:6.0,,a\na.ts\nb.ts\n";
        assert!(matches!(parse(bad), Err(PlaylistError::Bad(_))));

        let bad_dur = "#EXTM3U\n#EXTINF:notnum,,a\na.ts\n";
        assert!(matches!(parse(bad_dur), Err(PlaylistError::Bad(_))));
        for odd in ["nan", "inf", "-1.0"] {
            let text = format!("#EXTM3U\n#EXTINF:{odd},\na.ts\n");
            assert!(matches!(parse(&text), Err(PlaylistError::Bad(_))), "{odd}");
        }
    }

    #[test]
    fn a_master_playlist_s_first_stream_and_a_segment_s_address() {
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=128000,CODECS=\"mp4a.40.2\"\n\naudio/128k.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=64000\nlow.m3u8\n";
        assert_eq!(first_variant(master).as_deref(), Some("audio/128k.m3u8"));
        assert_eq!(first_variant("#EXTM3U\n#EXTINF:6,\na.ts\n"), None);
        let base = "https://cdn.example.com/live/radio/index.m3u8?t=1";
        assert_eq!(
            resolve(base, "seg1.ts").unwrap(),
            "https://cdn.example.com/live/radio/seg1.ts"
        );
        assert_eq!(
            resolve(base, "../a/seg1.aac").unwrap(),
            "https://cdn.example.com/live/a/seg1.aac"
        );
        assert_eq!(
            resolve(base, "/x.ts").unwrap(),
            "https://cdn.example.com/x.ts"
        );
        assert_eq!(
            resolve(base, "//other.example.net/y.ts").unwrap(),
            "https://other.example.net/y.ts"
        );
        assert_eq!(
            resolve(base, "http://h.example/z.ts").unwrap(),
            "http://h.example/z.ts"
        );
    }

    #[test]
    fn a_segment_s_format_from_its_first_bytes() {
        let mut ts = vec![0u8; 376];
        ts[0] = 0x47;
        ts[188] = 0x47;
        assert_eq!(segment_format(&ts).unwrap(), "mpegts");
        // ID3 (size 5, syncsafe) then ADTS.
        let mut aac = b"ID3\x04\x00\x00\x00\x00\x00\x05hello".to_vec();
        aac.extend_from_slice(&[0xff, 0xf1, 0x50, 0x80]);
        assert_eq!(strip_id3(&aac), &[0xff, 0xf1, 0x50, 0x80]);
        assert_eq!(segment_format(&aac).unwrap(), "aac");
        assert_eq!(segment_format(&[0xff, 0xfb, 0x90, 0x00]).unwrap(), "mp3");
        assert!(segment_format(b"\x00\x00\x00\x18ftypiso6").is_err());
        assert_eq!(strip_id3(b"ID3\x04\x00"), b"ID3\x04\x00");
    }

    #[tokio::test]
    async fn segments_are_paced_as_they_play() {
        let (tx, mut rx) = mpsc::channel(64);
        let mut pacer = Pacer::default();
        let t0 = Instant::now();
        // Three seconds of sound in two segments: up to a second early, so not before two.
        assert!(
            pacer
                .send(
                    &tx,
                    Bytes::from(vec![1u8; 1500]),
                    Duration::from_millis(1500)
                )
                .await
        );
        assert!(
            pacer
                .send(
                    &tx,
                    Bytes::from(vec![2u8; 1500]),
                    Duration::from_millis(1500)
                )
                .await
        );
        let took = t0.elapsed();
        drop(tx);
        let mut total = 0;
        while let Some(b) = rx.recv().await {
            total += b.len();
        }
        assert_eq!(total, 3000);
        assert!(took >= Duration::from_millis(1700), "{took:?}");
        assert!(took < Duration::from_millis(2600), "{took:?}");
    }

    #[tokio::test]
    async fn a_live_playlist_is_followed_through_a_master_and_its_segments_come_in_order() {
        use crate::pull::tests::{ok, serve};
        let ts = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/tone-44k-mp2.ts"
        ))
        .unwrap();
        let (a, b) = ts.split_at(ts.len() / 188 / 2 * 188);
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=128000\nmedia/live.m3u8\n";
        let media = "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:7\n#EXTINF:0.2,\nseg7.ts\n#EXTINF:0.2,\nseg8.ts\n";
        // Read again after a second: the stream has ended, with nothing new.
        let ended = format!("{media}#EXT-X-ENDLIST\n");
        let (port, server) = serve(vec![
            ok("application/vnd.apple.mpegurl", master.as_bytes()),
            ok("application/vnd.apple.mpegurl", media.as_bytes()),
            ok("video/mp2t", a),
            ok("video/mp2t", b),
            ok("application/vnd.apple.mpegurl", master.as_bytes()),
            ok("application/vnd.apple.mpegurl", ended.as_bytes()),
            ok("application/vnd.apple.mpegurl", ended.as_bytes()),
        ]);
        let mut o = crate::pull::open(&format!("http://127.0.0.1:{port}/radio/Index.m3u8"), true)
            .await
            .unwrap();
        assert_eq!(o.format, "mpegts");
        let mut got = Vec::new();
        while let Some(b) = o.input.recv().await {
            got.extend_from_slice(&b);
        }
        assert_eq!(got, ts);
        // Opened again, an ended playlist is refused rather than replayed.
        let e = crate::pull::open(&format!("http://127.0.0.1:{port}/radio/Index.m3u8"), true).await;
        assert!(matches!(e, Err(PullError::Playlist(_))), "{:?}", e.err());
        assert_eq!(
            server.join().unwrap(),
            vec![
                "GET /radio/Index.m3u8 HTTP/1.1",
                "GET /radio/media/live.m3u8 HTTP/1.1",
                "GET /radio/media/seg7.ts HTTP/1.1",
                "GET /radio/media/seg8.ts HTTP/1.1",
                "GET /radio/Index.m3u8 HTTP/1.1",
                "GET /radio/media/live.m3u8 HTTP/1.1",
                "GET /radio/Index.m3u8 HTTP/1.1",
            ]
        );
    }

    #[test]
    fn refresh_after_clamps_to_the_window() {
        let pl = |t: u64| Playlist {
            target_duration_s: t,
            media_sequence: 0,
            ended: false,
            segments: Vec::new(),
        };
        assert_eq!(refresh_after(&pl(6)), Duration::from_secs(3));
        assert_eq!(refresh_after(&pl(1)), Duration::from_secs(1));
        assert_eq!(refresh_after(&pl(60)), Duration::from_secs(10));
    }
}
