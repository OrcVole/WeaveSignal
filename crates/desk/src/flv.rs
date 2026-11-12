// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! FLV tags for RTMP out (ADR-0229, ADR-0230), written by the desk itself.
//!
//! RTMP carries FLV tags. GStreamer 1.28's `flvmux` and `eflvmux` take neither AV1 nor Opus
//! (research `2026-11-06-rtmp-out.md`), so the desk writes the tags here, from the encoders'
//! own output, and hands them one tag per buffer to `rtmp2sink`, which sends each as it is. One
//! writer serves both profiles:
//!
//! - **compatible**: classic FLV, H.264 (codec id 7, the `avcC` record as its sequence header)
//!   and AAC (sound format 10, the AudioSpecificConfig as its sequence header);
//! - **open**: Enhanced RTMP (Veovera's E-RTMP v1): AV1 under the FourCC `av01`, its sequence
//!   start the `av1C` record, and Opus under `Opus`, its sequence start the Opus ID header.
//!
//! A destination that connects (or connects again) is sent the file header, the metadata and
//! both sequence starts first, then nothing until a picture keyframe, with every timestamp
//! counted from that keyframe ([`Tag::bytes`]). [`Interleave`] puts the picture's and the
//! sound's tags in time order, as a server expects them.

use std::collections::VecDeque;

/// What a tag carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Audio,
    Video,
    Script,
}

impl Kind {
    fn code(self) -> u8 {
        match self {
            Kind::Audio => 8,
            Kind::Video => 9,
            Kind::Script => 18,
        }
    }
}

/// One FLV tag: its body, and its decode time in milliseconds on the desk's timeline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub kind: Kind,
    pub dts_ms: u64,
    /// A picture keyframe: a destination starts here.
    pub key: bool,
    pub body: Vec<u8>,
}

impl Tag {
    /// The whole tag as sent: the 11-byte header with `ts` (milliseconds from the
    /// destination's first keyframe), the body, and the trailing size.
    pub fn bytes(&self, ts: u32) -> Vec<u8> {
        let size = self.body.len() as u32;
        let mut v = Vec::with_capacity(self.body.len() + 15);
        v.push(self.kind.code());
        v.extend_from_slice(&size.to_be_bytes()[1..]);
        v.extend_from_slice(&ts.to_be_bytes()[1..]);
        v.push((ts >> 24) as u8);
        v.extend_from_slice(&[0, 0, 0]);
        v.extend_from_slice(&self.body);
        v.extend_from_slice(&(size + 11).to_be_bytes());
        v
    }
}

/// The FLV file header and the first "previous tag size" (0): what `rtmp2sink` expects
/// before the tags.
pub fn file_header(audio: bool, video: bool) -> Vec<u8> {
    let flags = (u8::from(audio) << 2) | u8::from(video);
    vec![b'F', b'L', b'V', 1, flags, 0, 0, 0, 9, 0, 0, 0, 0]
}

// ---------------------------------------------------------------------------------------
// Metadata (AMF0).

/// A value in `onMetaData`.
#[derive(Clone, Debug, PartialEq)]
pub enum Amf {
    Number(f64),
    Bool(bool),
    Text(String),
}

fn amf_string(v: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    v.extend_from_slice(&(b.len() as u16).to_be_bytes());
    v.extend_from_slice(b);
}

/// `onMetaData` as a script tag: an ECMA array of the stream's properties.
pub fn metadata(props: &[(&str, Amf)]) -> Tag {
    let mut v = vec![2];
    amf_string(&mut v, "onMetaData");
    v.push(8);
    v.extend_from_slice(&(props.len() as u32).to_be_bytes());
    for (k, val) in props {
        amf_string(&mut v, k);
        match val {
            Amf::Number(n) => {
                v.push(0);
                v.extend_from_slice(&n.to_be_bytes());
            }
            Amf::Bool(b) => {
                v.push(1);
                v.push(u8::from(*b));
            }
            Amf::Text(s) => {
                v.push(2);
                amf_string(&mut v, s);
            }
        }
    }
    v.extend_from_slice(&[0, 0, 9]);
    Tag {
        kind: Kind::Script,
        dts_ms: 0,
        key: false,
        body: v,
    }
}

