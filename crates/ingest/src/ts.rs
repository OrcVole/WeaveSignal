// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The sound out of MPEG-TS, as SRT carries it (ADR-0280): the programme map from the PAT, the
//! first audio stream from the map, and that stream's PES payloads joined into the elementary
//! stream a reader takes (ADTS for AAC, frames for MPEG audio). Picture and data streams are
//! skipped (*Your picture is not used yet*). Nothing is probed: the stream type names the codec.

use std::io::{self, Read};

const PACKET: usize = 188;
/// How far into a stream its map must appear before it is given up as not MPEG-TS with sound.
const MAP_WITHIN: usize = 4 * 1024 * 1024;

/// The sound a TS carries, by its stream type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Audio {
    /// `0x0F`: AAC in ADTS.
    Aac,
    /// `0x03` or `0x04`: MPEG audio (layer II or III).
    Mpeg,
}

/// A stream type the station does not take, named for the refusal.
pub fn unsupported(stream_type: u8) -> Option<&'static str> {
    match stream_type {
        0x11 => Some("AAC in LATM"),
        0x81 | 0x06 => Some("AC-3, Opus or another private audio stream"),
        0x87 => Some("E-AC-3"),
        _ => None,
    }
}

pub struct TsAudio<R> {
    inner: R,
    pmt_pid: Option<u16>,
    audio: Option<(u16, Audio)>,
    /// A stream type seen in the map that the station does not take.
    refused: Option<u8>,
    /// Elementary-stream bytes not yet read out.
    pending: Vec<u8>,
    taken: usize,
    read_in: usize,
}

impl<R: Read> TsAudio<R> {
    pub fn new(inner: R) -> Self {
        TsAudio {
            inner,
            pmt_pid: None,
            audio: None,
            refused: None,
            pending: Vec::new(),
            taken: 0,
            read_in: 0,
        }
    }

    /// Read until the map names an audio stream; which kind it is.
    pub fn audio(&mut self) -> io::Result<Audio> {
        while self.audio.is_none() {
            if self.read_in > MAP_WITHIN {
                let what = self
                    .refused
                    .and_then(unsupported)
                    .map_or("no sound the station reads".to_owned(), str::to_owned);
                return Err(io::Error::new(io::ErrorKind::InvalidData, what));
            }
            if !self.packet()? {
                let what = self
                    .refused
                    .and_then(unsupported)
                    .unwrap_or("no sound the station reads");
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{what} (the stream ended before its map named one)"),
                ));
            }
        }
        Ok(self.audio.expect("set above").1)
    }

    /// Read one packet and take what it holds; `false` at the end.
    fn packet(&mut self) -> io::Result<bool> {
        let mut p = [0u8; PACKET];
        // Find the sync byte: a stream joined part-way, or a lost byte, starts anywhere.
        loop {
            let mut b = [0u8; 1];
            if self.inner.read(&mut b)? == 0 {
                return Ok(false);
            }
            self.read_in += 1;
            if b[0] == 0x47 {
                p[0] = 0x47;
                break;
            }
        }
        if let Err(e) = self.inner.read_exact(&mut p[1..]) {
            return if e.kind() == io::ErrorKind::UnexpectedEof {
                Ok(false)
            } else {
                Err(e)
            };
        }
        self.read_in += PACKET - 1;
        let start = p[1] & 0x40 != 0;
        let pid = (u16::from(p[1] & 0x1F) << 8) | u16::from(p[2]);
        let afc = (p[3] >> 4) & 0x3;
        let mut at = 4;
        if afc == 2 || afc == 0 {
            return Ok(true); // no payload
        }
        if afc == 3 {
            at += 1 + usize::from(p[4]);
            if at >= PACKET {
                return Ok(true);
            }
        }
        let payload = &p[at..];
        if pid == 0 && start {
            self.pat(payload);
        } else if Some(pid) == self.pmt_pid && start && self.audio.is_none() {
            self.pmt(payload);
        } else if let Some((apid, _)) = self.audio
            && pid == apid
        {
            self.pes(payload, start);
        }
        Ok(true)
    }

    fn pat(&mut self, payload: &[u8]) {
        let Some(s) = section(payload) else { return };
        // After the 8-byte section header, 4-byte entries up to the CRC.
        let len = s.len().saturating_sub(4);
        let mut i = 8;
        while i + 4 <= len {
            let program = u16::from_be_bytes([s[i], s[i + 1]]);
            let pid = (u16::from(s[i + 2] & 0x1F) << 8) | u16::from(s[i + 3]);
            if program != 0 {
                self.pmt_pid = Some(pid);
                return;
            }
            i += 4;
        }
    }

    fn pmt(&mut self, payload: &[u8]) {
        let Some(s) = section(payload) else { return };
        if s.len() < 12 {
            return;
        }
        let len = s.len() - 4;
        let info = usize::from(u16::from_be_bytes([s[10] & 0x0F, s[11]]));
        let mut i = 12 + info;
        while i + 5 <= len {
            let stream_type = s[i];
            let pid = (u16::from(s[i + 1] & 0x1F) << 8) | u16::from(s[i + 2]);
            let es_info = usize::from(u16::from_be_bytes([s[i + 3] & 0x0F, s[i + 4]]));
            let kind = match stream_type {
                0x0F => Some(Audio::Aac),
                0x03 | 0x04 => Some(Audio::Mpeg),
                t if unsupported(t).is_some() => {
                    self.refused.get_or_insert(t);
                    None
                }
                _ => None,
            };
            if let Some(k) = kind {
                self.audio = Some((pid, k));
                return;
            }
            i += 5 + es_info;
        }
    }

    fn pes(&mut self, payload: &[u8], start: bool) {
        if start {
            // 00 00 01, stream id, length (2), flags (2), header length, then the header.
            if payload.len() < 9 || payload[..3] != [0, 0, 1] {
                return;
            }
            let at = 9 + usize::from(payload[8]);
            if at <= payload.len() {
                self.pending.extend_from_slice(&payload[at..]);
            }
        } else if !self.pending.is_empty() || self.taken > 0 {
            self.pending.extend_from_slice(payload);
        }
    }
}

