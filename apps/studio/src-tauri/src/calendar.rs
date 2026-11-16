// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Download .ics (Subscribe…, ADR-0281): the calendar file the page read from the station,
//! written into the person's Downloads folder. The window's webview has no download of its own
//! that the page can count on (WebKitGTK saves a link's download without a word, and a webview
//! elsewhere may do nothing), so the page hands the file here and is told where it went.
//!
//! The page can ask for nothing but this: a name that is a show's path (or `schedule`) and
//! `.ics`, with no folder in it; text that is a calendar, of a calendar's size; a new file in
//! the Downloads folder, never over one that is there.
//!
//! And the other way (Import a calendar…, the 1.0.5 test run): a path the person pasted, or
//! the path of a file they dropped on the window, read here because a page cannot read a
//! path. `calendar_read` reads one kind of file and no other: a regular file whose name ends
//! `.ics`, of at most 1 MB, that is text in UTF-8 and begins as a calendar. Anything else is
//! refused in a sentence that says nothing of what the file holds, so the command cannot be
//! made to show the page another file.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use tauri::{AppHandle, Manager};

/// A calendar file is at most this long: the station's own are a few kilobytes a show.
const MAX_BYTES: usize = 8 * 1024 * 1024;
/// `schedule.ics`, then `schedule (2).ics` … up to this many of one name.
const MAX_COPIES: u32 = 99;
/// A calendar file read for an import is at most this long, as the station takes (1 MB).
const MAX_READ_BYTES: u64 = 1024 * 1024;

/// The file's name as it may be written: a show's path (lower-case letters, digits and
/// hyphens, as the station makes them) and `.ics`. Nothing that could name a folder.
pub fn file_name(name: &str) -> Result<&str, String> {
    let stem = name
        .strip_suffix(".ics")
        .ok_or("a calendar file's name ends in .ics")?;
    let ok = !stem.is_empty()
        && stem.len() <= 100
        && stem
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !stem.starts_with('-');
    if ok {
        Ok(name)
    } else {
        Err("a calendar file is named for the schedule or a show's path".to_owned())
    }
}

/// Whether the text is a calendar file: it opens `BEGIN:VCALENDAR` (after a byte-order mark,
/// if one is there), as RFC 5545 has it.
pub fn is_calendar(text: &str) -> bool {
    let t = text.trim_start_matches('\u{feff}').trim_start();
    t.as_bytes()
        .get(..15)
        .is_some_and(|b| b.eq_ignore_ascii_case(b"BEGIN:VCALENDAR"))
}

/// Write the file into `dir` under `name`, or under `name (2)`, `name (3)` … when that is
/// taken: a file that is there is never written over. Answers where it went.
pub fn save_in(dir: &Path, name: &str, text: &str) -> Result<PathBuf, String> {
    let name = file_name(name)?;
    if text.len() > MAX_BYTES {
        return Err("that is too large to be a calendar file".to_owned());
    }
    if !is_calendar(text) {
        return Err("that is not a calendar file".to_owned());
    }
    let stem = name.trim_end_matches(".ics");
    for n in 1..=MAX_COPIES {
        let path = dir.join(if n == 1 {
            name.to_owned()
        } else {
            format!("{stem} ({n}).ics")
        });
        // `create_new` fails on a file (or a link) that is there, so nothing is followed or
        // written over.
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                return f
                    .write_all(text.as_bytes())
                    .and_then(|()| f.sync_all())
                    .map(|()| path.clone())
                    .map_err(|e| {
                        let _ = std::fs::remove_file(&path);
                        format!("the file could not be written: {e}")
                    });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(format!(
                    "the file could not be written in {}: {e}",
                    dir.display()
                ));
            }
        }
    }
    Err(format!(
        "there are already {MAX_COPIES} files named {name} in {}",
        dir.display()
    ))
}

