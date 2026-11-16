// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The stations this computer keeps (ADR-0285): a list in a small file of the app's own, one
//! row for one address, and for each station its key where a single station's has always been
//! (ADR-0121): the system's keychain under the station's address, or, where there is no
//! keyring, a file of its own that only this user can read.
//!
//! The list holds no key, token or password. It names which station the window is on; every
//! request to a station names that station too (`station.rs`, `station_fetch`), so one left
//! over from before a switch is refused rather than sent on with another station's key.
//!
//! Nothing here needs Tauri: the list works on a folder and a key store, so it is tested with
//! a scratch folder and a key store in memory (the tests below).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The keychain entry's service; the account is the station's address, so each station has an
/// entry of its own and an install from before the list needs nothing moved.
pub const SERVICE: &str = "weavesignal-studio";

/// The longest label a station is given on this computer.
pub const LABEL_MAX: usize = 60;

/// Where a key is kept, as the setup shows it.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kept {
    Keychain,
    File,
}

/// One station as this computer keeps it. `name`, `who` and `version` are what the window last
/// saw there, for the list; they are never asked of a station the window is not on.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Station {
    /// This computer's own name for the row: `s1`, `s2`, never used again once removed.
    pub id: String,
    pub url: String,
    /// The name the person gave it here; the station is not told.
    #[serde(default)]
    pub label: Option<String>,
    /// The station's own name, as it last said it.
    #[serde(default)]
    pub name: Option<String>,
    /// Who was signed in there, as the frame says it (a name and a role, or the station key).
    #[serde(default)]
    pub who: Option<String>,
    /// The station's version, as last read.
    #[serde(default)]
    pub version: Option<String>,
    /// Where its key is; none while signed out.
    #[serde(default)]
    pub kept: Option<Kept>,
}

/// The list as it is on disk.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Book {
    /// The number the next station's id takes.
    #[serde(default)]
    next: u32,
    /// The station the window is on.
    #[serde(default)]
    pub current: Option<String>,
    #[serde(default)]
    pub stations: Vec<Station>,
}

impl Book {
    pub fn get(&self, id: &str) -> Option<&Station> {
        self.stations.iter().find(|s| s.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Option<&mut Station> {
        self.stations.iter_mut().find(|s| s.id == id)
    }

    /// The station the window is on, signed in or not.
    pub fn here(&self) -> Option<&Station> {
        self.current.as_deref().and_then(|id| self.get(id))
    }
}

/// Somewhere keys are kept by account. The keychain in the app; a map in the tests.
pub trait KeyStore {
    /// Keep a key and read it back; false when it was not kept (no keyring, or one that
    /// accepts a write it does not keep).
    fn set(&self, account: &str, key: &str) -> bool;
    fn get(&self, account: &str) -> Result<String, String>;
    fn delete(&self, account: &str);
}

/// The system's keychain (Secret Service, Credential Manager, the Keychain).
pub struct Keychain;

impl KeyStore for Keychain {
    fn set(&self, account: &str, key: &str) -> bool {
        keyring::Entry::new(SERVICE, account)
            .and_then(|e| {
                e.set_password(key)?;
                // Read back: some keyrings accept a write they do not keep.
                e.get_password().map(|k| k == key)
            })
            .unwrap_or(false)
    }

    fn get(&self, account: &str) -> Result<String, String> {
        keyring::Entry::new(SERVICE, account)
            .and_then(|e| e.get_password())
            .map_err(|e| format!("the station's key is not in the keychain: {e}"))
    }

    fn delete(&self, account: &str) {
        if let Ok(e) = keyring::Entry::new(SERVICE, account) {
            let _ = e.delete_credential();
        }
    }
}

/// Write the key to a file only this user can read.
pub fn write_key_file(path: &Path, key: &str) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    use std::io::Write;
    let mut f = opts.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(key.as_bytes())
}

/// What a single-station build left in the settings folder (`station.json`).
#[derive(Deserialize)]
struct Legacy {
    url: String,
    kept: Kept,
}

/// The list in its folder, with the place its keys go. Every method reads the file, and the
/// ones that change it write it again: the caller holds one lock around each (`station.rs`).
pub struct Shelf<'a> {
    pub dir: &'a Path,
    pub keys: &'a dyn KeyStore,
    /// Whether an address may still be used (ADR-0139): a station saved by a build that took
    /// plain http anywhere is not carried over unless it passes today's rule.
    pub allowed: fn(&str) -> bool,
}

