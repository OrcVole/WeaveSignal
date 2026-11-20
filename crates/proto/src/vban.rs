// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! VBAN's audio packets (the presenter kit's server half, Phase 17g), written from VB-Audio's
//! description (*VBAN Protocol*, revision 13, September 2025, whose protocol base is "free to
//! use AS IS"); none of its C is copied, and VB-Audio's own codecs (VBCA, VBCV) are refused.
//!
//! A packet is UDP: a 28-byte header, little-endian, then at most 1,436 bytes of samples.
//!
//! ```text
//! 0..4    "VBAN"
//! 4       sample-rate index (bits 0–4) and sub-protocol (bits 5–7; 0 is audio)
//! 5       samples per packet, minus one
//! 6       channels, minus one
//! 7       data type (bits 0–2), a reserved bit (3, always 0), codec (bits 4–7; 0 is PCM)
//! 8..24   the stream's name, ASCII, padded with zeros
//! 24..28  the packet counter
//! ```
//!
//! VBAN has no authentication and no encryption: anyone on the path can send or read it. The
//! ingest drops every packet whose sender and name are not bound to a show key, and the desk
//! sends only to one named address. [`parse`] is the only reader, and it refuses anything it
//! does not fully understand.

use std::fmt;

/// The four bytes every packet starts with.
pub const MAGIC: [u8; 4] = *b"VBAN";
pub const HEADER_LEN: usize = 28;
/// The most sample data one packet may carry.
pub const MAX_DATA: usize = 1436;
pub const MAX_PACKET: usize = HEADER_LEN + MAX_DATA;
/// VBAN's usual port.
pub const DEFAULT_PORT: u16 = 6980;
/// The most samples (frames) one packet may carry.
pub const MAX_SAMPLES: usize = 256;
/// The longest stream name.
pub const NAME_LEN: usize = 16;

/// The sample rates by their index in the header (the specification's table).
pub const RATES: [u32; 21] = [
    6_000, 12_000, 24_000, 48_000, 96_000, 192_000, 384_000, 8_000, 16_000, 32_000, 64_000,
    128_000, 256_000, 512_000, 11_025, 22_050, 44_100, 88_200, 176_400, 352_800, 705_600,
];

/// The index of a rate in [`RATES`].
pub fn rate_index(rate: u32) -> Option<u8> {
    RATES.iter().position(|&r| r == rate).map(|i| i as u8)
}

/// How the samples are written. The station reads and writes the four PCM types below; 8-bit,
/// 64-bit float and the packed 10- and 12-bit types are refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataType {
    I16,
    I24,
    I32,
    F32,
}

impl DataType {
    pub fn code(self) -> u8 {
        match self {
            DataType::I16 => 1,
            DataType::I24 => 2,
            DataType::I32 => 3,
            DataType::F32 => 4,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(DataType::I16),
            2 => Some(DataType::I24),
            3 => Some(DataType::I32),
            4 => Some(DataType::F32),
            _ => None,
        }
    }

    /// Bytes per sample.
    pub fn bytes(self) -> usize {
        match self {
            DataType::I16 => 2,
            DataType::I24 => 3,
            DataType::I32 | DataType::F32 => 4,
        }
    }

    /// As a setting names it: `s16`, `s24`, `s32`, `f32`.
    pub fn name(self) -> &'static str {
        match self {
            DataType::I16 => "s16",
            DataType::I24 => "s24",
            DataType::I32 => "s32",
            DataType::F32 => "f32",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "s16" => Some(DataType::I16),
            "s24" => Some(DataType::I24),
            "s32" => Some(DataType::I32),
            "f32" => Some(DataType::F32),
            _ => None,
        }
    }
}

/// A stream's name: printable ASCII, 1 to 16 characters.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamName {
    bytes: [u8; NAME_LEN],
    len: u8,
}

impl StreamName {
    /// `None` for an empty name, one over 16 characters, or one with anything but printable
    /// ASCII.
    pub fn new(name: &str) -> Option<Self> {
        let b = name.as_bytes();
        if b.is_empty() || b.len() > NAME_LEN || !b.iter().all(|c| (0x20..0x7f).contains(c)) {
            return None;
        }
        let mut bytes = [0u8; NAME_LEN];
        bytes[..b.len()].copy_from_slice(b);
        Some(StreamName {
            bytes,
            len: b.len() as u8,
        })
    }

