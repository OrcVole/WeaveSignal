// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The bridge's commands (Phase 17g): Settings › Bridge starts the local listeners
//! ([`crate::bridge_local`]) and the uplink ([`crate::bridge_up`]), and shows where they stand.
//!
//! The show key (`<key id>-<secret>`, the Icecast push's password) and the local password are
//! kept in the system keychain, never in a file and never in the page after they are saved;
//! the station's public address and the two local ports are kept in `bridge.json` in the app's
//! settings folder. The local password is shown to copy into the host's software.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tokio::sync::mpsc;

use crate::bridge_local::{ICECAST_PORT, Local, SRT_PORT};
use crate::bridge_up::{Status, Target};
use crate::stations::{KeyStore, Keychain};

const KEY_ACCOUNT: &str = "bridge-show-key";
const LOCAL_ACCOUNT: &str = "bridge-local-password";
const FILE: &str = "bridge.json";

/// What is kept on this computer besides the keychain's two secrets.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Saved {
    /// The station's public address (`https://radio.example.org`): on Cloudron the listeners'
    /// address, not the studio's, which is behind the login.
    pub url: String,
    /// The show key's id (public), for the screen.
    pub key_id: String,
    pub icecast_port: u16,
    pub srt_port: u16,
}

struct Running {
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

/// The bridge's state in the app.
pub struct Bridge {
    running: Mutex<Option<Running>>,
    status: Arc<Mutex<Status>>,
    /// The page was asked whether to close the window and has not answered.
    close_asked: AtomicBool,
}

impl Bridge {
    pub fn new() -> Self {
        Bridge {
            running: Mutex::new(None),
            status: Arc::new(Mutex::new(Status {
                uplink: "off".into(),
                ..Status::default()
            })),
            close_asked: AtomicBool::new(false),
        }
    }

    /// Whether the bridge runs now.
    pub fn running(&self) -> bool {
        self.running.lock().expect("bridge").is_some()
    }

    /// The window's close button (or the system's close) was pressed: what happens. Asking
    /// marks the question open, so a second press while it is open closes: a page that cannot
    /// answer never keeps the window open.
    pub fn on_close_requested(&self) -> OnClose {
        let asked = self.close_asked.swap(true, Ordering::SeqCst);
        let d = on_close(self.running(), asked);
        if d == OnClose::Close {
            self.close_asked.store(false, Ordering::SeqCst);
        }
        d
    }
}

/// What a request to close the window does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnClose {
    /// Close: the bridge is off, or the question was already put and not answered.
    Close,
    /// Keep the window open and ask the page (the `close-asked` event).
    Ask,
}

/// The event the page hears when the window would close with the bridge running.
pub const CLOSE_ASKED: &str = "close-asked";

/// Closing stops the bridge, and with it the host's software's way to the station: the first
/// request while it runs is a question; a second, while the question is open, closes.
pub fn on_close(running: bool, asked_already: bool) -> OnClose {
    if running && !asked_already {
        OnClose::Ask
    } else {
        OnClose::Close
    }
}

/// The page's answer: keep the window open (a later close asks again).
#[tauri::command]
pub fn window_close_keep(bridge: State<'_, Bridge>) {
    bridge.close_asked.store(false, Ordering::SeqCst);
}

/// The page's answer: stop the bridge, then close the window. The window is destroyed, not
/// closed, so the request does not come round again.
#[tauri::command]
pub fn window_close_stop(
    window: tauri::WebviewWindow,
    bridge: State<'_, Bridge>,
) -> Result<(), String> {
    bridge.running.lock().expect("bridge").take();
    bridge.close_asked.store(false, Ordering::SeqCst);
    window.destroy().map_err(|e| e.to_string())
}

/// What the screen shows.
#[derive(Serialize)]
pub struct View {
    pub saved: Saved,
    /// A show key is in the keychain.
    pub has_key: bool,
    pub running: bool,
    /// The local password, to copy into the host's software (it reaches nothing but this
    /// computer's loopback).
    pub local_password: Option<String>,
    pub status: Status,
}

