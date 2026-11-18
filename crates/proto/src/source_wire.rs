// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! How the ingest hands a host's own studio to the desk (ADR-0280): one loopback connection
//! for each source standing by, in the shape of an Icecast source request, as playout's feed
//! reaches the desk (ADR-0172).
//!
//! ```text
//! PUT /sources/source-k3yid0ab HTTP/1.0
//! Authorization: Basic <any user>:<WEAVESIGNAL_DESK_SOURCES_PASSWORD>
//! X-Source-Name: Finlay Grant
//! X-Source-Way: srt
//! X-Source-Format: aac 48 kHz stereo
//! X-Source-Key: k3yid0ab
//! X-Source-Show: steeped
//!
//! <a WAV header: 32-bit float, 48 kHz, stereo> <samples until the connection closes>
//! ```
//!
//! The desk answers `HTTP/1.0 200 OK` and reads until the connection closes; the source then
//! leaves the desk. Header values are plain text the ingest has already cleaned; the desk
//! cleans them again.

use std::io::{self, Read};

/// The sample rate everything at the desk runs at.
pub const RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;
/// The path's prefix; the source's desk identity follows.
pub const PATH_PREFIX: &str = "/sources/";
pub const NAME: &str = "X-Source-Name";
pub const WAY: &str = "X-Source-Way";
pub const FORMAT: &str = "X-Source-Format";
pub const KEY: &str = "X-Source-Key";
pub const SHOW: &str = "X-Source-Show";
/// The longest value the desk keeps from any of these headers.
pub const MAX_VALUE: usize = 200;

/// The desk identity of the source a show key brings: `source-` and the key's id.
pub fn identity(key_id: &str) -> String {
    format!("source-{key_id}")
}

/// A header value as the desk keeps it: control characters dropped, at most [`MAX_VALUE`]
/// characters, trimmed.
pub fn clean(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_VALUE)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The WAV header the ingest sends before the samples: 32-bit IEEE float, 48 kHz, stereo, with
/// no real length (the stream never ends on purpose).
pub fn wav_f32_header() -> Vec<u8> {
    let block = 4 * CHANNELS;
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&u32::MAX.to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&3u16.to_le_bytes());
    h.extend_from_slice(&CHANNELS.to_le_bytes());
    h.extend_from_slice(&RATE.to_le_bytes());
    h.extend_from_slice(&(RATE * u32::from(block)).to_le_bytes());
    h.extend_from_slice(&block.to_le_bytes());
    h.extend_from_slice(&32u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&u32::MAX.to_le_bytes());
    h
}

/// Read a WAV header up to its samples, and refuse anything but 32-bit float at 48 kHz, mono or
/// stereo; gives the channel count.
pub fn read_wav_f32_header(reader: &mut impl Read) -> io::Result<u16> {
    let bad = |m: String| io::Error::new(io::ErrorKind::InvalidData, m);
    let mut riff = [0u8; 12];
    reader.read_exact(&mut riff)?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return Err(bad("not WAV".into()));
    }
    let mut channels = None;
    loop {
        let mut chunk = [0u8; 8];
        reader.read_exact(&mut chunk)?;
        let size = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        match &chunk[0..4] {
            b"fmt " => {
                if !(16..=64).contains(&size) {
                    return Err(bad(format!("a WAV fmt chunk of {size} bytes")));
                }
                let mut fmt = vec![0u8; size as usize + (size as usize & 1)];
                reader.read_exact(&mut fmt)?;
                let word = |i: usize| u16::from_le_bytes([fmt[i], fmt[i + 1]]);
                let (format, ch, bits) = (word(0), word(2), word(14));
                let rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
                if format != 3 || bits != 32 || rate != RATE || !(1..=2).contains(&ch) {
                    return Err(bad(format!(
                        "{ch} channels of {bits}-bit format {format} at {rate} Hz; a source is 32-bit float at 48 kHz, mono or stereo"
                    )));
                }
                channels = Some(ch);
            }
            b"data" => {
                return channels.ok_or_else(|| bad("a WAV data chunk before its fmt chunk".into()));
            }
            _ => {
                if size > 1 << 16 {
                    return Err(bad(format!(
                        "a WAV chunk of {size} bytes before the samples"
                    )));
                }
                let skip = u64::from(size) + u64::from(size & 1);
                io::copy(&mut (&mut *reader).take(skip), &mut io::sink())?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_is_read_back_and_other_formats_are_refused() {
        let h = wav_f32_header();
        assert_eq!(h.len(), 44);
        assert_eq!(read_wav_f32_header(&mut &h[..]).unwrap(), 2);
        let mut s16 = h.clone();
        s16[20] = 1; // PCM
        s16[34] = 16;
        assert!(read_wav_f32_header(&mut &s16[..]).is_err());
        let mut rate = h.clone();
        rate[24..28].copy_from_slice(&44_100u32.to_le_bytes());
        assert!(read_wav_f32_header(&mut &rate[..]).is_err());
    }

    #[test]
    fn values_are_cleaned_and_cut() {
        assert_eq!(clean("  Finlay\u{7}\r\n Grant "), "Finlay Grant");
        assert_eq!(clean(&"x".repeat(500)).len(), MAX_VALUE);
        assert_eq!(identity("abcd2345"), "source-abcd2345");
    }
}