/// A PSI section from a payload that starts one: past the pointer field, cut to its length.
fn section(payload: &[u8]) -> Option<&[u8]> {
    let pointer = usize::from(*payload.first()?);
    let s = payload.get(1 + pointer..)?;
    if s.len() < 3 {
        return None;
    }
    let len = usize::from(u16::from_be_bytes([s[1] & 0x0F, s[2]]));
    s.get(..3 + len)
}

impl<R: Read> Read for TsAudio<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.pending.is_empty() {
            if self.audio.is_none() {
                self.audio()?;
                continue;
            }
            if !self.packet()? {
                return Ok(0);
            }
        }
        let n = out.len().min(self.pending.len());
        out[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        self.taken += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn the_sound_is_found_behind_a_picture_and_comes_out_as_adts() {
        let ts = fixture("tone-48k-aac-video.ts");
        let mut a = TsAudio::new(&ts[..]);
        assert_eq!(a.audio().unwrap(), Audio::Aac);
        let mut es = Vec::new();
        a.read_to_end(&mut es).unwrap();
        // ADTS frames: each starts with the 12-bit sync word 0xFFF.
        assert_eq!((es[0], es[1] & 0xF0), (0xFF, 0xF0));
        let frames = es
            .windows(2)
            .filter(|w| w[0] == 0xFF && w[1] & 0xF6 == 0xF0)
            .count();
        assert!(frames >= 40, "about 47 AAC frames in a second: {frames}");
    }

    #[test]
    fn layer_ii_is_mpeg_audio() {
        let ts = fixture("tone-44k-mp2.ts");
        let mut a = TsAudio::new(&ts[..]);
        assert_eq!(a.audio().unwrap(), Audio::Mpeg);
    }

    #[test]
    fn a_stream_part_way_in_is_found_from_the_next_map() {
        let ts = fixture("tone-48k-aac-video.ts");
        // Start 1,000 bytes in: not on a packet's start.
        let mut a = TsAudio::new(&ts[1_000..]);
        assert_eq!(a.audio().unwrap(), Audio::Aac);
    }

    #[test]
    fn something_else_is_refused_in_words() {
        let junk = vec![0u8; MAP_WITHIN + 2 * PACKET];
        let e = TsAudio::new(&junk[..]).audio().unwrap_err();
        assert!(e.to_string().contains("no sound"), "{e}");
        assert_eq!(
            unsupported(0x81),
            Some("AC-3, Opus or another private audio stream")
        );
    }
}
