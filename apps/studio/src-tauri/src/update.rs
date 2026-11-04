// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The update screen's Rust side (ADR-0015, ADR-0140): a check against the signed feed, a
//! download that is verified before anything is installed, an install, and a way back.
//!
//! The page calls these commands; it never reaches the updater plugin's own commands (the
//! window's capability does not grant `updater:*`), so the page cannot install anything the
//! checks here did not find. Nothing here runs by itself: a check happens when the page asks,
//! and an install only on a command. Whether a show is on air is judged twice: by the page, from
//! the desk's view and the held talk keys (`apps/studio/src/lib/update.ts`), asked again between
//! the download and the install; and here, before an install or a rollback restarts the app,
//! from the station's view fetched fresh (`station::desk_busy`), since the page's can be stale.
//!
//! The feed is a build setting (`WEAVESIGNAL_UPDATER_ENDPOINT`, `deploy/desktop/build.sh`). A
//! build made without a key has no endpoint, and `update_check` then says the build does not
//! update itself.
//!
//! Rolling back. The updater's own AppImage backup is a temporary directory that is deleted
//! when the install succeeds, so nothing it keeps outlives the install. Before an AppImage is
//! replaced, the running file (`$APPIMAGE`, the file the updater writes and the restart runs;
//! not `current_exe`, which is inside the AppImage's mount) is copied to the app's local data
//! folder, and becomes the kept copy only once the install succeeded; `update_rollback` puts it
//! back and restarts. One copy is kept, the size of the AppImage. The `.deb`, `.rpm` and the Windows installer replace the app through the
//! system's package tools or an installer, so for those the way back is installing the earlier
//! package by hand (the how-to says how). The feed cannot roll anyone back: the updater offers
//! only a newer version (`allowDowngrades` is off), and `requireSignedVersion` stops a feed
//! from passing an older signed package off as a newer one.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::station::{Stations, desk_busy};

/// The update found by the last check and, once downloaded, its verified bytes.
#[derive(Default)]
pub struct Updates {
    found: Mutex<Option<Update>>,
    bytes: Mutex<Option<Vec<u8>>>,
}

/// An update the feed offers.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Available {
    pub version: String,
    /// The release notes, as the feed gives them (plain text).
    pub notes: Option<String>,
    /// The release's date, in seconds since 1970, if the feed gives one.
    pub date: Option<i64>,
    /// This version was rolled back from: the frame does not nag about it, the panel still
    /// offers it.
    pub skipped: bool,
    /// Downloaded and verified, waiting for the install.
    pub downloaded: bool,
}

/// What a check found, and what the panel can offer besides.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Check {
    /// The build has a key and a feed. Without them it never updates itself.
    pub enabled: bool,
    pub current: String,
    pub available: Option<Available>,
    /// The version a rollback would go back to, if one is kept.
    pub previous: Option<String>,
}

/// Download progress, over a channel.
#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub done: u64,
    pub total: Option<u64>,
}

/// What the app remembers between runs: the version kept for a rollback, and a version rolled
/// back from.
#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
struct Memory {
    previous: Option<String>,
    skip: Option<String>,
}

const MEMORY: &str = "updates.json";
const KEPT: &str = "previous.AppImage";

/// The running AppImage, when the updater would install over it: `$APPIMAGE` is set and the
/// build is not a .deb or .rpm (whose installs go through the package tools). A `.deb` started
/// with an `APPIMAGE` left in its environment (from another AppImage that launched it) is not
/// an AppImage, and its rollback must never write over that other file.
/// Elsewhere than Linux there is no AppImage, so there is nothing to roll back to.
#[cfg(not(target_os = "linux"))]
fn running_appimage(_app: &AppHandle) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "linux")]
fn running_appimage(app: &AppHandle) -> Option<PathBuf> {
    use tauri::utils::config::BundleType;
    if matches!(
        tauri::utils::platform::bundle_type(),
        Some(BundleType::Deb | BundleType::Rpm)
    ) {
        return None;
    }
    let path = PathBuf::from(app.env().appimage?);
    path.is_absolute().then_some(path)
}

fn dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map_err(|e| format!("no folder for the app's data: {e}"))
}

fn load(dir: &Path) -> Memory {
    std::fs::read_to_string(dir.join(MEMORY))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(dir: &Path, memory: &Memory) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    let text = serde_json::to_string(memory).map_err(|e| e.to_string())?;
    let path = dir.join(MEMORY);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
}

fn staged(dir: &Path) -> PathBuf {
    dir.join(format!("{KEPT}.tmp"))
}