/// A FourCC as `onMetaData`'s codec id carries it under E-RTMP (as FFmpeg writes it).
pub fn fourcc_number(cc: &[u8; 4]) -> f64 {
    f64::from(u32::from_be_bytes(*cc))
}

// ---------------------------------------------------------------------------------------
// compatible: H.264 and AAC, classic FLV.

/// The H.264 sequence header: the `avcC` record (`h264parse`'s `codec_data` in avc form).
pub fn avc_sequence(avcc: &[u8]) -> Tag {
    let mut body = vec![0x17, 0, 0, 0, 0];
    body.extend_from_slice(avcc);
    Tag {
        kind: Kind::Video,
        dts_ms: 0,
        key: false,
        body,
    }
}

/// One H.264 access unit, length-prefixed as `avcC` says; `cts_ms` is its composition offset.
pub fn avc_frame(dts_ms: u64, key: bool, cts_ms: i32, au: &[u8]) -> Tag {
    let mut body = vec![if key { 0x17 } else { 0x27 }, 1];
    body.extend_from_slice(&cts_ms.to_be_bytes()[1..]);
    body.extend_from_slice(au);
    Tag {
        kind: Kind::Video,
        dts_ms,
        key,
        body,
    }
}

/// AAC's first byte: sound format 10, "44 kHz", 16-bit, stereo (FLV's fixed values for AAC;
/// the real rate is the AudioSpecificConfig's).
const AAC_FLAGS: u8 = 0xAF;

/// The AAC sequence header: the AudioSpecificConfig (the AAC encoder's `codec_data`).
pub fn aac_sequence(asc: &[u8]) -> Tag {
    let mut body = vec![AAC_FLAGS, 0];
    body.extend_from_slice(asc);
    Tag {
        kind: Kind::Audio,
        dts_ms: 0,
        key: false,
        body,
    }
}

pub fn aac_frame(dts_ms: u64, raw: &[u8]) -> Tag {
    let mut body = vec![AAC_FLAGS, 1];
    body.extend_from_slice(raw);
    Tag {
        kind: Kind::Audio,
        dts_ms,
        key: false,
        body,
    }
}

// ---------------------------------------------------------------------------------------
// open: AV1 and Opus, Enhanced RTMP.

pub const AV1: [u8; 4] = *b"av01";
pub const OPUS: [u8; 4] = *b"Opus";

/// E-RTMP's video packet types.
const SEQUENCE_START: u8 = 0;
const CODED_FRAMES: u8 = 1;

fn ex_video(key: bool, packet: u8) -> u8 {
    let frame_type: u8 = if key { 1 } else { 2 };
    0x80 | (frame_type << 4) | packet
}

/// The AV1 sequence start: the `av1C` record.
pub fn av1_sequence(av1c: &[u8]) -> Tag {
    let mut body = vec![ex_video(true, SEQUENCE_START)];
    body.extend_from_slice(&AV1);
    body.extend_from_slice(av1c);
    Tag {
        kind: Kind::Video,
        dts_ms: 0,
        key: false,
        body,
    }
}

/// One AV1 temporal unit as coded frames: its OBUs without the temporal delimiter (the
/// low-overhead format AV1-in-ISOBMFF and E-RTMP use).
pub fn av1_frame(dts_ms: u64, key: bool, tu: &[u8]) -> Tag {
    let mut body = vec![ex_video(key, CODED_FRAMES)];
    body.extend_from_slice(&AV1);
    for o in obus(tu) {
        if o.kind != OBU_TEMPORAL_DELIMITER {
            body.extend_from_slice(o.bytes);
        }
    }
    Tag {
        kind: Kind::Video,
        dts_ms,
        key,
        body,
    }
}

