// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! What the decoding worker writes to the supervisor: frames of `[kind: u8][length: u32 LE]
//! [payload]`. [`PCM`] is 48 kHz stereo 32-bit float, little-endian; [`INFO`] is a line of text
//! naming what the host's software sends (`mp3 44.1 kHz stereo`).

use std::io::{self, Read, Write};

pub const PCM: u8 = 1;
pub const INFO: u8 = 2;
/// The largest frame the supervisor reads: a second of sound is 384 KB.
pub const MAX: usize = 1024 * 1024;

pub fn write(w: &mut impl Write, kind: u8, payload: &[u8]) -> io::Result<()> {
    let mut head = [0u8; 5];
    head[0] = kind;
    head[1..].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    w.write_all(&head)?;
    w.write_all(payload)
}

/// The next frame, or `None` at a clean end.
pub fn read(r: &mut impl Read) -> io::Result<Option<(u8, Vec<u8>)>> {
    let mut head = [0u8; 5];
    match r.read_exact(&mut head) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len > MAX {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a frame over 1 MB",
        ));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    Ok(Some((head[0], payload)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_a_huge_one_is_refused() {
        let mut buf = Vec::new();
        write(&mut buf, INFO, b"mp3 44.1 kHz stereo").unwrap();
        write(&mut buf, PCM, &[1, 2, 3, 4]).unwrap();
        let mut r = &buf[..];
        assert_eq!(
            read(&mut r).unwrap(),
            Some((INFO, b"mp3 44.1 kHz stereo".to_vec()))
        );
        assert_eq!(read(&mut r).unwrap(), Some((PCM, vec![1, 2, 3, 4])));
        assert_eq!(read(&mut r).unwrap(), None);
        let huge = [PCM, 0xFF, 0xFF, 0xFF, 0x7F];
        assert!(read(&mut &huge[..]).is_err());
    }
}
