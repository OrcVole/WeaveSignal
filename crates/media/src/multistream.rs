// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Opus multistream packets (surround, Ogg mapping family 1), decoded stream by stream.
//!
//! opus-decoder 0.1.1's own multistream decoder reads each stream's self-delimiting length as
//! if it came before the stream's packet, so it refuses every real surround packet (the demo
//! pack's 5.1 drum roll among them). In Opus the length follows the TOC byte and any frame
//! counts, and gives the size of the last frame (RFC 6716 appendix B). This splits a
//! multistream packet correctly into ordinary packets and decodes each with the single-stream
//! decoder.

use opus_decoder::{OpusDecoder, OpusError};

/// An Opus frame length: one byte under 252, else two (RFC 6716 section 3.2.1).
fn frame_len(d: &[u8]) -> Option<(usize, usize)> {
    let first = usize::from(*d.first()?);
    if first < 252 {
        Some((first, 1))
    } else {
        Some((first + 4 * usize::from(*d.get(1)?), 2))
    }
}

/// One self-delimited packet at the start of `p`: the ordinary packet it stands for (the same
/// bytes without the self-delimiting length) and how many bytes it took.
pub(crate) fn split_self_delimited(p: &[u8]) -> Option<(Vec<u8>, usize)> {
    let toc = *p.first()?;
    let (sd_at, sd_used, consumed) = match toc & 3 {
        0 => {
            let (n, u) = frame_len(p.get(1..)?)?;
            (1, u, 1 + u + n)
        }
        1 => {
            let (n, u) = frame_len(p.get(1..)?)?;
            (1, u, 1 + u + 2 * n)
        }
        2 => {
            let (n1, u1) = frame_len(p.get(1..)?)?;
            let (n2, u2) = frame_len(p.get(1 + u1..)?)?;
            (1 + u1, u2, 1 + u1 + u2 + n1 + n2)
        }
        _ => {
            let m = *p.get(1)?;
            let count = usize::from(m & 0x3f);
            if count == 0 {
                return None;
            }
            let mut i = 2;
            let mut padding = 0usize;
            if m & 0x40 != 0 {
                loop {
                    let b = *p.get(i)?;
                    i += 1;
                    padding += if b == 255 { 254 } else { usize::from(b) };
                    if b != 255 {
                        break;
                    }
                }
            }
            if m & 0x80 != 0 {
                let mut sum = 0;
                for _ in 0..count - 1 {
                    let (n, u) = frame_len(p.get(i..)?)?;
                    i += u;
                    sum += n;
                }
                let (n, u) = frame_len(p.get(i..)?)?;
                (i, u, i + u + sum + n + padding)
            } else {
                let (n, u) = frame_len(p.get(i..)?)?;
                (i, u, i + u + count * n + padding)
            }
        }
    };
    if consumed > p.len() {
        return None;
    }
    let mut normal = Vec::with_capacity(consumed - sd_used);
    normal.extend_from_slice(&p[..sd_at]);
    normal.extend_from_slice(&p[sd_at + sd_used..consumed]);
    Some((normal, consumed))
}

pub(crate) struct Multistream {
    decoders: Vec<OpusDecoder>,
    coupled: usize,
    mapping: Vec<u8>,
    bufs: Vec<Vec<f32>>,
}

impl Multistream {
    pub(crate) fn new(streams: usize, coupled: usize, mapping: &[u8]) -> Result<Self, OpusError> {
        if streams == 0 || coupled > streams {
            return Err(OpusError::InvalidArgument("streams"));
        }
        let decoders = (0..streams)
            .map(|s| OpusDecoder::new(48_000, if s < coupled { 2 } else { 1 }))
            .collect::<Result<Vec<_>, _>>()?;
        let bufs = (0..streams)
            .map(|s| vec![0f32; 5760 * if s < coupled { 2 } else { 1 }])
            .collect();
        Ok(Self {
            decoders,
            coupled,
            mapping: mapping.to_vec(),
            bufs,
        })
    }

    /// Decode one multistream packet into `out`, interleaved by output channel. Returns the
    /// samples per channel.
    pub(crate) fn decode_float(
        &mut self,
        packet: &[u8],
        out: &mut [f32],
    ) -> Result<usize, OpusError> {
        let streams = self.decoders.len();
        let mut rest = packet;
        let mut n = None;
        for s in 0..streams {
            let sub = if s + 1 < streams {
                let (normal, used) = split_self_delimited(rest).ok_or(OpusError::InvalidPacket)?;
                rest = &rest[used..];
                normal
            } else {
                rest.to_vec()
            };
            let got = self.decoders[s].decode_float(&sub, &mut self.bufs[s], false)?;
            if n.is_some_and(|n| n != got) {
                return Err(OpusError::InvalidPacket);
            }
            n = Some(got);
        }
        let n = n.unwrap_or(0);
        let channels = self.mapping.len();
        if out.len() < n * channels {
            return Err(OpusError::BufferTooSmall);
        }
        for (c, &slot) in self.mapping.iter().enumerate() {
            let slot = usize::from(slot);
            let (stream, ch, width) = if slot == 255 {
                (usize::MAX, 0, 1)
            } else if slot < 2 * self.coupled {
                (slot / 2, slot % 2, 2)
            } else {
                (self.coupled + slot - 2 * self.coupled, 0, 1)
            };
            for i in 0..n {
                out[i * channels + c] = if stream < streams {
                    self.bufs[stream][i * width + ch]
                } else {
                    0.0
                };
            }
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_self_delimited_packet_loses_only_its_length() {
        // Code 0: TOC, length 3, three bytes, then the next stream's bytes.
        let p = [0x7c, 3, 0xa, 0xb, 0xc, 0xff];
        assert_eq!(
            split_self_delimited(&p),
            Some((vec![0x7c, 0xa, 0xb, 0xc], 5))
        );
        // Code 2: TOC, N1 = 1, N2 (self-delimiting) = 2, frames.
        let p = [0x7e, 1, 2, 0xa, 0xb, 0xc, 0xff];
        assert_eq!(
            split_self_delimited(&p),
            Some((vec![0x7e, 1, 0xa, 0xb, 0xc], 6))
        );
        // Code 3, CBR, two frames of 2, no padding.
        let p = [0x7f, 0x02, 2, 1, 2, 3, 4];
        assert_eq!(
            split_self_delimited(&p),
            Some((vec![0x7f, 0x02, 1, 2, 3, 4], 7))
        );
        // Truncated: refused, not read past.
        assert_eq!(split_self_delimited(&[0x7c, 9, 1]), None);
    }
}
