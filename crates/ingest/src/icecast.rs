// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Icecast's source protocol as a host's software speaks it (ADR-0280; the build's recce): a
//! `PUT` (newer clients, often with `Expect: 100-continue`) or `SOURCE` (older ones) to any
//! mount, with basic authentication whose password is the key id and the secret joined by a
//! hyphen, and the container in `Content-Type`. Metadata for MP3 and AAC comes as a separate
//! `GET /admin/metadata?mode=updinfo&song=…` with the same credentials.
//!
//! Only the request's head is read here; the media after it goes to the decoding worker.

use base64::Engine;

/// The most a request's head may be.
pub const MAX_HEAD: usize = 8 * 1024;
/// The longest title kept from the host's software.
pub const MAX_TITLE: usize = 200;

/// The key a request carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credentials {
    pub key_id: String,
    pub secret: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    /// A source: what it sends, as a worker's format name, with its key.
    Source {
        credentials: Credentials,
        /// `mp3`, `aac`, `ogg` or `flac`; `Err` with the type as sent when it is none of them.
        format: Result<&'static str, String>,
        /// The mount asked for (any is taken).
        mount: String,
    },
    /// A metadata update for the source with this key.
    Metadata {
        credentials: Credentials,
        title: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Bad {
    /// Not a request the ingest serves (405).
    Method,
    /// No usable credentials (401).
    Credentials,
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|l| {
        let (n, v) = l.split_once(':')?;
        n.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// The worker's format for a `Content-Type`, never a guess from the bytes.
pub fn format_of(content_type: &str) -> Result<&'static str, String> {
    let t = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match t.as_str() {
        "audio/mpeg" | "audio/mp3" | "audio/mpeg3" => Ok("mp3"),
        "audio/aac" | "audio/aacp" | "audio/x-aac" => Ok("aac"),
        "application/ogg" | "audio/ogg" | "audio/opus" | "audio/vorbis" | "audio/x-vorbis+ogg" => {
            Ok("ogg")
        }
        "audio/flac" | "audio/x-flac" => Ok("flac"),
        "" => Err("nothing named".into()),
        _ => Err(t.chars().filter(|c| !c.is_control()).take(60).collect()),
    }
}

/// The key from a basic `Authorization`: `<key id>-<secret>` as the password with any user, or
/// the key id as the user and the secret as the password.
pub fn credentials(head: &str) -> Option<Credentials> {
    credentials_value(header(head, "authorization")?)
}

/// The key from an `Authorization` header's value, as [`credentials`] reads it (the bridge's
/// upgrade carries the same password).
pub fn credentials_value(value: &str) -> Option<Credentials> {
    let b64 = value
        .strip_prefix("Basic ")
        .or_else(|| value.strip_prefix("basic "))?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()?;
    let text = String::from_utf8(raw).ok()?;
    let (user, password) = text.split_once(':')?;
    let looks = |s: &str| {
        s.len() == 8
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c))
    };
    if let Some((key_id, secret)) = password.split_once('-')
        && looks(key_id)
    {
        return Some(Credentials {
            key_id: key_id.to_owned(),
            secret: secret.to_owned(),
        });
    }
    looks(user).then(|| Credentials {
        key_id: user.to_owned(),
        secret: password.to_owned(),
    })
}