impl Shelf<'_> {
    fn book_path(&self) -> PathBuf {
        self.dir.join("stations.json")
    }

    fn key_file(&self, id: &str) -> PathBuf {
        self.dir.join("keys").join(format!("{id}.key"))
    }

    /// The list. The first time, it is made from what a single-station build left: its station
    /// becomes the first row and the one the window is on, with its key where it already is.
    pub fn load(&self) -> Book {
        if let Ok(text) = std::fs::read_to_string(self.book_path()) {
            let mut book: Book = serde_json::from_str(&text).unwrap_or_default();
            // An address today's rule refuses is kept in the list (so it can be removed) but is
            // never the station the window is on.
            if book.here().is_some_and(|s| !(self.allowed)(&s.url)) {
                book.current = None;
            }
            return book;
        }
        self.carry_over()
    }

    /// Carry an existing install over (ADR-0285). The key is not moved: the keychain entry is
    /// already under the station's address, which is where the list looks, so this is one file
    /// written and cannot fail on a locked keyring. A key kept in the single `station.key` is
    /// renamed to the station's own file. Done twice, it changes nothing.
    fn carry_over(&self) -> Book {
        let old = self.dir.join("station.json");
        let legacy = std::fs::read_to_string(&old)
            .ok()
            .and_then(|t| serde_json::from_str::<Legacy>(&t).ok())
            .filter(|l| (self.allowed)(&l.url));
        let last = std::fs::read_to_string(self.dir.join("last-station.txt"))
            .ok()
            .map(|u| u.trim().to_owned())
            .filter(|u| (self.allowed)(u));
        let first = match (legacy, last) {
            (Some(l), _) => {
                let mut kept = Some(l.kept);
                if l.kept == Kept::File {
                    let (from, to) = (self.dir.join("station.key"), self.key_file("s1"));
                    let moved = std::fs::create_dir_all(self.dir.join("keys")).is_ok()
                        && (std::fs::rename(&from, &to).is_ok() || to.exists());
                    if !moved {
                        // The key file is gone: the row is kept, signed out.
                        kept = None;
                    }
                }
                Station {
                    id: "s1".to_owned(),
                    url: l.url,
                    label: None,
                    name: None,
                    who: None,
                    version: None,
                    kept,
                }
            }
            // A signed-out install has only its last address: a signed-out row.
            (None, Some(url)) => Station {
                id: "s1".to_owned(),
                url,
                label: None,
                name: None,
                who: None,
                version: None,
                kept: None,
            },
            (None, None) => return Book::default(),
        };
        let book = Book {
            next: 2,
            current: Some(first.id.clone()),
            stations: vec![first],
        };
        // The list first, then the old file goes: stopped between the two, the next start finds
        // the list and never reads the old file again.
        if self.store(&book).is_ok() {
            let _ = std::fs::remove_file(old);
        }
        book
    }

    /// Write the list: to a new file, then renamed into place, so a crash leaves the old one.
    pub fn store(&self, book: &Book) -> Result<(), String> {
        std::fs::create_dir_all(self.dir).map_err(|e| e.to_string())?;
        let tmp = self.dir.join("stations.json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(book).expect("the list serialises"),
        )
        .map_err(|e| format!("the list of stations could not be written: {e}"))?;
        std::fs::rename(&tmp, self.book_path())
            .map_err(|e| format!("the list of stations could not be written: {e}"))
    }

    /// A station's key, from where it is kept.
    pub fn key_of(&self, station: &Station) -> Result<String, String> {
        match station.kept {
            Some(Kept::Keychain) => self.keys.get(&station.url),
            Some(Kept::File) => std::fs::read_to_string(self.key_file(&station.id))
                .map(|k| k.trim().to_owned())
                .map_err(|e| format!("the station's key file cannot be read: {e}")),
            None => Err("this computer is signed out of that station".to_owned()),
        }
    }

    /// Forget a station's key, wherever it is: its keychain entry and its key file.
    fn forget_key(&self, station: &Station) {
        self.keys.delete(&station.url);
        let _ = std::fs::remove_file(self.key_file(&station.id));
    }

    /// Keep a station and its bearer (the station key, or a staff session's token): a new row,
    /// or the row that address already has, signed in again. No other station's key is touched.
    ///
    /// The window moves to it only when it is on no station, on this one, or on one it is
    /// signed out of (the sign-in screen is showing, so nothing is live there). Otherwise the
    /// station only joins the list, and the page switches to it by the path every switch takes,
    /// which closes the old station's links first. The answer says which.
    pub fn keep(&self, url: &str, key: &str) -> Result<(Station, bool), String> {
        if key.is_empty() {
            return Err("The key is empty.".to_owned());
        }
        let mut book = self.load();
        let id = match book.stations.iter().find(|s| s.url == url) {
            Some(s) => s.id.clone(),
            None => {
                let n = book.next.max(1);
                book.next = n + 1;
                let id = format!("s{n}");
                book.stations.push(Station {
                    id: id.clone(),
                    url: url.to_owned(),
                    label: None,
                    name: None,
                    who: None,
                    version: None,
                    kept: None,
                });
                id
            }
        };
        let kept = if self.keys.set(url, key) {
            let _ = std::fs::remove_file(self.key_file(&id));
            Kept::Keychain
        } else {
            std::fs::create_dir_all(self.dir.join("keys")).map_err(|e| e.to_string())?;
            write_key_file(&self.key_file(&id), key)
                .map_err(|e| format!("the key could not be kept: {e}"))?;
            Kept::File
        };
        let here_is_live = book.here().is_some_and(|s| s.kept.is_some() && s.id != id);
        if !here_is_live {
            book.current = Some(id.clone());
        }
        let station = book.get_mut(&id).expect("the row just found or made");
        station.kept = Some(kept);
        // A new sign-in: who was signed in before is no longer true.
        station.who = None;
        let station = station.clone();
        let now_here = book.current.as_deref() == Some(id.as_str());
        self.store(&book)?;
        Ok((station, now_here))
    }

    /// Make another station the one the window is on.
    pub fn switch(&self, id: &str) -> Result<Book, String> {
        let mut book = self.load();
        let station = book.get(id).ok_or("That station is not in the list.")?;
        if !(self.allowed)(&station.url) {
            return Err("That station's address is no longer allowed. Remove it and add it again over https.".to_owned());
        }
        book.current = Some(id.to_owned());
        self.store(&book)?;
        Ok(book)
    }

    /// Give a station a name on this computer; empty takes it away.
    pub fn rename(&self, id: &str, label: &str) -> Result<Book, String> {
        let mut book = self.load();
        let station = book.get_mut(id).ok_or("That station is not in the list.")?;
        let label: String = label.split_whitespace().collect::<Vec<_>>().join(" ");
        if label.chars().count() > LABEL_MAX {
            return Err(format!("A name here is at most {LABEL_MAX} characters."));
        }
        station.label = (!label.is_empty()).then_some(label);
        self.store(&book)?;
        Ok(book)
    }

    /// What the window last saw at the station it is on, for the list. Refused for any other
    /// station: a page that has not caught up with a switch must not write over another's row.
    pub fn note(
        &self,
        id: &str,
        name: Option<String>,
        who: Option<String>,
        version: Option<String>,
    ) -> Result<Book, String> {
        let mut book = self.load();
        if book.current.as_deref() != Some(id) {
            return Err("This window has moved to another station.".to_owned());
        }
        let station = book.get_mut(id).ok_or("That station is not in the list.")?;
        let clean = |v: Option<String>| {
            v.map(|s| s.trim().chars().take(120).collect::<String>())
                .filter(|s| !s.is_empty())
        };
        if let Some(n) = clean(name) {
            station.name = Some(n);
        }
        if let Some(w) = clean(who) {
            station.who = Some(w);
        }
        if let Some(v) = clean(version) {
            station.version = Some(v);
        }
        self.store(&book)?;
        Ok(book)
    }

    /// Sign out of a station: its key is forgotten and its row stays, with its address and its
    /// names. Answers the station and the key it had, so the caller can tell the station to end
    /// the session; none when it was signed out already.
    pub fn sign_out(&self, id: &str) -> Result<(Book, Option<(Station, String)>), String> {
        let mut book = self.load();
        let station = book
            .get(id)
            .ok_or("That station is not in the list.")?
            .clone();
        let had = self.key_of(&station).ok().map(|k| (station.clone(), k));
        self.forget_key(&station);
        let row = book.get_mut(id).expect("the row just read");
        row.kept = None;
        row.who = None;
        self.store(&book)?;
        Ok((book, had))
    }

    /// Remove a station from this computer: its key and its row. The station the window was on
    /// leaves the window on none.
    pub fn remove(&self, id: &str) -> Result<(Book, Option<(Station, String)>), String> {
        let (mut book, had) = self.sign_out(id)?;
        book.stations.retain(|s| s.id != id);
        if book.current.as_deref() == Some(id) {
            book.current = None;
        }
        self.store(&book)?;
        Ok((book, had))
    }
}