/// E-RTMP's audio: sound format 9 (the extended header) with the packet type below it.
fn ex_audio(packet: u8) -> u8 {
    (9 << 4) | packet
}

/// The Opus sequence start: the ID header (`OpusHead`, `opusenc`'s first `streamheader`).
pub fn opus_sequence(head: &[u8]) -> Tag {
    let mut body = vec![ex_audio(SEQUENCE_START)];
    body.extend_from_slice(&OPUS);
    body.extend_from_slice(head);
    Tag {
        kind: Kind::Audio,
        dts_ms: 0,
        key: false,
        body,
    }
}

pub fn opus_frame(dts_ms: u64, packet: &[u8]) -> Tag {
    let mut body = vec![ex_audio(CODED_FRAMES)];
    body.extend_from_slice(&OPUS);
    body.extend_from_slice(packet);
    Tag {
        kind: Kind::Audio,
        dts_ms,
        key: false,
        body,
    }
}

// ---------------------------------------------------------------------------------------
// AV1's OBUs and the av1C record.

pub const OBU_SEQUENCE_HEADER: u8 = 1;
pub const OBU_TEMPORAL_DELIMITER: u8 = 2;

/// One OBU of a temporal unit, with its header and size field.
#[derive(Debug, PartialEq, Eq)]
pub struct Obu<'a> {
    pub kind: u8,
    pub bytes: &'a [u8],
}

fn leb128(b: &[u8]) -> Option<(u64, usize)> {
    let mut v = 0u64;
    for (i, byte) in b.iter().take(8).enumerate() {
        v |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

/// The OBUs of a temporal unit in `obu-stream` form (each with its size field); a malformed
/// tail is left out.
pub fn obus(tu: &[u8]) -> Vec<Obu<'_>> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < tu.len() {
        let h = tu[p];
        let kind = (h >> 3) & 0x0f;
        let ext = h & 0x04 != 0;
        let has_size = h & 0x02 != 0;
        let mut q = p + 1 + usize::from(ext);
        let end = if has_size {
            let Some((size, n)) = tu.get(q..).and_then(leb128) else {
                break;
            };
            q += n;
            q + size as usize
        } else {
            tu.len()
        };
        if end > tu.len() {
            break;
        }
        out.push(Obu {
            kind,
            bytes: &tu[p..end],
        });
        p = end;
    }
    out
}

/// What `av1parse`'s caps say about the stream, for the `av1C` record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Av1Format {
    /// 0 main, 1 high, 2 professional.
    pub profile: u8,
    /// `seq_level_idx`, from a level such as "3.0" (4) or "3.1" (5).
    pub level_idx: u8,
    pub high_tier: bool,
    pub bit_depth: u8,
    pub monochrome: bool,
    pub subsampling_x: bool,
    pub subsampling_y: bool,
}

impl Av1Format {
    /// From the caps' words: `profile` (main, high, professional), `level` ("3.0"), `tier`
    /// (main, high), `bit-depth-luma`, `chroma-format` ("4:2:0", "4:4:4", "4:2:2", "4:0:0").
    pub fn from_caps(
        profile: &str,
        level: &str,
        tier: &str,
        bit_depth: u32,
        chroma: &str,
    ) -> Option<Av1Format> {
        let profile = match profile {
            "main" => 0,
            "high" => 1,
            "professional" => 2,
            _ => return None,
        };
        let (major, minor) = level.split_once('.')?;
        let (major, minor): (u8, u8) = (major.parse().ok()?, minor.parse().ok()?);
        if !(2..=7).contains(&major) || minor > 3 {
            return None;
        }
        let (sx, sy, mono) = match chroma {
            "4:2:0" => (true, true, false),
            "4:2:2" => (true, false, false),
            "4:4:4" => (false, false, false),
            "4:0:0" => (true, true, true),
            _ => return None,
        };
        Some(Av1Format {
            profile,
            level_idx: (major - 2) * 4 + minor,
            high_tier: tier == "high",
            bit_depth: u8::try_from(bit_depth).ok()?,
            monochrome: mono,
            subsampling_x: sx,
            subsampling_y: sy,
        })
    }
}