    pub fn as_str(&self) -> &str {
        // Only printable ASCII is ever stored.
        std::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or_default()
    }
}

impl fmt::Debug for StreamName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl fmt::Display for StreamName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An audio packet's header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub rate: u32,
    /// Frames in this packet, 1 to 256.
    pub samples: u16,
    /// 1 to 256.
    pub channels: u16,
    pub data: DataType,
    pub name: StreamName,
    /// The packet counter.
    pub frame: u32,
}

impl Header {
    /// How many bytes of samples this header promises.
    pub fn data_len(&self) -> usize {
        usize::from(self.samples) * usize::from(self.channels) * self.data.bytes()
    }
}

/// Why a packet was dropped; each is counted separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// Shorter than a header, or longer than [`MAX_PACKET`].
    Size,
    /// Not `VBAN`.
    Magic,
    /// A sample-rate index past the table.
    Rate,
    /// Not the audio sub-protocol (serial, text, service).
    NotAudio,
    /// The reserved bit is set.
    Reserved,
    /// A codec other than PCM (VB-Audio's own are not ours to read).
    Codec,
    /// A data type the station does not read.
    DataType,
    /// The name is empty or not printable ASCII.
    Name,
    /// The samples do not fill exactly what the header says.
    Length,
}

impl Refusal {
    /// The word counted in the status.
    pub fn word(self) -> &'static str {
        match self {
            Refusal::Size => "size",
            Refusal::Magic => "magic",
            Refusal::Rate => "rate",
            Refusal::NotAudio => "not_audio",
            Refusal::Reserved => "reserved",
            Refusal::Codec => "codec",
            Refusal::DataType => "data_type",
            Refusal::Name => "name",
            Refusal::Length => "length",
        }
    }
}

/// Read one packet: its header and its samples.
pub fn parse(packet: &[u8]) -> Result<(Header, &[u8]), Refusal> {
    if packet.len() < HEADER_LEN || packet.len() > MAX_PACKET {
        return Err(Refusal::Size);
    }
    if packet[0..4] != MAGIC {
        return Err(Refusal::Magic);
    }
    let sr = packet[4];
    if sr & 0xE0 != 0 {
        return Err(Refusal::NotAudio);
    }
    let rate = *RATES.get(usize::from(sr & 0x1F)).ok_or(Refusal::Rate)?;
    let bit = packet[7];
    if bit & 0x08 != 0 {
        return Err(Refusal::Reserved);
    }
    if bit & 0xF0 != 0 {
        return Err(Refusal::Codec);
    }
    let data = DataType::from_code(bit & 0x07).ok_or(Refusal::DataType)?;
    let raw = &packet[8..24];
    let end = raw.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
    if raw[end..].iter().any(|&b| b != 0) {
        return Err(Refusal::Name);
    }
    let name = std::str::from_utf8(&raw[..end])
        .ok()
        .and_then(StreamName::new)
        .ok_or(Refusal::Name)?;
    let header = Header {
        rate,
        samples: u16::from(packet[5]) + 1,
        channels: u16::from(packet[6]) + 1,
        data,
        name,
        frame: u32::from_le_bytes([packet[24], packet[25], packet[26], packet[27]]),
    };
    let payload = &packet[HEADER_LEN..];
    if payload.len() != header.data_len() {
        return Err(Refusal::Length);
    }
    Ok((header, payload))
}

/// Write a header; `None` when it cannot be written (a rate not in the table, or sizes out of
/// range or over [`MAX_DATA`]).
pub fn write(h: &Header, out: &mut Vec<u8>) -> Option<()> {
    let sr = rate_index(h.rate)?;
    if !(1..=MAX_SAMPLES as u16).contains(&h.samples)
        || !(1..=256).contains(&h.channels)
        || h.data_len() > MAX_DATA
    {
        return None;
    }
    out.extend_from_slice(&MAGIC);
    out.push(sr);
    out.push((h.samples - 1) as u8);
    out.push((h.channels - 1) as u8);
    out.push(h.data.code());
    out.extend_from_slice(&h.name.bytes);
    out.extend_from_slice(&h.frame.to_le_bytes());
    Some(())
}

