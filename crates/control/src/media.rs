// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Run the media worker on an upload (ADR-0048 rule 2): a separate, restricted process that
//! reads the untrusted bytes and hands back clean FLAC and a report. The control plane never
//! parses the upload itself; it only writes the FLAC the worker made into the cart directory.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;
use weavesignal_media::Report;

use crate::config::CartsConfig;

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    /// What the worker said is wrong with the file: the uploader's to fix.
    #[error("{0}")]
    Refused(String),
    /// The worker could not run, or broke: the station's to fix.
    #[error("the media worker failed: {0}")]
    Failed(String),
}

/// The worker's program, run as `<program> media-worker normalise ...`: the configured one (a
/// `weavesignal-control`), or this program.
fn program(cfg: &CartsConfig) -> Result<(PathBuf, Vec<String>), WorkerError> {
    let exe = match &cfg.worker {
        Some(p) => p.clone(),
        None => std::env::current_exe()
            .map_err(|e| WorkerError::Failed(format!("cannot find this program: {e}")))?,
    };
    Ok((exe, vec!["media-worker".to_owned()]))
}

/// Run the media worker with `args` (after `media-worker`) on one upload: what it wrote to
/// standard output and its one JSON line.
async fn run_worker(
    cfg: &CartsConfig,
    args: Vec<String>,
    bytes: Vec<u8>,
) -> Result<(Vec<u8>, serde_json::Value), WorkerError> {
    let (exe, mut full) = program(cfg)?;
    full.extend(args);
    let args = full;
    let mut child = tokio::process::Command::new(&exe)
        .args(&args)
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| WorkerError::Failed(format!("cannot start {}: {e}", exe.display())))?;
    let mut stdin = child.stdin.take().expect("piped");
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let feed = async move {
        // The worker may refuse early and close its input; that is its answer, not ours.
        let _ = stdin.write_all(&bytes).await;
        drop(stdin);
    };
    let read_out = async move {
        let mut v = Vec::new();
        stdout.read_to_end(&mut v).await.map(|_| v)
    };
    let read_err = async move {
        let mut v = Vec::new();
        stderr.read_to_end(&mut v).await.map(|_| v)
    };
    let work = async {
        let ((), out, err, status) = tokio::join!(feed, read_out, read_err, child.wait());
        (out, err, status)
    };
    let (out, err, status) = tokio::time::timeout(Duration::from_secs(cfg.worker_timeout_s), work)
        .await
        .map_err(|_| {
            WorkerError::Failed(format!(
                "no result in {} s (`carts.worker_timeout_s`)",
                cfg.worker_timeout_s
            ))
        })?;
    let out = out.map_err(|e| WorkerError::Failed(e.to_string()))?;
    let err = err.map_err(|e| WorkerError::Failed(e.to_string()))?;
    let status = status.map_err(|e| WorkerError::Failed(e.to_string()))?;
    let text = String::from_utf8_lossy(&err);
    let last = text
        .lines()
        .rev()
        .find(|l| l.starts_with('{'))
        .unwrap_or("");
    let json: serde_json::Value = serde_json::from_str(last).unwrap_or_default();
    if let Some(e) = json.get("error").and_then(|e| e.as_str()) {
        return Err(if status.code() == Some(3) {
            WorkerError::Refused(e.to_owned())
        } else {
            WorkerError::Failed(e.to_owned())
        });
    }
    if !status.success() {
        return Err(WorkerError::Failed(format!(
            "it stopped ({status}): {}",
            text.trim().chars().take(300).collect::<String>()
        )));
    }
    let sandbox = json["sandbox"].as_str().unwrap_or("unknown");
    if sandbox != "landlock" {
        tracing::warn!(%sandbox, "the media worker ran without a full Landlock sandbox");
    }
    Ok((out, json))
}

/// Normalise one upload: the FLAC and the worker's report.
pub async fn normalise(
    cfg: &CartsConfig,
    bytes: Vec<u8>,
) -> Result<(Vec<u8>, Report), WorkerError> {
    let (out, json) = run_worker(
        cfg,
        vec![
            "normalise".to_owned(),
            "--target-lufs".to_owned(),
            cfg.loudness_lufs.to_string(),
            "--ceiling-dbtp".to_owned(),
            cfg.ceiling_dbtp.to_string(),
            "--max-seconds".to_owned(),
            cfg.max_seconds.to_string(),
        ],
        bytes,
    )
    .await?;
    let report: Report = serde_json::from_value(json)
        .map_err(|e| WorkerError::Failed(format!("not a report: {e}")))?;
    if out.len() < 4 || &out[..4] != b"fLaC" {
        return Err(WorkerError::Failed("its output is not FLAC".into()));
    }
    Ok((out, report))
}

/// Make one uploaded picture into a clean PNG that fits `max_width` by `max_height` (the
/// video slate's still, ADR-0010): the PNG and the worker's report.
pub async fn still(
    cfg: &CartsConfig,
    bytes: Vec<u8>,
    max_width: u32,
    max_height: u32,
) -> Result<(Vec<u8>, weavesignal_media::still::StillReport), WorkerError> {
    let (out, json) = run_worker(
        cfg,
        vec![
            "still".to_owned(),
            "--max-width".to_owned(),
            max_width.to_string(),
            "--max-height".to_owned(),
            max_height.to_string(),
        ],
        bytes,
    )
    .await?;
    let report: weavesignal_media::still::StillReport = serde_json::from_value(json)
        .map_err(|e| WorkerError::Failed(format!("not a report: {e}")))?;
    if !out.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(WorkerError::Failed("its output is not a PNG".into()));
    }
    Ok((out, report))
}

/// Write a worker's FLAC into the cart directory under a fresh name, all or nothing.
pub async fn store(dir: &Path, flac: &[u8]) -> std::io::Result<String> {
    tokio::fs::create_dir_all(dir).await?;
    let name = format!("{}.flac", Uuid::new_v4().hyphenated());
    let tmp = dir.join(format!(".{name}.part"));
    tokio::fs::write(&tmp, flac).await?;
    tokio::fs::rename(&tmp, dir.join(&name)).await?;
    Ok(name)
}