fn off_runtime<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(f).join().expect("the keychain thread")
}

fn file(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|d| d.join(FILE))
        .map_err(|e| format!("no folder for the app's settings: {e}"))
}

fn load(app: &AppHandle) -> Saved {
    let mut s: Saved = file(app)
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if s.icecast_port == 0 {
        s.icecast_port = ICECAST_PORT;
    }
    if s.srt_port == 0 {
        s.srt_port = SRT_PORT;
    }
    s
}

fn store(app: &AppHandle, s: &Saved) -> Result<(), String> {
    let p = file(app)?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?)
        .map_err(|e| format!("cannot save the bridge's settings: {e}"))
}

/// A new local password: 20 characters of base 32 (100 bits).
pub fn new_password() -> String {
    const ABC: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut b = [0u8; 20];
    getrandom::fill(&mut b).expect("the system's random numbers");
    b.iter().map(|x| ABC[usize::from(x & 31)] as char).collect()
}

/// The local password, made and kept the first time.
fn local_password() -> Result<String, String> {
    off_runtime(|| {
        if let Ok(p) = Keychain.get(LOCAL_ACCOUNT) {
            return Ok(p);
        }
        let p = new_password();
        if Keychain.set(LOCAL_ACCOUNT, &p) {
            Ok(p)
        } else {
            Err("the keychain did not keep the bridge's local password".into())
        }
    })
}

/// The key id and secret from what a key's details show as the Icecast password.
pub fn split_key(password: &str) -> Result<(String, String), String> {
    let p = password.trim();
    let (id, secret) = p
        .split_once('-')
        .ok_or("Paste the key's Icecast password: its id, a hyphen, then its secret.")?;
    let ok = id.len() == 8
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c))
        && !secret.is_empty()
        && !secret.chars().any(|c| c.is_control() || c.is_whitespace());
    if ok {
        Ok((id.to_owned(), secret.to_owned()))
    } else {
        Err("That is not a show key's password: 8 characters, a hyphen, then the secret.".into())
    }
}

fn view(app: &AppHandle, bridge: &Bridge) -> View {
    let saved = load(app);
    let has_key = off_runtime(|| Keychain.get(KEY_ACCOUNT).is_ok());
    View {
        saved,
        has_key,
        running: bridge.running.lock().expect("bridge").is_some(),
        local_password: local_password().ok(),
        status: bridge.status.lock().expect("bridge status").clone(),
    }
}

#[tauri::command]
pub fn bridge_view(app: AppHandle, bridge: State<'_, Bridge>) -> View {
    view(&app, &bridge)
}

/// Keep the station's public address, the ports and the show key (into the keychain).
#[tauri::command]
pub fn bridge_save(
    app: AppHandle,
    bridge: State<'_, Bridge>,
    url: String,
    password: Option<String>,
    icecast_port: u16,
    srt_port: u16,
) -> Result<View, String> {
    let url = crate::station::clean_url(&url)?;
    if icecast_port < 1024 || srt_port < 1024 || icecast_port == srt_port {
        return Err("Use two different ports from 1024 up for the local side.".into());
    }
    let mut saved = load(&app);
    if let Some(p) = password.filter(|p| !p.trim().is_empty()) {
        let (id, _) = split_key(&p)?;
        let p = p.trim().to_owned();
        if !off_runtime(move || Keychain.set(KEY_ACCOUNT, &p)) {
            return Err("The keychain did not keep the key.".into());
        }
        saved.key_id = id;
    }
    saved.url = url;
    saved.icecast_port = icecast_port;
    saved.srt_port = srt_port;
    store(&app, &saved)?;
    Ok(view(&app, &bridge))
}

