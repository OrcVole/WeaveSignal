// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Sign-in codes by email: piped to a sendmail-compatible program (msmtp, sendmail -t), or
//! written to an outbox directory (development and tests). The address is used for this one
//! email and never stored (only its hash is, `chat_account.email_hash`).

use std::process::Stdio;

use tokio::io::AsyncWriteExt;

use crate::config::{MailConfig, MailMode};

#[derive(Debug, thiserror::Error)]
pub enum MailError {
    #[error("email sign-in is off at this station")]
    Off,
    #[error("the email could not be sent: {0}")]
    Failed(String),
}

/// One plain-text email, with headers that cannot be injected (no line breaks in them).
pub fn compose(from: &str, to: &str, subject: &str, body: &str) -> Result<String, MailError> {
    for h in [from, to, subject] {
        if h.contains(['\r', '\n']) {
            return Err(MailError::Failed("a header holds a line break".into()));
        }
    }
    Ok(format!(
        "From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\nAuto-Submitted: auto-generated\r\n\r\n{}\r\n",
        body.replace('\n', "\r\n")
    ))
}

pub async fn send(cfg: &MailConfig, to: &str, subject: &str, body: &str) -> Result<(), MailError> {
    let text = compose(&cfg.from, to, subject, body)?;
    match cfg.mode {
        MailMode::Off => Err(MailError::Off),
        MailMode::Outbox => {
            tokio::fs::create_dir_all(&cfg.outbox_dir)
                .await
                .map_err(|e| MailError::Failed(e.to_string()))?;
            let name = format!(
                "{}-{}.eml",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0),
                uuid::Uuid::new_v4().simple()
            );
            tokio::fs::write(cfg.outbox_dir.join(name), text)
                .await
                .map_err(|e| MailError::Failed(e.to_string()))
        }
        MailMode::Command => {
            let (prog, args) = cfg
                .command
                .split_first()
                .ok_or_else(|| MailError::Failed("no mail command".into()))?;
            let mut child = tokio::process::Command::new(prog)
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                // A mailer past the timeout is stopped, not left running on its own.
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| MailError::Failed(format!("{prog}: {e}")))?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(text.as_bytes())
                    .await
                    .map_err(|e| MailError::Failed(e.to_string()))?;
            }
            let out =
                tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output())
                    .await
                    .map_err(|_| MailError::Failed(format!("{prog} took longer than 30 s")))?
                    .map_err(|e| MailError::Failed(e.to_string()))?;
            if out.status.success() {
                Ok(())
            } else {
                // The error is logged, and mailers name the recipient in theirs.
                Err(MailError::Failed(format!(
                    "{prog} exited with {}: {}",
                    out.status,
                    without_addresses(String::from_utf8_lossy(&out.stderr).trim())
                )))
            }
        }
    }
}

/// A mailer's message with every word that holds an `@` replaced, so a log of it names no one.
fn without_addresses(text: &str) -> String {
    text.split_inclusive(char::is_whitespace)
        .map(|w| {
            let word = w.trim_end();
            if word.contains('@') {
                format!("[address]{}", &w[word.len()..])
            } else {
                w.to_owned()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mailer_error_names_no_address() {
        let shown = without_addresses(
            "msmtp: recipient address <Morag.Henderson@mail.example> not accepted\nsendmail: 550 morag@x.example: unknown",
        );
        assert!(!shown.contains('@'), "{shown}");
        assert!(
            shown.contains("recipient address [address] not accepted\n"),
            "{shown}"
        );
        assert_eq!(
            without_addresses("connection refused"),
            "connection refused"
        );
    }

    #[test]
    fn headers_cannot_carry_line_breaks() {
        assert!(compose("a@b.example", "x@y.example\r\nBcc: z@z.example", "s", "b").is_err());
        let m = compose("a@b.example", "x@y.example", "Your code", "123456\nbye").unwrap();
        assert!(m.contains("To: x@y.example\r\nSubject: Your code\r\n"));
        assert!(m.ends_with("123456\r\nbye\r\n"));
    }
}
