// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Save as… (keyed shows, 1.3.2): a list of keys as CSV, or a key's QR code as a picture,
//! written where the person says. The window's webview has no download the page can count on
//! (WebKitGTK saves a link's download without a word, and a webview elsewhere may do nothing),
//! and a file of keys is not something to drop in a folder unasked: the page hands the file
//! here, the system's own save dialog asks where it goes, and the page is told where it went.
//!
//! The page gives a name to start the dialog with and the file's text, and **no path**: the only
//! path this command ever writes to is the one the dialog answers with, so the page cannot be
//! made to write anywhere a person did not choose. What it may hand over is narrow too: a name
//! that is a plain file name ending `.csv` or `.svg` (lower-case letters, digits and hyphens, as
//! the page makes them), and text that is that kind of file, of that kind of size. Cancel is an
//! answer, not an error: nothing is written and the page is told so.
//!
//! The dialog is tauri-plugin-dialog's, called from Rust. The window is granted none of the
//! plugin's own commands (capabilities/default.json), so the page cannot open a dialog of its
//! own choosing, or read what one would answer.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

/// A list of keys is a few kilobytes a hundred keys, and a QR picture a few kilobytes: nothing
/// the page saves comes near this.
const MAX_BYTES: usize = 4 * 1024 * 1024;
/// A file's name without its ending is at most this long.
const MAX_STEM: usize = 100;

/// What the page may save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Comma-separated values: a family's keys, a made list, the leak report.
    Csv,
    /// A picture as SVG: a key's QR code.
    Svg,
}

impl Kind {
    /// The ending, without its dot.
    pub fn ending(self) -> &'static str {
        match self {
            Kind::Csv => "csv",
            Kind::Svg => "svg",
        }
    }

    /// The dialog's name for files of this kind.
    fn label(self) -> &'static str {
        match self {
            Kind::Csv => "Comma-separated values",
            Kind::Svg => "SVG picture",
        }
    }

    /// The dialog's title.
    fn title(self) -> &'static str {
        match self {
            Kind::Csv => "Save the list as CSV",
            Kind::Svg => "Save the QR code as a picture",
        }
    }
}

/// The name the dialog starts with, and the kind of file it names: lower-case letters, digits
/// and hyphens, then `.csv` or `.svg`. Nothing that could name a folder, a drive or a hidden
/// file, so the page's name can never be a path.
pub fn file_name(name: &str) -> Result<Kind, String> {
    let (stem, kind) = if let Some(stem) = name.strip_suffix(".csv") {
        (stem, Kind::Csv)
    } else if let Some(stem) = name.strip_suffix(".svg") {
        (stem, Kind::Svg)
    } else {
        return Err("only a .csv or an .svg file is saved from here".to_owned());
    };
    let ok = !stem.is_empty()
        && stem.len() <= MAX_STEM
        && !stem.starts_with('-')
        && stem
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(kind)
    } else {
        Err("a saved file is named in lower-case letters, digits and hyphens".to_owned())
    }
}

/// Whether the text is a file of its kind: not empty, of a file's size, with no NUL in it; and
/// an SVG opens as one (after a byte-order mark or an XML declaration, if either is there).
pub fn checked(kind: Kind, text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err("there is nothing to save".to_owned());
    }
    if text.len() > MAX_BYTES {
        return Err("that is too large to be saved from here".to_owned());
    }
    if text.contains('\0') {
        return Err("that is not a text file".to_owned());
    }
    if kind == Kind::Svg {
        let mut t = text.trim_start_matches('\u{feff}').trim_start();
        if t.starts_with("<?xml") {
            t = t.split_once("?>").map_or("", |(_, rest)| rest).trim_start();
        }
        if !t.starts_with("<svg") {
            return Err("that is not an SVG picture".to_owned());
        }
    }
    Ok(())
}

/// Write the text at the path the dialog answered with. The dialog has already asked about a
/// file that is there, so this writes over it; the path is used exactly as it was given, with
/// nothing added and nothing resolved by this code.
pub fn write_to(path: &Path, text: &str) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("the save dialog gave no whole path".to_owned());
    }
    let mut f =
        File::create(path).map_err(|e| format!("the file could not be written there: {e}"))?;
    f.write_all(text.as_bytes())
        .and_then(|()| f.sync_all())
        .map_err(|e| format!("the file could not be written there: {e}"))
}