/// `%XX` and `+` decoded; anything malformed kept as it was.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = |c: u8| (c as char).to_digit(16);
                match (hex(b[i + 1]), hex(b[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A title as the station keeps it: data only, control characters dropped, at most
/// [`MAX_TITLE`] characters (ADR-0048).
pub fn clean_title(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(MAX_TITLE)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Read a request's head.
pub fn parse(head: &str) -> Result<Request, Bad> {
    let first = head.lines().next().unwrap_or_default();
    let mut parts = first.split_whitespace();
    let (method, target) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or("/"),
    );
    match method {
        "PUT" | "SOURCE" => {
            let credentials = credentials(head).ok_or(Bad::Credentials)?;
            Ok(Request::Source {
                credentials,
                format: format_of(header(head, "content-type").unwrap_or_default()),
                mount: target
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(100)
                    .collect(),
            })
        }
        "GET" if target.starts_with("/admin/metadata") => {
            let credentials = credentials(head).ok_or(Bad::Credentials)?;
            let query = target.split_once('?').map(|(_, q)| q).unwrap_or_default();
            let song = query
                .split('&')
                .find_map(|kv| kv.strip_prefix("song="))
                .map(unescape)
                .unwrap_or_default();
            Ok(Request::Metadata {
                credentials,
                title: clean_title(&song),
            })
        }
        _ => Err(Bad::Method),
    }
}

/// The answer to a refused source: a status line Icecast clients understand, and a short body
/// with the reason for a person reading a log.
pub fn refusal(status: u16, why: &str) -> String {
    let text = match status {
        401 => "Unauthorized",
        403 => "Forbidden",
        405 => "Method Not Allowed",
        415 => "Unsupported Media Type",
        _ => "Service Unavailable",
    };
    let body = clean_title(why);
    let mut head = format!(
        "HTTP/1.0 {status} {text}\r\nServer: weavesignal-ingest\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n",
        body.len() + 1
    );
    if status == 401 {
        head.push_str("WWW-Authenticate: Basic realm=\"WeaveSignal\"\r\n");
    }
    format!("{head}\r\n{body}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth(user: &str, password: &str) -> String {
        format!(
            "Authorization: Basic {}\r\n",
            base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))
        )
    }

    #[test]
    fn ffmpegs_put_is_a_source_with_its_key_and_container() {
        // What ffmpeg 9 sends (the recce's capture), with a show key as the password.
        let head = format!(
            "PUT /live HTTP/1.1\r\nUser-Agent: Lavf/63.1.102\r\nExpect: 100-continue\r\nContent-Type: audio/mpeg\r\nIcy-MetaData: 1\r\n{}\r\n",
            auth("source", "abcd2345-thesecretthesecretthesec")
        );
        assert_eq!(
            parse(&head).unwrap(),
            Request::Source {
                credentials: Credentials {
                    key_id: "abcd2345".into(),
                    secret: "thesecretthesecretthesec".into()
                },
                format: Ok("mp3"),
                mount: "/live".into(),
            }
        );
    }

    #[test]
    fn source_with_the_key_id_as_user_and_an_ogg_type() {
        let head = format!(
            "SOURCE /stream.ogg ICE/1.0\r\ncontent-type: application/ogg\r\n{}\r\n",
            auth("abcd2345", "s3cr3t")
        );
        let Request::Source {
            credentials,
            format,
            ..
        } = parse(&head).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            (credentials.key_id.as_str(), format),
            ("abcd2345", Ok("ogg"))
        );
    }

    #[test]
    fn a_type_the_station_does_not_read_is_named_and_nothing_is_guessed() {
        assert_eq!(format_of("audio/x-ms-wma"), Err("audio/x-ms-wma".into()));
        assert_eq!(format_of("audio/aacp; charset=x"), Ok("aac"));
        assert_eq!(format_of("Audio/FLAC"), Ok("flac"));
        assert!(format_of("").is_err());
    }

    #[test]
    fn no_key_no_source_and_other_methods_are_refused() {
        assert_eq!(parse("PUT /live HTTP/1.1\r\n\r\n"), Err(Bad::Credentials));
        let head = format!("PUT /live HTTP/1.1\r\n{}\r\n", auth("source", "hackme"));
        assert_eq!(parse(&head), Err(Bad::Credentials));
        assert_eq!(parse("GET / HTTP/1.1\r\n\r\n"), Err(Bad::Method));
        assert_eq!(parse("DELETE /live HTTP/1.1\r\n\r\n"), Err(Bad::Method));
    }

    #[test]
    fn metadata_is_decoded_cleaned_and_cut() {
        let head = format!(
            "GET /admin/metadata?mount=/live&mode=updinfo&song=Blue+Peter%20%E2%80%94%20Theme%07 HTTP/1.0\r\n{}\r\n",
            auth("source", "abcd2345-x")
        );
        let Request::Metadata { title, .. } = parse(&head).unwrap() else {
            panic!()
        };
        assert_eq!(title, "Blue Peter — Theme");
        assert_eq!(clean_title(&"x".repeat(500)).chars().count(), MAX_TITLE);
        assert_eq!(unescape("100%"), "100%");
        assert_eq!(unescape("%zz"), "%zz");
        assert_eq!(unescape("%é1"), "%é1");
    }

    #[test]
    fn a_refusal_is_an_answer_icecast_clients_read() {
        let r = refusal(401, "The key is wrong.");
        assert!(r.starts_with("HTTP/1.0 401 Unauthorized\r\n"));
        assert!(r.contains("WWW-Authenticate"));
        assert!(r.ends_with("\r\n\r\nThe key is wrong.\n"));
    }
}