/// One sample as `f32`; anything not finite is silence.
fn sample(data: DataType, b: &[u8]) -> f32 {
    let v = match data {
        DataType::I16 => f32::from(i16::from_le_bytes([b[0], b[1]])) / 32_768.0,
        DataType::I24 => {
            let v = i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8;
            v as f32 / 8_388_608.0
        }
        DataType::I32 => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0,
        DataType::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
    };
    if v.is_finite() {
        v.clamp(-4.0, 4.0)
    } else {
        0.0
    }
}

/// A packet's samples as interleaved stereo `f32`: channels `pair` (0-based) when it has more
/// than one, mono heard in both ears. The header must be [`parse`]'s.
pub fn to_stereo(h: &Header, payload: &[u8], pair: (u16, u16), out: &mut Vec<f32>) {
    let ch = usize::from(h.channels);
    let size = h.data.bytes();
    let (l, r) = if ch == 1 {
        (0, 0)
    } else {
        (
            usize::from(pair.0).min(ch - 1),
            usize::from(pair.1).min(ch - 1),
        )
    };
    for frame in payload.chunks_exact(ch * size) {
        out.push(sample(h.data, &frame[l * size..(l + 1) * size]));
        out.push(sample(h.data, &frame[r * size..(r + 1) * size]));
    }
}