/// Copy the running AppImage aside, before the updater touches the file. It becomes the copy
/// to roll back to only when the install succeeds (`keep_staged`); until then a copy kept by
/// an earlier update stays.
fn stage_previous(dir: &Path, appimage: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    std::fs::copy(appimage, staged(dir)).map_err(|e| {
        let _ = std::fs::remove_file(staged(dir));
        format!("cannot keep {} for a rollback: {e}", appimage.display())
    })?;
    Ok(())
}

/// The install succeeded: the staged copy is the version to roll back to.
fn keep_staged(dir: &Path, version: &str) -> Result<(), String> {
    std::fs::rename(staged(dir), dir.join(KEPT))
        .map_err(|e| format!("cannot keep the rollback copy: {e}"))?;
    let mut memory = load(dir);
    memory.previous = Some(version.to_string());
    save(dir, &memory)
}

/// The install failed: the running file is still this version, so the staged copy is dropped
/// and whatever was kept before stays.
fn drop_staged(dir: &Path) {
    let _ = std::fs::remove_file(staged(dir));
}

/// Put the kept AppImage back over the running one, and remember not to nag about the version
/// being left. The file is staged beside the target, so the rename stays on one device and the
/// running copy is never half written. Returns the version restored.
fn restore_previous(dir: &Path, appimage: &Path, leaving: &str) -> Result<String, String> {
    let mut memory = load(dir);
    let version = memory
        .previous
        .clone()
        .ok_or("no earlier version is kept")?;
    let kept = dir.join(KEPT);
    if !kept.is_file() {
        return Err("the kept copy of the earlier version is gone".into());
    }
    // The running file's mode; if a failed install left no file there, the copy's own (a
    // copy keeps the mode of the file it was made from).
    let permissions = std::fs::metadata(appimage)
        .or_else(|_| std::fs::metadata(&kept))
        .map_err(|e| format!("cannot read {}: {e}", appimage.display()))?
        .permissions();
    let stage = appimage.with_extension("rollback");
    let swapped = std::fs::copy(&kept, &stage)
        .map_err(|e| format!("cannot stage the earlier version: {e}"))
        .and_then(|_| std::fs::set_permissions(&stage, permissions).map_err(|e| e.to_string()))
        .and_then(|_| {
            std::fs::rename(&stage, appimage)
                .map_err(|e| format!("cannot replace {}: {e}", appimage.display()))
        });
    if let Err(e) = swapped {
        // The running file is untouched; leave no half-made copy beside it.
        let _ = std::fs::remove_file(&stage);
        return Err(e);
    }
    memory.previous = None;
    memory.skip = Some(leaving.to_string());
    save(dir, &memory)?;
    // The copy is spent; a failure to delete it only wastes space.
    let _ = std::fs::remove_file(&kept);
    Ok(version)
}

/// The version kept for a rollback, if the file is still there.
fn previous_of(dir: &Path) -> Option<String> {
    load(dir).previous.filter(|_| dir.join(KEPT).is_file())
}

fn describe(update: &Update, memory: &Memory, downloaded: bool) -> Available {
    Available {
        version: update.version.clone(),
        notes: update.body.clone().filter(|n| !n.trim().is_empty()),
        date: update.date.map(|d| d.unix_timestamp()),
        skipped: memory.skip.as_deref() == Some(update.version.as_str()),
        downloaded,
    }
}

/// Ask the feed. A build without a key answers `enabled: false`; a feed that cannot be
/// reached is an error the panel shows, never a reason to stop the app.
#[tauri::command]
pub async fn update_check(app: AppHandle, state: State<'_, Updates>) -> Result<Check, String> {
    let current = app.package_info().version.to_string();
    let dir = dir(&app)?;
    let memory = load(&dir);
    let previous = running_appimage(&app).and_then(|_| previous_of(&dir));
    let updater = match app.updater() {
        Ok(u) => u,
        Err(tauri_plugin_updater::Error::EmptyEndpoints) => {
            return Ok(Check {
                enabled: false,
                current,
                available: None,
                previous,
            });
        }
        Err(e) => return Err(e.to_string()),
    };
    let found = updater.check().await.map_err(|e| e.to_string())?;
    let available = match &found {
        Some(update) => {
            // The release already downloaded keeps its bytes; any other release drops them.
            let same = state
                .found
                .lock()
                .expect("update state")
                .as_ref()
                .is_some_and(|u| u.version == update.version);
            if !same {
                *state.bytes.lock().expect("update state") = None;
            }
            let downloaded = state.bytes.lock().expect("update state").is_some();
            Some(describe(update, &memory, downloaded))
        }
        None => {
            *state.bytes.lock().expect("update state") = None;
            None
        }
    };
    *state.found.lock().expect("update state") = found;
    Ok(Check {
        enabled: true,
        current,
        available,
        previous,
    })
}