/// Save a file the page made, where the person says in the system's save dialog. `name` only
/// starts the dialog (a plain `.csv` or `.svg` name); `text` is the file. Answers the path it
/// was saved at, for the page to say, or nothing when the person closed the dialog without
/// saving; an error is a sentence for the page.
#[tauri::command]
pub async fn save_file(
    app: AppHandle,
    window: WebviewWindow,
    name: String,
    text: String,
) -> Result<Option<String>, String> {
    let kind = file_name(&name)?;
    checked(kind, &text)?;
    let mut dialog = app
        .dialog()
        .file()
        .set_parent(&window)
        .set_title(kind.title())
        .set_file_name(&name)
        .add_filter(kind.label(), &[kind.ending()]);
    if let Ok(dir) = app.path().download_dir() {
        dialog = dialog.set_directory(dir);
    }
    // The dialog runs on the toolkit's own thread and answers through the closure: this command
    // is async so that waiting for a person never holds the window's event loop.
    let (tx, rx) = tokio::sync::oneshot::channel();
    dialog.save_file(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(picked) = rx
        .await
        .map_err(|_| "the save dialog closed without an answer".to_owned())?
    else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|_| "the save dialog gave a place this app cannot write to".to_owned())?;
    write_to(&path, &text)?;
    Ok(Some(path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::{Kind, MAX_BYTES, checked, file_name, write_to};

    const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 29 29"><path d="M0 0h1v1H0z"/></svg>"#;

    #[test]
    fn a_name_is_a_plain_csv_or_svg_name_and_never_a_path() {
        for (ok, kind) in [
            ("keys-patrons.csv", Kind::Csv),
            ("leak-report.csv", Kind::Csv),
            ("keys.csv", Kind::Csv),
            ("key-isla-fraser.svg", Kind::Svg),
            ("key-qr.svg", Kind::Svg),
            ("6.csv", Kind::Csv),
        ] {
            assert_eq!(file_name(ok), Ok(kind), "{ok}");
        }
        // The page gives no path: a name that reads as one, in any way, is refused before the
        // dialog opens, so the only path ever written is the one the dialog answers with.
        for bad in [
            "",
            ".csv",
            ".svg",
            "keys",
            "keys.txt",
            "keys.csv.exe",
            "keys.svg.sh",
            "../keys.csv",
            "a/b.csv",
            "a\\b.csv",
            "/etc/cron.d/x.csv",
            "C:\\Users\\x.csv",
            "c:x.csv",
            "~/keys.csv",
            "..csv",
            ".hidden.csv",
            "-rf.csv",
            "Keys.csv",
            "a b.csv",
            "a.b.csv",
            "caf\u{e9}.csv",
            "keys.CSV",
            "keys.csv\0.svg",
            "keys\n.csv",
        ] {
            assert!(file_name(bad).is_err(), "{bad:?}");
        }
        assert!(file_name(&format!("{}.csv", "a".repeat(100))).is_ok());
        assert!(file_name(&format!("{}.csv", "a".repeat(101))).is_err());
    }

    #[test]
    fn the_text_is_a_file_of_its_kind_and_size() {
        assert_eq!(
            checked(Kind::Csv, "name,key\r\nIsla Fraser,ABCD-EFGH-JKMN\r\n"),
            Ok(())
        );
        assert_eq!(checked(Kind::Svg, SVG), Ok(()));
        assert_eq!(
            checked(
                Kind::Svg,
                &format!("\u{feff}<?xml version=\"1.0\"?>\n{SVG}")
            ),
            Ok(())
        );
        assert!(checked(Kind::Csv, "").is_err());
        assert!(checked(Kind::Csv, "a,b\0c").is_err());
        assert!(checked(Kind::Svg, "<html><svg></svg></html>").is_err());
        assert!(checked(Kind::Svg, "<?xml version=\"1.0\"").is_err());
        assert!(checked(Kind::Svg, "name,key").is_err());
        assert!(checked(Kind::Csv, &"a".repeat(MAX_BYTES)).is_ok());
        assert!(checked(Kind::Csv, &"a".repeat(MAX_BYTES + 1)).is_err());
    }

    #[test]
    fn the_file_is_written_at_the_dialogs_path_and_nowhere_else() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys-patrons.csv");
        assert_eq!(write_to(&path, "name,key\r\n"), Ok(()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "name,key\r\n");
        // The dialog asked about a file that is there: saving again writes over it, whole.
        assert_eq!(write_to(&path, "a\r\n"), Ok(()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\r\n");
        // The name a person typed is kept as they typed it: nothing is added to it.
        let bare = dir.path().join("my keys");
        assert_eq!(write_to(&bare, "a\r\n"), Ok(()));
        assert!(bare.is_file());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn a_path_that_is_not_whole_or_cannot_be_written_is_a_sentence() {
        // Nothing is written relative to wherever the app happens to run.
        assert!(write_to(std::path::Path::new("keys.csv"), "a").is_err());
        assert!(!std::path::Path::new("keys.csv").exists());
        let dir = tempfile::tempdir().unwrap();
        let err = write_to(&dir.path().join("no-such-folder").join("keys.csv"), "a").unwrap_err();
        assert!(
            err.starts_with("the file could not be written there"),
            "{err}"
        );
        // A folder is not a file: said, and the folder is as it was.
        assert!(write_to(dir.path(), "a").is_err());
        assert!(dir.path().is_dir());
    }
}