/// Save a calendar file the page read from the station into the Downloads folder (the home
/// folder where the system names none). Answers the file's path, for the page to say; an
/// error is a sentence for the page.
#[tauri::command]
pub fn calendar_save(app: AppHandle, name: String, text: String) -> Result<String, String> {
    let dir = app
        .path()
        .download_dir()
        .or_else(|_| app.path().home_dir())
        .map_err(|e| format!("no Downloads folder was found: {e}"))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("{} could not be made: {e}", dir.display()))?;
    save_in(&dir, &name, &text).map(|p| p.display().to_string())
}

/// A calendar file read from this computer: its name, for the page to say, and its text.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct CalendarFile {
    pub name: String,
    pub text: String,
}

/// One `%XX` of a `file://` address read back; anything else stays as it is.
fn percent_decoded(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = (b[i] == b'%')
            .then(|| s.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

/// The path a person pasted, as a path: the spaces around it gone, one pair of quotes around
/// it gone (a file manager's *Copy path* gives `'/home/iona/schedule (2).ics'` or `"C:\…"`),
/// Subscribe…'s own *Saved as* before it gone, a `file://` address read as its path, and `~`
/// as the home folder. It must then be a whole path: nothing is read relative to wherever the
/// app happens to run.
pub fn pasted_path(raw: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    let mut p = raw.trim();
    if let Some(rest) = p.strip_prefix("Saved as ") {
        p = rest.trim();
    }
    for q in ['"', '\''] {
        if p.len() >= 2 && p.starts_with(q) && p.ends_with(q) {
            p = p[1..p.len() - 1].trim();
            break;
        }
    }
    if p.is_empty() {
        return Err("Paste the path of a calendar file (.ics).".to_owned());
    }
    if p.contains('\0') {
        return Err("That is not a path.".to_owned());
    }
    let decoded;
    if let Some(rest) = p
        .strip_prefix("file://localhost/")
        .or_else(|| p.strip_prefix("file:///"))
    {
        decoded = percent_decoded(rest);
        // `file:///C:/Users/…` on Windows is `C:/Users/…`; elsewhere the path begins at `/`.
        let windows = decoded.as_bytes().get(1) == Some(&b':');
        let whole = if windows {
            decoded.clone()
        } else {
            format!("/{decoded}")
        };
        return whole_path(PathBuf::from(whole));
    }
    let path = if p == "~" {
        home.map(Path::to_path_buf)
            .ok_or("This computer names no home folder: give the whole path.")?
    } else if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        home.ok_or("This computer names no home folder: give the whole path.")?
            .join(rest)
    } else {
        PathBuf::from(p)
    };
    whole_path(path)
}

fn whole_path(path: PathBuf) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Err("Give the whole path of the file, from the top folder or from ~.".to_owned())
    }
}

/// Whether a path's name ends `.ics` (in either case, as Windows writes them).
fn is_ics(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("ics"))
        && path.file_stem().is_some_and(|s| !s.is_empty())
}