/// Start the local listeners and the uplink.
#[tauri::command]
pub async fn bridge_start(app: AppHandle, bridge: State<'_, Bridge>) -> Result<View, String> {
    if bridge.running.lock().expect("bridge").is_some() {
        return Ok(view(&app, &bridge));
    }
    let saved = load(&app);
    if saved.url.is_empty() {
        return Err("Save the station's address and a show key first.".into());
    }
    let password = off_runtime(|| Keychain.get(KEY_ACCOUNT))
        .map_err(|_| "Save a show key first: it is not in the keychain.".to_owned())?;
    let local_pw = local_password()?;
    let (tx, events) = mpsc::channel(8);
    let local = Local::new(local_pw, tx);
    let (_, ice) = local
        .icecast(saved.icecast_port)
        .await
        .map_err(|e| format!("Port {} is taken on this computer: {e}", saved.icecast_port))?;
    let (_, srt) = match local.srt(saved.srt_port).await {
        Ok(x) => x,
        Err(e) => {
            ice.abort();
            return Err(format!(
                "Port {} is taken on this computer: {e}",
                saved.srt_port
            ));
        }
    };
    let up = tokio::spawn(crate::bridge_up::run(
        Target {
            url: saved.url.clone(),
            password,
        },
        events,
        bridge.status.clone(),
    ));
    *bridge.running.lock().expect("bridge") = Some(Running {
        tasks: vec![ice, srt, up],
    });
    Ok(view(&app, &bridge))
}

/// Stop: the host's software loses its server, and the source leaves the desk.
#[tauri::command]
pub fn bridge_stop(app: AppHandle, bridge: State<'_, Bridge>) -> View {
    bridge.running.lock().expect("bridge").take();
    *bridge.status.lock().expect("bridge status") = Status {
        uplink: "off".into(),
        ..Status::default()
    };
    view(&app, &bridge)
}

/// A new local password (the bridge stopped first): the host's software needs it again.
#[tauri::command]
pub fn bridge_new_password(app: AppHandle, bridge: State<'_, Bridge>) -> Result<View, String> {
    if bridge.running.lock().expect("bridge").is_some() {
        return Err("Stop the bridge first.".into());
    }
    let p = new_password();
    if !off_runtime(move || Keychain.set(LOCAL_ACCOUNT, &p)) {
        return Err("The keychain did not keep the new password.".into());
    }
    Ok(view(&app, &bridge))
}

/// Forget the show key.
#[tauri::command]
pub fn bridge_forget(app: AppHandle, bridge: State<'_, Bridge>) -> View {
    bridge.running.lock().expect("bridge").take();
    off_runtime(|| Keychain.delete(KEY_ACCOUNT));
    let mut saved = load(&app);
    saved.key_id.clear();
    let _ = store(&app, &saved);
    view(&app, &bridge)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_asks_once_while_the_bridge_runs_and_never_holds_the_window() {
        assert_eq!(on_close(false, false), OnClose::Close);
        assert_eq!(on_close(true, false), OnClose::Ask);
        // Asked and not answered (the page hung, or the person pressed close again): close.
        assert_eq!(on_close(true, true), OnClose::Close);
        assert_eq!(on_close(false, true), OnClose::Close);

        let b = Bridge::new();
        assert_eq!(b.on_close_requested(), OnClose::Close);
        *b.running.lock().unwrap() = Some(Running { tasks: Vec::new() });
        assert_eq!(b.on_close_requested(), OnClose::Ask);
        assert_eq!(b.on_close_requested(), OnClose::Close);
        // A close that went through leaves no open question: the next one asks again.
        assert_eq!(b.on_close_requested(), OnClose::Ask);
        // Keep the window open answers it, so the next press asks again too.
        b.close_asked.store(false, Ordering::SeqCst);
        assert_eq!(b.on_close_requested(), OnClose::Ask);
    }

    #[test]
    fn a_key_is_its_icecast_password_and_a_local_password_is_strong() {
        assert_eq!(
            split_key(" abcd2345-thesecret ").unwrap(),
            ("abcd2345".into(), "thesecret".into())
        );
        assert!(split_key("abcd2345").is_err());
        assert!(split_key("ABCD2345-x").is_err());
        assert!(split_key("abcd2345-two words").is_err());
        let p = new_password();
        assert_eq!(p.len(), 20);
        assert_ne!(p, new_password());
        // Long enough for SRT's passphrase (10 to 79 characters).
        assert!((10..=79).contains(&p.len()));
    }
}
