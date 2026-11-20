// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! What the supervisor writes to a WHIP source's decoding worker (17d): each Opus payload with
//! its RTP timestamp, as `[timestamp: u32 LE][length: u16 LE][payload]`. RTP carries no
//! container, so this is the worker's `opus-rtp` format.
//!
//! The worker places each payload **by its timestamp, never by when it arrived** (the caller
//! return's trap, AGENTS.md): [`gap`] says whether a payload follows on, leaves samples missing
//! (concealed), came late (dropped) or jumped so far that the stream starts again from it.

use std::io::{self, Read};

/// The most a payload may be: one UDP datagram's worth.
pub const MAX_PAYLOAD: usize = 1500;
/// A jump of more than this many samples at 48 kHz (200 ms), either way, starts again rather
/// than concealing or dropping.
pub const RESYNC_SAMPLES: u32 = 9_600;

/// One payload on the wire.
pub fn write(ts: u32, payload: &[u8]) -> Vec<u8> {
    let len = payload.len().min(MAX_PAYLOAD);
    let mut out = Vec::with_capacity(6 + len);
    out.extend_from_slice(&ts.to_le_bytes());
    out.extend_from_slice(&(len as u16).to_le_bytes());
    out.extend_from_slice(&payload[..len]);
    out
}

/// The next payload with its timestamp, or `None` at a clean end.
pub fn read(r: &mut impl Read) -> io::Result<Option<(u32, Vec<u8>)>> {
    let mut head = [0u8; 6];
    match r.read_exact(&mut head) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let ts = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
    let len = usize::from(u16::from_le_bytes([head[4], head[5]]));
    if len > MAX_PAYLOAD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "an Opus payload over 1,500 bytes",
        ));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    Ok(Some((ts, payload)))
}

/// Where a payload stands against the end of the one before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gap {
    /// It starts where the last ended.
    InOrder,
    /// This many samples (48 kHz, per channel) are missing before it: conceal them.
    Missing(u32),
    /// It starts before the last ended: a late or repeated payload, dropped.
    Late,
    /// More than [`RESYNC_SAMPLES`] either way: start again from it.
    Resync,
}

/// Where a payload stamped `ts` stands when the last one ended at `prev_end`; safe across the
/// timestamp's wrap.
pub fn gap(prev_end: u32, ts: u32) -> Gap {
    let d = ts.wrapping_sub(prev_end) as i32;
    if d.unsigned_abs() > RESYNC_SAMPLES {
        Gap::Resync
    } else if d == 0 {
        Gap::InOrder
    } else if d > 0 {
        Gap::Missing(d as u32)
    } else {
        Gap::Late
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payloads_round_trip_and_a_huge_one_is_refused() {
        let mut buf = write(7, b"abc");
        buf.extend(write(u32::MAX, b""));
        let mut r = &buf[..];
        assert_eq!(read(&mut r).unwrap(), Some((7, b"abc".to_vec())));
        assert_eq!(read(&mut r).unwrap(), Some((u32::MAX, Vec::new())));
        assert_eq!(read(&mut r).unwrap(), None);
        let huge = [0, 0, 0, 0, 0xFF, 0xFF];
        assert!(read(&mut &huge[..]).is_err());
        assert_eq!(write(1, &[0u8; 4000]).len(), 6 + MAX_PAYLOAD);
    }

    #[test]
    fn in_order_and_a_lost_packet() {
        assert_eq!(gap(960, 960), Gap::InOrder);
        assert_eq!(gap(960, 1920), Gap::Missing(960));
    }

    #[test]
    fn a_late_one_is_dropped() {
        assert_eq!(gap(1920, 960), Gap::Late);
        assert_eq!(gap(1920, 1919), Gap::Late);
    }

    #[test]
    fn a_jump_either_way_starts_again() {
        assert_eq!(gap(960, 960 + RESYNC_SAMPLES), Gap::Missing(RESYNC_SAMPLES));
        assert_eq!(gap(960, 960 + RESYNC_SAMPLES + 1), Gap::Resync);
        assert_eq!(gap(100_000, 50_000), Gap::Resync);
    }

    #[test]
    fn the_wrap_at_u32_max() {
        assert_eq!(gap(u32::MAX - 479, 480), Gap::Missing(960));
        assert_eq!(gap(u32::MAX - 479, u32::MAX - 479), Gap::InOrder);
        assert_eq!(gap(480, u32::MAX - 479), Gap::Late);
        assert_eq!(gap(0, u32::MAX), Gap::Late);
        assert_eq!(gap(u32::MAX, 0), Gap::Missing(1));
    }
}