/// Read a calendar file at a whole path. Only a regular file whose name ends `.ics` (where a
/// link leads, that file's name too), of at most 1 MB, that is text in UTF-8 and begins as a
/// calendar; a refusal is a sentence for the page and never holds a byte of the file.
pub fn read_at(path: &Path) -> Result<CalendarFile, String> {
    const NOT_ICS: &str = "Only a calendar file is read: its name must end in .ics.";
    if !is_ics(path) {
        return Err(NOT_ICS.to_owned());
    }
    // Where the path leads, links followed: what is read is that file, and it too is an .ics.
    let real = std::fs::canonicalize(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "No file is at that path.".to_owned(),
        std::io::ErrorKind::PermissionDenied => {
            "This app is not allowed to read that file.".to_owned()
        }
        _ => "That file could not be read.".to_owned(),
    })?;
    if !is_ics(&real) {
        return Err(NOT_ICS.to_owned());
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file = File::open(&real).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => {
            "This app is not allowed to read that file.".to_owned()
        }
        _ => "That file could not be read.".to_owned(),
    })?;
    let meta = file
        .metadata()
        .map_err(|_| "That file could not be read.".to_owned())?;
    if meta.is_dir() {
        return Err("That is a folder, not a file.".to_owned());
    }
    if !meta.is_file() {
        return Err("That is not a file that can be read.".to_owned());
    }
    const TOO_LARGE: &str = "That file is larger than 1 MB. Export a shorter range of dates, or one calendar at a time.";
    if meta.len() > MAX_READ_BYTES {
        return Err(TOO_LARGE.to_owned());
    }
    // The size was the file's a moment ago: the read is bounded all the same.
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(MAX_READ_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "That file could not be read.".to_owned())?;
    if bytes.len() as u64 > MAX_READ_BYTES {
        return Err(TOO_LARGE.to_owned());
    }
    if bytes.is_empty() {
        return Err("That file is empty.".to_owned());
    }
    let text = String::from_utf8(bytes).map_err(|_| {
        "That file is not text in UTF-8, so it is not a calendar file we can read.".to_owned()
    })?;
    if !is_calendar(&text) {
        return Err(
            "That is not a calendar file: it does not begin with BEGIN:VCALENDAR.".to_owned(),
        );
    }
    Ok(CalendarFile { name, text })
}

