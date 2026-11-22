// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A sound dropped on the cart wall (1.2.1). The window's webview takes a file drop itself, so
//! the page's own `drop` carries no file: Tauri tells the page the path, and the page asks here
//! for the bytes, to send to the station as a cart's sound (`PUT` on the slot).
//!
//! `sound_read` reads one kind of file and no other: a regular file at a whole path, whose name
//! ends as a sound's does (where a link leads, that file's name too), that begins as one of the
//! formats the station's media worker reads, and is no larger than the station takes. Anything
//! else is refused in a sentence that says nothing of what the file holds, so the command
//! cannot be made to show the page another file. The station's worker still reads the sound in
//! its own sandbox and has the last word on what it is.

use std::fs::File;
use std::io::Read;
use std::path::Path;

/// The endings a sound's name may have: what the media worker reads (`crates/media`).
const ENDINGS: [&str; 10] = [
    "wav", "wave", "flac", "mp3", "ogg", "oga", "opus", "m4a", "mp4", "aac",
];
/// The station's own limit is 1 to 1024 MB (`carts.max_upload_mb`); the page says which.
const MAX_MB: u64 = 1024;

const NOT_A_SOUND: &str =
    "Only a sound is read: WAV, FLAC, MP3, Ogg or M4A, by the ending of its name.";
const UNREADABLE: &str = "That file could not be read.";

/// Whether a path's name ends as a sound's does (in either case, as Windows writes them).
fn is_sound_name(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ENDINGS.iter().any(|k| e.eq_ignore_ascii_case(k)))
        && path.file_stem().is_some_and(|s| !s.is_empty())
}

/// Whether the bytes begin as a sound the worker reads: RIFF/WAVE, FLAC, Ogg, an MP4 box, an
/// ID3 tag or an MPEG or ADTS frame. A look at the first twelve bytes, not a parse: the worker
/// does the reading, in its sandbox.
pub fn begins_as_sound(b: &[u8]) -> bool {
    if b.len() < 12 {
        return false;
    }
    (&b[..4] == b"RIFF" && &b[8..12] == b"WAVE")
        || &b[..4] == b"fLaC"
        || &b[..4] == b"OggS"
        || &b[4..8] == b"ftyp"
        || &b[..3] == b"ID3"
        || (b[0] == 0xFF && b[1] & 0xE0 == 0xE0)
}

/// Read a sound at a whole path, of at most `max_mb` megabytes. A refusal is a sentence for the
/// page and never holds a byte of the file.
pub fn read_at(path: &Path, max_mb: u64) -> Result<Vec<u8>, String> {
    let max_mb = max_mb.clamp(1, MAX_MB);
    let max = max_mb * 1024 * 1024;
    if !path.is_absolute() {
        return Err("Give the whole path of the file.".to_owned());
    }
    if !is_sound_name(path) {
        return Err(NOT_A_SOUND.to_owned());
    }
    // Where the path leads, links followed: what is read is that file, and it too is a sound.
    let real = std::fs::canonicalize(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "No file is at that path.".to_owned(),
        std::io::ErrorKind::PermissionDenied => {
            "This app is not allowed to read that file.".to_owned()
        }
        _ => UNREADABLE.to_owned(),
    })?;
    if !is_sound_name(&real) {
        return Err(NOT_A_SOUND.to_owned());
    }
    let file = File::open(&real).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => {
            "This app is not allowed to read that file.".to_owned()
        }
        _ => UNREADABLE.to_owned(),
    })?;
    let meta = file.metadata().map_err(|_| UNREADABLE.to_owned())?;
    if meta.is_dir() {
        return Err("That is a folder, not a file.".to_owned());
    }
    if !meta.is_file() {
        return Err("That is not a file that can be read.".to_owned());
    }
    let too_large =
        || format!("That file is larger than {max_mb} MB, the most this station takes for a cart.");
    if meta.len() > max {
        return Err(too_large());
    }
    // The size was the file's a moment ago: the read is bounded all the same.
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| UNREADABLE.to_owned())?;
    if bytes.len() as u64 > max {
        return Err(too_large());
    }
    if bytes.is_empty() {
        return Err("That file is empty.".to_owned());
    }
    if !begins_as_sound(&bytes) {
        return Err(
            "That file does not begin as a sound: it is not WAV, FLAC, MP3, Ogg or M4A inside."
                .to_owned(),
        );
    }
    Ok(bytes)
}

/// Read the sound at the path of a file dropped on the cart wall. Answers its bytes as they
/// are (not as JSON); an error is a sentence for the page.
#[tauri::command]
pub fn sound_read(path: String, max_mb: u64) -> Result<tauri::ipc::Response, String> {
    read_at(Path::new(&path), max_mb).map(tauri::ipc::Response::new)
}