/// The `av1C` record (AV1 in ISOBMFF, §2.3): four bytes from the format, then the sequence
/// header OBU as its config OBUs.
pub fn av1c(f: &Av1Format, sequence_header: &[u8]) -> Vec<u8> {
    let high = f.bit_depth > 8;
    let twelve = f.bit_depth == 12;
    let mut v = vec![
        0x81,
        (f.profile << 5) | (f.level_idx & 0x1f),
        (u8::from(f.high_tier) << 7)
            | (u8::from(high) << 6)
            | (u8::from(twelve) << 5)
            | (u8::from(f.monochrome) << 4)
            | (u8::from(f.subsampling_x) << 3)
            | (u8::from(f.subsampling_y) << 2),
        0,
    ];
    v.extend_from_slice(sequence_header);
    v
}

/// The sequence header OBU of a temporal unit, if it has one (every keyframe's does).
pub fn sequence_header(tu: &[u8]) -> Option<&[u8]> {
    obus(tu)
        .into_iter()
        .find(|o| o.kind == OBU_SEQUENCE_HEADER)
        .map(|o| o.bytes)
}

// ---------------------------------------------------------------------------------------
// Interleaving.

/// Puts the picture's and the sound's tags in decode-time order. Each side waits for the
/// other, except when the other has given nothing for `wait_ms` of its own stream, so a
/// stalled encoder never holds the other back for long.
#[derive(Debug)]
pub struct Interleave {
    video: VecDeque<Tag>,
    audio: VecDeque<Tag>,
    wait_ms: u64,
}

impl Interleave {
    pub fn new(wait_ms: u64) -> Interleave {
        Interleave {
            video: VecDeque::new(),
            audio: VecDeque::new(),
            wait_ms,
        }
    }

    /// Take a tag in, and give back every tag that is now due, oldest first.
    pub fn push(&mut self, tag: Tag) -> Vec<Tag> {
        match tag.kind {
            Kind::Video => self.video.push_back(tag),
            _ => self.audio.push_back(tag),
        }
        let mut out = Vec::new();
        loop {
            let next = match (self.video.front(), self.audio.front()) {
                (Some(v), Some(a)) => {
                    if v.dts_ms <= a.dts_ms {
                        self.video.pop_front()
                    } else {
                        self.audio.pop_front()
                    }
                }
                (Some(v), None) => {
                    let newest = self.video.back().map_or(v.dts_ms, |t| t.dts_ms);
                    if newest.saturating_sub(v.dts_ms) > self.wait_ms {
                        self.video.pop_front()
                    } else {
                        None
                    }
                }
                (None, Some(a)) => {
                    let newest = self.audio.back().map_or(a.dts_ms, |t| t.dts_ms);
                    if newest.saturating_sub(a.dts_ms) > self.wait_ms {
                        self.audio.pop_front()
                    } else {
                        None
                    }
                }
                (None, None) => None,
            };
            match next {
                Some(t) => out.push(t),
                None => break,
            }
        }
        out
    }

    pub fn clear(&mut self) {
        self.video.clear();
        self.audio.clear();
    }
}

/// How long a destination waits before trying again after its `n`th failure in a row
/// (counted from 1): doubling from `min_s`, at most `max_s`.
pub fn backoff_s(n: u32, min_s: u32, max_s: u32) -> u32 {
    let shift = n.saturating_sub(1).min(16);
    min_s.saturating_mul(1u32 << shift).min(max_s).max(min_s)
}