/// Read the calendar file at a path the person pasted into Import a calendar…, or dropped on
/// the window. Answers its name and its text; an error is a sentence for the page.
#[tauri::command]
pub fn calendar_read(app: AppHandle, path: String) -> Result<CalendarFile, String> {
    let home = app.path().home_dir().ok();
    read_at(&pasted_path(&path, home.as_deref())?)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{CalendarFile, file_name, is_calendar, pasted_path, read_at, save_in};

    const ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n";

    #[test]
    fn a_name_is_the_schedule_or_a_shows_path_and_never_a_folder() {
        for ok in [
            "schedule.ics",
            "the-late-kettle.ics",
            "folk-night-2.ics",
            "6.ics",
        ] {
            assert_eq!(file_name(ok), Ok(ok));
        }
        for bad in [
            "",
            ".ics",
            "schedule",
            "schedule.ics.exe",
            "../schedule.ics",
            "a/b.ics",
            "a\\b.ics",
            "/etc/cron.d/x.ics",
            "..ics",
            "-rf.ics",
            "Schedule.ics",
            "a b.ics",
            "a.b.ics",
            "caf\u{e9}.ics",
            "schedule.ICS",
        ] {
            assert!(file_name(bad).is_err(), "{bad}");
        }
        assert!(file_name(&format!("{}.ics", "a".repeat(101))).is_err());
    }

    #[test]
    fn only_a_calendar_is_written() {
        assert!(is_calendar(ICS));
        assert!(is_calendar("\u{feff}begin:vcalendar\r\n"));
        assert!(!is_calendar(""));
        assert!(!is_calendar("<!doctype html><title>Sign in</title>"));
        assert!(!is_calendar("#!/bin/sh\nBEGIN:VCALENDAR"));
        assert!(!is_calendar(
            "C\u{e8}ilidh na h-oidhche \u{2014} BEGIN:VCALENDAR"
        ));
        let dir = tempfile::tempdir().unwrap();
        assert!(save_in(dir.path(), "schedule.ics", "#!/bin/sh\n").is_err());
        assert!(save_in(dir.path(), "../schedule.ics", ICS).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_file_that_is_there_is_never_written_over() {
        let dir = tempfile::tempdir().unwrap();
        let first = save_in(dir.path(), "schedule.ics", ICS).unwrap();
        assert_eq!(first, dir.path().join("schedule.ics"));
        assert_eq!(std::fs::read_to_string(&first).unwrap(), ICS);
        let other = format!("{ICS}X-NOTE:second\r\n");
        let second = save_in(dir.path(), "schedule.ics", &other).unwrap();
        assert_eq!(second, dir.path().join("schedule (2).ics"));
        assert_eq!(std::fs::read_to_string(&first).unwrap(), ICS, "untouched");
        assert_eq!(std::fs::read_to_string(&second).unwrap(), other);
        assert_eq!(
            save_in(dir.path(), "schedule.ics", ICS).unwrap(),
            dir.path().join("schedule (3).ics")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_under_the_name_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere.txt");
        std::fs::write(&target, "kept").unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("schedule.ics")).unwrap();
        let saved = save_in(dir.path(), "schedule.ics", ICS).unwrap();
        assert_eq!(saved, dir.path().join("schedule (2).ics"));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "kept");
    }

    #[cfg(unix)]
    #[test]
    fn a_pasted_path_loses_its_quotes_and_takes_the_home_folder_for_a_tilde() {
        let home = Path::new("/home/iona");
        let want = PathBuf::from("/home/iona/Downloads/schedule (2).ics");
        for raw in [
            "/home/iona/Downloads/schedule (2).ics",
            "  /home/iona/Downloads/schedule (2).ics \n",
            "'/home/iona/Downloads/schedule (2).ics'",
            "\"/home/iona/Downloads/schedule (2).ics\"",
            "~/Downloads/schedule (2).ics",
            "'~/Downloads/schedule (2).ics'",
            // What Subscribe… says after Download .ics, copied whole.
            "Saved as /home/iona/Downloads/schedule (2).ics",
            "file:///home/iona/Downloads/schedule%20(2).ics",
            "file://localhost/home/iona/Downloads/schedule%20%282%29.ics",
        ] {
            assert_eq!(pasted_path(raw, Some(home)), Ok(want.clone()), "{raw}");
        }
        assert_eq!(pasted_path("~", Some(home)), Ok(home.to_path_buf()));
        // A name with a quote in it keeps it; only a pair around the whole goes.
        assert_eq!(
            pasted_path("/home/iona/Iona's shows.ics", Some(home)),
            Ok(PathBuf::from("/home/iona/Iona's shows.ics"))
        );
        // Nothing is read relative to where the app runs, and `~iona` is not a home folder.
        for bad in [
            "",
            "   ",
            "''",
            "schedule.ics",
            "./schedule.ics",
            "../schedule.ics",
            "Downloads/schedule.ics",
            "~iona/schedule.ics",
            "/home/iona/a\0b.ics",
        ] {
            assert!(pasted_path(bad, Some(home)).is_err(), "{bad:?}");
        }
        assert!(pasted_path("~/schedule.ics", None).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn a_pasted_windows_path_loses_its_quotes() {
        let home = Path::new(r"C:\Users\Iona");
        let want = PathBuf::from(r"C:\Users\Iona\Downloads\schedule (2).ics");
        for raw in [
            r#""C:\Users\Iona\Downloads\schedule (2).ics""#,
            r"C:\Users\Iona\Downloads\schedule (2).ics",
            r"~\Downloads\schedule (2).ics",
        ] {
            assert_eq!(pasted_path(raw, Some(home)), Ok(want.clone()), "{raw}");
        }
        assert_eq!(
            pasted_path(
                "file:///C:/Users/Iona/Downloads/schedule%20(2).ics",
                Some(home)
            ),
            Ok(PathBuf::from("C:/Users/Iona/Downloads/schedule (2).ics"))
        );
    }

    #[test]
    fn a_calendar_file_is_read_with_its_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("schedule (2).ics");
        std::fs::write(&path, ICS).unwrap();
        assert_eq!(
            read_at(&path),
            Ok(CalendarFile {
                name: "schedule (2).ics".into(),
                text: ICS.into()
            })
        );
        // The ending in capitals, as Windows may write it; a byte-order mark; exactly 1 MB.
        let caps = dir.path().join("OLD.ICS");
        let big = format!("\u{feff}{ICS}");
        let big = format!("{big}{}", "X".repeat(1024 * 1024 - big.len()));
        assert_eq!(big.len(), 1024 * 1024);
        std::fs::write(&caps, &big).unwrap();
        assert_eq!(read_at(&caps).map(|f| f.text.len()), Ok(1024 * 1024));
        // What the page pasted goes through the same door.
        let quoted = format!("'{}'", path.display());
        assert_eq!(
            pasted_path(&quoted, None)
                .and_then(|p| read_at(&p))
                .map(|f| f.name),
            Ok("schedule (2).ics".into())
        );
    }

    #[test]
    fn nothing_but_a_calendar_file_is_read_and_a_refusal_holds_none_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let at = |name: &str, body: &[u8]| {
            let p = dir.path().join(name);
            std::fs::write(&p, body).unwrap();
            p
        };
        const SECRET: &str = "hunter2-the-station-key";
        let refusal = |p: &Path| {
            let e = read_at(p).expect_err("refused");
            assert!(!e.contains(SECRET), "a refusal holds none of the file: {e}");
            assert!(e.ends_with('.'), "a sentence: {e}");
            e
        };

        // A wrong ending, whatever the file holds: a calendar under another name is not read.
        for name in [
            "notes.txt",
            "schedule.ics.txt",
            "schedule",
            "key.pem",
            ".ics",
            "ics",
        ] {
            let p = at(name, ICS.as_bytes());
            assert_eq!(
                refusal(&p),
                "Only a calendar file is read: its name must end in .ics.",
                "{name}"
            );
        }
        // Over 1 MB, by one byte.
        let large = format!("{ICS}{}", "X".repeat(1024 * 1024 + 1 - ICS.len()));
        assert_eq!(large.len(), 1024 * 1024 + 1);
        assert!(
            refusal(&at("large.ics", large.as_bytes()))
                .starts_with("That file is larger than 1 MB.")
        );
        // Not a calendar: another file named as one says nothing of what it holds.
        let key = format!("station_key = \"{SECRET}\"\n");
        assert_eq!(
            refusal(&at("settings.ics", key.as_bytes())),
            "That is not a calendar file: it does not begin with BEGIN:VCALENDAR."
        );
        assert_eq!(
            refusal(&at(
                "late.ics",
                format!("{SECRET}\nBEGIN:VCALENDAR\n").as_bytes()
            )),
            "That is not a calendar file: it does not begin with BEGIN:VCALENDAR."
        );
        // Not text, and empty.
        assert!(refusal(&at("binary.ics", b"BEGIN:VCALENDAR\r\n\xff\xfe\x00")).contains("UTF-8"));
        assert_eq!(refusal(&at("empty.ics", b"")), "That file is empty.");
        // Missing, and a folder named as a file.
        assert_eq!(
            refusal(&dir.path().join("gone.ics")),
            "No file is at that path."
        );
        std::fs::create_dir(dir.path().join("folder.ics")).unwrap();
        assert_eq!(
            refusal(&dir.path().join("folder.ics")),
            "That is a folder, not a file."
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_named_as_a_calendar_does_not_read_another_file() {
        let dir = tempfile::tempdir().unwrap();
        // A calendar's words in a file that is not one by name: the link's own name is not enough.
        let other = dir.path().join("passwd");
        std::fs::write(&other, ICS).unwrap();
        let link = dir.path().join("looks-like.ics");
        std::os::unix::fs::symlink(&other, &link).unwrap();
        assert_eq!(
            read_at(&link),
            Err("Only a calendar file is read: its name must end in .ics.".to_owned())
        );
        // A link to a real calendar file is read, under the name that was asked for.
        let real = dir.path().join("schedule.ics");
        std::fs::write(&real, ICS).unwrap();
        let good = dir.path().join("mine.ics");
        std::os::unix::fs::symlink(&real, &good).unwrap();
        assert_eq!(read_at(&good).map(|f| f.name), Ok("mine.ics".into()));
        // A link that leads nowhere.
        let dangling = dir.path().join("dangling.ics");
        std::os::unix::fs::symlink(dir.path().join("nowhere.ics"), &dangling).unwrap();
        assert_eq!(
            read_at(&dangling),
            Err("No file is at that path.".to_owned())
        );
    }
}