/// Download the update found by the last check. The updater verifies the signature (and the
/// signed version) before it returns the bytes; nothing is installed.
#[tauri::command]
pub async fn update_download(
    state: State<'_, Updates>,
    progress: Channel<Progress>,
) -> Result<(), String> {
    let update = state
        .found
        .lock()
        .expect("update state")
        .clone()
        .ok_or("no update has been found; check first")?;
    let mut done: u64 = 0;
    let bytes = update
        .download(
            |chunk, total| {
                done += chunk as u64;
                let _ = progress.send(Progress { done, total });
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    *state.bytes.lock().expect("update state") = Some(bytes);
    Ok(())
}

/// Install the downloaded update and restart. On Windows the updater starts the installer and
/// ends the process itself; elsewhere the app restarts here (an AppImage restarts as the
/// file it was started from, which now holds the new version). Refused while the station's
/// own view shows a strip open, whatever the page believed.
#[tauri::command]
pub async fn update_install(
    app: AppHandle,
    state: State<'_, Updates>,
    stations: State<'_, Stations>,
) -> Result<(), String> {
    let update = state
        .found
        .lock()
        .expect("update state")
        .clone()
        .ok_or("no update has been found; check first")?;
    if state.bytes.lock().expect("update state").is_none() {
        return Err("the update has not been downloaded".into());
    }
    if let Some(why) = desk_busy(&app, &stations).await {
        return Err(why);
    }
    let appimage = running_appimage(&app);
    let data = dir(&app)?;
    if let Some(appimage) = &appimage {
        stage_previous(&data, appimage)?;
    }
    let Some(bytes) = state.bytes.lock().expect("update state").take() else {
        drop_staged(&data);
        return Err("the update has not been downloaded".into());
    };
    // `install` is blocking (a .deb or .rpm waits on the system's password prompt).
    let (result, bytes) = tauri::async_runtime::spawn_blocking(move || {
        let r = update.install(&bytes);
        (r, bytes)
    })
    .await
    .map_err(|e| e.to_string())?;
    if let Err(e) = result {
        match &appimage {
            // The updater could not put the running file back after a failed write: the staged
            // copy is now the only one, so it is kept for `update_rollback` to restore.
            Some(path) if !path.is_file() => {
                let _ = keep_staged(&data, &app.package_info().version.to_string());
            }
            _ => drop_staged(&data),
        }
        // Verified bytes stay, so a retry needs no second download.
        *state.bytes.lock().expect("update state") = Some(bytes);
        return Err(e.to_string());
    }
    if appimage.is_some() {
        // The new version is in place; a failure to keep the copy only loses the way back.
        let _ = keep_staged(&data, &app.package_info().version.to_string());
    }
    app.restart()
}

/// Put the kept AppImage back and restart. Only an AppImage keeps a copy. Refused while the
/// station's own view shows a strip open.
#[tauri::command]
pub async fn update_rollback(app: AppHandle, stations: State<'_, Stations>) -> Result<(), String> {
    let appimage = running_appimage(&app)
        .ok_or("only an AppImage can roll back from here; install the earlier package by hand")?;
    if let Some(why) = desk_busy(&app, &stations).await {
        return Err(why);
    }
    let dir = dir(&app)?;
    restore_previous(&dir, &appimage, &app.package_info().version.to_string())?;
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch folder in the worktree's `target/`, not the system's temporary folder.
    fn scratch() -> tempfile::TempDir {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target");
        std::fs::create_dir_all(&root).unwrap();
        tempfile::Builder::new()
            .prefix("update-test-")
            .tempdir_in(root)
            .unwrap()
    }

    #[test]
    fn a_replaced_appimage_comes_back_and_the_bad_version_is_not_nagged_about() {
        let tmp = scratch();
        let data = tmp.path().join("data");
        let app = tmp.path().join("WeaveSignal.AppImage");
        std::fs::write(&app, b"version 0.1.0").unwrap();
        stage_previous(&data, &app).unwrap();
        assert_eq!(
            previous_of(&data),
            None,
            "nothing is kept before the install succeeds"
        );
        keep_staged(&data, "0.1.0").unwrap();
        assert_eq!(previous_of(&data).as_deref(), Some("0.1.0"));

        // The updater replaces the running file.
        std::fs::write(&app, b"version 0.2.0").unwrap();
        let back = restore_previous(&data, &app, "0.2.0").unwrap();
        assert_eq!(back, "0.1.0");
        assert_eq!(std::fs::read(&app).unwrap(), b"version 0.1.0");
        // The kept copy is spent, and 0.2.0 is remembered as rolled back from.
        assert_eq!(previous_of(&data), None);
        assert_eq!(load(&data).skip.as_deref(), Some("0.2.0"));
        assert!(!data.join(KEPT).exists());
        assert!(
            !app.with_extension("rollback").exists(),
            "no staging file is left"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_rollback_keeps_the_appimage_executable() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = scratch();
        let data = tmp.path().join("data");
        let app = tmp.path().join("WeaveSignal.AppImage");
        std::fs::write(&app, b"old").unwrap();
        std::fs::set_permissions(&app, std::fs::Permissions::from_mode(0o755)).unwrap();
        stage_previous(&data, &app).unwrap();
        keep_staged(&data, "0.1.0").unwrap();
        restore_previous(&data, &app, "0.2.0").unwrap();
        let mode = std::fs::metadata(&app).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
    }

    #[test]
    fn nothing_is_restored_when_nothing_is_kept() {
        let tmp = scratch();
        let data = tmp.path().join("data");
        let app = tmp.path().join("WeaveSignal.AppImage");
        std::fs::write(&app, b"current").unwrap();
        assert!(restore_previous(&data, &app, "0.2.0").is_err());
        assert_eq!(std::fs::read(&app).unwrap(), b"current");

        // A memory with the version but no file (deleted by hand) is refused too.
        save(
            &data,
            &Memory {
                previous: Some("0.1.0".into()),
                skip: None,
            },
        )
        .unwrap();
        assert_eq!(previous_of(&data), None);
        assert!(restore_previous(&data, &app, "0.2.0").is_err());
        assert_eq!(std::fs::read(&app).unwrap(), b"current");
    }

    #[test]
    fn a_failed_install_keeps_the_earlier_copy_and_not_the_running_version() {
        let tmp = scratch();
        let data = tmp.path().join("data");
        let app = tmp.path().join("WeaveSignal.AppImage");
        // 0.1.0 was kept when 0.2.0 was installed.
        std::fs::write(&app, b"version 0.1.0").unwrap();
        stage_previous(&data, &app).unwrap();
        keep_staged(&data, "0.1.0").unwrap();
        std::fs::write(&app, b"version 0.2.0").unwrap();
        // Installing 0.3.0 fails: 0.2.0 still runs, and the way back is still 0.1.0.
        stage_previous(&data, &app).unwrap();
        drop_staged(&data);
        assert_eq!(previous_of(&data).as_deref(), Some("0.1.0"));
        assert_eq!(std::fs::read(data.join(KEPT)).unwrap(), b"version 0.1.0");
        assert!(!staged(&data).exists());
    }

    #[test]
    #[cfg(unix)]
    fn an_appimage_lost_by_a_failed_install_comes_back_from_the_staged_copy() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = scratch();
        let data = tmp.path().join("data");
        let app = tmp.path().join("WeaveSignal.AppImage");
        std::fs::write(&app, b"version 0.2.0").unwrap();
        std::fs::set_permissions(&app, std::fs::Permissions::from_mode(0o755)).unwrap();
        stage_previous(&data, &app).unwrap();
        // The install failed and the file is gone: the staged copy is kept, as update_install does.
        std::fs::remove_file(&app).unwrap();
        keep_staged(&data, "0.2.0").unwrap();
        assert_eq!(restore_previous(&data, &app, "0.3.0").unwrap(), "0.2.0");
        assert_eq!(std::fs::read(&app).unwrap(), b"version 0.2.0");
        let mode = std::fs::metadata(&app).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
    }

    #[test]
    #[cfg(unix)]
    fn a_rollback_that_cannot_replace_the_file_leaves_it_and_no_stage() {
        let tmp = scratch();
        let data = tmp.path().join("data");
        let app = tmp.path().join("WeaveSignal.AppImage");
        std::fs::write(&app, b"version 0.1.0").unwrap();
        stage_previous(&data, &app).unwrap();
        keep_staged(&data, "0.1.0").unwrap();
        std::fs::write(&app, b"version 0.2.0").unwrap();
        // A directory where the AppImage was: the rename over it fails.
        std::fs::remove_file(&app).unwrap();
        std::fs::create_dir(&app).unwrap();
        std::fs::write(app.join("inside"), b"x").unwrap();
        assert!(restore_previous(&data, &app, "0.2.0").is_err());
        assert!(!app.with_extension("rollback").exists());
        assert_eq!(
            previous_of(&data).as_deref(),
            Some("0.1.0"),
            "the copy is not spent"
        );
    }

    #[test]
    fn a_damaged_memory_file_reads_as_empty() {
        let tmp = scratch();
        std::fs::write(tmp.path().join(MEMORY), "{ not json").unwrap();
        assert_eq!(load(tmp.path()), Memory::default());
    }
}