fn put(data: DataType, v: f32, out: &mut Vec<u8>) {
    let v = if v.is_finite() {
        v.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    match data {
        DataType::I16 => {
            out.extend_from_slice(&((v * 32_767.0).round() as i16).to_le_bytes());
        }
        DataType::I24 => {
            let s = (f64::from(v) * 8_388_607.0).round() as i32;
            out.extend_from_slice(&s.to_le_bytes()[..3]);
        }
        DataType::I32 => {
            let s = (f64::from(v) * 2_147_483_647.0).round() as i32;
            out.extend_from_slice(&s.to_le_bytes());
        }
        DataType::F32 => out.extend_from_slice(&v.to_le_bytes()),
    }
}

/// The frames per packet the desk sends for each type, in stereo: 960 bytes of samples, four
/// or eight packets to a 20 ms block (16-bit 240, 24-bit 160, 32-bit 120).
pub fn frames_per_packet(data: DataType) -> usize {
    960 / (2 * data.bytes())
}

/// Interleaved stereo blocks from the desk into packets, by the blocks' place in the mix.
///
/// A block that follows the one before (its position is where that one ended) carries on
/// filling packets of [`frames_per_packet`]; one that does not (the mix restarted from the
/// clock after a stall) first sends what was left as a shorter packet. The counter only ever
/// goes up, by one a packet, as receivers expect.
pub struct Packetiser {
    name: StreamName,
    data: DataType,
    per_packet: usize,
    pending: Vec<f32>,
    /// Where the next block should start, in frames of the mix.
    next_pos: Option<i64>,
    counter: u32,
}

impl Packetiser {
    pub fn new(name: StreamName, data: DataType) -> Self {
        let per_packet = frames_per_packet(data);
        Packetiser {
            name,
            data,
            per_packet,
            pending: Vec::with_capacity(2 * per_packet),
            next_pos: None,
            counter: 0,
        }
    }

    /// The counter the next packet will carry.
    pub fn counter(&self) -> u32 {
        self.counter
    }

    /// Frames waiting for the next packet.
    pub fn pending_frames(&self) -> usize {
        self.pending.len() / 2
    }

    fn packet(&mut self, frames: &[f32], out: &mut Vec<Vec<u8>>) {
        let n = frames.len() / 2;
        let h = Header {
            rate: 48_000,
            samples: n as u16,
            channels: 2,
            data: self.data,
            name: self.name,
            frame: self.counter,
        };
        let mut p = Vec::with_capacity(HEADER_LEN + h.data_len());
        if write(&h, &mut p).is_none() {
            return;
        }
        for &v in frames {
            put(self.data, v, &mut p);
        }
        self.counter = self.counter.wrapping_add(1);
        out.push(p);
    }

    /// Send what is waiting as a shorter packet.
    pub fn flush(&mut self, out: &mut Vec<Vec<u8>>) {
        if !self.pending.is_empty() {
            let rest = std::mem::take(&mut self.pending);
            self.packet(&rest, out);
        }
    }

    /// Take the block of interleaved stereo at `pos`; add the packets it completes to `out`.
    pub fn push(&mut self, pos: i64, block: &[f32], out: &mut Vec<Vec<u8>>) {
        if self.next_pos.is_some_and(|p| p != pos) {
            self.flush(out);
        }
        self.next_pos = Some(pos + (block.len() / 2) as i64);
        self.pending
            .extend_from_slice(&block[..block.len() / 2 * 2]);
        let whole = 2 * self.per_packet;
        let mut start = 0;
        while self.pending.len() - start >= whole {
            let piece: Vec<f32> = self.pending[start..start + whole].to_vec();
            self.packet(&piece, out);
            start += whole;
        }
        self.pending.drain(..start);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(samples: u16, channels: u16, data: DataType) -> Header {
        Header {
            rate: 48_000,
            samples,
            channels,
            data,
            name: StreamName::new("Stream1").unwrap(),
            frame: 7,
        }
    }

    fn packet(h: &Header) -> Vec<u8> {
        let mut p = Vec::new();
        write(h, &mut p).unwrap();
        p.resize(HEADER_LEN + h.data_len(), 0);
        p
    }

    #[test]
    fn the_rate_table_is_the_specifications() {
        assert_eq!(rate_index(48_000), Some(3));
        assert_eq!(rate_index(44_100), Some(16));
        assert_eq!(rate_index(705_600), Some(20));
        assert_eq!(rate_index(47_999), None);
    }

    #[test]
    fn a_header_round_trips_byte_for_byte() {
        for data in [DataType::I16, DataType::I24, DataType::I32, DataType::F32] {
            let h = header(frames_per_packet(data) as u16, 2, data);
            let p = packet(&h);
            assert_eq!(&p[0..4], b"VBAN");
            assert_eq!(p[4], 3, "48 kHz, audio");
            assert_eq!(p[7], data.code());
            let (back, payload) = parse(&p).unwrap();
            assert_eq!(back, h);
            assert_eq!(payload.len(), 960);
        }
        // A 16-character name, which has no zero after it.
        let mut h = header(1, 1, DataType::I16);
        h.name = StreamName::new("ABCDEFGHIJKLMNOP").unwrap();
        assert_eq!(
            parse(&packet(&h)).unwrap().0.name.as_str(),
            "ABCDEFGHIJKLMNOP"
        );
    }

    #[test]
    fn every_refusal_has_its_reason() {
        let good = packet(&header(240, 2, DataType::I16));
        let with = |i: usize, v: u8| {
            let mut p = good.clone();
            p[i] = v;
            p
        };
        assert_eq!(parse(&good[..27]), Err(Refusal::Size));
        assert_eq!(parse(&vec![0u8; MAX_PACKET + 1]), Err(Refusal::Size));
        assert_eq!(parse(&with(0, b'X')), Err(Refusal::Magic));
        assert_eq!(parse(&with(4, 21)), Err(Refusal::Rate));
        assert_eq!(parse(&with(4, 0x23)), Err(Refusal::NotAudio));
        assert_eq!(parse(&with(4, 0x43)), Err(Refusal::NotAudio));
        assert_eq!(parse(&with(7, 0x09)), Err(Refusal::Reserved));
        assert_eq!(parse(&with(7, 0x11)), Err(Refusal::Codec));
        assert_eq!(parse(&with(7, 0x00)), Err(Refusal::DataType));
        assert_eq!(parse(&with(7, 0x05)), Err(Refusal::DataType));
        assert_eq!(parse(&with(8, 0)), Err(Refusal::Name), "empty");
        assert_eq!(parse(&with(9, 0x07)), Err(Refusal::Name), "a control");
        assert_eq!(
            parse(&with(9, 0)),
            Err(Refusal::Name),
            "bytes after the end"
        );
        assert_eq!(parse(&good[..good.len() - 1]), Err(Refusal::Length));
        let mut long = good.clone();
        long.push(0);
        assert_eq!(parse(&long), Err(Refusal::Length));
        // 256 stereo frames of 32-bit: 2,048 bytes, over a packet's 1,436.
        let mut p = good.clone();
        p[5] = 255;
        p[7] = 4;
        p.resize(HEADER_LEN + 2048, 0);
        assert_eq!(parse(&p), Err(Refusal::Size));
        assert!(write(&header(256, 2, DataType::F32), &mut Vec::new()).is_none());
    }

    #[test]
    fn random_bytes_never_panic_and_rarely_pass() {
        // A fuzz-style loop without a dependency: xorshift over sizes and contents, half of
        // them starting with the magic so the deeper checks are reached.
        let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let mut passed = 0;
        for i in 0..200_000 {
            let len = (next() % (MAX_PACKET as u64 + 40)) as usize;
            let mut p: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            if i % 2 == 0 && len >= 4 {
                p[..4].copy_from_slice(b"VBAN");
            }
            if let Ok((h, payload)) = parse(&p) {
                passed += 1;
                assert_eq!(payload.len(), h.data_len());
                let mut out = Vec::new();
                to_stereo(&h, payload, (0, 1), &mut out);
                assert_eq!(out.len(), 2 * usize::from(h.samples));
                assert!(out.iter().all(|v| v.is_finite()));
            }
        }
        assert!(passed < 50, "{passed} random packets passed");
    }

    #[test]
    fn samples_read_back_as_written_in_every_type() {
        let block: Vec<f32> = (0..480).map(|i| ((i as f32) * 0.01).sin() * 0.8).collect();
        for (data, tolerance) in [
            (DataType::I16, 1.0 / 16_000.0),
            (DataType::I24, 1.0 / 4_000_000.0),
            (DataType::I32, 1e-6),
            (DataType::F32, 0.0),
        ] {
            let mut p = Packetiser::new(StreamName::new("Out").unwrap(), data);
            let mut packets = Vec::new();
            p.push(0, &block, &mut packets);
            p.flush(&mut packets);
            let mut back = Vec::new();
            for pk in &packets {
                let (h, payload) = parse(pk).unwrap();
                to_stereo(&h, payload, (0, 1), &mut back);
            }
            assert_eq!(back.len(), block.len(), "{data:?}");
            for (a, b) in back.iter().zip(&block) {
                assert!((a - b).abs() <= tolerance, "{data:?}: {a} against {b}");
            }
        }
    }

    #[test]
    fn a_pair_is_picked_from_many_channels_and_mono_is_doubled() {
        let mut h = header(2, 4, DataType::I16);
        let mut payload = Vec::new();
        for v in [1i16, 2, 3, 4, 5, 6, 7, 8] {
            payload.extend_from_slice(&(v * 1000).to_le_bytes());
        }
        let mut out = Vec::new();
        to_stereo(&h, &payload, (2, 3), &mut out);
        let want: Vec<f32> = [3, 4, 7, 8]
            .iter()
            .map(|v| (v * 1000) as f32 / 32_768.0)
            .collect();
        assert_eq!(out, want);
        h.channels = 1;
        out.clear();
        to_stereo(&h, &payload[..4], (0, 1), &mut out);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0], out[1]);
    }

    #[test]
    fn packets_follow_the_mix_position_and_the_counter_only_rises() {
        let mut p = Packetiser::new(StreamName::new("Programme").unwrap(), DataType::I16);
        let mut out = Vec::new();
        // A 20 ms block is four packets of 240.
        p.push(0, &vec![0.1; 1920], &mut out);
        assert_eq!(out.len(), 4);
        // A block of 1,000 frames (not a multiple of 240): four packets, 40 frames waiting.
        out.clear();
        p.push(960, &vec![0.1; 2000], &mut out);
        assert_eq!(out.len(), 4);
        // The mix restarts from the clock: what waited goes as a short packet first.
        out.clear();
        p.push(50_000, &vec![0.1; 1920], &mut out);
        let sizes: Vec<u16> = out.iter().map(|pk| parse(pk).unwrap().0.samples).collect();
        assert_eq!(sizes, vec![40, 240, 240, 240, 240]);
        let counters: Vec<u32> = out.iter().map(|pk| parse(pk).unwrap().0.frame).collect();
        assert_eq!(counters, vec![8, 9, 10, 11, 12]);
        assert_eq!(p.counter(), 13);
    }

    #[test]
    fn names_are_checked() {
        assert!(StreamName::new("").is_none());
        assert!(StreamName::new("ABCDEFGHIJKLMNOPQ").is_none());
        assert!(StreamName::new("Tab\there").is_none());
        assert!(StreamName::new("Stùdio").is_none());
        assert_eq!(StreamName::new("Studio A").unwrap().as_str(), "Studio A");
        assert_eq!(DataType::from_name("s24"), Some(DataType::I24));
        assert_eq!(DataType::I24.name(), "s24");
    }
}