#[cfg(test)]
mod tests {
    use super::{begins_as_sound, read_at};

    /// The first bytes of a WAV: enough to begin as one.
    const WAV: &[u8] = b"RIFF\x24\x00\x00\x00WAVEfmt \x10\x00\x00\x00";

    #[test]
    fn a_sound_is_read_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Top of the hour.WAV");
        std::fs::write(&path, WAV).unwrap();
        assert_eq!(read_at(&path, 64).as_deref(), Ok(WAV));
        // Each format the worker reads begins in its own way.
        for (name, head) in [
            ("a.flac", &b"fLaC\x00\x00\x00\x22\x10\x00\x10\x00"[..]),
            ("a.opus", &b"OggS\x00\x02\x00\x00\x00\x00\x00\x00"[..]),
            ("a.m4a", &b"\x00\x00\x00\x20ftypM4A \x00\x00\x00\x00"[..]),
            ("a.mp3", &b"ID3\x04\x00\x00\x00\x00\x00\x00\x00\x00"[..]),
            (
                "b.mp3",
                &b"\xFF\xFB\x90\x64\x00\x00\x00\x00\x00\x00\x00\x00"[..],
            ),
        ] {
            let p = dir.path().join(name);
            std::fs::write(&p, head).unwrap();
            assert_eq!(read_at(&p, 64).as_deref(), Ok(head), "{name}");
        }
    }

    #[test]
    fn nothing_but_a_sound_is_read_and_a_refusal_holds_none_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        const SECRET: &[u8] = b"-----BEGIN OPENSSH PRIVATE KEY-----\nnot for the page\n";
        // Not a sound by its name.
        let key = dir.path().join("id_ed25519");
        std::fs::write(&key, SECRET).unwrap();
        let said = read_at(&key, 64).unwrap_err();
        assert!(said.starts_with("Only a sound is read"), "{said}");
        // A sound by its name and not inside: refused, and the sentence has none of it.
        let named = dir.path().join("key.wav");
        std::fs::write(&named, SECRET).unwrap();
        let said = read_at(&named, 64).unwrap_err();
        assert!(said.contains("does not begin as a sound"), "{said}");
        assert!(!said.contains("OPENSSH") && !said.contains("not for the page"));
        // A folder named as a sound, a path that is not whole, a file that is not there, an
        // empty one.
        let folder = dir.path().join("folder.mp3");
        std::fs::create_dir(&folder).unwrap();
        assert_eq!(
            read_at(&folder, 64).unwrap_err(),
            "That is a folder, not a file."
        );
        assert_eq!(
            read_at(std::path::Path::new("ident.wav"), 64).unwrap_err(),
            "Give the whole path of the file."
        );
        assert_eq!(
            read_at(&dir.path().join("gone.wav"), 64).unwrap_err(),
            "No file is at that path."
        );
        let empty = dir.path().join("empty.wav");
        std::fs::write(&empty, b"").unwrap();
        assert_eq!(read_at(&empty, 64).unwrap_err(), "That file is empty.");
        assert!(!begins_as_sound(b"RIFF"), "too short to say");
    }

    #[test]
    fn a_file_larger_than_the_station_takes_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.wav");
        let mut big = WAV.to_vec();
        big.resize(1024 * 1024 + 1, 0);
        std::fs::write(&path, &big).unwrap();
        assert_eq!(
            read_at(&path, 1).unwrap_err(),
            "That file is larger than 1 MB, the most this station takes for a cart."
        );
        assert_eq!(read_at(&path, 2).map(|b| b.len()), Ok(big.len()));
        // The page cannot ask for more than a station can be set to take.
        assert_eq!(read_at(&path, u64::MAX).map(|b| b.len()), Ok(big.len()));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_named_as_a_sound_does_not_read_another_file() {
        let dir = tempfile::tempdir().unwrap();
        let secret = dir.path().join("passwords.txt");
        std::fs::write(&secret, WAV).unwrap();
        let link = dir.path().join("ident.wav");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        let said = read_at(&link, 64).unwrap_err();
        assert!(said.starts_with("Only a sound is read"), "{said}");
        // A link to a sound is the sound.
        let real = dir.path().join("real.flac");
        std::fs::write(&real, b"fLaC\x00\x00\x00\x22\x10\x00\x10\x00").unwrap();
        let ok = dir.path().join("ident.flac");
        std::os::unix::fs::symlink(&real, &ok).unwrap();
        assert!(read_at(&ok, 64).is_ok());
    }
}