/// Whether a request that names a station may go out: only to the one the window is on. A
/// request left over from before a switch names the old station, and is refused here rather
/// than sent on with the new station's key.
pub fn same_station(named: Option<&str>, current: &str) -> Result<(), String> {
    match named {
        Some(id) if id == current => Ok(()),
        Some(_) => Err("This window has moved to another station.".to_owned()),
        None => Err("The request names no station.".to_owned()),
    }
}

/// The window's title: the station's name here, then the app's, so the task bar says where
/// the window is.
pub fn title(book: &Book) -> String {
    match book
        .here()
        .and_then(|s| s.label.as_deref().or(s.name.as_deref()))
    {
        Some(name) => format!("{name} · WeaveSignal"),
        None => "WeaveSignal".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A key store in memory; `broken` is a desktop with no keyring.
    #[derive(Default)]
    struct Memory {
        keys: RefCell<HashMap<String, String>>,
        broken: bool,
    }

    impl KeyStore for Memory {
        fn set(&self, account: &str, key: &str) -> bool {
            if self.broken {
                return false;
            }
            self.keys
                .borrow_mut()
                .insert(account.to_owned(), key.to_owned());
            true
        }
        fn get(&self, account: &str) -> Result<String, String> {
            self.keys
                .borrow()
                .get(account)
                .cloned()
                .ok_or_else(|| "not in the keychain".to_owned())
        }
        fn delete(&self, account: &str) {
            self.keys.borrow_mut().remove(account);
        }
    }

    fn https(url: &str) -> bool {
        url.starts_with("https://") || url.starts_with("http://127.0.0.1")
    }

    fn shelf<'a>(dir: &'a Path, keys: &'a Memory) -> Shelf<'a> {
        Shelf {
            dir,
            keys,
            allowed: https,
        }
    }

    const KETTLE: &str = "https://kettle.example";
    const ROSS: &str = "https://radio-ross.example";

    #[test]
    fn an_existing_install_carries_over_without_signing_in_again() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Memory::default();
        // What a single-station build left: the address in station.json, the key in the
        // keychain under that address.
        std::fs::write(
            dir.path().join("station.json"),
            format!(r#"{{"url":"{KETTLE}","kept":"keychain"}}"#),
        )
        .unwrap();
        std::fs::write(dir.path().join("last-station.txt"), KETTLE).unwrap();
        keys.set(KETTLE, "rss_kettle-session");
        let s = shelf(dir.path(), &keys);

        let book = s.load();
        assert_eq!(book.current.as_deref(), Some("s1"));
        assert_eq!(book.stations.len(), 1);
        let here = book.here().unwrap();
        assert_eq!(
            (here.url.as_str(), here.kept),
            (KETTLE, Some(Kept::Keychain))
        );
        // The key was not moved and is found.
        assert_eq!(s.key_of(here).unwrap(), "rss_kettle-session");
        assert_eq!(keys.keys.borrow().len(), 1);
        // The old file is gone, the list is written, and it holds no key.
        assert!(!dir.path().join("station.json").exists());
        let text = std::fs::read_to_string(dir.path().join("stations.json")).unwrap();
        assert!(!text.contains("rss_"), "{text}");
        // Twice changes nothing.
        assert_eq!(s.load(), book);
    }

    #[test]
    fn a_key_kept_in_the_single_file_carries_over_to_the_stations_own_file() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Memory {
            broken: true,
            ..Memory::default()
        };
        std::fs::write(
            dir.path().join("station.json"),
            format!(r#"{{"url":"{KETTLE}","kept":"file"}}"#),
        )
        .unwrap();
        write_key_file(&dir.path().join("station.key"), "admin-key\n").unwrap();
        let s = shelf(dir.path(), &keys);
        let book = s.load();
        let here = book.here().unwrap();
        assert_eq!(here.kept, Some(Kept::File));
        assert_eq!(s.key_of(here).unwrap(), "admin-key");
        assert!(!dir.path().join("station.key").exists());
        assert!(dir.path().join("keys/s1.key").exists());
        assert_eq!(s.load(), book);
    }

    #[test]
    fn a_signed_out_install_carries_over_as_a_signed_out_row() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Memory::default();
        std::fs::write(dir.path().join("last-station.txt"), format!("{KETTLE}\n")).unwrap();
        let s = shelf(dir.path(), &keys);
        let book = s.load();
        let here = book.here().unwrap();
        assert_eq!((here.url.as_str(), here.kept), (KETTLE, None));
        assert!(s.key_of(here).is_err());
    }

    #[test]
    fn nothing_saved_is_an_empty_list_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Memory::default();
        let book = shelf(dir.path(), &keys).load();
        assert_eq!(book, Book::default());
        assert!(!dir.path().join("stations.json").exists());
    }

    #[test]
    fn an_address_todays_rule_refuses_is_not_carried_over() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Memory::default();
        std::fs::write(
            dir.path().join("station.json"),
            r#"{"url":"http://radio.example.org","kept":"keychain"}"#,
        )
        .unwrap();
        assert_eq!(shelf(dir.path(), &keys).load(), Book::default());
    }

    #[test]
    fn a_second_station_has_a_key_of_its_own_and_leaves_the_firsts_alone() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Memory::default();
        let s = shelf(dir.path(), &keys);
        let (kettle, here) = s.keep(KETTLE, "rss_kettle").unwrap();
        assert!(here, "the first station takes the window");
        let (ross, here) = s.keep(ROSS, "rss_ross").unwrap();
        assert!(
            !here,
            "a second station joins the list; the window stays where it is"
        );
        assert_ne!(kettle.id, ross.id);
        let book = s.load();
        assert_eq!(book.current.as_deref(), Some(kettle.id.as_str()));
        // Distinct entries, each its own station's.
        assert_eq!(
            s.key_of(book.get(&kettle.id).unwrap()).unwrap(),
            "rss_kettle"
        );
        assert_eq!(s.key_of(book.get(&ross.id).unwrap()).unwrap(), "rss_ross");
        assert_eq!(keys.keys.borrow().len(), 2);
        // Switching changes only which one the window is on.
        let book = s.switch(&ross.id).unwrap();
        assert_eq!(book.current.as_deref(), Some(ross.id.as_str()));
        assert_eq!(s.key_of(book.here().unwrap()).unwrap(), "rss_ross");
        assert!(s.switch("s99").is_err());
        assert_eq!(s.load().current.as_deref(), Some(ross.id.as_str()));
    }

    #[test]
    fn one_address_is_one_row_and_signing_in_again_replaces_its_key() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Memory::default();
        let s = shelf(dir.path(), &keys);
        let (first, _) = s.keep(KETTLE, "rss_one").unwrap();
        s.note(
            &first.id,
            Some("Kettle FM".into()),
            Some("Eilidh Fraser-MacLeod, presenter".into()),
            Some("1.0.5".into()),
        )
        .unwrap();
        let (again, here) = s.keep(KETTLE, "rss_two").unwrap();
        assert_eq!(first.id, again.id);
        assert!(here);
        let book = s.load();
        assert_eq!(book.stations.len(), 1);
        assert_eq!(s.key_of(book.here().unwrap()).unwrap(), "rss_two");
        // The station's own name stays; who was signed in is no longer known.
        assert_eq!(book.here().unwrap().name.as_deref(), Some("Kettle FM"));
        assert_eq!(book.here().unwrap().who, None);
    }

    #[test]
    fn signing_in_while_the_window_is_signed_out_of_its_station_moves_the_window() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Memory::default();
        let s = shelf(dir.path(), &keys);
        let (kettle, _) = s.keep(KETTLE, "rss_kettle").unwrap();
        s.sign_out(&kettle.id).unwrap();
        // The sign-in screen is showing for Kettle FM; the person signs in to Radio Ross.
        let (ross, here) = s.keep(ROSS, "rss_ross").unwrap();
        assert!(here);
        assert_eq!(s.load().current.as_deref(), Some(ross.id.as_str()));
    }

    #[test]
    fn without_a_keyring_each_station_has_a_key_file_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Memory {
            broken: true,
            ..Memory::default()
        };
        let s = shelf(dir.path(), &keys);
        let (kettle, _) = s.keep(KETTLE, "key-kettle").unwrap();
        let (ross, _) = s.keep(ROSS, "key-ross").unwrap();
        assert_eq!(
            (kettle.kept, ross.kept),
            (Some(Kept::File), Some(Kept::File))
        );
        let book = s.load();
        assert_eq!(
            s.key_of(book.get(&kettle.id).unwrap()).unwrap(),
            "key-kettle"
        );
        assert_eq!(s.key_of(book.get(&ross.id).unwrap()).unwrap(), "key-ross");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for id in [&kettle.id, &ross.id] {
                let mode = std::fs::metadata(dir.path().join("keys").join(format!("{id}.key")))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777;
                assert_eq!(mode, 0o600);
            }
        }
    }

    #[test]
    fn signing_out_keeps_the_row_and_removing_deletes_the_entry() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Memory::default();
        let s = shelf(dir.path(), &keys);
        let (kettle, _) = s.keep(KETTLE, "rss_kettle").unwrap();
        let (ross, _) = s.keep(ROSS, "rss_ross").unwrap();

        // Sign out of the other station: its key goes, its row stays, and the key it had is
        // handed back so the station can be told.
        let (book, had) = s.sign_out(&ross.id).unwrap();
        assert_eq!(
            had.map(|(st, k)| (st.url, k)),
            Some((ROSS.to_owned(), "rss_ross".to_owned()))
        );
        assert_eq!(book.get(&ross.id).unwrap().kept, None);
        assert!(keys.get(ROSS).is_err());
        assert_eq!(keys.get(KETTLE).unwrap(), "rss_kettle");
        // Again: nothing to hand back.
        assert!(s.sign_out(&ross.id).unwrap().1.is_none());

        // Remove the station the window is on: its entry is deleted and the window is on none.
        let (book, had) = s.remove(&kettle.id).unwrap();
        assert!(had.is_some());
        assert!(keys.keys.borrow().is_empty());
        assert_eq!(book.current, None);
        assert_eq!(
            book.stations
                .iter()
                .map(|x| x.id.as_str())
                .collect::<Vec<_>>(),
            vec![ross.id.as_str()]
        );
        // An id is never used again.
        let (third, _) = s.keep("https://third.example", "k").unwrap();
        assert_eq!(third.id, "s3");
        assert!(s.remove("s99").is_err());
    }

    #[test]
    fn removing_a_station_without_a_keyring_deletes_its_key_file() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Memory {
            broken: true,
            ..Memory::default()
        };
        let s = shelf(dir.path(), &keys);
        let (kettle, _) = s.keep(KETTLE, "key-kettle").unwrap();
        let file = dir.path().join("keys").join(format!("{}.key", kettle.id));
        assert!(file.exists());
        s.remove(&kettle.id).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn a_label_is_set_tidied_and_taken_away() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Memory::default();
        let s = shelf(dir.path(), &keys);
        let (kettle, _) = s.keep(KETTLE, "k").unwrap();
        assert_eq!(
            s.rename(&kettle.id, "  Kettle FM   test ")
                .unwrap()
                .here()
                .unwrap()
                .label
                .as_deref(),
            Some("Kettle FM test")
        );
        assert_eq!(
            s.rename(&kettle.id, "   ").unwrap().here().unwrap().label,
            None
        );
        assert!(s.rename(&kettle.id, &"x".repeat(LABEL_MAX + 1)).is_err());
        assert!(s.rename("s99", "x").is_err());
    }

    #[test]
    fn what_was_last_seen_is_noted_only_for_the_station_the_window_is_on() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Memory::default();
        let s = shelf(dir.path(), &keys);
        let (kettle, _) = s.keep(KETTLE, "k").unwrap();
        let (ross, _) = s.keep(ROSS, "k2").unwrap();
        s.note(
            &kettle.id,
            Some("Kettle FM".into()),
            None,
            Some("1.0.5".into()),
        )
        .unwrap();
        // A page that has not caught up with a switch cannot write over another station's row.
        assert!(
            s.note(&ross.id, Some("Kettle FM".into()), None, None)
                .is_err()
        );
        let book = s.load();
        assert_eq!(
            book.get(&kettle.id).unwrap().name.as_deref(),
            Some("Kettle FM")
        );
        assert_eq!(book.get(&ross.id).unwrap().name, None);
        assert_eq!(title(&book), "Kettle FM · WeaveSignal");
        assert_eq!(
            title(&s.rename(&kettle.id, "Kettle FM test").unwrap()),
            "Kettle FM test · WeaveSignal"
        );
        assert_eq!(title(&Book::default()), "WeaveSignal");
    }

    #[test]
    fn a_request_goes_only_to_the_station_it_names() {
        assert!(same_station(Some("s1"), "s1").is_ok());
        // Left over from before a switch: refused, not sent on with the other station's key.
        assert_eq!(
            same_station(Some("s1"), "s2").unwrap_err(),
            "This window has moved to another station."
        );
        assert!(same_station(None, "s1").is_err());
    }

    #[test]
    fn a_list_from_a_newer_build_with_fields_this_one_does_not_know_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Memory::default();
        std::fs::write(
            dir.path().join("stations.json"),
            format!(r#"{{"next":3,"current":"s2","colour":"teal","stations":[{{"id":"s2","url":"{ROSS}","kept":"keychain","logo":"x"}}]}}"#),
        )
        .unwrap();
        let book = shelf(dir.path(), &keys).load();
        assert_eq!(book.here().unwrap().url, ROSS);
    }
}