/// `text` with every appearance of `secret` replaced, so a stream key never reaches a log, a
/// status or an error message (GStreamer's RTMP errors can repeat the location).
pub fn redact(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        return text.to_owned();
    }
    text.replace(secret, "•••")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tag read back: type, timestamp, body.
    fn parse(b: &[u8]) -> (u8, u32, Vec<u8>) {
        let size = u32::from_be_bytes([0, b[1], b[2], b[3]]) as usize;
        let ts = u32::from_be_bytes([b[7], b[4], b[5], b[6]]);
        let body = b[11..11 + size].to_vec();
        let prev = u32::from_be_bytes(b[11 + size..15 + size].try_into().unwrap());
        assert_eq!(prev as usize, size + 11, "the trailing size is the tag's");
        assert_eq!(b.len(), size + 15);
        (b[0], ts, body)
    }

    #[test]
    fn a_tag_carries_its_size_its_time_with_the_extended_byte_and_its_trailing_size() {
        let t = aac_frame(0, &[1, 2, 3]);
        let (kind, ts, body) = parse(&t.bytes(0x0123_4567));
        assert_eq!(kind, 8);
        assert_eq!(ts, 0x0123_4567);
        assert_eq!(body, vec![0xAF, 1, 1, 2, 3]);
    }

    #[test]
    fn the_file_header_says_which_streams_follow() {
        assert_eq!(file_header(true, true)[..9], *b"FLV\x01\x05\0\0\0\x09");
        assert_eq!(file_header(false, true)[4], 1);
        assert_eq!(file_header(true, true).len(), 13);
    }

    #[test]
    fn classic_h264_and_aac_tags_are_as_flv_has_them() {
        let s = avc_sequence(&[1, 0x64, 0, 0x1f]);
        assert_eq!(s.body[..5], [0x17, 0, 0, 0, 0]);
        let k = avc_frame(40, true, 0, &[0, 0, 0, 1, 0x65]);
        assert_eq!(k.body[..5], [0x17, 1, 0, 0, 0]);
        assert!(k.key);
        let p = avc_frame(80, false, -33, &[0, 0, 0, 1, 0x41]);
        assert_eq!(p.body[0], 0x27);
        assert_eq!(p.body[2..5], (-33i32).to_be_bytes()[1..]);
        assert_eq!(aac_sequence(&[0x12, 0x10]).body, vec![0xAF, 0, 0x12, 0x10]);
    }

    #[test]
    fn enhanced_av1_and_opus_tags_carry_their_fourcc_and_packet_type() {
        // FFmpeg's own E-FLV (2026-11-06): av01 start 0x90, coded frames 0x91 key / 0xA1
        // inter; Opus start 0x90, coded frames 0x91.
        assert_eq!(
            av1_sequence(&[0x81]).body[..5],
            [0x90, b'a', b'v', b'0', b'1']
        );
        assert_eq!(
            av1_frame(0, true, &[]).body[..5],
            [0x91, b'a', b'v', b'0', b'1']
        );
        assert_eq!(av1_frame(0, false, &[]).body[0], 0xA1);
        assert_eq!(
            opus_sequence(b"OpusHead").body[..5],
            [0x90, b'O', b'p', b'u', b's']
        );
        assert_eq!(
            opus_frame(20, &[7]).body,
            vec![0x91, b'O', b'p', b'u', b's', 7]
        );
    }

    /// A temporal delimiter, a sequence header of 3 bytes and a frame of 2, each with a size.
    const TU: [u8; 11] = [0x12, 0x00, 0x0A, 0x03, 1, 2, 3, 0x32, 0x02, 9, 9];

    #[test]
    fn obus_are_split_by_their_size_fields_and_the_delimiter_is_dropped_from_coded_frames() {
        let o = obus(&TU);
        assert_eq!(o.len(), 3);
        assert_eq!(o[0].kind, OBU_TEMPORAL_DELIMITER);
        assert_eq!(o[1].kind, OBU_SEQUENCE_HEADER);
        assert_eq!(o[1].bytes, &[0x0A, 0x03, 1, 2, 3]);
        assert_eq!(o[2].kind, 6);
        assert_eq!(sequence_header(&TU), Some(&[0x0A, 0x03, 1, 2, 3][..]));
        let f = av1_frame(0, true, &TU);
        assert_eq!(f.body[5..], TU[2..]);
        // A cut-off OBU is not passed on.
        assert_eq!(obus(&TU[..9]).len(), 2);
    }

    #[test]
    fn the_av1c_record_follows_the_caps() {
        let f = Av1Format::from_caps("main", "3.0", "main", 10, "4:2:0").unwrap();
        assert_eq!(f.level_idx, 4);
        let r = av1c(&f, &[0x0A, 0]);
        assert_eq!(r, vec![0x81, 0x04, 0b0100_1100, 0, 0x0A, 0]);
        let g = Av1Format::from_caps("high", "5.1", "high", 8, "4:4:4").unwrap();
        assert_eq!(av1c(&g, &[])[..3], [0x81, (1 << 5) | 13, 0x80]);
        assert!(Av1Format::from_caps("main", "9.0", "main", 8, "4:2:0").is_none());
    }

    #[test]
    fn metadata_is_an_amf0_ecma_array_named_on_metadata() {
        let t = metadata(&[("width", Amf::Number(1280.0)), ("stereo", Amf::Bool(true))]);
        assert_eq!(t.kind, Kind::Script);
        assert_eq!(t.body[0], 2);
        assert_eq!(&t.body[3..13], b"onMetaData");
        assert_eq!(t.body[13], 8);
        assert_eq!(t.body[14..18], 2u32.to_be_bytes());
        assert_eq!(t.body[t.body.len() - 3..], [0, 0, 9]);
        assert_eq!(fourcc_number(&AV1), 1_635_135_537.0);
    }

    fn v(ms: u64) -> Tag {
        Tag {
            kind: Kind::Video,
            dts_ms: ms,
            key: false,
            body: vec![],
        }
    }
    fn a(ms: u64) -> Tag {
        Tag {
            kind: Kind::Audio,
            ..v(ms)
        }
    }

    #[test]
    fn interleaving_gives_tags_in_time_order_and_waits_for_the_other_side_only_so_long() {
        let mut i = Interleave::new(500);
        assert!(i.push(v(0)).is_empty());
        assert!(i.push(v(50)).is_empty());
        let out: Vec<u64> = i.push(a(20)).iter().map(|t| t.dts_ms).collect();
        assert_eq!(out, vec![0, 20]);
        let out: Vec<u64> = i.push(a(40)).iter().map(|t| t.dts_ms).collect();
        assert_eq!(out, vec![40]);
        let out: Vec<u64> = i.push(a(60)).iter().map(|t| t.dts_ms).collect();
        assert_eq!(out, vec![50]);
        // The picture stops: the sound waits 500 ms of itself, then goes on alone.
        for ms in (80..=560).step_by(20) {
            assert!(i.push(a(ms)).is_empty(), "{ms}");
        }
        let out = i.push(a(580));
        assert_eq!(out.first().map(|t| t.dts_ms), Some(60));
        assert!(out.iter().all(|t| t.kind == Kind::Audio));
    }

    #[test]
    fn backoff_doubles_to_its_ceiling() {
        let s: Vec<u32> = (1..=8).map(|n| backoff_s(n, 2, 60)).collect();
        assert_eq!(s, vec![2, 4, 8, 16, 32, 60, 60, 60]);
        assert_eq!(backoff_s(0, 2, 60), 2);
        assert_eq!(backoff_s(40, 2, 60), 60);
    }

    #[test]
    fn a_stream_key_is_never_repeated() {
        let e = "Could not connect to rtmp://h/live/sk-123: refused (sk-123)";
        assert_eq!(
            redact(e, "sk-123"),
            "Could not connect to rtmp://h/live/•••: refused (•••)"
        );
        assert_eq!(redact("x", ""), "x");
    }
}
