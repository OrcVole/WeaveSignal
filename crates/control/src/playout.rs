// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The control plane's link to playout: Liquidsoap's server socket, one command a line, each
//! answer ending with a line `END`. Every word sent is data the fixed script
//! (`playout/weavesignal.liq`) checks, never code. Carts no longer go through it: they sound in
//! the desk (ADR-0117), so the fixed script registers no commands of its own yet.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// How long one command may take before playout counts as not answering.
const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PlayoutError {
    #[error("playout's control socket is not configured (`playout.socket`)")]
    NotConfigured,
    #[error("playout is not answering on {path}: {reason}")]
    Unreachable { path: String, reason: String },
    #[error("playout refused: {0}")]
    Refused(String),
}

pub struct PlayoutClient {
    path: Option<PathBuf>,
}

impl PlayoutClient {
    pub fn new(path: Option<&Path>) -> Self {
        Self {
            path: path.map(Path::to_path_buf),
        }
    }

    pub fn configured(&self) -> bool {
        self.path.is_some()
    }

    /// Send one command and read its answer. A connection a command: the socket is local and
    /// a fresh one never carries a half-read answer from a timed-out command.
    pub async fn command(&self, line: &str) -> Result<String, PlayoutError> {
        let Some(path) = &self.path else {
            return Err(PlayoutError::NotConfigured);
        };
        if line.contains(['\n', '\r']) {
            return Err(PlayoutError::Refused("a command is one line".into()));
        }
        let unreachable = |reason: String| PlayoutError::Unreachable {
            path: path.display().to_string(),
            reason,
        };
        let exchange = async {
            let stream = UnixStream::connect(path).await.map_err(|e| e.to_string())?;
            let (r, mut w) = stream.into_split();
            w.write_all(format!("{line}\n").as_bytes())
                .await
                .map_err(|e| e.to_string())?;
            let mut lines = BufReader::new(r).lines();
            let mut answer = Vec::new();
            loop {
                match lines.next_line().await.map_err(|e| e.to_string())? {
                    None => return Err("playout closed the connection".to_owned()),
                    Some(l) if l.trim_end() == "END" => break,
                    Some(l) => answer.push(l.trim_end().to_owned()),
                }
            }
            let _ = w.write_all(b"quit\n").await;
            Ok(answer.join("\n"))
        };
        match tokio::time::timeout(TIMEOUT, exchange).await {
            Ok(Ok(a)) => Ok(a),
            Ok(Err(reason)) => Err(unreachable(reason)),
            Err(_) => Err(unreachable(format!("no answer in {} s", TIMEOUT.as_secs()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_missing_playout_is_unreachable_and_unconfigured_is_said() {
        let dir = crate::desk::tests::tempdir();
        let p = PlayoutClient::new(Some(&dir.join("none.sock")));
        assert!(matches!(
            p.command("help").await,
            Err(PlayoutError::Unreachable { .. })
        ));
        assert_eq!(
            PlayoutClient::new(None).command("help").await,
            Err(PlayoutError::NotConfigured)
        );
        assert!(matches!(
            PlayoutClient::new(Some(&dir.join("x")))
                .command("a\nb")
                .await,
            Err(PlayoutError::Refused(_))
        ));
        let _ = std::fs::remove_dir_all(dir);
    }
}
