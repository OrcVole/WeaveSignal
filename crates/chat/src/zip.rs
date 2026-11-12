// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A ZIP archive with every file stored, not compressed (ADR-0257's evidence export). A picture is
//! already compressed, the text files are small, and a writer this short has no dependency to
//! trust with evidence. Names are UTF-8, there are no data descriptors, no ZIP64 (an export is
//! far below 4 GiB and 65,535 files), and the time is the DOS epoch's start, so the same inputs
//! give the same bytes.

/// DOS date 1980-01-01 and time 00:00:00.
const DOS_DATE: u16 = (1 << 5) | 1;
const DOS_TIME: u16 = 0;

/// The archive of `files` (name and bytes), in order.
pub fn stored(files: &[(&str, &[u8])]) -> Vec<u8> {
    assert!(
        files.len() < usize::from(u16::MAX),
        "too many files for a ZIP"
    );
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let offset = u32::try_from(out.len()).expect("an archive under 4 GiB");
        let size = u32::try_from(data.len()).expect("a file under 4 GiB");
        let crc = crc32fast::hash(data);
        let name = name.as_bytes();
        let name_len = u16::try_from(name.len()).expect("a short name");
        // Local file header.
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0x0800u16.to_le_bytes()); // flags: UTF-8 names
        out.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        out.extend_from_slice(&DOS_TIME.to_le_bytes());
        out.extend_from_slice(&DOS_DATE.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra
        out.extend_from_slice(name);
        out.extend_from_slice(data);
        // Central directory entry.
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // made by
        central.extend_from_slice(&20u16.to_le_bytes()); // needed
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&DOS_TIME.to_le_bytes());
        central.extend_from_slice(&DOS_DATE.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let start = u32::try_from(out.len()).expect("an archive under 4 GiB");
    let count = u16::try_from(files.len()).expect("few files");
    let central_len = u32::try_from(central.len()).expect("a short directory");
    out.extend_from_slice(&central);
    // End of central directory.
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&central_len.to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// Read an archive made by [`stored`] back: each name and its bytes, the checksums checked.
/// For tests; it reads only what this writer writes.
pub fn read_stored(zip: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let u16_at = |i: usize| -> Result<u16, String> {
        zip.get(i..i + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .ok_or_else(|| "cut short".to_owned())
    };
    let u32_at = |i: usize| -> Result<u32, String> {
        zip.get(i..i + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| "cut short".to_owned())
    };
    if zip.len() < 22 || u32_at(zip.len() - 22)? != 0x0605_4b50 {
        return Err("no end of central directory".into());
    }
    let end = zip.len() - 22;
    let count = usize::from(u16_at(end + 10)?);
    let mut at = u32_at(end + 16)? as usize;
    let mut out = Vec::new();
    for _ in 0..count {
        if u32_at(at)? != 0x0201_4b50 {
            return Err("bad central directory entry".into());
        }
        let crc = u32_at(at + 16)?;
        let size = u32_at(at + 24)? as usize;
        let name_len = usize::from(u16_at(at + 28)?);
        let offset = u32_at(at + 42)? as usize;
        let name = zip
            .get(at + 46..at + 46 + name_len)
            .ok_or("cut short")?
            .to_vec();
        if u32_at(offset)? != 0x0403_4b50 {
            return Err("bad local header".into());
        }
        let local_name = usize::from(u16_at(offset + 26)?);
        let local_extra = usize::from(u16_at(offset + 28)?);
        let from = offset + 30 + local_name + local_extra;
        let data = zip.get(from..from + size).ok_or("cut short")?.to_vec();
        if crc32fast::hash(&data) != crc {
            return Err("checksum mismatch".into());
        }
        out.push((String::from_utf8(name).map_err(|e| e.to_string())?, data));
        at += 46 + name_len;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_archive_reads_back_with_its_names_bytes_and_checksums() {
        let big: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
        let zip = stored(&[
            ("README.txt", b"hello"),
            ("picture/original.bin", &big),
            ("métadonnées.json", b"{}"),
            ("empty", b""),
        ]);
        assert!(zip.starts_with(b"PK\x03\x04"));
        let back = read_stored(&zip).unwrap();
        assert_eq!(back.len(), 4);
        assert_eq!(back[0], ("README.txt".to_owned(), b"hello".to_vec()));
        assert_eq!(back[1].1, big);
        assert_eq!(back[2].0, "métadonnées.json");
        assert_eq!(back[3].1, Vec::<u8>::new());
        let mut bad = zip.clone();
        bad[40] ^= 0xff;
        assert!(read_stored(&bad).is_err(), "a changed byte is caught");
    }

    #[test]
    fn the_same_files_give_the_same_bytes() {
        assert_eq!(stored(&[("a", b"1")]), stored(&[("a", b"1")]));
    }
}
